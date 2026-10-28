// Port of apps/macos/ContextAgent/Views/Chat/TranscriptItemViews.swift: the
// plain transcript items: the user's bubble (with its image thumbnails, hover
// timestamp, Edit pencil and inline edit mode), the assistant's markdown
// message (with its hover toolbar and Thinking row) and the muted notices.
// Tool groups, approvals and outcome cards live in their own files (Cards);
// `TranscriptRow` in ChatView.tsx dispatches to all of them.
//
// Also the helpers the Swift file shares with the other transcript views:
// `TranscriptTime`, `TranscriptHighlight`, `TranscriptPlainText`, and the
// React stand-in for SwiftUI's `\.fileReferences` environment value
// (`FileReferencesContext` / `useFileReferences`), which ChatView provides.

import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { createContext, memo, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { BrailleSpinner, brailleFrame, useReducedMotion } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { ContextMenu } from '../../components/ContextMenu'
import { CopyButton } from '../../components/CopyButton'
import { DisclosureRow } from '../../components/DisclosureRow'
import { IconButton } from '../../components/IconButton'
import type { MenuItem } from '../../components/Menu'
import { formatShortcut, isModKey } from '../../components/platform'
import { Tooltip } from '../../components/Tooltip'
import type { FileReferenceContext } from '../../markdown/fileReferences'
import { MarkdownView } from '../../markdown/MarkdownView'
import { RESPONSE_REPORT_REASONS, type ResponseReportReason } from '../../backend/types'
import { Popover } from '../../components/Popover'
import { assistantTurnReportKey, makeNoticeItem, outgoingPrompt, userTurnIsRewindable, type AssistantTurn, type NoticeItem, type UserTurn } from '../../models/transcript'
import { useApp, useChatValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import { reportResponse, useGateway, useGatewayRoutingActive } from '../../state/gateway'
import { useCanRewind } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { RadioGroup } from '../Settings/SettingsControls'
import { SentPromptView } from './SentPromptView'
import { ThinkingView } from './ThinkingView'
import styles from './TranscriptItemViews.module.css'

// MARK: - File references (the `\.fileReferences` environment value)

/// `EnvironmentValues.fileReferences` (Markdown/FileReferences.swift): the
/// chat's project context for file chips in rendered markdown. ChatView
/// provides it (memoised on the project root, as the Swift context compares
/// equal by root); every `MarkdownView` under the chat reads it with
/// `useFileReferences()` and passes it as `files`.
export const FileReferencesContext = createContext<FileReferenceContext | null>(null)

/** The chat's `FileReferenceContext`, or null outside a ChatView. */
export function useFileReferences(): FileReferenceContext | null {
  return useContext(FileReferencesContext)
}

// MARK: - User message

export interface UserMessageViewProps {
  turn: UserTurn
  /** Find-in-chat query; matching runs are highlighted. */
  highlight?: string | null
  /** The chat the message belongs to; null disables editing (previews). */
  chat?: ChatState | null
}

/**
 * `UserMessageView` (TranscriptItemViews.swift): right-aligned
 * `surface-raised` bubble (max 70 % of the column, 16 radius), timestamp and
 * an Edit pencil on hover, context menu Copy / Resend / Edit message… / Undo
 * from this message…. While the message is being edited
 * (`chat.editingTurnId`) the bubble is an editor with Cancel / Send.
 * Runtime-injected messages (`isRuntime`) render as a muted notice instead.
 */
export const UserMessageView = memo(function UserMessageView({ turn, highlight = null, chat = null }: UserMessageViewProps) {
  const editing = useChatValue(chat, (c) => c.editingTurnId === turn.id, false)
  const rewindable = useCanRewind(chat)
  const [hovering, setHovering] = useState(false)
  const runtimeNotice = useMemo(
    () => (turn.isRuntime ? makeNoticeItem({ id: turn.id, lines: [turn.text], date: turn.date }) : null),
    [turn.isRuntime, turn.id, turn.text, turn.date],
  )

  if (runtimeNotice) return <NoticeView item={runtimeNotice} highlight={highlight} />
  if (chat && editing) return <UserMessageEditor chat={chat} />

  /// Editing needs the persisted message id and an idle project.
  const canEdit = chat !== null && userTurnIsRewindable(turn) && rewindable
  const clock = TranscriptTime.clock(turn.date)

  const menu = (): MenuItem[] => {
    const app = useApp.getState()
    const items: MenuItem[] = [
      { label: 'Copy', onSelect: () => app.copyToPasteboard(turn.text) },
      { label: 'Resend', onSelect: () => void app.send(outgoingPrompt(turn.text, turn.images, turn.files)) },
    ]
    if (chat) {
      items.push(
        { type: 'separator' },
        { label: 'Edit message…', disabled: !canEdit, onSelect: () => app.beginEdit(turn, chat) },
        { label: 'Undo from this message…', disabled: !canEdit, onSelect: () => app.undo(turn, chat) },
      )
    }
    return items
  }

  return (
    <div className={styles.userRow}>
      <div
        className={[styles.userColumn, turn.sentPrompt ? styles.userColumnFull : ''].filter(Boolean).join(' ')}
        onMouseEnter={() => setHovering(true)}
        onMouseLeave={() => setHovering(false)}
        role="group"
        aria-label={`You, ${clock}`}
      >
        {turn.sentPrompt ? (
          <div className={styles.sentPrompt}>
            <SentPromptView prompt={turn.sentPrompt} />
          </div>
        ) : null}
        {turn.images.length > 0 ? <UserImageStrip paths={turn.images} /> : null}
        <ContextMenu items={menu}>
          <div className={`${styles.bubble} selectable`}>{TranscriptHighlight.render(turn.text, highlight, styles.hit)}</div>
        </ContextMenu>
        {/* Under the bubble; it keeps its space while hidden so the strip
            below the bubble is part of the hover area too. */}
        <div className={[styles.userMeta, hovering ? styles.shown : ''].filter(Boolean).join(' ')} aria-hidden={!hovering}>
          <span className={styles.clock}>{clock}</span>
          {canEdit && chat ? (
            <Tooltip content="Edit this message and send it again (later messages are removed)">
              <IconButton
                icon="edit"
                label="Edit message"
                size={12}
                noTooltip
                tabIndex={hovering ? 0 : -1}
                onClick={() => useApp.getState().beginEdit(turn, chat)}
              />
            </Tooltip>
          ) : null}
        </div>
      </div>
    </div>
  )
})

/** The bubble as an editor: Esc cancels, mod+Return (or Send) asks keep-or-revert and re-sends. */
function UserMessageEditor({ chat }: { chat: ChatState }) {
  const text = useChatValue(chat, (c) => c.editingText, '')
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const empty = text.trim().length === 0

  // `.onAppear { editorFocused = true }`, caret at the end.
  useEffect(() => {
    const element = textareaRef.current
    if (!element) return
    element.focus()
    element.setSelectionRange(element.value.length, element.value.length)
  }, [])

  // `.frame(minHeight: 56, maxHeight: 240)`: the editor grows with its text.
  useLayoutEffect(() => {
    const element = textareaRef.current
    if (!element) return
    element.style.height = '0px'
    // `scrollHeight` counts the padding, not the border.
    const content = element.scrollHeight - EDITOR_PADDING_Y + EDITOR_BORDER_Y
    element.style.height = `${Math.min(Math.max(content, 56), 240) + EDITOR_PADDING_Y}px`
  }, [text])

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'Escape' && !event.shiftKey && !event.altKey && !isModKey(event)) {
      event.preventDefault()
      event.stopPropagation()
      useApp.getState().cancelEdit(chat)
    } else if (event.key === 'Enter' && isModKey(event)) {
      // ⌘⏎ → `primaryAction` → `submitEdit`.
      event.preventDefault()
      event.stopPropagation()
      useApp.getState().submitEdit(chat)
    }
  }

  return (
    <div className={styles.userRow}>
      <div className={styles.editor} role="group" aria-label="Editing your message">
        <textarea
          ref={textareaRef}
          className={styles.editorField}
          value={text}
          onChange={(event) => chat.setEditingText(event.target.value)}
          onKeyDown={onKeyDown}
          aria-label="Message"
          spellCheck
        />
        <div className={styles.editorFooter}>
          <span className={styles.editorHint}>Sending removes this message&apos;s reply and everything after it.</span>
          <span className={styles.flexSpacer} />
          <Button size="small" onClick={() => useApp.getState().cancelEdit(chat)}>
            Cancel
          </Button>
          <Tooltip content={`Send the edited message (${formatShortcut('mod+Enter')})`}>
            <Button size="small" variant="prominent" disabled={empty} onClick={() => useApp.getState().submitEdit(chat)}>
              Send
            </Button>
          </Tooltip>
        </div>
      </div>
    </div>
  )
}

/** The editor's vertical padding (8 + 8) plus its 1 px border on each side. */
const EDITOR_PADDING_Y = 18
/** The border part of `EDITOR_PADDING_Y`. */
const EDITOR_BORDER_Y = 2

/// `UserImageStrip`: the images sent with a user message, as thumbnails
/// above the bubble: click opens one in the default viewer. A file deleted
/// since (pasted images are pruned after a month) shows a placeholder.
function UserImageStrip({ paths }: { paths: readonly string[] }) {
  return (
    <div className={styles.imageStrip}>
      {paths.map((path) => (
        <UserImageThumbnail key={path} path={path} />
      ))}
    </div>
  )
}

/** `UserImageThumbnail.height`. */
const THUMBNAIL_HEIGHT = 96

type ThumbnailState = { kind: 'loading' } | { kind: 'loaded'; src: string; width: number; height: number } | { kind: 'missing' }

function UserImageThumbnail({ path }: { path: string }) {
  const [state, setState] = useState<ThumbnailState>({ kind: 'loading' })

  useEffect(() => {
    let cancelled = false
    setState({ kind: 'loading' })
    let src: string
    try {
      src = convertFileSrc(path)
    } catch {
      setState({ kind: 'missing' })
      return
    }
    const image = new Image()
    image.onload = () => {
      if (cancelled) return
      // `.resizable().scaledToFit().frame(maxWidth: 192, maxHeight: 96)`.
      const natural = { width: image.naturalWidth || 1, height: image.naturalHeight || 1 }
      const scale = Math.min((THUMBNAIL_HEIGHT * 2) / natural.width, THUMBNAIL_HEIGHT / natural.height)
      setState({ kind: 'loaded', src, width: natural.width * scale, height: natural.height * scale })
    }
    image.onerror = () => {
      if (!cancelled) setState({ kind: 'missing' })
    }
    image.src = src
    return () => {
      cancelled = true
    }
  }, [path])

  const name = path.split(/[\\/]/).pop() ?? path
  if (state.kind === 'loaded') {
    return (
      <Tooltip content={path}>
        <img
          src={state.src}
          alt={`Image ${name}`}
          className={styles.thumbnail}
          style={{ width: state.width, height: state.height }}
          draggable={false}
          onClick={() => {
            void invoke<void>('open_with_default_app', { path }).catch(() => undefined)
          }}
        />
      </Tooltip>
    )
  }
  const missing = state.kind === 'missing'
  return (
    <Tooltip content={missing ? 'The image is no longer available' : path}>
      <div className={styles.thumbnailPlaceholder} role="img" aria-label={missing ? 'Image, no longer available' : `Image ${name}`}>
        {missing ? <Icon name="image" size={16} /> : null}
      </div>
    </Tooltip>
  )
}

// MARK: - Assistant message

export interface AssistantMessageViewProps {
  turn: AssistantTurn
  /** The project of the run; null (previews, sub-agent views) hides Report. */
  projectId?: string | null
}

/**
 * `AssistantMessageView` (TranscriptItemViews.swift): left-aligned markdown
 * without a bubble. A hover toolbar (top-right) carries the timestamp and a
 * `CopyButton` for the raw markdown; the context menu offers Copy as
 * Markdown / Copy as plain text. While streaming a Braille spinner trails
 * the text, and a `ThinkingView` sits above the text whenever the turn
 * exposed reasoning. A finished message that went through ModelGateway shows
 * Report under it on hover (`ReportResponsePopover`).
 */
export const AssistantMessageView = memo(function AssistantMessageView({ turn, projectId = null }: AssistantMessageViewProps) {
  const files = useFileReferences()
  const canRun = useApp((state) => state.selectedProjectId !== null)
  const [hovering, setHovering] = useState(false)
  const onRunCommand = canRun ? runInTerminal : undefined
  const streamingText = turn.isStreaming && !turn.isThinking
  const inlineSpinner = canInlineSpinner(turn.text)
  const clock = TranscriptTime.clock(turn.date)

  const menu = (): MenuItem[] => {
    const app = useApp.getState()
    return [
      { label: 'Copy as Markdown', onSelect: () => app.copyToPasteboard(turn.text) },
      { label: 'Copy as plain text', onSelect: () => app.copyToPasteboard(TranscriptPlainText.strip(turn.text)) },
    ]
  }

  return (
    <ContextMenu items={menu}>
      <div
        className={styles.assistant}
        // The whole message width counts as hovered, not only the glyphs:
        // otherwise moving across the blank space towards the toolbar ends
        // the hover and hides the Copy button before it can be clicked.
        onMouseEnter={() => setHovering(true)}
        onMouseLeave={() => setHovering(false)}
        role="group"
        aria-label={`Assistant, ${clock}`}
      >
        {turn.reasoning.length > 0 ? <ThinkingView turn={turn} /> : null}
        {turn.text.length > 0 ? (
          streamingText && inlineSpinner ? (
            <StreamingMarkdown text={turn.text} files={files} onRunCommand={onRunCommand} />
          ) : (
            <MarkdownView markdown={turn.text} files={files} onRunCommand={onRunCommand} />
          )
        ) : null}
        {streamingText && (turn.text.length === 0 || !inlineSpinner) ? (
          <div className={styles.writing}>
            <BrailleSpinner size={13} tint="var(--text-secondary)" />
            {turn.text.length === 0 ? <span className={styles.writingLabel}>Writing…</span> : null}
          </div>
        ) : null}
        {projectId !== null && turn.taskId !== null && !turn.isStreaming && turn.text.length > 0 ? (
          <ReportRow turn={turn} projectId={projectId} hovering={hovering} />
        ) : null}
        {!turn.isStreaming ? (
          // Invisible until hovered, but always in the accessibility tree
          // so Copy as Markdown is reachable without a mouse.
          <div className={[styles.toolbar, hovering ? styles.shown : ''].filter(Boolean).join(' ')}>
            <span className={styles.toolbarClock}>{clock}</span>
            <CopyButton text={turn.text} label="Copy as Markdown" />
          </div>
        ) : null}
      </div>
    </ContextMenu>
  )
})

/**
 * The Report row under a finished answer of a run routed through
 * ModelGateway: invisible until hovered (it keeps its height so the
 * transcript does not move), but always in the accessibility tree.
 */
function ReportRow({ turn, projectId, hovering }: { turn: AssistantTurn; projectId: string; hovering: boolean }) {
  const routed = useGatewayRoutingActive()
  const reported = useGateway((state) => state.reportedResponses.has(assistantTurnReportKey(turn)))
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement>(null)
  if (!routed) return null
  return (
    <div className={[styles.reportRow, hovering || open || reported ? styles.shown : ''].filter(Boolean).join(' ')}>
      <Tooltip content="Report a bad response to ModelGateway">
        <button
          ref={anchorRef}
          type="button"
          className={[styles.reportButton, reported ? styles.reported : ''].filter(Boolean).join(' ')}
          disabled={reported}
          onClick={() => setOpen(true)}
          aria-label={reported ? 'Response reported' : 'Report response'}
        >
          <Icon name="report" size={12} />
          <span>{reported ? 'Reported' : 'Report'}</span>
        </button>
      </Tooltip>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={anchorRef} placement="below" align="start" width={320} padded role="dialog" aria-label="Report response">
        <ReportResponsePopover turn={turn} projectId={projectId} onClose={() => setOpen(false)} />
      </Popover>
    </div>
  )
}

/**
 * `ReportResponsePopover`: why the response was bad and an optional note.
 * Send files it with `gateway/report`, which counts it against the provider
 * that served the message, and closes on success.
 */
function ReportResponsePopover({ turn, projectId, onClose }: { turn: AssistantTurn; projectId: string; onClose: () => void }) {
  const [reason, setReason] = useState<ResponseReportReason>('incorrect')
  const [comment, setComment] = useState('')
  const [sending, setSending] = useState(false)
  const send = async () => {
    setSending(true)
    const filed = await reportResponse(projectId, turn, reason, comment.trim().slice(0, 1000))
    setSending(false)
    if (filed) onClose()
  }
  return (
    <div className={styles.reportPanel}>
      <div className={styles.reportHeader}>
        <span className={styles.reportTitle}>Report response</span>
        <span className={styles.reportSubtitle}>ModelGateway counts it against the provider that served this message.</span>
      </div>
      <RadioGroup value={reason} options={RESPONSE_REPORT_REASONS} onChange={setReason} aria-label="Reason" />
      <textarea
        className={styles.reportComment}
        rows={3}
        value={comment}
        placeholder="What went wrong? (optional)"
        onChange={(event) => setComment(event.target.value)}
        maxLength={1000}
      />
      <div className={styles.reportButtons}>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="prominent" loading={sending} onClick={() => void send()}>
          {sending ? 'Sending…' : 'Send Report'}
        </Button>
      </div>
    </div>
  )
}

/** `AppState.runInTerminal`: a code block's play button (stable, so memoised markdown keeps its blocks). */
function runInTerminal(command: string): void {
  useApp.getState().runInTerminal(command)
}

/// The spinner trails the text (contract) by riding along as the last glyph
/// of the markdown; inside an open code fence it would land in the code
/// block, so there it falls back to its own line.
function canInlineSpinner(text: string): boolean {
  return (text.split('```').length - 1) % 2 === 0
}

/// `StreamingMarkdown`: streaming markdown with the Braille frame appended to
/// the text, so the spinner sits right after the last word (static ⠿ with
/// Reduce Motion).
function StreamingMarkdown({ text, files, onRunCommand }: { text: string; files: FileReferenceContext | null; onRunCommand?: (command: string) => void }) {
  const reduced = useReducedMotion()
  const frame = useBrailleFrame(!reduced)
  return <MarkdownView markdown={text + ' ' + (reduced ? '⠿' : frame)} files={files} onRunCommand={onRunCommand} />
}

/** `TimelineView(.periodic(from: .now, by: BrailleSpinner.frameInterval))`: the current spinner frame. */
function useBrailleFrame(active: boolean): string {
  const [frame, setFrame] = useState(() => brailleFrame())
  useEffect(() => {
    if (!active) return
    const timer = window.setInterval(() => setFrame(brailleFrame()), 40)
    return () => window.clearInterval(timer)
  }, [active])
  return frame
}

// MARK: - Notice

export interface NoticeViewProps {
  item: NoticeItem
  /** Find-in-chat query; matching runs are highlighted. */
  highlight?: string | null
}

/**
 * `NoticeView` (TranscriptItemViews.swift): muted 12.5 px lines
 * (`text-tertiary`); `danger` for errors. Low-value `detailLines` (task
 * boundary notes, checkpoints) sit behind a disclosure.
 */
export const NoticeView = memo(function NoticeView({ item, highlight = null }: NoticeViewProps) {
  const [detailsExpanded, setDetailsExpanded] = useState(false)
  const lineClass = [styles.noticeLine, item.isError ? styles.noticeError : '', 'selectable'].filter(Boolean).join(' ')
  return (
    <div className={styles.notice}>
      {item.lines.map((line, index) => (
        <div key={index} className={styles.noticeRow}>
          {item.isError ? <Icon name="warning" size={11} strokeWidth={2} className={styles.noticeIcon} aria-label="Error" /> : null}
          <span className={lineClass}>{TranscriptHighlight.render(line, highlight, styles.hit)}</span>
        </div>
      ))}
      {item.detailLines.length > 0 ? (
        <DisclosureRow
          expanded={detailsExpanded}
          onToggle={setDetailsExpanded}
          className={styles.noticeDisclosure}
          label={<span className={styles.noticeDetailsLabel}>{noticeDetailsLabel(item)}</span>}
        >
          <div className={styles.noticeDetails}>
            {item.detailLines.map((line, index) => (
              <span key={index} className={lineClass}>
                {TranscriptHighlight.render(line, highlight, styles.hit)}
              </span>
            ))}
          </div>
        </DisclosureRow>
      ) : null}
    </div>
  )
})

/// "2 more notes" under visible lines; with nothing above, the disclosure is
/// the whole notice and says so.
function noticeDetailsLabel(item: NoticeItem): string {
  const count = item.detailLines.length
  if (item.lines.length === 0) return `Run notes · ${count}`
  return `${count} more note${count === 1 ? '' : 's'}`
}

// MARK: - Helpers shared by the transcript views

const clockFormat = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' })
const dayFormat = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' })
const dayYearFormat = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' })

function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
}

/** `TranscriptTime` (TranscriptItemViews.swift). */
export const TranscriptTime = {
  /** "3:42 PM" (the user's locale, like `date.formatted(date: .omitted, time: .shortened)`). */
  clock(date: Date): string {
    return clockFormat.format(date)
  },

  /** `Calendar.isDate(_:inSameDayAs:)`. */
  sameDay,

  /** `Calendar.isDateInToday`. */
  isToday(date: Date, now: Date = new Date()): boolean {
    return sameDay(date, now)
  },

  /** "Today", "Yesterday", "Oct 12", "Oct 12, 2024". */
  dayLabel(date: Date, now: Date = new Date()): string {
    if (sameDay(date, now)) return 'Today'
    const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
    if (sameDay(date, yesterday)) return 'Yesterday'
    if (date.getFullYear() === now.getFullYear()) return dayFormat.format(date)
    return dayYearFormat.format(date)
  },
}

/** One run of `TranscriptHighlight.runs`: plain text or a match. */
export interface HighlightRun {
  text: string
  hit: boolean
}

/// A string folded for `[.caseInsensitive, .diacriticInsensitive]`
/// comparison, with each folded UTF-16 unit's range in the original.
interface Folded {
  text: string
  starts: number[]
  ends: number[]
}

/** Plain ASCII folds by lowercasing alone (no code point above U+007F). */
function isASCII(text: string): boolean {
  for (let i = 0; i < text.length; i += 1) if (text.charCodeAt(i) > 0x7f) return false
  return true
}

/** Case- and diacritic-folded text (no position map: counting only). */
function foldPlain(text: string): string {
  if (isASCII(text)) return text.toLowerCase()
  return text.normalize('NFD').replace(/\p{M}+/gu, '').toLowerCase()
}

function foldMapped(text: string): Folded {
  if (isASCII(text)) {
    const starts: number[] = []
    const ends: number[] = []
    for (let i = 0; i < text.length; i += 1) {
      starts.push(i)
      ends.push(i + 1)
    }
    return { text: text.toLowerCase(), starts, ends }
  }
  let out = ''
  const starts: number[] = []
  const ends: number[] = []
  let index = 0
  for (const character of text) {
    const folded = character.normalize('NFD').replace(/\p{M}+/gu, '').toLowerCase()
    for (let k = 0; k < folded.length; k += 1) {
      starts.push(index)
      ends.push(index + character.length)
    }
    out += folded
    index += character.length
  }
  return { text: out, starts, ends }
}

/** `TranscriptHighlight` (TranscriptItemViews.swift): find-in-chat highlighting for plain text. */
export const TranscriptHighlight = {
  /** Number of case- and diacritic-insensitive occurrences of `query` in `text`. */
  occurrences(query: string, text: string): number {
    if (query.length === 0) return 0
    const needle = foldPlain(query)
    if (needle.length === 0) return 0
    const haystack = foldPlain(text)
    let count = 0
    let cursor = haystack.indexOf(needle)
    while (cursor >= 0) {
      count += 1
      cursor = haystack.indexOf(needle, cursor + needle.length)
    }
    return count
  },

  /** `text` split into plain runs and the case-insensitive occurrences of `query`. */
  runs(text: string, query: string | null | undefined): HighlightRun[] {
    if (!query) return [{ text, hit: false }]
    const needle = foldPlain(query)
    if (needle.length === 0) return [{ text, hit: false }]
    const folded = foldMapped(text)
    const runs: HighlightRun[] = []
    let cursor = 0
    let search = folded.text.indexOf(needle)
    while (search >= 0) {
      const start = folded.starts[search] ?? text.length
      const end = folded.ends[search + needle.length - 1] ?? text.length
      if (start >= cursor && end > start) {
        if (start > cursor) runs.push({ text: text.slice(cursor, start), hit: false })
        runs.push({ text: text.slice(start, end), hit: true })
        cursor = end
      }
      search = folded.text.indexOf(needle, search + needle.length)
    }
    if (cursor < text.length) runs.push({ text: text.slice(cursor), hit: false })
    return runs
  },

  /// `attributed(_:query:)`: `text` with every occurrence of `query` on a
  /// `warning` background (`hitClassName` styles the `<mark>`).
  render(text: string, query: string | null | undefined, hitClassName?: string): ReactNode {
    if (!query) return text
    const runs = TranscriptHighlight.runs(text, query)
    if (runs.length === 1 && runs[0] && !runs[0].hit) return text
    return runs.map((run, index) =>
      run.hit ? (
        <mark key={index} className={hitClassName}>
          {run.text}
        </mark>
      ) : (
        <span key={index}>{run.text}</span>
      ),
    )
  },
}

/// `TranscriptPlainText` (TranscriptItemViews.swift): "Copy as plain text",
/// a conservative markdown stripper (emphasis, code spans, headings, links,
/// fences, quote markers). Lists keep their markers.
export const TranscriptPlainText = {
  strip(markdown: string): string {
    const out: string[] = []
    let inFence = false
    for (const rawLine of markdown.split('\n')) {
      let line = rawLine
      const trimmed = line.trim()
      if (trimmed.startsWith('```') || trimmed.startsWith('~~~')) {
        inFence = !inFence
        continue
      }
      if (inFence) {
        out.push(line)
        continue
      }
      if (trimmed === '---' || trimmed === '***' || trimmed === '___') {
        out.push('')
        continue
      }
      line = line.replace(/^\s{0,3}#{1,6}\s+/, '')
      line = line.replace(/^\s{0,3}>\s?/, '')
      line = line.replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
      line = line.replace(/`([^`]*)`/g, '$1')
      line = line.replace(/\*\*([^*]+)\*\*/g, '$1')
      line = line.replace(/__([^_]+)__/g, '$1')
      line = line.replace(/(?<![\w*])\*([^*\n]+)\*(?![\w*])/g, '$1')
      line = line.replace(/(?<![\w_])_([^_\n]+)_(?![\w_])/g, '$1')
      out.push(line)
    }
    return out.join('\n')
  },
}
