import { useSyncExternalStore, type CSSProperties } from 'react'
import styles from './BrailleSpinner.module.css'

/** The ten frames of the spinner. */
export const BRAILLE_FRAMES: readonly string[] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']
const FRAME_INTERVAL = 80
/** The static glyph with Reduce Motion (no ticker subscription at all). */
const STATIC_FRAME = '⠿'

// One shared ticker for every spinner on screen; the frame is derived from
// the absolute time so re-created spinners never restart the cycle.
// The current frame is cached between ticks (a snapshot must not change
// between two reads in one render).
const listeners = new Set<() => void>()
let ticker: number | null = null
let currentFrame = STATIC_FRAME

function tick() {
  const next = brailleFrame()
  if (next === currentFrame) return
  currentFrame = next
  listeners.forEach((fn) => fn())
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  if (ticker === null) {
    // Catch up with the clock; React re-reads the snapshot after subscribing.
    currentFrame = brailleFrame()
    ticker = window.setInterval(tick, FRAME_INTERVAL / 2)
  }
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0 && ticker !== null) {
      window.clearInterval(ticker)
      ticker = null
    }
  }
}

/** The frame for an absolute time (ms). */
export function brailleFrame(at: number = Date.now()): string {
  const ticks = Math.floor(at / FRAME_INTERVAL)
  const index = ((ticks % BRAILLE_FRAMES.length) + BRAILLE_FRAMES.length) % BRAILLE_FRAMES.length
  return BRAILLE_FRAMES[index] ?? STATIC_FRAME
}

function getSnapshot(): string {
  return currentFrame
}

function subscribeNever(): () => void {
  return () => {}
}
function staticSnapshot(): string {
  return STATIC_FRAME
}

const reducedMotionQuery = typeof window !== 'undefined' && typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-reduced-motion: reduce)') : null

function subscribeReducedMotion(listener: () => void): () => void {
  reducedMotionQuery?.addEventListener('change', listener)
  return () => reducedMotionQuery?.removeEventListener('change', listener)
}

function reducedMotionSnapshot(): boolean {
  return reducedMotionQuery?.matches ?? false
}

/** Subscribes to the Reduce Motion setting. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribeReducedMotion, reducedMotionSnapshot, () => false)
}

export interface BrailleSpinnerProps {
  /** Glyph size in px; 12 by default. */
  size?: number
  /** Any CSS colour; `var(--text-secondary)` by default, `"inherit"` takes the surrounding colour. */
  tint?: string
  className?: string
  style?: CSSProperties
}

/**
 * `BrailleSpinner` of the macOS app, the inline loading indicator:
 * ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ cycling every 80 ms, a static ⠿ with Reduce Motion, labelled
 * "Working" either way. Never a native progress control.
 */
export function BrailleSpinner({ size = 12, tint = 'var(--text-secondary)', className, style }: BrailleSpinnerProps) {
  const reduced = useReducedMotion()
  const frame = useSyncExternalStore(reduced ? subscribeNever : subscribe, reduced ? staticSnapshot : getSnapshot, staticSnapshot)
  return (
    <span
      role="img"
      aria-label="Working"
      aria-live="off"
      className={[styles.spinner, className ?? ''].filter(Boolean).join(' ')}
      style={{ fontSize: size, minWidth: size, minHeight: size, color: tint, ...style }}
    >
      {frame}
    </span>
  )
}
