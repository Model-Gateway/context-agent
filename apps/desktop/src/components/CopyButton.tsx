import { useEffect, useRef, useState } from 'react'
import { Icon } from '../theme/icons'
import { copyText } from './clipboard'
import { Tooltip } from './Tooltip'
import styles from './CopyButton.module.css'

export { copyText } from './clipboard'

/** How long the checkmark shows after a copy (1.2 s in the Swift view; 1.5 s per the port's brief). */
const COPIED_INTERVAL = 1500

export interface CopyButtonProps {
  /** What a press copies. */
  text: string
  /** The tooltip and accessible name; "Copy" by default. */
  label?: string
  /** Glyph size; 13 by default (the button is 28 px square). */
  size?: number
  className?: string
}

/**
 * `CopyButton` of the macOS app: icon only, the glyph turns into a green
 * checkmark for a moment after copying and the tooltip / accessible name
 * read "Copied" (no label, so the button never changes size). Writes
 * through the Tauri clipboard plugin with a `navigator.clipboard` fallback.
 */
export function CopyButton({ text, label = 'Copy', size = 13, className }: CopyButtonProps) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | null>(null)

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current)
    },
    [],
  )

  const copy = () => {
    void copyText(text)
    setCopied(true)
    if (timer.current !== null) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      setCopied(false)
      timer.current = null
    }, COPIED_INTERVAL)
  }

  return (
    <Tooltip content={copied ? 'Copied' : label}>
      <button
        type="button"
        className={[styles.button, copied ? styles.copied : '', className ?? ''].filter(Boolean).join(' ')}
        aria-label={copied ? 'Copied' : label}
        onClick={copy}
      >
        <Icon name={copied ? 'copied' : 'copy'} size={size} strokeWidth={2} />
      </button>
    </Tooltip>
  )
}
