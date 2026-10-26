import type { CSSProperties } from 'react'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { cx } from './inspectorShared'
import styles from './MiddleText.module.css'

export interface MiddleTextProps {
  text: string
  /** Fills the row's free space (the Swift text is followed by a `Spacer`). */
  grow?: boolean
  /** Caps the natural width (`.frame(maxWidth: 140, alignment: .leading)`). */
  maxWidth?: number
  className?: string
  style?: CSSProperties
}

/**
 * `Text(…).lineLimit(1).truncationMode(.middle)` in the Inspector's rows
 * (components/MiddleTruncate). With `grow` the text takes the row's free
 * space; otherwise it keeps its natural width (at most `maxWidth`) and gives
 * way when the row is too narrow, as a SwiftUI `Text` in an `HStack` does.
 */
export function MiddleText({ text, grow = false, maxWidth, className, style }: MiddleTextProps) {
  if (grow) return <MiddleTruncate text={text} className={cx(styles.grow, className)} style={style} />
  return <MiddleTruncate text={text} natural maxWidth={maxWidth} className={className} style={style} />
}
