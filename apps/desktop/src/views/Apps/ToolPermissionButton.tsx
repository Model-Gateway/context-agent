import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { McpToolInfo, McpToolPermission } from '../../backend/types'
import { Popover } from '../../components/Popover'
import { Tooltip } from '../../components/Tooltip'
import { Icon, type IconName } from '../../theme/icons'
import styles from './ToolPermissionButton.module.css'

/** `ToolPermissionLook`: icon, tint and wording for one permission state. */
interface ToolPermissionLook {
  title: string
  detail: string
  icon: IconName
  tint: string
}

const ALLOW: ToolPermissionLook = { title: 'Always allow', detail: 'Runs without asking', icon: 'checkFill', tint: 'var(--success)' }
const ASK: ToolPermissionLook = { title: 'Always ask', detail: 'Asks every time, under every profile but Bypass', icon: 'approval', tint: 'var(--accent)' }

/** `ToolPermissionLook.annotated(_:)`: what the server's annotations and the chat's profile decide. */
function annotated(tool: McpToolInfo): ToolPermissionLook {
  if (tool.read_only) return { title: 'Default', detail: 'Read-only: runs without asking', icon: 'check', tint: 'var(--success)' }
  if (tool.destructive) return { title: 'Default', detail: 'Destructive: asks under Ask and Auto', icon: 'warningFill', tint: 'var(--danger)' }
  return { title: 'Default', detail: 'Asks under the Ask profile', icon: 'approval', tint: 'var(--text-tertiary)' }
}

/** `ToolPermissionLook.current(tool:override:bypass:)`. */
function currentLook(tool: McpToolInfo, override: McpToolPermission | null, bypass: boolean): ToolPermissionLook {
  switch (override) {
    case 'allow':
      return ALLOW
    case 'ask':
      return ASK
    default:
      if (bypass) return { title: 'Bypassed', detail: 'Runs without asking (the app bypasses permissions)', icon: 'boltFill', tint: 'var(--warning)' }
      return annotated(tool)
  }
}

export interface ToolPermissionButtonProps {
  tool: McpToolInfo
  /** The tool's own setting (`[mcp.servers.<name>.permissions]`); null is Default. */
  override: McpToolPermission | null
  /** The app's "Bypass permissions" switch. */
  bypass: boolean
  enabled: boolean
  onSelect: (permission: McpToolPermission | null) => void
}

/**
 * `ToolPermissionButton` (Views/Apps/AppsView.swift): a tool's permission
 * icon on the app page — what happens when the agent calls it (its own
 * setting, else the app's bypass, else the server's annotations) with a
 * small chevron. Clicking opens `ToolPermissionPopover` below it.
 */
export function ToolPermissionButton({ tool, override, bypass, enabled, onSelect }: ToolPermissionButtonProps) {
  const [presented, setPresented] = useState(false)
  const anchorRef = useRef<HTMLButtonElement>(null)
  const look = currentLook(tool, override, bypass)
  return (
    <>
      <Tooltip content={`${look.title}: ${look.detail} · click to change`}>
        <button
          ref={anchorRef}
          type="button"
          className={styles.button}
          disabled={!enabled}
          aria-label={`Permission for ${tool.name}: ${look.title}`}
          aria-haspopup="dialog"
          aria-expanded={presented}
          onClick={() => setPresented(true)}
        >
          <Icon name={look.icon} size={12} style={{ color: look.tint }} />
          <Icon name="chevronDown" size={7} strokeWidth={3} className={styles.chevron} />
        </button>
      </Tooltip>
      <Popover
        open={presented}
        onClose={() => setPresented(false)}
        anchorRef={anchorRef}
        placement="below"
        align="center"
        width={260}
        initialFocus="none"
        aria-label={`Permission for ${tool.name}`}
      >
        <ToolPermissionPopover
          tool={tool}
          override={override}
          bypass={bypass}
          onChoose={(choice) => {
            setPresented(false)
            if (choice !== override) onSelect(choice)
          }}
        />
      </Popover>
    </>
  )
}

interface ToolPermissionPopoverProps {
  tool: McpToolInfo
  override: McpToolPermission | null
  bypass: boolean
  onChoose: (permission: McpToolPermission | null) => void
}

/// `ToolPermissionPopover`: Default (described for this tool), Always
/// allow, Always ask, the current one checked. Arrow keys and Return pick,
/// like the composer's permissions popover.
function ToolPermissionPopover({ tool, override, bypass, onChoose }: ToolPermissionPopoverProps) {
  const fallback: ToolPermissionLook = bypass
    ? { title: 'Default', detail: 'Runs without asking while the app bypasses permissions', icon: 'boltFill', tint: 'var(--warning)' }
    : annotated(tool)
  const options: { value: McpToolPermission | null; look: ToolPermissionLook }[] = [
    { value: null, look: fallback },
    { value: 'allow', look: ALLOW },
    { value: 'ask', look: ASK },
  ]
  const initial = options.findIndex((option) => option.value === override)
  const [highlighted, setHighlighted] = useState<number | null>(initial >= 0 ? initial : null)
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    // Popover content is attached a beat after creation; focusing then sticks.
    const handle = window.setTimeout(() => rootRef.current?.focus({ preventScroll: true }), 60)
    return () => window.clearTimeout(handle)
  }, [])

  // Capture phase: the popover's focus trap stops Return at the panel
  // before React's bubbling handlers would see it.
  const onKeyDownCapture = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setHighlighted((current) => Math.max((current ?? options.length) - 1, 0))
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      setHighlighted((current) => Math.min((current ?? -1) + 1, options.length - 1))
    } else if (event.key === 'Enter' && event.target === rootRef.current) {
      event.preventDefault()
      const option = highlighted !== null ? options[highlighted] : undefined
      if (option) onChoose(option.value)
    }
  }

  return (
    <div ref={rootRef} className={styles.popover} tabIndex={-1} onKeyDownCapture={onKeyDownCapture}>
      <div className={styles.popoverTitle}>{tool.name}</div>
      {options.map((option, index) => {
        const isCurrent = option.value === override
        return (
          <button
            key={option.value ?? 'default'}
            type="button"
            className={[styles.option, highlighted === index ? styles.highlighted : ''].filter(Boolean).join(' ')}
            aria-label={`${option.look.title}: ${option.look.detail}`}
            aria-pressed={isCurrent}
            onMouseEnter={() => setHighlighted(index)}
            onClick={() => onChoose(option.value)}
          >
            <span className={styles.optionIcon} aria-hidden="true">
              <Icon name={option.look.icon} size={12} style={{ color: option.look.tint }} />
            </span>
            <span className={styles.optionText}>
              <span className={[styles.optionTitle, isCurrent ? styles.current : ''].filter(Boolean).join(' ')}>{option.look.title}</span>
              <span className={styles.optionDetail}>{option.look.detail}</span>
            </span>
            <span className={styles.optionCheck} style={{ opacity: isCurrent ? 1 : 0 }} aria-hidden="true">
              <Icon name="checkmark" size={11} strokeWidth={3} />
            </span>
          </button>
        )
      })}
    </div>
  )
}
