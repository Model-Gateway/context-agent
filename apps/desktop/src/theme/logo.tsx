import type { CSSProperties } from 'react'

/** The mark's bounding square in Icons.swift's `LogoShape`. */
const UNIT = 885

/** The tall arm (left half); the right half mirrors it about x = 442.5. */
const ARM: ReadonlyArray<readonly [number, number]> = [
  [420, 0],
  [261, 116],
  [261, 388.5],
  [34.5, 528],
  [8.5, 744.5],
  [330, 546.5],
  [331.5, 459],
  [420, 395],
]

/** The lower wedge (left half). */
const WEDGE: ReadonlyArray<readonly [number, number]> = [
  [420, 640],
  [348, 593],
  [34.5, 790],
  [241.5, 885],
  [420, 771.5],
]

function polygon(points: ReadonlyArray<readonly [number, number]>, mirrored: boolean): string {
  return points.map(([x, y]) => `${mirrored ? UNIT - x : x},${y}`).join(' ')
}

/** The five facets as one path: arm, mirrored arm, wedge, mirrored wedge. */
const LOGO_PATH = [ARM, WEDGE]
  .flatMap((shape) => [polygon(shape, false), polygon(shape, true)])
  .map((points) => `M${points.replace(/ /g, 'L')}Z`)
  .join('')

export interface LogoViewProps {
  /** Side length in px; 22 by default like `Icons.LogoView`. */
  size?: number
  className?: string
  style?: CSSProperties
  /** Accessible name; "Context" by default. Pass an empty string for decorative use. */
  label?: string
}

/**
 * `Icons.LogoView`: the Context mark (two tall arms and two lower wedges
 * around an open hexagonal core) traced from `LogoShape`'s polygons on an
 * 885-unit square, filled with the current text colour.
 */
export function LogoView({ size = 22, className, style, label = 'Context' }: LogoViewProps) {
  return (
    <svg
      viewBox={`0 0 ${UNIT} ${UNIT}`}
      width={size}
      height={size}
      className={className}
      style={style}
      fill="currentColor"
      role={label ? 'img' : undefined}
      aria-label={label || undefined}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <path d={LOGO_PATH} fillRule="nonzero" />
    </svg>
  )
}
