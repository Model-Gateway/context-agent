import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { useReducedMotion } from '../components/BrailleSpinner'
import { Button } from '../components/Button'
import { ResizeHandle } from '../components/ResizeHandle'
import { Sheet } from '../components/Sheet'
import { ToastHost } from '../components/ToastHost'
import type { ApprovalDecision } from '../backend/types'
import { useApp } from '../state/app'
import { installGlobalKeys } from '../state/keys'
import { usePendingDiscountDrop, type PendingApproval } from '../state/selectors'
import { startUpdater } from '../state/updater'
import { RewindSheet } from './Chat/RewindSheet'
import { DetailView } from './Detail/DetailView'
import { InspectorView } from './Inspector/InspectorView'
import { CommandPalette } from './Palette/CommandPalette'
import { SidebarView } from './Sidebar/SidebarView'
import { TopBar } from './TopBar'
import { UpdateSheet } from './Updates/UpdateSheet'
import styles from './MainWindow.module.css'

/** `Theme.Metrics.sidebarWidth`. */
const SIDEBAR_WIDTH = 248
/** `Theme.Metrics.inspectorWidth`, the width a double-click restores. */
const INSPECTOR_WIDTH = 320
const INSPECTOR_MIN_WIDTH = 260
const INSPECTOR_MAX_WIDTH = 720
/** The inspector never squeezes the centre pane below this. */
const CENTRE_MIN_WIDTH = 420
/** `.easeInOut(duration: 0.2)` of the side panes. */
const PANE_ANIMATION_MS = 200

/**
 * `MainWindow` (Views/MainWindow.swift): the window-wide `TopBar` (52 px)
 * over `SidebarView` (248 px) · `DetailView` · `InspectorView` (320 px by
 * default, resizable 260–720 px without leaving the centre under 420 px,
 * double-click restores 320, the width persisted), the side panes sliding
 * in and out over 0.2 s; the toasts (bottom-right), the command palette, the
 * rewind sheet, the "lower discount" alert and the update sheet; folders
 * dropped on the Welcome screen open as projects. Installs the global
 * shortcuts and starts following the updater.
 */
export function MainWindow() {
  const sidebarVisible = useApp((state) => state.sidebarVisible)
  const inspectorVisible = useApp((state) => state.inspectorVisible)
  const storedInspectorWidth = useApp((state) => state.inspectorWidth)
  const onWelcome = useApp((state) => state.sidebarItem === 'welcome')
  const panesRef = useRef<HTMLDivElement>(null)
  const detailRef = useRef<HTMLDivElement>(null)
  const totalWidth = useElementWidth(panesRef)
  const dropTargeted = useFolderDrop(onWelcome, detailRef)

  useEffect(() => installGlobalKeys(), [])
  useEffect(() => startUpdater(), [])

  const maxInspector = maxInspectorWidth(totalWidth, sidebarVisible)
  const inspectorWidth = Math.min(Math.max(storedInspectorWidth, INSPECTOR_MIN_WIDTH), maxInspector)

  return (
    <div className={styles.window}>
      <TopBar />
      <div ref={panesRef} className={styles.panes}>
        <SlidingPane visible={sidebarVisible} width={SIDEBAR_WIDTH} edge="leading">
          <div className={`${styles.sidebar} ${styles.hairlineTrailing}`}>
            <SidebarView />
          </div>
        </SlidingPane>
        <div ref={detailRef} className={styles.detail}>
          <DetailView />
          {dropTargeted && onWelcome ? <DropHighlight message="Drop a folder to open it as a project" /> : null}
        </div>
        <SlidingPane
          visible={inspectorVisible}
          width={inspectorWidth}
          edge="trailing"
          handle={
            <ResizeHandle
              width={inspectorWidth}
              min={INSPECTOR_MIN_WIDTH}
              max={maxInspector}
              defaultWidth={INSPECTOR_WIDTH}
              label="Resize inspector"
              onResize={(width) => useApp.getState().setInspectorWidth(width)}
            />
          }
        >
          <div className={`${styles.inspector} ${styles.hairlineLeading}`}>
            <InspectorView />
          </div>
        </SlidingPane>
      </div>
      <ToastHost />
      <CommandPalette />
      <RewindSheet />
      <DiscountDropAlert />
      <UpdateSheet />
    </div>
  )
}

/// `MainWindow.maxInspectorWidth(total:sidebarVisible:)`: the inspector
/// never squeezes the centre pane below 420.
function maxInspectorWidth(total: number, sidebarVisible: boolean): number {
  const available = total - (sidebarVisible ? SIDEBAR_WIDTH : 0) - CENTRE_MIN_WIDTH
  return Math.max(INSPECTOR_MIN_WIDTH, Math.min(INSPECTOR_MAX_WIDTH, available))
}

/** The element's width, followed with a `ResizeObserver` (the `GeometryReader`). */
function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(() => (typeof window === 'undefined' ? 1180 : window.innerWidth))
  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    const update = () => setWidth(element.clientWidth)
    update()
    const observer = new ResizeObserver(update)
    observer.observe(element)
    return () => observer.disconnect()
  }, [ref])
  return width
}

// MARK: - Side panes

/// A side pane that slides in from its window edge and out again
/// (`.transition(.move(edge:))` with `.easeInOut(duration: 0.2)`; with Reduce
/// Motion it appears at once): the slot animates its width while the
/// content keeps its own, anchored at the window edge. Hidden panes are
/// unmounted once they are out, like the Swift `if`.
function SlidingPane({ visible, width, edge, handle, children }: { visible: boolean; width: number; edge: 'leading' | 'trailing'; handle?: ReactNode; children: ReactNode }) {
  const reduced = useReducedMotion()
  const [present, setPresent] = useState(visible)
  const [open, setOpen] = useState(visible)
  const [animating, setAnimating] = useState(false)

  useEffect(() => {
    if (reduced) {
      setPresent(visible)
      setOpen(visible)
      setAnimating(false)
      return
    }
    let frame = 0
    let timer = 0
    if (visible) {
      setPresent(true)
      setAnimating(true)
      // One frame at width 0, then the transition to full width.
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => setOpen(true))
      })
      timer = window.setTimeout(() => setAnimating(false), PANE_ANIMATION_MS + 50)
    } else {
      setAnimating(true)
      setOpen(false)
      timer = window.setTimeout(() => {
        setPresent(false)
        setAnimating(false)
      }, PANE_ANIMATION_MS + 50)
    }
    return () => {
      cancelAnimationFrame(frame)
      window.clearTimeout(timer)
    }
  }, [visible, reduced])

  if (!present) return null
  return (
    <div
      className={[styles.pane, animating ? styles.paneAnimating : ''].filter(Boolean).join(' ')}
      style={{ width: open ? width : 0 }}
      aria-hidden={!visible || undefined}
      inert={!visible}
    >
      <div className={[styles.paneClip, edge === 'leading' ? styles.anchorTrailing : styles.anchorLeading].join(' ')}>
        <div className={styles.paneContent} style={{ width }}>
          {children}
        </div>
      </div>
      {handle && visible && open ? <div className={styles.handleSlot}>{handle}</div> : null}
    </div>
  )
}

// MARK: - Folder drop (Welcome)

/// Files and folders dropped on the Welcome screen open as projects (files
/// open their parent folder, see `openProject`). Other screens do not take
/// drops to open projects; the composer's drops stay attachments. Returns
/// whether a drag is over the screen (the highlight).
function useFolderDrop(enabled: boolean, targetRef: RefObject<HTMLElement | null>): boolean {
  const [targeted, setTargeted] = useState(false)
  useEffect(() => {
    if (!enabled) {
      setTargeted(false)
      return
    }
    let disposed = false
    let unlisten: (() => void) | null = null
    let paths: string[] = []
    const inside = (position: { x: number; y: number }) => {
      const target = targetRef.current
      if (!target) return false
      const rect = target.getBoundingClientRect()
      const scale = window.devicePixelRatio || 1
      const x = position.x / scale
      const y = position.y / scale
      return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
    }
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const payload = event.payload
          switch (payload.type) {
            case 'enter':
              paths = payload.paths
              setTargeted(paths.length > 0 && inside(payload.position))
              break
            case 'over':
              setTargeted(paths.length > 0 && inside(payload.position))
              break
            case 'drop': {
              setTargeted(false)
              const dropped = payload.paths.length > 0 ? payload.paths : paths
              paths = []
              if (dropped.length === 0 || !inside(payload.position)) break
              for (const path of dropped) void useApp.getState().openProject(path)
              break
            }
            default:
              setTargeted(false)
              paths = []
          }
        })
        .then((stop) => {
          if (disposed) stop()
          else unlisten = stop
        })
        .catch(() => undefined)
    } catch {
      // Outside the Tauri shell (the browser preview): no file drops.
    }
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [enabled, targetRef])
  return targeted
}

/** `DropHighlight`: an accent frame and caption while a folder is dragged over the Welcome screen. */
function DropHighlight({ message }: { message: string }) {
  return (
    <div className={styles.dropHighlight} aria-hidden="true">
      <div className={styles.dropFrame}>
        <span className={styles.dropCaption}>{message}</span>
      </div>
    </div>
  )
}

// MARK: - Lower-discount alert

/// A run's "go on at a much lower discount?" question
/// (`AppState.pendingDiscountDrop`) as an alert over the window: Continue
/// (default), Always Continue (stops asking; Settings → Spending turns it
/// back on) and Cancel. The answered question stays off screen until its
/// answer is sent.
function DiscountDropAlert() {
  const pending = usePendingDiscountDrop()
  const [answered, setAnswered] = useState<string | null>(null)
  const continueRef = useRef<HTMLButtonElement>(null)
  const visible = pending !== null && pending.request.id !== answered

  const answer = (target: PendingApproval, decision: ApprovalDecision) => {
    setAnswered(target.request.id)
    void useApp.getState().respond(target.request.id, target.chat, decision)
  }

  if (!pending) return null
  return (
    <Sheet
      open={visible}
      onClose={() => answer(pending, 'deny')}
      title={pending.request.summary}
      width={440}
      initialFocus={continueRef}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button variant="bordered" onClick={() => answer(pending, 'deny')}>
            Cancel
          </Button>
          <Button variant="bordered" onClick={() => answer(pending, 'approve_for_session')}>
            Always Continue
          </Button>
          <Button ref={continueRef} variant="prominent" onClick={() => answer(pending, 'approve')}>
            Continue
          </Button>
        </>
      }
    >
      <p className={styles.alertMessage}>
        {`${pending.request.details}\n\nContinue uses them for the rest of this task in “${pending.chat.title}”. Always Continue stops asking; turn it back on in Settings → Spending.`}
      </p>
    </Sheet>
  )
}
