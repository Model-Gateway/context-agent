import { useEffect, useRef, type CSSProperties } from 'react'
import styles from './HalftoneHeader.module.css'

export interface HalftoneHeaderProps {
  /** Band height in px; 120 by default. */
  height?: number
  className?: string
  style?: CSSProperties
}

/**
 * `HalftoneHeader` of the macOS app: the decorative band at the top of the
 * empty chat, clusters of tiny dots that thin out towards the bottom and
 * the sides. A port of the Swift `Canvas`: `textPrimary` dots on a 6 px
 * grid, picked, sized and clustered by the same integer hash, so the band
 * looks the same as in the macOS app and never flickers on re-render. It
 * redraws when its width or the appearance changes. Decorative: hidden from
 * assistive technology and never hit-tested.
 */
export function HalftoneHeader({ height = 120, className, style }: HalftoneHeaderProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    let frame = 0
    const redraw = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => draw(canvas, height))
    }
    redraw()
    const resize = typeof ResizeObserver === 'function' ? new ResizeObserver(redraw) : null
    resize?.observe(canvas)
    // The dots are `textPrimary`: follow the appearance (Settings or the system).
    const theme = new MutationObserver(redraw)
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'class', 'style'] })
    const scheme = window.matchMedia?.('(prefers-color-scheme: dark)')
    scheme?.addEventListener?.('change', redraw)
    return () => {
      cancelAnimationFrame(frame)
      resize?.disconnect()
      theme.disconnect()
      scheme?.removeEventListener?.('change', redraw)
    }
  }, [height])

  return (
    <canvas
      ref={canvasRef}
      className={[styles.band, className ?? ''].filter(Boolean).join(' ')}
      style={{ height, ...style }}
      aria-hidden="true"
    />
  )
}

/** The Swift `Canvas` body: one pass over the 6 px grid. */
function draw(canvas: HTMLCanvasElement, height: number): void {
  const width = canvas.clientWidth
  const ratio = window.devicePixelRatio || 1
  const pixelWidth = Math.max(1, Math.round(width * ratio))
  const pixelHeight = Math.max(1, Math.round(height * ratio))
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight
  const context = canvas.getContext('2d')
  if (!context) return
  context.setTransform(ratio, 0, 0, ratio, 0, 0)
  context.clearRect(0, 0, width, height)
  if (width <= 0 || height <= 0) return

  const rgb = getComputedStyle(canvas).getPropertyValue('--text-primary-rgb').trim() || '26, 26, 28'
  const spacing = 6
  const columns = Math.floor(width / spacing) + 1
  const rows = Math.floor(height / spacing) + 1
  const halfWidth = width / 2

  for (let row = 0; row < rows; row += 1) {
    const y = row * spacing + spacing / 2
    const vertical = Math.max(0, 1 - y / height)
    const verticalFade = vertical * vertical
    if (verticalFade < 0.015) continue

    for (let column = 0; column < columns; column += 1) {
      const x = column * spacing + spacing / 2
      const distance = Math.abs(x - halfWidth) / halfWidth
      const horizontalFade = Math.max(0, 1 - distance * distance * 1.1)
      if (horizontalFade <= 0) continue

      // Low-frequency noise shapes the clusters, high-frequency noise picks dots.
      const cluster = noise(Math.floor(column / 6), Math.floor(row / 3), 7)
      const pick = noise(column, row, 1)
      const density = verticalFade * horizontalFade * (0.35 + 0.65 * cluster)
      if (!(pick < density * 0.9)) continue

      const radius = 0.7 + 1.3 * noise(column, row, 3)
      const alpha = 0.05 + 0.17 * density
      context.fillStyle = `rgba(${rgb}, ${alpha.toFixed(3)})`
      context.beginPath()
      context.arc(x, y, radius, 0, Math.PI * 2)
      context.fill()
    }
  }
}

/** `HalftoneHeader.noise(_:_:seed:)`: a stable pseudo-random value in 0..<1 for a grid cell (UInt32 wrapping arithmetic). */
function noise(x: number, y: number, seed: number): number {
  let h = Math.imul(x, 374_761_393) >>> 0
  h = (h + (Math.imul(y, 668_265_263) >>> 0)) >>> 0
  h = (h + (Math.imul(seed, 2_246_822_519) >>> 0)) >>> 0
  h = Math.imul((h ^ (h >>> 13)) >>> 0, 1_274_126_177) >>> 0
  h = (h ^ (h >>> 16)) >>> 0
  return (h & 0xffff) / 65_535
}
