import { useState, type MouseEvent, type ReactNode } from 'react'
import { Menu, type MenuItem } from './Menu'
import styles from './ContextMenu.module.css'

export interface ContextMenuProps {
  /** The items, or a function building them at the moment the menu opens. */
  items: MenuItem[] | (() => MenuItem[])
  disabled?: boolean
  /** Exactly one element (or a fragment) that reacts to a right click. */
  children: ReactNode
  'aria-label'?: string
}

/**
 * `.contextMenu { … }` of the macOS views: wraps an element so a right
 * click (the `contextmenu` event) opens a `Menu` at the pointer. The
 * wrapper has no box of its own (`display: contents`).
 */
export function ContextMenu({ items, disabled = false, children, ...aria }: ContextMenuProps) {
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null)
  const [resolved, setResolved] = useState<MenuItem[]>([])

  const onContextMenu = (event: MouseEvent<HTMLElement>) => {
    if (disabled) return
    event.preventDefault()
    event.stopPropagation()
    const next = typeof items === 'function' ? items() : items
    if (next.length === 0) return
    setResolved(next)
    // The context-menu key / Shift+F10 reports no pointer position: open
    // under the focused element instead.
    if (event.clientX === 0 && event.clientY === 0 && event.target instanceof Element) {
      const rect = event.target.getBoundingClientRect()
      setPosition({ x: rect.left, y: rect.bottom })
    } else {
      setPosition({ x: event.clientX, y: event.clientY })
    }
  }

  return (
    <>
      <span className={styles.wrapper} onContextMenu={onContextMenu}>
        {children}
      </span>
      <Menu open={position !== null} onClose={() => setPosition(null)} items={resolved} position={position} aria-label={aria['aria-label']} />
    </>
  )
}
