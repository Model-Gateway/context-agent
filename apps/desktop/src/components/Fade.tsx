import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { prefersReducedMotion } from './overlays'
import styles from './Fade.module.css'

/** How long `Fade` takes (`.easeInOut(duration: 0.18)`). */
const FADE_MS = 180
const EASE_IN_OUT = 'ease-in-out'
const EASE_OUT = 'cubic-bezier(0.2, 0, 0, 1)'

export interface FadeProps {
  show: boolean
  children: ReactNode
  /** `opacity` (`.transition(.opacity)`) or `moveBottom` (`.opacity.combined(with: .move(edge: .bottom))`). */
  variant?: 'opacity' | 'moveBottom'
  className?: string
}

/**
 * SwiftUI's `if … { view.transition(.opacity) }` inside `withAnimation`:
 * children that appear after the first render fade in while the space they
 * take opens up (the views below move down instead of jumping), and stay
 * for the fade-out while it closes again after `show` turns false. Present
 * from the start, they appear at once (rows the tail window loads do not
 * flash). No animation with Reduce Motion.
 */
export function Fade({ show, children, variant = 'opacity', className }: FadeProps) {
  const [present, setPresent] = useState(show)
  const [entering, setEntering] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const animation = useRef<Animation | null>(null)
  // Shown (again): present at once, opening up (a render-time adjustment, no extra frame).
  if (show && !present) {
    setPresent(true)
    setEntering(true)
  }

  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    const reduce = prefersReducedMotion() || typeof element.animate !== 'function'
    const run = (opening: boolean, done: () => void) => {
      const from = element.getBoundingClientRect().height
      animation.current?.cancel()
      if (reduce) {
        done()
        return
      }
      const height = opening ? element.getBoundingClientRect().height : from
      const move = variant === 'moveBottom'
      const closed: Keyframe = { height: '0px', opacity: 0, transform: move ? `translateY(${height}px)` : 'none' }
      const open: Keyframe = { height: `${height}px`, opacity: 1, transform: 'none' }
      element.style.overflow = 'hidden'
      const running = element.animate(opening ? [closed, open] : [open, closed], {
        duration: FADE_MS,
        easing: move ? EASE_OUT : EASE_IN_OUT,
        fill: opening ? 'none' : 'forwards',
      })
      animation.current = running
      const finish = () => {
        if (animation.current !== running) return
        animation.current = null
        element.style.overflow = ''
        done()
      }
      running.onfinish = finish
      running.oncancel = () => {
        if (animation.current === running) animation.current = null
        element.style.overflow = ''
      }
    }
    if (show && entering) {
      run(true, () => setEntering(false))
    } else if (!show && present) {
      run(false, () => setPresent(false))
    } else if (show && animation.current) {
      // Shown again while closing: open from where it is.
      animation.current.cancel()
      run(true, () => setEntering(false))
    }
  }, [show, present, entering, variant])

  useEffect(() => () => animation.current?.cancel(), [])

  if (!show && !present) return null
  return (
    <div ref={ref} className={[styles.fade, className ?? ''].filter(Boolean).join(' ')}>
      {children}
    </div>
  )
}
