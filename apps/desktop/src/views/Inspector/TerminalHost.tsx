// The xterm.js half of the Inspector's Terminal tab (TerminalTab.tsx), in
// its own module so xterm loads only when a shell is first shown, not at
// startup (TerminalTab imports it with React.lazy).

import '@xterm/xterm/css/xterm.css'
import { useLayoutEffect, useRef } from 'react'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal, type ITheme } from '@xterm/xterm'
import { isMac } from '../../components/platform'
import type { TerminalSession, TerminalWorkspace } from '../../state/terminal'
import styles from './TerminalTab.module.css'

// MARK: - Terminal view

/** The xterm.js side of one session, kept on `session.view` for the session's lifetime. */
interface XtermView {
  readonly kind: 'xterm'
  readonly term: Terminal
  readonly fit: FitAddon
  /** The element xterm renders into; moved between hosts, never re-opened. */
  readonly host: HTMLDivElement
  opened: boolean
  fitSoon: () => void
  dispose: () => void
}

/** Every live xterm view, for theme changes. */
const liveViews = new Set<XtermView>()
let themeWatchInstalled = false

function isXtermView(value: unknown): value is XtermView {
  return typeof value === 'object' && value !== null && (value as { kind?: unknown }).kind === 'xterm'
}

/// Cascadia Mono / Consolas on Windows (and Linux), SF Mono / Menlo on macOS
/// (`monospacedSystemFont`). SF Mono is reachable only as `ui-monospace` in
/// WebKit and Chromium; the tail keeps Linux (and a Windows without Cascadia)
/// off the generic `monospace`, which is Courier in some engines.
const TERMINAL_FONT = isMac
  ? 'ui-monospace, "SF Mono", Menlo, monospace'
  : '"Cascadia Mono", Consolas, ui-monospace, "DejaVu Sans Mono", "Liberation Mono", Menlo, monospace'

/// SwiftTerm's default ANSI palette (`Color.terminalAppColors`, the
/// Terminal.app colours), which TerminalSession.swift keeps.
const TERMINAL_APP_COLORS = {
  black: '#000000',
  red: '#c23621',
  green: '#25bc24',
  yellow: '#adad27',
  blue: '#492ee1',
  magenta: '#d338d3',
  cyan: '#33bbc8',
  white: '#cbcccd',
  brightBlack: '#818383',
  brightRed: '#fc391f',
  brightGreen: '#31e722',
  brightYellow: '#eaec23',
  brightBlue: '#5833ff',
  brightMagenta: '#f935f8',
  brightCyan: '#14f0f0',
  brightWhite: '#e9ebeb',
} satisfies ITheme

/// SwiftTerm's defaults that TerminalSession.swift keeps: 500 lines of
/// scrollback and a blinking block caret.
const TERMINAL_SCROLLBACK = 500

/// The SwiftTerm colours of TerminalSession.swift from the tokens: canvas
/// background, primary text, the accent caret (and selection at 25 %), over
/// SwiftTerm's ANSI palette.
function terminalTheme(): ITheme {
  const css = getComputedStyle(document.documentElement)
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback
  const canvas = token('--canvas', '#ffffff')
  const accent = token('--accent', '#2f7bf6')
  return {
    ...TERMINAL_APP_COLORS,
    background: canvas,
    foreground: token('--text-primary', '#1a1a1c'),
    cursor: accent,
    cursorAccent: canvas,
    selectionBackground: withAlpha(accent, 0.25),
  }
}

/** `#rrggbb` (or `#rgb`) with an alpha, as `rgba()`; other colours unchanged. */
function withAlpha(color: string, alpha: number): string {
  const hex = color.startsWith('#') ? color.slice(1) : ''
  const full = hex.length === 3 ? hex.replace(/./g, (c) => c + c) : hex
  if (!/^[0-9a-fA-F]{6}$/.test(full)) return color
  const value = Number.parseInt(full, 16)
  return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, ${alpha})`
}

/** Re-themes every terminal when the appearance changes (Settings or the system). */
function installThemeWatch(): void {
  if (themeWatchInstalled || typeof document === 'undefined') return
  themeWatchInstalled = true
  const apply = () => {
    const theme = terminalTheme()
    for (const view of liveViews) view.term.options.theme = theme
  }
  new MutationObserver(apply).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'class', 'style'] })
  window.matchMedia?.('(prefers-color-scheme: dark)').addEventListener('change', apply)
}

/// The session's xterm view, created on first use and wired to the session:
/// output → `term.write`, keystrokes → `session.write`, size →
/// `session.resize`, OSC 0/2 → `titleChanged`, OSC 7 → `directoryChanged`.
/// It goes away when the session leaves the workspace (closed, exited, or
/// the project closed).
function xtermView(session: TerminalSession, workspace: TerminalWorkspace): XtermView {
  const existing = session.view
  if (isXtermView(existing)) return existing
  installThemeWatch()

  const term = new Terminal({
    fontFamily: TERMINAL_FONT,
    fontSize: 11.5,
    theme: terminalTheme(),
    scrollback: TERMINAL_SCROLLBACK,
    cursorStyle: 'block',
    cursorBlink: true,
    allowTransparency: false,
  })
  const fit = new FitAddon()
  term.loadAddon(fit)
  const host = document.createElement('div')
  host.className = styles.xterm ?? ''

  const cleanups: Array<() => void> = []
  let frame = 0
  let disposed = false

  const fitSoon = () => {
    if (frame !== 0 || disposed) return
    frame = requestAnimationFrame(() => {
      frame = 0
      if (disposed || !host.isConnected || host.clientWidth === 0 || host.clientHeight === 0) return
      try {
        fit.fit()
      } catch {
        // Not laid out yet.
      }
      session.resize(term.cols, term.rows)
    })
  }

  const view: XtermView = {
    kind: 'xterm',
    term,
    fit,
    host,
    opened: false,
    fitSoon,
    dispose: () => {
      if (disposed) return
      disposed = true
      if (frame !== 0) cancelAnimationFrame(frame)
      for (const cleanup of cleanups) cleanup()
      liveViews.delete(view)
      host.remove()
      term.dispose()
      if (session.view === view) session.view = null
    },
  }

  cleanups.push(session.onData((data) => term.write(data)))
  const disposables = [
    term.onData((data) => session.write(data)),
    term.onBinary((data) => session.write(data)),
    term.onResize(({ cols, rows }) => session.resize(cols, rows)),
    term.onTitleChange((title) => session.titleChanged(title)),
    term.parser.registerOscHandler(7, (data) => {
      session.directoryChanged(data)
      return true
    }),
  ]
  cleanups.push(() => disposables.forEach((disposable) => disposable.dispose()))
  cleanups.push(
    workspace.subscribe(() => {
      if (!workspace.sessions.includes(session)) view.dispose()
    }),
  )

  session.view = view
  liveViews.add(view)
  return view
}

/// `TerminalHost`: lays the selected session's long-lived view into the
/// pane, swapping it when the selection changes, keeps it fitted to the
/// pane and hands it the keyboard.
export function TerminalHost({ session, workspace }: { session: TerminalSession; workspace: TerminalWorkspace }) {
  const containerRef = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    const container = containerRef.current
    if (!container) return
    const view = xtermView(session, workspace)
    container.appendChild(view.host)
    if (!view.opened) {
      view.term.open(view.host)
      view.opened = true
    }
    view.fitSoon()
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => view.fitSoon())
    observer?.observe(container)
    const focus = requestAnimationFrame(() => view.term.focus())
    return () => {
      observer?.disconnect()
      cancelAnimationFrame(focus)
      if (view.host.parentElement === container) container.removeChild(view.host)
    }
  }, [session, workspace])

  return <div ref={containerRef} className={styles.host} />
}
