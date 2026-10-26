// Port of apps/macos/ContextAgent/Views/Composer/PermissionsPopover.swift:
// the permissions popover opened from the composer's mode chip (and the top
// bar's approval chip).

import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Popover } from '../../components/Popover'
import { BYPASS_PERMISSION_PROFILE, permissionProfileOptions, type PermissionProfileOption } from '../../models/permissions'
import { selectedProject, useApp } from '../../state/app'
import { Icon } from '../../theme/icons'
import { isPlainReturn, useNativeKeyDown } from './nativeKeyDown'
import styles from './PermissionsPopover.module.css'

export interface PermissionsPopoverProps {
  open: boolean
  /** Esc, an outside press, or a choice (`dismiss()`). */
  onClose: () => void
  /** The chip it hangs off. */
  anchorRef: RefObject<HTMLElement | null>
  /** Give focus back to the chip on close; the composer refocuses itself instead (false by default). */
  returnFocus?: boolean
}

/// `PermissionsPopover` (PermissionsPopover.swift): one row per profile
/// (Ask · Auto · Bypass, as in the TUI's `/approval`) with its icon and
/// description, the current one checked. A click or Return switches the
/// selected project's mode for its next runs (`permissions/set` through
/// `setApprovalMode`) and closes the popover; ↑↓ move, Esc closes. 260 px.
export function PermissionsPopover({ open, onClose, anchorRef, returnFocus = false }: PermissionsPopoverProps) {
  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="below" align="center" width={262} returnFocus={returnFocus} aria-label="Permissions" role="listbox">
      <PermissionList onClose={onClose} />
    </Popover>
  )
}

function PermissionList({ onClose }: { onClose: () => void }) {
  const hasProject = useApp((state) => selectedProject(state) !== null)
  const projectProfile = useApp((state) => selectedProject(state)?.info.permission_profile ?? null)
  const configProfile = useApp((state) => state.config?.permissions.profile ?? null)
  const current = projectProfile ?? configProfile ?? 'approval_required'
  const options = permissionProfileOptions(current)
  /** The profile a `permissions/set` is in flight for. */
  const [pendingProfile, setPendingProfile] = useState<string | null>(null)
  /** Keyboard / hover highlight, an index into the options. */
  const [highlighted, setHighlighted] = useState<number | null>(() => {
    const index = options.findIndex((option) => option.id === current)
    return index >= 0 ? index : null
  })
  const containerRef = useRef<HTMLDivElement>(null)
  const enabled = hasProject && pendingProfile === null

  // Popover content is attached a beat after creation; focusing then sticks.
  useEffect(() => {
    const timer = window.setTimeout(() => containerRef.current?.focus({ preventScroll: true }), 60)
    return () => window.clearTimeout(timer)
  }, [])

  const select = (profile: string) => {
    const target = selectedProject(useApp.getState())
    if (pendingProfile !== null || !target) return
    if (target.info.permission_profile === profile) {
      onClose()
      return
    }
    setPendingProfile(profile)
    void (async () => {
      await useApp.getState().setApprovalMode(profile)
      setPendingProfile(null)
      onClose()
    })()
  }

  const move = (delta: number) => {
    if (options.length === 0) return
    setHighlighted((index) => Math.min(Math.max((index ?? -1) + delta, 0), options.length - 1))
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    }
  }

  // Return switches to the highlighted profile (natively: see nativeKeyDown.ts).
  useNativeKeyDown(containerRef, (event) => {
    if (!isPlainReturn(event)) return
    event.preventDefault()
    event.stopPropagation()
    const option = highlighted !== null ? options[highlighted] : undefined
    if (option) select(option.id)
  })

  return (
    <div ref={containerRef} className={styles.list} tabIndex={0} onKeyDown={onKeyDown}>
      {options.map((option, index) => (
        <PermissionRow
          key={option.id}
          option={option}
          isCurrent={option.id === current}
          isPending={option.id === pendingProfile}
          isHighlighted={highlighted === index}
          enabled={enabled}
          onSelect={() => select(option.id)}
          onHover={() => setHighlighted(index)}
        />
      ))}
    </div>
  )
}

interface PermissionRowProps {
  option: PermissionProfileOption
  isCurrent: boolean
  isPending: boolean
  isHighlighted: boolean
  enabled: boolean
  onSelect: () => void
  onHover: () => void
}

function PermissionRow({ option, isCurrent, isPending, isHighlighted, enabled, onSelect, onHover }: PermissionRowProps) {
  // Bypass never asks, so its icon stays in the warning tint.
  const risky = option.id === BYPASS_PERMISSION_PROFILE.id
  const iconTint = risky ? 'var(--warning)' : isCurrent ? 'var(--accent)' : 'var(--text-secondary)'
  const iconBackground = risky
    ? 'color-mix(in srgb, var(--warning) 12%, transparent)'
    : isCurrent
      ? 'var(--accent-soft)'
      : 'var(--surface-raised)'
  return (
    <button
      type="button"
      role="option"
      tabIndex={-1}
      aria-selected={isCurrent}
      aria-label={`${option.title}: ${option.description}`}
      disabled={!enabled}
      className={[styles.row, isHighlighted ? styles.highlighted : ''].filter(Boolean).join(' ')}
      onClick={onSelect}
      onMouseEnter={onHover}
    >
      <span className={styles.icon} style={{ color: iconTint, background: iconBackground }} aria-hidden="true">
        <Icon name={option.icon} size={12} strokeWidth={2} />
      </span>
      <span className={styles.text}>
        <span className={styles.title} style={{ fontWeight: isCurrent ? 600 : 500 }}>
          {option.title}
        </span>
        <span className={styles.description}>{option.description}</span>
      </span>
      <span className={styles.trailing}>
        {isPending ? (
          <BrailleSpinner size={11} tint="var(--text-secondary)" />
        ) : isCurrent ? (
          <Icon name="checkmark" size={11} strokeWidth={3} className={styles.check} />
        ) : null}
      </span>
    </button>
  )
}
