import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Icon, type IconName } from '../theme/icons'
import { formatShortcut } from './platform'
import { positionAnchored, positionAtPoint, prefersReducedMotion, restoreFocus, useOverlay, type Alignment, type Placement } from './overlays'
import styles from './Menu.module.css'

/** One entry of a `Menu`: an action, a separator or a section header. */
export type MenuItem =
  | {
      type?: 'item'
      /** Stable key; the label by default. */
      id?: string
      label: string
      icon?: IconName
      /** Shown at the right, spelled for the platform ("mod+C"). */
      shortcut?: string
      /** `role: .destructive`: rendered in `danger`. */
      destructive?: boolean
      disabled?: boolean
      /** A checkmark before the label (the current choice). */
      checked?: boolean
      /** The item's `.help` text: a native tooltip, shown on disabled items too. */
      help?: string
      onSelect: () => void
    }
  | { type: 'separator'; id?: string }
  | { type: 'header'; label: string; id?: string }

export interface MenuProps {
  open: boolean
  onClose: () => void
  items: MenuItem[]
  /** A dropdown hangs off this element… */
  anchorRef?: RefObject<HTMLElement | null>
  /** …or a context menu opens at this point (viewport coordinates). */
  position?: { x: number; y: number } | null
  placement?: Placement
  align?: Alignment
  minWidth?: number
  'aria-label'?: string
  className?: string
}

interface Box {
  top: number
  left: number
  maxHeight: number
}

function isAction(item: MenuItem): item is Extract<MenuItem, { onSelect: () => void }> {
  return item.type === undefined || item.type === 'item'
}

function itemKey(item: MenuItem, index: number): string {
  if (item.id) return item.id
  if (item.type === 'separator') return `separator-${index}`
  return `${item.type ?? 'item'}-${item.label}-${index}`
}

/**
 * The macOS `Menu` / `.contextMenu`: a floating list of actions with icon,
 * label, shortcut hint, destructive and disabled states, separators and
 * section headers; no submenus. Opens as a dropdown under `anchorRef` or
 * as a context menu at `position`. Arrow keys move, Enter / Space select,
 * type-ahead jumps, Esc closes.
 */
export function Menu({ open, onClose, items, anchorRef, position, placement = 'below', align = 'start', minWidth = 180, className, ...aria }: MenuProps) {
  const panelRef = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const [box, setBox] = useState<Box | null>(null)
  const [active, setActive] = useState<number>(-1)
  const previousFocus = useRef<HTMLElement | null>(null)
  const typeahead = useRef<{ text: string; at: number }>({ text: '', at: 0 })

  const anchors = anchorRef ? [anchorRef] : []
  useOverlay({ open, ref: panelRef, anchors, onDismiss: onClose })

  const enabledIndices = items.map((item, index) => (isAction(item) && !item.disabled ? index : -1)).filter((index) => index >= 0)

  const reposition = useCallback(() => {
    const panel = panelRef.current
    if (!panel) return
    const size = { width: panel.offsetWidth, height: panel.offsetHeight }
    let next: Box
    if (position) {
      const at = positionAtPoint(position, size)
      next = { top: at.top, left: at.left, maxHeight: at.maxHeight }
    } else if (anchorRef?.current) {
      const at = positionAnchored(anchorRef.current.getBoundingClientRect(), size, placement, align, 4)
      next = { top: at.top, left: at.left, maxHeight: at.maxHeight }
    } else {
      next = { top: 8, left: 8, maxHeight: window.innerHeight - 16 }
    }
    setBox((previous) => (previous && previous.top === next.top && previous.left === next.left && previous.maxHeight === next.maxHeight ? previous : next))
  }, [anchorRef, position, placement, align])

  useLayoutEffect(() => {
    if (!open) {
      setBox(null)
      setActive(-1)
      return
    }
    reposition()
  }, [open, reposition, items])

  useEffect(() => {
    if (!open) return
    previousFocus.current = (document.activeElement as HTMLElement | null) ?? null
    const handle = requestAnimationFrame(() => panelRef.current?.focus({ preventScroll: true }))
    const onChange = () => reposition()
    window.addEventListener('resize', onChange)
    window.addEventListener('scroll', onChange, true)
    return () => {
      cancelAnimationFrame(handle)
      window.removeEventListener('resize', onChange)
      window.removeEventListener('scroll', onChange, true)
      const target = previousFocus.current
      if (target && document.contains(target)) restoreFocus(target)
    }
  }, [open, reposition])

  // Keyboard navigation keeps the active item in view in a long menu.
  useEffect(() => {
    if (!open || active < 0) return
    panelRef.current?.querySelector<HTMLElement>(`[data-menu-index="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [open, active])

  const select = (index: number) => {
    const item = items[index]
    if (!item || !isAction(item) || item.disabled) return
    onClose()
    item.onSelect()
  }

  const move = (delta: number) => {
    if (enabledIndices.length === 0) return
    const current = enabledIndices.indexOf(active)
    const next = current < 0 ? (delta > 0 ? 0 : enabledIndices.length - 1) : (current + delta + enabledIndices.length) % enabledIndices.length
    setActive(enabledIndices[next] ?? -1)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        move(1)
        return
      case 'ArrowUp':
        event.preventDefault()
        move(-1)
        return
      case 'Home':
        event.preventDefault()
        setActive(enabledIndices[0] ?? -1)
        return
      case 'End':
        event.preventDefault()
        setActive(enabledIndices[enabledIndices.length - 1] ?? -1)
        return
      case 'Enter':
      case ' ':
        event.preventDefault()
        event.stopPropagation()
        if (active >= 0) select(active)
        return
      case 'Tab':
        event.preventDefault()
        onClose()
        return
      case 'Escape':
        // Handled by the overlay stack (document capture); never reaches the composer.
        event.stopPropagation()
        return
      default:
        break
    }
    if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const now = Date.now()
      const text = (now - typeahead.current.at < 600 ? typeahead.current.text : '') + event.key.toLowerCase()
      typeahead.current = { text, at: now }
      const match = enabledIndices.find((index) => {
        const item = items[index]
        return item && isAction(item) && item.label.toLowerCase().startsWith(text)
      })
      if (match !== undefined) setActive(match)
    }
  }

  if (!open) return null

  const style: CSSProperties = {
    top: box?.top ?? -9999,
    left: box?.left ?? -9999,
    maxHeight: box?.maxHeight,
    minWidth,
    visibility: box ? 'visible' : 'hidden',
  }
  const hasChecks = items.some((item) => isAction(item) && item.checked)

  return createPortal(
    <div
      ref={panelRef}
      role="menu"
      aria-label={aria['aria-label']}
      aria-activedescendant={active >= 0 ? `${menuId}-${active}` : undefined}
      tabIndex={-1}
      className={[styles.menu, box && !prefersReducedMotion() ? styles.enter : '', className ?? ''].filter(Boolean).join(' ')}
      style={style}
      onKeyDown={onKeyDown}
      onPointerLeave={() => setActive(-1)}
    >
      {items.map((item, index) => {
        const key = itemKey(item, index)
        if (item.type === 'separator') return <div key={key} role="separator" className={styles.separator} />
        if (item.type === 'header') {
          return (
            <div key={key} role="presentation" className={styles.header}>
              {item.label}
            </div>
          )
        }
        const classes = [
          styles.item,
          index === active ? styles.active : '',
          item.destructive ? styles.destructive : '',
          item.disabled ? styles.disabled : '',
        ]
          .filter(Boolean)
          .join(' ')
        return (
          <div
            key={key}
            role={item.checked !== undefined ? 'menuitemcheckbox' : 'menuitem'}
            id={`${menuId}-${index}`}
            data-menu-index={index}
            aria-disabled={item.disabled || undefined}
            aria-checked={item.checked}
            title={item.help}
            className={classes}
            onPointerEnter={() => setActive(item.disabled ? -1 : index)}
            onClick={(event) => {
              event.preventDefault()
              select(index)
            }}
          >
            {hasChecks ? (
              <span className={styles.check}>{item.checked ? <Icon name="copied" size={12} strokeWidth={2.5} /> : null}</span>
            ) : null}
            {item.icon ? <Icon name={item.icon} size={14} strokeWidth={2} className={styles.icon} /> : null}
            <span className={styles.label}>{item.label}</span>
            {item.shortcut ? <span className={styles.shortcut}>{formatShortcut(item.shortcut)}</span> : null}
          </div>
        )
      })}
    </div>,
    document.body,
  )
}
