import {
  Fragment,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  type Ref,
  type RefObject,
  type WheelEvent,
} from 'react'
import { prefersReducedMotion } from './overlays'
import { Skeleton } from './Skeleton'
import { TailWindowModel } from './tailWindowModel'
import styles from './TailWindow.module.css'

/** What `ref` exposes. */
export interface TailWindowHandle {
  /**
   * `scrollToBottom(animated:settle:)` of the macOS ChatView ("Jump to
   * latest", a new run): follows the bottom again, loads the newest page
   * when the window stops short of it, scrolls to the bottom (smoothly
   * when `animated` and the newest rows are already loaded) and scrolls
   * there once more after 80 ms for rows that size in late.
   */
  jumpToLatest: (options?: { animated?: boolean }) => void
  /**
   * A find match: stops following, loads two pages around `index` unless
   * it is loaded already (`TailWindow.reveal`), then scrolls the row into
   * view (centred by default; smoothly when it was already loaded).
   */
  loadAround: (index: number, options?: { block?: 'start' | 'center' | 'end'; animated?: boolean }) => void
  /** Stops following the bottom (`FollowState.stop`). */
  stopFollowing: () => void
  /** Scrolls to the bottom when following (the live turn grew: `followIfNeeded`). */
  followIfNeeded: () => void
  /** The scrolling element. */
  readonly element: HTMLDivElement | null
  /** `FollowState.atBottom`: the window reaches the newest item and the view is within 48 px of the bottom. */
  readonly atBottom: boolean
  /** Whether new content keeps the newest row in view. */
  readonly isFollowing: boolean
  /** Whether the window ends at the newest item. */
  readonly followsTail: boolean
}

export interface TailWindowProps<T> {
  /** The whole list; only the window's slice is rendered. */
  items: readonly T[]
  itemKey: (item: T, index: number) => string | number
  renderItem: (item: T, index: number) => ReactNode
  /** Items per page; 20 by default (the transcript). */
  page?: number
  /** Loaded items at most; 60 by default. */
  limit?: number
  /** Loaded rows (px) kept above / below the visible ones; 600 by default. */
  threshold?: number
  /**
   * Overrides whether the view keeps the newest content in view; `null`
   * (default) uses the built-in `FollowState`: following stops as soon as
   * the user scrolls up (wheel, keys, scrollbar drag, touch) and resumes
   * only at the bottom of a window that reaches the newest item, reached
   * by scrolling down or by a gesture that ends there.
   */
  followsBottom?: boolean | null
  /** Space between rows (and between header, rows and footer), px. */
  rowGap?: number
  /** Skeleton rows shown in the space of unloaded items next to the loaded ones. */
  renderPlaceholder?: (index: number) => ReactNode
  /** One placeholder row's height, to know how many fill the space; 160 by default. */
  placeholderRowHeight?: number
  /** How much of a spacer shows placeholders; 960 px by default. */
  placeholderHeight?: number
  /** Content above the rows (inside the scroller). */
  header?: ReactNode
  /** Content below the rows (the live turn, action row). */
  footer?: ReactNode
  className?: string
  contentClassName?: string
  rowsClassName?: string
  style?: CSSProperties
  role?: string
  'aria-label'?: string
  /** `FollowState.atBottom` changed: shows / hides "Jump to latest". */
  onAtBottomChange?: (atBottom: boolean) => void
  /** Following the bottom started or stopped. */
  onFollowingChange?: (following: boolean) => void
  /** The window started or stopped ending at the newest item. */
  onFollowsTailChange?: (followsTail: boolean) => void
  ref?: Ref<TailWindowHandle>
}

type Pending =
  | { kind: 'bottom'; animated: boolean }
  | { kind: 'index'; index: number; block: 'start' | 'center' | 'end'; animated: boolean }

interface ScrollState {
  /** A window change is waiting for the content to settle (`TailWindowScrolling.busy`). */
  busy: boolean
  /** Distance from the bottom of the content while held (`ScrollKeeper`). */
  held: number | null
  /** The scrollTop the keeper last set, so its own scroll event is not taken for the user's. */
  keeperTop: number | null
  heightChangedAt: number
  lastHeight: number
  lastViewport: number
  rows: { height: number; count: number }
  renderedCount: number
  syncedCount: number
  pending: Pending | null
  stepQueued: boolean
  /** No window changes until then (a smooth programmatic scroll is under way). */
  quietUntil: number
  quietTimer: number | null
  settleTimer: number | null
  // FollowState
  followLatest: boolean
  atBottom: boolean
  /** The user's last scroll went towards earlier content. */
  leftBottom: boolean
  /** A scrollbar drag or touch gesture is in progress. */
  liveScroll: boolean
  reportedFollowsTail: boolean
}

/** `FollowState`: within 48 px of the bottom counts as at the bottom. */
const NEAR_BOTTOM = 48
/** A smooth scroll is given this long before the window may change again. */
const SMOOTH_SCROLL_QUIET = 450

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable || target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT'
}

/**
 * `TailWindow` + `TailWindowScrolling` + `ScrollKeeper` + `TailWindowSpacer`
 * of the macOS app, and the ChatView's `FollowState`, on a plain scrolling
 * `div`: a windowed list that loads the newest page first, a page more as
 * the user scrolls near the top, never more than `limit` (older pages
 * unload the newest, and scrolling back down loads them again). Unloaded
 * items stand as empty space at the measured average row height so the
 * scroller is the size of the whole list; a scroller dragged into that
 * space loads the items that belong there. While rows load or unload above
 * the visible ones the distance from the bottom of the content is held, so
 * the visible rows stay put; while following, new content keeps the newest
 * row in view, and following stops as soon as the user scrolls up.
 */
export function TailWindow<T>({
  items,
  itemKey,
  renderItem,
  page = 20,
  limit = 60,
  threshold = 600,
  followsBottom = null,
  rowGap = 0,
  renderPlaceholder,
  placeholderRowHeight = 160,
  placeholderHeight = 960,
  header,
  footer,
  className,
  contentClassName,
  rowsClassName,
  style,
  role,
  onAtBottomChange,
  onFollowingChange,
  onFollowsTailChange,
  ref,
  ...aria
}: TailWindowProps<T>) {
  const count = items.length
  const scrollerRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const rowsRef = useRef<HTMLDivElement>(null)
  const aboveRef = useRef<HTMLDivElement>(null)
  const modelRef = useRef<TailWindowModel | null>(null)
  if (modelRef.current === null) modelRef.current = new TailWindowModel(count, page, limit)
  const model = modelRef.current
  model.page = page
  model.limit = limit

  const [, setTick] = useState(0)
  const rerender = useCallback(() => setTick((tick) => tick + 1), [])

  const stateRef = useRef<ScrollState>({
    busy: false,
    held: null,
    keeperTop: null,
    heightChangedAt: 0,
    lastHeight: 0,
    lastViewport: 0,
    rows: { height: 0, count: 0 },
    renderedCount: 0,
    syncedCount: count,
    pending: null,
    stepQueued: false,
    quietUntil: 0,
    quietTimer: null,
    settleTimer: null,
    followLatest: true,
    atBottom: true,
    leftBottom: false,
    liveScroll: false,
    reportedFollowsTail: true,
  })
  const latest = useRef({ count, followsBottom, threshold, rowGap, onAtBottomChange, onFollowingChange, onFollowsTailChange })
  useLayoutEffect(() => {
    latest.current = { count, followsBottom, threshold, rowGap, onAtBottomChange, onFollowingChange, onFollowsTailChange }
  })

  // The list changed: removals and bulk loads start over at the tail,
  // appended items extend a following window (idempotent, so safe in render).
  if (stateRef.current.syncedCount !== count) {
    model.sync(count)
    stateRef.current.syncedCount = count
  }

  // MARK: Follow state

  const following = useCallback(() => latest.current.followsBottom ?? stateRef.current.followLatest, [])

  const setFollowLatest = useCallback((value: boolean) => {
    const state = stateRef.current
    if (state.followLatest === value) return
    state.followLatest = value
    latest.current.onFollowingChange?.(value)
  }, [])

  const isNearBottom = useCallback(() => {
    const element = scrollerRef.current
    if (!element) return true
    return model.followsTail && element.scrollHeight - (element.scrollTop + element.clientHeight) < NEAR_BOTTOM
  }, [model])

  /**
   * `FollowState.evaluate`: reaching the bottom re-enables following,
   * unless the user is scrolling or last scrolled up (a gesture that starts
   * at the bottom spends its first frames within reach of it).
   */
  const evaluate = useCallback(() => {
    const state = stateRef.current
    const near = isNearBottom()
    if (near !== state.atBottom) {
      state.atBottom = near
      latest.current.onAtBottomChange?.(near)
    }
    if (near && !state.followLatest && !state.leftBottom && !state.liveScroll) setFollowLatest(true)
  }, [isNearBottom, setFollowLatest])

  const stopFollowing = useCallback(() => {
    stateRef.current.leftBottom = true
    setFollowLatest(false)
  }, [setFollowLatest])

  const resumeFollowing = useCallback(() => {
    stateRef.current.leftBottom = false
    setFollowLatest(true)
  }, [setFollowLatest])

  // MARK: Scroll keeper

  const geometry = useCallback(() => {
    const element = scrollerRef.current
    if (!element) return { top: 0, bottom: 0, height: 0 }
    const top = Math.round(element.scrollTop)
    return { top, bottom: Math.round(element.scrollTop + element.clientHeight), height: Math.round(element.scrollHeight) }
  }, [])

  const keepBottom = useCallback(() => {
    const element = scrollerRef.current
    if (element) stateRef.current.held = element.scrollHeight - element.scrollTop
  }, [])

  const release = useCallback(() => {
    stateRef.current.held = null
    stateRef.current.keeperTop = null
  }, [])

  /** `ScrollKeeper.documentResized`: move with the content so the held distance stays. */
  const applyHeld = useCallback(() => {
    const element = scrollerRef.current
    const state = stateRef.current
    if (!element || state.held === null) return
    const target = Math.max(0, element.scrollHeight - state.held)
    if (Math.abs(element.scrollTop - target) > 0.5) {
      element.scrollTop = target
      state.keeperTop = element.scrollTop
    }
  }, [])

  const scrollToBottom = useCallback((smooth = false) => {
    const element = scrollerRef.current
    if (!element) return
    const target = element.scrollHeight - element.clientHeight
    if (smooth) {
      stateRef.current.quietUntil = performance.now() + SMOOTH_SCROLL_QUIET
      element.scrollTo({ top: target, behavior: 'smooth' })
    } else if (Math.abs(element.scrollTop - target) > 0.5) {
      element.scrollTop = target
    }
  }, [])

  /** Content height changed (a commit or a late layout pass): hold, or follow the bottom. */
  const contentChanged = useCallback(() => {
    const element = scrollerRef.current
    const state = stateRef.current
    if (!element) return
    const height = element.scrollHeight
    if (height === state.lastHeight) return
    state.lastHeight = height
    state.heightChangedAt = performance.now()
    if (state.held !== null) applyHeld()
    else if (following() && performance.now() >= state.quietUntil) scrollToBottom()
    evaluate()
  }, [applyHeld, following, scrollToBottom, evaluate])

  // MARK: Window steps

  const stepRef = useRef<() => void>(() => {})

  const queueStep = useCallback(() => {
    const state = stateRef.current
    if (state.stepQueued) return
    state.stepQueued = true
    requestAnimationFrame(() => {
      state.stepQueued = false
      stepRef.current()
    })
  }, [])

  /**
   * Changes the window (holding the distance from the bottom when the
   * change is above the visible rows), waits until the content's height
   * has held for 100 ms (at most 600 ms), then runs `next` or looks for
   * more work.
   */
  const run = useCallback(
    (keepingBottom: boolean, change: () => void, next?: () => void) => {
      const state = stateRef.current
      state.busy = true
      if (keepingBottom) keepBottom()
      change()
      state.heightChangedAt = performance.now()
      rerender()
      const started = performance.now()
      const settle = () => {
        const now = performance.now()
        if (now - state.heightChangedAt < 100 && now - started < 600) {
          state.settleTimer = window.setTimeout(settle, 16)
          return
        }
        state.settleTimer = null
        release()
        state.busy = false
        if (next) {
          next()
          if (state.busy) return
        }
        stepRef.current()
      }
      state.settleTimer = window.setTimeout(settle, 16)
    },
    [keepBottom, release, rerender],
  )

  /** `TailWindowScrolling.step`. */
  const step = useCallback(() => {
    const state = stateRef.current
    if (state.busy) return
    const now = performance.now()
    if (now < state.quietUntil) {
      if (state.quietTimer === null) {
        state.quietTimer = window.setTimeout(() => {
          state.quietTimer = null
          queueStep()
        }, state.quietUntil - now)
      }
      return
    }
    const scroller = scrollerRef.current
    const rows = rowsRef.current
    if (!scroller || !rows) return
    const { count: total, threshold: limitPx } = latest.current
    const range = model.range(total)
    const loaded = range.upper - range.lower

    // The average row height changes the empty space above and below:
    // only while the newest item is loaded (no space below), holding the
    // bottom. A measurement taken before the window last changed is stale.
    if (model.followsTail && state.rows.count === loaded && model.measureWouldChange(state.rows.height, state.rows.count)) {
      const { height, count: measured } = state.rows
      run(true, () => model.measure(height, measured))
      return
    }

    const g = geometry()
    const rowsTop = rows.offsetTop
    const rowsBottom = rowsTop + rows.offsetHeight
    const spacerStart = aboveRef.current ? aboveRef.current.offsetTop : rowsTop
    const spacerAbove = model.spacerAbove
    const spacerBelow = model.spacerBelow(total)
    if (model.rowHeight > 0 && ((spacerAbove > 0 && g.bottom < rowsTop) || (spacerBelow > 0 && g.top > rowsBottom))) {
      // The visible area is entirely in the empty space of unloaded items
      // (the scroller dragged there): load the items that belong there.
      const middle = (g.top + g.bottom) / 2
      const index = Math.min(Math.max(0, Math.floor((middle - spacerStart) / model.rowHeight)), Math.max(0, total - 1))
      run(false, () => model.place(index, total))
      return
    }
    const above = g.top - rowsTop
    const below = rowsBottom - g.bottom
    if (above < limitPx && model.hasOlder) {
      run(
        true,
        () => model.loadOlder(),
        () => {
          if (model.isOverLimit) run(false, () => model.trimNewest())
        },
      )
    } else if (below < limitPx && model.hasNewer(total)) {
      run(
        false,
        () => model.loadNewer(total),
        () => {
          if (model.isOverLimit) run(true, () => model.trimOldest(model.span - model.limit))
        },
      )
    } else if (model.followsTail && following() && model.span > model.page && model.rowHeight > 0) {
      // Following the newest item: rows more than 1.5 × threshold above the
      // visible ones unload down to one page (the gap between the load and
      // unload thresholds keeps them from undoing each other).
      const spare = Math.floor((above - limitPx * 1.5) / model.rowHeight)
      const unload = Math.min(model.span - model.page, spare)
      if (unload > 0) run(true, () => model.trimOldest(unload))
    }
  }, [model, run, geometry, following, queueStep])
  useLayoutEffect(() => {
    stepRef.current = step
  }, [step])

  // MARK: Observers

  // The content (height changes), the rows (measurement) and the viewport.
  useEffect(() => {
    const scroller = scrollerRef.current
    const content = contentRef.current
    const rows = rowsRef.current
    if (!scroller || !content || !rows || typeof ResizeObserver === 'undefined') return
    const contentObserver = new ResizeObserver(() => {
      contentChanged()
      queueStep()
    })
    const rowsObserver = new ResizeObserver(() => {
      const state = stateRef.current
      const height = rows.getBoundingClientRect().height
      state.rows = { height: height > 0 ? height + latest.current.rowGap : 0, count: state.renderedCount }
      queueStep()
    })
    const viewportObserver = new ResizeObserver(() => {
      const state = stateRef.current
      const viewport = scroller.clientHeight
      if (viewport === state.lastViewport) return
      state.lastViewport = viewport
      // The pane changed height (the composer grew, the window resized):
      // keep the newest row in view while following.
      if (state.held === null && following()) scrollToBottom()
      evaluate()
      queueStep()
    })
    contentObserver.observe(content)
    rowsObserver.observe(rows)
    viewportObserver.observe(scroller)
    return () => {
      contentObserver.disconnect()
      rowsObserver.disconnect()
      viewportObserver.disconnect()
    }
  }, [contentChanged, queueStep, following, scrollToBottom, evaluate])

  // Pending settle timers die with the view.
  useEffect(
    () => () => {
      const state = stateRef.current
      if (state.settleTimer !== null) window.clearTimeout(state.settleTimer)
      if (state.quietTimer !== null) window.clearTimeout(state.quietTimer)
      state.settleTimer = null
      state.quietTimer = null
      state.busy = false
    },
    [],
  )

  // First layout: open at the bottom (`defaultScrollAnchor(.bottom, for: .initialOffset)`).
  useLayoutEffect(() => {
    const element = scrollerRef.current
    if (!element) return
    stateRef.current.lastHeight = element.scrollHeight
    stateRef.current.lastViewport = element.clientHeight
    scrollToBottom()
  }, [scrollToBottom])

  // After every commit: keep the held distance, perform a requested scroll,
  // follow the bottom, take a fresh measurement after a window change, and
  // report a window change.
  useLayoutEffect(() => {
    const state = stateRef.current
    const scroller = scrollerRef.current
    if (!scroller) return
    if (state.held !== null) {
      state.lastHeight = scroller.scrollHeight
      applyHeld()
    }
    const pending = state.pending
    if (pending) {
      state.pending = null
      state.lastHeight = scroller.scrollHeight
      const smooth = pending.animated && !prefersReducedMotion()
      if (pending.kind === 'bottom') {
        scrollToBottom(smooth)
        // Rows can size in after the first pass: settle once layout has caught up.
        window.setTimeout(() => {
          if (following()) scrollToBottom()
          evaluate()
        }, 80)
      } else {
        const row = rowsRef.current?.querySelector<HTMLElement>(`:scope > [data-tw-index="${pending.index}"]`)
        if (row) {
          const rowTop = row.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
          let target = rowTop
          if (pending.block === 'center') target = rowTop - (scroller.clientHeight - row.offsetHeight) / 2
          if (pending.block === 'end') target = rowTop - scroller.clientHeight + row.offsetHeight
          target = Math.max(0, Math.min(target, scroller.scrollHeight - scroller.clientHeight))
          if (smooth) {
            state.quietUntil = performance.now() + SMOOTH_SCROLL_QUIET
            scroller.scrollTo({ top: target, behavior: 'smooth' })
          } else {
            scroller.scrollTop = target
          }
        }
      }
    } else {
      contentChanged()
    }
    if (state.rows.count !== state.renderedCount && rowsRef.current) {
      const height = rowsRef.current.offsetHeight
      state.rows = { height: height > 0 ? height + latest.current.rowGap : 0, count: state.renderedCount }
    }
    if (model.followsTail !== state.reportedFollowsTail) {
      state.reportedFollowsTail = model.followsTail
      latest.current.onFollowsTailChange?.(model.followsTail)
    }
    evaluate()
  })

  // The list changed: look for work.
  useLayoutEffect(() => {
    queueStep()
  }, [count, queueStep])

  // MARK: User intent (FollowState's scroll-wheel monitor and live scrolls)

  const onScroll = useCallback(() => {
    const element = scrollerRef.current
    const state = stateRef.current
    if (!element) return
    // The user scrolled while a distance is held: hold the new one.
    if (state.held !== null && (state.keeperTop === null || Math.abs(element.scrollTop - state.keeperTop) > 1)) {
      state.held = element.scrollHeight - element.scrollTop
    }
    evaluate()
    queueStep()
  }, [evaluate, queueStep])

  const onWheel = useCallback(
    (event: WheelEvent<HTMLDivElement>) => {
      if (event.deltaY === 0 || event.ctrlKey) return
      // Negative deltaY reveals earlier content.
      if (event.deltaY < 0) stopFollowing()
      else stateRef.current.leftBottom = false
    },
    [stopFollowing],
  )

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (isEditable(event.target) || event.metaKey || event.ctrlKey || event.altKey) return
      const up = event.key === 'ArrowUp' || event.key === 'PageUp' || event.key === 'Home' || (event.key === ' ' && event.shiftKey)
      const down = event.key === 'ArrowDown' || event.key === 'PageDown' || event.key === 'End' || (event.key === ' ' && !event.shiftKey)
      if (up) stopFollowing()
      else if (down) stateRef.current.leftBottom = false
    },
    [stopFollowing],
  )

  /** `FollowState.liveScrollStarted`: a scrollbar drag or a touch gesture. */
  const liveScrollStarted = useCallback(() => {
    const state = stateRef.current
    if (state.liveScroll) return
    state.liveScroll = true
    stopFollowing()
    const end = () => {
      window.removeEventListener('pointerup', end, true)
      window.removeEventListener('pointercancel', end, true)
      window.removeEventListener('touchend', end, true)
      window.removeEventListener('touchcancel', end, true)
      // The gesture is over: ending at the bottom follows again.
      state.liveScroll = false
      state.leftBottom = !isNearBottom()
      evaluate()
    }
    window.addEventListener('pointerup', end, true)
    window.addEventListener('pointercancel', end, true)
    window.addEventListener('touchend', end, true)
    window.addEventListener('touchcancel', end, true)
  }, [stopFollowing, isNearBottom, evaluate])

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const element = scrollerRef.current
      if (!element || event.target !== element) return
      // A press on the scrollbar itself (outside the client area).
      const rect = element.getBoundingClientRect()
      if (event.clientX - rect.left >= element.clientWidth + element.clientLeft) liveScrollStarted()
    },
    [liveScrollStarted],
  )

  // MARK: Handle

  useImperativeHandle(
    ref,
    () => ({
      jumpToLatest: (options?: { animated?: boolean }) => {
        const state = stateRef.current
        resumeFollowing()
        // Scrolled up past the newest items: load the newest page again.
        const wasAtTail = model.followsTail
        if (!wasAtTail) model.reset(latest.current.count)
        release()
        state.pending = { kind: 'bottom', animated: (options?.animated ?? true) && wasAtTail }
        rerender()
      },
      loadAround: (index: number, options?: { block?: 'start' | 'center' | 'end'; animated?: boolean }) => {
        const state = stateRef.current
        const total = latest.current.count
        if (total === 0) return
        const clamped = Math.min(Math.max(0, index), total - 1)
        stopFollowing()
        const loaded = model.contains(clamped, total)
        if (!loaded) model.reveal(clamped, total)
        release()
        state.pending = { kind: 'index', index: clamped, block: options?.block ?? 'center', animated: (options?.animated ?? true) && loaded }
        rerender()
      },
      stopFollowing,
      followIfNeeded: () => {
        if (following()) scrollToBottom()
      },
      get element() {
        return scrollerRef.current
      },
      get atBottom() {
        return stateRef.current.atBottom
      },
      get isFollowing() {
        return following()
      },
      get followsTail() {
        return model.followsTail
      },
    }),
    [model, release, rerender, resumeFollowing, stopFollowing, following, scrollToBottom],
  )

  // MARK: Rendering

  const range = model.range(count)
  stateRef.current.renderedCount = range.upper - range.lower
  const spacerAbove = model.spacerAbove
  const spacerBelow = model.spacerBelow(count)
  const gapStyle = { ['--tw-gap' as string]: `${rowGap}px` } as CSSProperties

  const rows: ReactNode[] = []
  for (let index = range.lower; index < range.upper; index += 1) {
    const item = items[index] as T
    rows.push(
      <div key={itemKey(item, index)} data-tw-index={index} className={styles.row}>
        {renderItem(item, index)}
      </div>,
    )
  }

  return (
    <div
      ref={scrollerRef}
      role={role}
      aria-label={aria['aria-label']}
      className={[styles.scroller, className ?? ''].filter(Boolean).join(' ')}
      style={style}
      onScroll={onScroll}
      onWheel={onWheel}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onTouchStart={liveScrollStarted}
    >
      <div ref={contentRef} className={[styles.content, contentClassName ?? ''].filter(Boolean).join(' ')} style={gapStyle}>
        {header}
        {spacerAbove > 0 ? (
          <Spacer
            elementRef={aboveRef}
            height={spacerAbove}
            rowsEdge="bottom"
            scrollerRef={scrollerRef}
            renderPlaceholder={renderPlaceholder}
            placeholderRowHeight={placeholderRowHeight}
            placeholderHeight={placeholderHeight}
          />
        ) : null}
        <div ref={rowsRef} className={[styles.rows, rowsClassName ?? ''].filter(Boolean).join(' ')}>
          {rows}
        </div>
        {spacerBelow > 0 ? (
          <Spacer
            height={spacerBelow}
            rowsEdge="top"
            scrollerRef={scrollerRef}
            renderPlaceholder={renderPlaceholder}
            placeholderRowHeight={placeholderRowHeight}
            placeholderHeight={placeholderHeight}
          />
        ) : null}
        {footer}
      </div>
    </div>
  )
}

interface SpacerProps {
  height: number
  /** The edge that meets the loaded rows. */
  rowsEdge: 'top' | 'bottom'
  scrollerRef: RefObject<HTMLDivElement | null>
  renderPlaceholder?: (index: number) => ReactNode
  placeholderRowHeight: number
  placeholderHeight: number
  elementRef?: RefObject<HTMLDivElement | null>
}

/**
 * `TailWindowSpacer`: the space standing for unloaded items above or
 * below the window; next to the loaded rows up to `placeholderHeight` of
 * it shows placeholder rows (a skeleton of what loads there as the user
 * scrolls on), the rest stays empty. The skeleton pulses only on screen.
 */
function Spacer({ height, rowsEdge, scrollerRef, renderPlaceholder, placeholderRowHeight, placeholderHeight, elementRef }: SpacerProps) {
  const ownRef = useRef<HTMLDivElement>(null)
  const [onScreen, setOnScreen] = useState(false)
  const hasPlaceholders = !!renderPlaceholder

  useEffect(() => {
    const element = ownRef.current
    if (!element || !hasPlaceholders || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[entries.length - 1]
        if (entry) setOnScreen(entry.isIntersecting)
      },
      { root: scrollerRef.current, threshold: 0.01 },
    )
    observer.observe(element)
    return () => observer.disconnect()
  }, [hasPlaceholders, scrollerRef])

  const setRefs = useCallback(
    (element: HTMLDivElement | null) => {
      ownRef.current = element
      if (elementRef) elementRef.current = element
    },
    [elementRef],
  )

  const shown = Math.min(height, placeholderHeight)
  const rowCount = Math.ceil(shown / Math.max(placeholderRowHeight, 1)) + 1

  return (
    <div ref={setRefs} className={styles.spacer} style={{ height }} aria-hidden="true">
      {renderPlaceholder ? (
        <div className={rowsEdge === 'bottom' ? styles.placeholderAtBottom : styles.placeholderAtTop} style={{ height: shown }}>
          <Skeleton active={onScreen} className={styles.placeholderPulse}>
            <div className={rowsEdge === 'bottom' ? styles.placeholderStackBottom : styles.placeholderStackTop}>
              {Array.from({ length: rowCount }, (_, index) => (
                <Fragment key={index}>{renderPlaceholder(rowsEdge === 'bottom' ? rowCount - 1 - index : index)}</Fragment>
              ))}
            </div>
          </Skeleton>
        </div>
      ) : null}
    </div>
  )
}
