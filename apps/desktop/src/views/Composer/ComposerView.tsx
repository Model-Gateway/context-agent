// Port of apps/macos/ContextAgent/Views/Composer/ComposerView.swift: the chat
// composer (apps/macos/ARCHITECTURE.md "Composer"; apps/desktop/ARCHITECTURE.md
// "Composer text view"). `ComposerTextView` (an NSTextView) becomes a
// textarea that grows 1 → 8 lines (measured with a hidden clone), with
// IME-safe Return-to-send, Shift+Return newline, → to take the ghost
// suggestion, Esc collapsing the selection, and `@` mentions tinted through
// a mirror drawn behind the (then transparent) text. Files dropped on the
// card (Tauri's drag-and-drop events, hit-tested against the card) and
// images / PDFs pasted into it become attachment chips.

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ChangeEvent, type ClipboardEvent, type KeyboardEvent, type ReactNode, type RefObject } from 'react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { errorText } from '../../backend/errors'
import { modelStatSavingsLabel } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Kbd } from '../../components/Kbd'
import { formatShortcut, isMac, isModKey } from '../../components/platform'
import { toasts } from '../../components/toasts'
import { Tooltip } from '../../components/Tooltip'
import {
  appendingAttachments,
  attachmentIsImage,
  prepareAttachmentPrompt,
  savePastedImage,
  savePastedPDF,
  type ComposerAttachment,
} from '../../models/attachments'
import { useModelsDevVersion } from '../../models/modelsDev'
import { permissionProfileIcon } from '../../models/permissions'
import { approvalModeLabel, selectedProject, useApp, useChatValue, useProjectValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { projectById } from '../../state/selectors'
import { sendInNewWorktree } from '../../state/worktrees'
import { Icon, type IconName } from '../../theme/icons'
import { AttachmentDropHighlight, AttachmentRow } from './AttachmentViews'
import { EffortPopover, effortIsUltra, effortLevel, effortLevels, effortTint } from './EffortPopover'
import { MentionBridge, MentionMenu, mentionRanges } from './MentionMenu'
import { ModelLogo } from './ModelHoverCard'
import { ModelPickerPopover } from './ModelPickerPopover'
import { PermissionsPopover } from './PermissionsPopover'
import { QueueRow, SteeringRow } from './QueueRow'
import styles from './ComposerView.module.css'

/** `ComposerView(chat:)` or `ComposerView(draftFor:)`. */
export type ComposerViewProps = { chat: ChatState; draftFor?: undefined } | { draftFor: ProjectState; chat?: undefined }

/// `ComposerView` (ComposerView.swift): a floating `surface` card with the
/// attachment chips (files dropped on the card, pasted, or picked from the +
/// button), a growing text area, the + button, the permissions chip
/// (`PermissionsPopover`), the reasoning-effort chip (`EffortPopover`), the
/// model chip (`ModelPickerPopover`) and the send / stop / queue button.
/// `{ chat }` edits a live chat's draft and shows its steering and queue
/// rows; `{ draftFor }` edits `pendingDraft` / `pendingAttachments` of the
/// project for the empty state.
export function ComposerView(props: ComposerViewProps) {
  if (props.chat) return <ChatComposer chat={props.chat} />
  if (props.draftFor) return <DraftComposer project={props.draftFor} />
  return null
}

// MARK: - Bindings

const NO_ATTACHMENTS: ComposerAttachment[] = []

function ChatComposer({ chat }: { chat: ChatState }) {
  const project = useApp((state) => projectById(state, chat.projectId) ?? selectedProject(state))
  const text = useChatValue(chat, (c) => c.draft, '')
  const attachments = useChatValue(chat, (c) => c.attachments, NO_ATTACHMENTS)
  const setText = useCallback((value: string) => chat.setDraft(value), [chat])
  const getAttachments = useCallback(() => chat.attachments, [chat])
  const setAttachments = useCallback((list: ComposerAttachment[]) => chat.setAttachments(list), [chat])
  if (!project) return null
  return (
    <div className={styles.chatComposer}>
      <SteeringRow chat={chat} />
      <QueueRow chat={chat} />
      <ComposerCard
        project={project}
        chat={chat}
        text={text}
        setText={setText}
        attachments={attachments}
        getAttachments={getAttachments}
        setAttachments={setAttachments}
      />
    </div>
  )
}

function DraftComposer({ project }: { project: ProjectState }) {
  const id = project.id
  const text = useApp((state) => state.pendingDraft[id] ?? '')
  const attachments = useApp((state) => state.pendingAttachments[id] ?? NO_ATTACHMENTS)
  const setText = useCallback((value: string) => useApp.getState().setPendingDraft(id, value), [id])
  const getAttachments = useCallback(() => useApp.getState().pendingAttachments[id] ?? NO_ATTACHMENTS, [id])
  const setAttachments = useCallback((list: ComposerAttachment[]) => useApp.getState().setPendingAttachments(id, list), [id])
  return (
    <ComposerCard
      project={project}
      chat={null}
      text={text}
      setText={setText}
      attachments={attachments}
      getAttachments={getAttachments}
      setAttachments={setAttachments}
    />
  )
}

// MARK: - The card

/** `ComposerTextView.font` line height: ceil(defaultLineHeight(14 pt system font)). */
const LINE_HEIGHT = 17
/** `ComposerTextView.verticalInset` (`textContainerInset.height`). */
const VERTICAL_INSET = 6

// The bottom row's glyphs, measured against docs/images/new-chat.png and
// running.png (sizes are the Icon's optical size, which already reads like
// the SF point size): the + (13 pt) is 14, the chip glyphs (11 pt) 11, the
// chevrons (9 pt) 11, since SF chevrons are large for their point size.
/** The send arrow (14 pt semibold), the filled stop square, the queue clock. */
const PRIMARY_GLYPH: Record<'send' | 'stop' | 'queue', number> = { send: 16, stop: 12.5, queue: 13.5 }

type PrimaryAction = 'send' | 'stop' | 'queue'

interface ComposerCardProps {
  project: ProjectState
  chat: ChatState | null
  text: string
  setText: (value: string) => void
  attachments: ComposerAttachment[]
  /** The attachments right now (async appends must not drop chips added meanwhile). */
  getAttachments: () => ComposerAttachment[]
  setAttachments: (list: ComposerAttachment[]) => void
}

/// "“<title>” is also working in this checkout" while other chats of this
/// project run (their agents are told about each other's files).
function concurrentHint(project: ProjectState, chat: ChatState | null): string | null {
  const others = [...project.runningSessionIds].filter((id) => id !== chat?.sessionId).sort()
  const first = others[0]
  if (first === undefined) return null
  const title = project.chatTitle(first) ?? 'another chat'
  const more = others.length > 1 ? ` and ${others.length - 1} more` : ''
  return `“${title}”${more} ${others.length > 1 ? 'are' : 'is'} also working in this checkout`
}

/** Inserts `text` at the caret as one undoable edit. */
function insertAtCaret(textarea: HTMLTextAreaElement, text: string): void {
  const inserted = typeof document.execCommand === 'function' && document.execCommand('insertText', false, text)
  if (!inserted) {
    textarea.setRangeText(text, textarea.selectionStart, textarea.selectionEnd, 'end')
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
  }
}

/** The platform's spelling of the Return-key hints in the Swift strings. */
const RETURN_KEY = isMac ? 'Return' : 'Enter'
const QUEUE_KEY = isMac ? '⌘Return' : 'Ctrl+Enter'
const WORKTREE_KEY = isMac ? '⇧⌘Return' : 'Ctrl+Shift+Enter'

function ComposerCard({ project, chat, text, setText, attachments, getAttachments, setAttachments }: ComposerCardProps) {
  // Store values this card shows; each re-renders it only when it changes.
  const focusToken = useApp((state) => state.composerFocusToken)
  const ready = useApp((state) => state.connection.kind === 'ready')
  const effort = useApp((state) => state.config?.model.reasoning_effort ?? null)
  const pickerVisible = useApp((state) => state.modelPickerVisible)
  const mcpServers = useApp((state) => state.mcpServers)
  useApp((state) => state.config?.model.base_url ?? '')
  useModelsDevVersion()
  const info = useProjectValue(project, (p) => p.info, project.info)
  const switchingModel = useProjectValue(project, (p) => p.switchingModel, null)
  const runningIds = useProjectValue(project, (p) => p.runningSessionIds, project.runningSessionIds)
  const creatingWorktree = useProjectValue(project, (p) => p.creatingWorktree, false)
  useProjectValue(project, (p) => p.models.stats, null)
  const hint = useApp(() => concurrentHint(project, chat))
  const chatRunning = useChatValue(chat, (c) => c.isRunning, false)
  const cancelRequested = useChatValue(chat, (c) => c.cancelRequested, false)
  const chatSuggestion = useChatValue(chat, (c) => c.suggestion, null)
  const hasQuestion = useChatValue(chat, (c) => c.pendingQuestion !== null, false)

  const [measuredHeight, setMeasuredHeight] = useState(0)
  const [scrollbarWidth, setScrollbarWidth] = useState(0)
  /** A file drag is over the card. */
  const [dropTargeted, setDropTargeted] = useState(false)
  const [permissionsPresented, setPermissionsPresented] = useState(false)
  const [effortSaving, setEffortSaving] = useState(false)
  const [effortPresented, setEffortPresented] = useState(false)
  /** An IME composition is in progress (no tinting, Return commits it). */
  const [composing, setComposing] = useState(false)
  const [mentions] = useState(() => new MentionBridge())

  const cardRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const measureRef = useRef<HTMLDivElement>(null)
  const mirrorRef = useRef<HTMLDivElement>(null)
  const permissionsRef = useRef<HTMLButtonElement>(null)
  const effortRef = useRef<HTMLButtonElement>(null)
  const modelRef = useRef<HTMLButtonElement>(null)

  // MARK: Derived

  const trimmedText = text.trim()
  const canSubmit = (trimmedText.length > 0 || attachments.length > 0) && switchingModel === null
  // Other chats of the project run alongside; only this chat's own run
  // (still settling) makes a prompt wait.
  const primaryAction: PrimaryAction = chat && chatRunning ? 'stop' : chat && runningIds.has(chat.sessionId) ? 'queue' : 'send'
  /** The chat's suggested next message while nothing is typed or running. */
  const suggestion = chat && !chatRunning && text.length === 0 ? chatSuggestion : null
  const installed = mcpServers.map((server) => server.name)
  const ranges = mentionRanges(text, installed)
  const tinted = !composing && ranges.length > 0
  const editorHeight = Math.min(Math.max(measuredHeight, LINE_HEIGHT + VERTICAL_INSET * 2), LINE_HEIGHT * 8 + VERTICAL_INSET * 2)

  // MARK: Effects

  useLayoutEffect(() => {
    mentions.textarea = textareaRef.current
  })

  /** The next focus-token change comes from this card and keeps the selection. */
  const keepSelectionRef = useRef(false)
  /** `composerFocusToken += 1` from the card itself (a popover, the picker or a drop closing). */
  const refocusKeepingSelection = useCallback(() => {
    keepSelectionRef.current = true
    useApp.getState().focusComposer()
  }, [])

  // A popover closing hands focus back to the composer (`composerFocusToken += 1`).
  const wasPresented = useRef({ effort: false, permissions: false, picker: false })
  useEffect(() => {
    const previous = wasPresented.current
    const closed = (previous.effort && !effortPresented) || (previous.permissions && !permissionsPresented) || (previous.picker && !pickerVisible)
    wasPresented.current = { effort: effortPresented, permissions: permissionsPresented, picker: pickerVisible }
    if (closed) refocusKeepingSelection()
  }, [effortPresented, permissionsPresented, pickerVisible])

  // The `@` menu needs the apps' state and catalog titles and logos.
  useEffect(() => {
    if (!ready) return
    const app = useApp.getState()
    void (async () => {
      if (app.mcpServers.length === 0) await app.refreshMcpStatus()
      await useApp.getState().loadMcpCatalog()
    })()
  }, [ready])

  // Focus on every token change, and once on mount (a token bumped before
  // the composer appeared, by a new chat or a selection, still focuses it).
  // The caret goes to the end of the text, as NSTextView's does when the
  // text is set from outside (starter cards, Edit, Resend, editing a queued
  // prompt), even when the text did not change; only the card's own
  // refocus (a popover, the file picker or a drop closing) keeps the
  // selection where it was.
  useEffect(() => {
    let attempts = 0
    let timer: number | null = null
    const keepSelection = keepSelectionRef.current
    keepSelectionRef.current = false
    const focus = () => {
      const textarea = textareaRef.current
      if (!textarea) return
      if (textarea.getClientRects().length > 0) {
        textarea.focus({ preventScroll: true })
        if (!keepSelection) {
          const end = textarea.value.length
          textarea.setSelectionRange(end, end)
          textarea.scrollTop = textarea.scrollHeight
        }
      } else if (attempts < 3) {
        attempts += 1
        timer = window.setTimeout(focus, 50)
      }
    }
    const frame = requestAnimationFrame(focus)
    return () => {
      cancelAnimationFrame(frame)
      if (timer !== null) window.clearTimeout(timer)
    }
  }, [focusToken])

  // Content height incl. insets (`measure()`), after every change and on width changes.
  const measure = useCallback(() => {
    const clone = measureRef.current
    const textarea = textareaRef.current
    if (!clone || !textarea) return
    const height = clone.offsetHeight
    setMeasuredHeight((previous) => (Math.abs(previous - height) > 0.5 ? height : previous))
    const scrollbar = Math.max(0, textarea.offsetWidth - textarea.clientWidth)
    setScrollbarWidth((previous) => (previous === scrollbar ? previous : scrollbar))
  }, [])

  useLayoutEffect(() => {
    measure()
  })

  useEffect(() => {
    const editor = editorRef.current
    if (!editor || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => measure())
    observer.observe(editor)
    return () => observer.disconnect()
  }, [measure])

  // The tint mirror scrolls with the text.
  useLayoutEffect(() => {
    const mirror = mirrorRef.current
    const textarea = textareaRef.current
    if (mirror && textarea) mirror.scrollTop = textarea.scrollTop
  })

  // Files dropped on the card attach to the draft (Tauri delivers file drops
  // to the webview, with paths, instead of HTML drag events).
  const refocusRef = useRef(refocusKeepingSelection)
  refocusRef.current = refocusKeepingSelection
  const attachRef = useRef<(paths: string[]) => void>(() => undefined)
  attachRef.current = (paths: string[]) => {
    void appendingAttachments(paths, getAttachments()).then((list) => setAttachments(list))
  }
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    let paths: string[] = []
    const inside = (position: { x: number; y: number }) => {
      const card = cardRef.current
      if (!card) return false
      const rect = card.getBoundingClientRect()
      if (rect.width === 0 || rect.height === 0) return false
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
              setDropTargeted(paths.length > 0 && inside(payload.position))
              break
            case 'over':
              setDropTargeted(paths.length > 0 && inside(payload.position))
              break
            case 'drop': {
              setDropTargeted(false)
              const dropped = payload.paths.length > 0 ? payload.paths : paths
              paths = []
              if (dropped.length > 0 && inside(payload.position)) {
                attachRef.current(dropped)
                refocusRef.current()
              }
              break
            }
            default:
              setDropTargeted(false)
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
  }, [])

  // MARK: Actions

  /// Return / send button: `send` creates the session when needed and,
  /// while this chat runs, sends the prompt to the running turn (mod+Return,
  /// `primaryAction`, queues it instead). Attachments go along as an
  /// "Attached files:" list of path references and, where the model can take
  /// them, as images and text (`prepareAttachmentPrompt`).
  const submit = () => {
    if (!canSubmit) return
    const prompt = trimmedText
    const list = attachments
    const root = project.root
    void (async () => {
      await useApp.getState().send(await prepareAttachmentPrompt(prompt, list, root))
    })()
  }

  const setEffort = (value: string) => {
    if (value === (useApp.getState().config?.model.reasoning_effort ?? '') || effortSaving) return
    setEffortSaving(true)
    void (async () => {
      try {
        await useApp.getState().patchConfig({ 'model.reasoning_effort': value.length === 0 ? null : value })
      } catch (error) {
        const message = errorText(error)
        toasts.push({ message: `Could not change the reasoning effort: ${message}`, level: 'error', details: message })
      } finally {
        setEffortSaving(false)
      }
    })()
  }

  /// Paste of an image (a screenshot, "Copy Image") or a PDF with no text on
  /// the clipboard: saved under Pasted Images, then a chip. Text wins over an
  /// image when both are there.
  const onPaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const data = event.clipboardData
    if (data.getData('text/plain').length > 0) return
    const items = Array.from(data.items)
    const image = items.find((item) => item.kind === 'file' && item.type.startsWith('image/'))
    const pdf = items.find((item) => item.kind === 'file' && item.type === 'application/pdf')
    const file = (image ?? pdf)?.getAsFile() ?? null
    if (!file) return
    event.preventDefault()
    const isImage = image !== undefined
    void (async () => {
      try {
        const path = isImage ? await savePastedImage(file) : await savePastedPDF(file)
        // The chip reads "Pasted image" / "Pasted document" (and the file is
        // sent as content, not a path) whether or not the saved path is
        // recognised as being under Pasted Images.
        const before = getAttachments()
        const existing = new Set(before.map((entry) => entry.id))
        const list = await appendingAttachments([path], before)
        setAttachments(list.map((entry) => (existing.has(entry.id) || entry.isPasted ? entry : { ...entry, isPasted: true })))
      } catch (error) {
        const message = errorText(error)
        toasts.push({ message: `Could not paste: ${message}`, level: 'error', details: message })
      }
    })()
  }

  /** The file picker → attachment chips. */
  const chooseAttachments = () => {
    void (async () => {
      try {
        const picked = await openDialog({ multiple: true, directory: false, defaultPath: project.root, title: 'Choose files to attach to the prompt' })
        const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked]
        if (paths.length > 0) setAttachments(await appendingAttachments(paths, getAttachments()))
      } catch {
        // The dialog failed to open: nothing attached.
      }
      refocusKeepingSelection()
    })()
  }

  const removeAttachment = useCallback(
    (attachment: ComposerAttachment) => setAttachments(getAttachments().filter((entry) => entry.id !== attachment.id)),
    [getAttachments, setAttachments],
  )

  // MARK: Text view

  const onChange = (event: ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value
    setText(value)
    // Typing (or accepting it) replaces the suggestion.
    if (value.length > 0 && chat?.suggestion != null) chat.setSuggestion(null)
    mentions.update(event.target, composing || (event.nativeEvent as InputEvent).isComposing === true)
  }

  const onSelectionChange = () => {
    const textarea = textareaRef.current
    if (textarea) mentions.update(textarea, composing)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const textarea = event.currentTarget
    const imeActive = composing || event.nativeEvent.isComposing || event.keyCode === 229
    const plain = !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey
    // The `@` menu owns ↑/↓, Return, Tab and Esc while it is open.
    if (mentions.isOpen && !imeActive) {
      if (event.key === 'ArrowUp' && plain) {
        event.preventDefault()
        event.stopPropagation()
        mentions.move(-1)
        return
      }
      if (event.key === 'ArrowDown' && plain) {
        event.preventDefault()
        event.stopPropagation()
        mentions.move(1)
        return
      }
      if ((event.key === 'Enter' && !isModKey(event)) || (event.key === 'Tab' && plain)) {
        event.preventDefault()
        event.stopPropagation()
        mentions.acceptSelected()
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        mentions.dismiss()
        return
      }
    }
    // F5 is NSTextView's `complete:` (as is Option-Esc): Swift collapses the
    // selection instead of showing completions. It must not reach the
    // webview either, where it would reload the window.
    if (event.key === 'F5') {
      event.preventDefault()
      if (imeActive) return
      const end = textarea.selectionEnd
      textarea.setSelectionRange(end, end)
      return
    }
    // Esc: collapse the selection. The event goes on to the window's
    // handler (state/keys.ts: the inline message edit), which in Swift runs
    // first. Esc never answers an approval.
    if (event.key === 'Escape') {
      if (imeActive) return
      const end = textarea.selectionEnd
      textarea.setSelectionRange(end, end)
      return
    }
    // → in the empty field takes the suggested next message (undoable).
    if (event.key === 'ArrowRight' && plain && textarea.value.length === 0 && !imeActive && suggestion) {
      event.preventDefault()
      insertAtCaret(textarea, suggestion)
      return
    }
    // A plain text view inserts a tab character.
    if (event.key === 'Tab' && plain && !imeActive) {
      event.preventDefault()
      insertAtCaret(textarea, '\t')
      return
    }
    if (event.key !== 'Enter') return
    // An IME composition commits with Return: let the input method have it.
    if (imeActive) return
    // mod+Return is the app's primary action (state/keys.ts): it queues while the chat runs.
    if (isModKey(event)) return
    if (event.shiftKey || event.altKey) {
      if (event.altKey && !event.shiftKey) {
        event.preventDefault()
        insertAtCaret(textarea, '\n')
      }
      return
    }
    if (event.ctrlKey || event.metaKey) return
    event.preventDefault()
    event.stopPropagation()
    submit()
  }

  // MARK: Render

  const placeholder = suggestion ?? (hasQuestion ? 'Answer in your own words…' : chatRunning ? `Steer the running task · ${QUEUE_KEY} to queue` : 'Ask anything…')
  const modelInfo = useApp.getState().modelInfo(info.model)
  const visionWarning = attachments.some(attachmentIsImage) && modelInfo && !modelInfo.vision ? `${modelInfo.name} may not accept images` : null

  return (
    <div className={styles.composer}>
      {hint !== null ? (
        <div className={styles.hint}>
          <BrailleSpinner size={10} tint="var(--text-tertiary)" />
          <span className={styles.hintText}>{hint}</span>
          {project.isGitRepository ? (
            <>
              <span className={styles.hintText}>·</span>
              <Tooltip content={`Run this prompt in a second checkout of the repository, apart from the other chats (${WORKTREE_KEY})`}>
                <button type="button" className={styles.hintAction} disabled={!canSubmit || creatingWorktree} onClick={() => void sendInNewWorktree()}>
                  {creatingWorktree ? 'Creating a worktree…' : 'Start in a new worktree instead'}
                </button>
              </Tooltip>
            </>
          ) : null}
        </div>
      ) : null}

      <div ref={cardRef} className={styles.card}>
        {attachments.length > 0 ? (
          <>
            {visionWarning !== null ? (
              <div className={styles.visionWarning}>
                <Icon name="warning" size={13} strokeWidth={2} />
                <span>{visionWarning}</span>
              </div>
            ) : null}
            <div className={styles.attachments}>
              <AttachmentRow attachments={attachments} onRemove={removeAttachment} />
            </div>
          </>
        ) : null}

        <div ref={editorRef} className={styles.editor}>
          <div className={styles.editorBox} style={{ height: editorHeight }}>
            {tinted ? (
              <div ref={mirrorRef} className={styles.mirror} style={{ paddingRight: 5 + scrollbarWidth }} aria-hidden="true">
                {renderTinted(text, ranges)}
              </div>
            ) : null}
            <textarea
              ref={textareaRef}
              className={[styles.textarea, tinted ? styles.textareaTinted : ''].filter(Boolean).join(' ')}
              style={{ height: editorHeight }}
              value={text}
              rows={1}
              aria-label="Message"
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              autoComplete="off"
              onChange={onChange}
              onKeyDown={onKeyDown}
              onSelect={onSelectionChange}
              onClick={onSelectionChange}
              onPaste={onPaste}
              onScroll={(event) => {
                const mirror = mirrorRef.current
                if (mirror) mirror.scrollTop = event.currentTarget.scrollTop
              }}
              onCompositionStart={() => setComposing(true)}
              onCompositionEnd={(event) => {
                setComposing(false)
                mentions.update(event.currentTarget, false)
              }}
            />
            {text.length === 0 ? (
              // The suggested next message (→ accepts it), else the placeholder.
              <div className={styles.placeholder} aria-hidden="true">
                <span className={styles.placeholderText}>{placeholder}</span>
                {suggestion !== null ? <Kbd>→</Kbd> : null}
              </div>
            ) : null}
            <div className={styles.measureBox} aria-hidden="true">
              <div ref={measureRef} className={styles.measure}>
                {text + '\u200b'}
              </div>
            </div>
          </div>
        </div>

        <div className={styles.controls}>
          <Tooltip content="Attach files · you can also drop or paste files and images on the composer">
            <button type="button" className={styles.attachButton} aria-label="Attach files" onClick={chooseAttachments}>
              <Icon name="attach" size={14} strokeWidth={1.75} />
            </button>
          </Tooltip>
          <PermissionsChip
            buttonRef={permissionsRef}
            profile={info.permission_profile}
            onClick={() => setPermissionsPresented((shown) => !shown)}
          />
          <span className={styles.spacer} />
          <EffortChip
            buttonRef={effortRef}
            effort={effort}
            saving={effortSaving}
            disabled={!ready || effortSaving}
            onClick={() => setEffortPresented((shown) => !shown)}
          />
          <ModelChip buttonRef={modelRef} project={project} model={info.model} switching={switchingModel !== null} />
          <PrimaryButton
            action={primaryAction}
            cancelling={cancelRequested}
            canSubmit={canSubmit}
            switching={switchingModel !== null}
            onSubmit={submit}
          />
        </div>

        <AttachmentDropHighlight visible={dropTargeted} />

        {/* The `@` menu floats just above the card's top-left corner. */}
        <div className={styles.mentionAnchor}>
          <MentionMenu bridge={mentions} projectId={project.id} />
        </div>
      </div>

      <PermissionsPopover open={permissionsPresented} onClose={() => setPermissionsPresented(false)} anchorRef={permissionsRef} />
      <EffortPopover
        open={effortPresented}
        onClose={() => setEffortPresented(false)}
        anchorRef={effortRef}
        current={effort}
        offered={effortLevels(modelInfo?.effortLevels)}
        onSelect={setEffort}
      />
      <ModelPickerPopover open={pickerVisible} onClose={() => useApp.getState().setModelPickerVisible(false)} anchorRef={modelRef} />
    </div>
  )
}

/** The mirror's text: the draft with every mention range in accent. */
function renderTinted(text: string, ranges: readonly { start: number; end: number }[]): ReactNode[] {
  const out: ReactNode[] = []
  let cursor = 0
  ranges.forEach((range, index) => {
    if (range.start > cursor) out.push(text.slice(cursor, range.start))
    out.push(
      <span key={index} className={styles.mention}>
        {text.slice(range.start, range.end)}
      </span>,
    )
    cursor = range.end
  })
  out.push(text.slice(cursor) + '\u200b')
  return out
}

// MARK: - Bottom row

interface ChipButtonProps {
  buttonRef: RefObject<HTMLButtonElement | null>
}

/** The permissions chip: opens `PermissionsPopover`; Bypass is tinted as a warning. */
function PermissionsChip({ buttonRef, profile, onClick }: ChipButtonProps & { profile: string; onClick: () => void }) {
  const label = approvalModeLabel(profile)
  const tint = profile === 'bypass_permissions' ? 'var(--warning)' : 'var(--text-secondary)'
  return (
    <Tooltip content={`Permissions: ${label} · click to change`}>
      <button ref={buttonRef} type="button" className={styles.permissionsChip} style={{ color: tint }} aria-label={`Permissions ${label}`} onClick={onClick}>
        <Icon name={permissionProfileIcon(profile)} size={11} strokeWidth={2} />
        <span className={styles.permissionsLabel}>{label}</span>
        <Chevron />
      </button>
    </Tooltip>
  )
}

/// Reasoning effort (`model.reasoning_effort`, all projects; idle projects
/// pick it up at once, a running task keeps its own). Opens `EffortPopover`;
/// Ultra (which also deploys sub-agents) shows in `--ultra` purple.
function EffortChip({ buttonRef, effort, saving, disabled, onClick }: ChipButtonProps & { effort: string | null; saving: boolean; disabled: boolean; onClick: () => void }) {
  const level = effortLevel(effort)
  const ultra = effortIsUltra(level)
  const icon: IconName = ultra ? 'agents' : 'effort'
  return (
    <Tooltip content="Reasoning effort for every project · Ultra also lets the model deploy sub-agents that explore and implement in parallel">
      <button ref={buttonRef} type="button" className={styles.plainChip} disabled={disabled} aria-label={`Reasoning effort ${level.title}`} onClick={onClick}>
        {saving ? (
          <BrailleSpinner size={11} tint="var(--text-secondary)" />
        ) : (
          <span className={styles.chipIcon} style={{ color: ultra ? 'var(--ultra)' : 'var(--text-secondary)' }}>
            <Icon name={icon} size={11} strokeWidth={2} />
          </span>
        )}
        <span className={styles.chipTitle} style={{ color: effortTint(level), fontWeight: ultra ? 600 : 500 }}>
          {level.title}
        </span>
        <Chevron />
      </button>
    </Tooltip>
  )
}

/// The model chip: the project's model with its lab logo and the gateway's
/// savings; "Switching…" with a spinner while `models/switch` runs. Opens
/// `ModelPickerPopover` through `modelPickerVisible`, so mod+Shift+M and the
/// palette open the same popover.
function ModelChip({ buttonRef, project, model, switching }: ChipButtonProps & { project: ProjectState; model: string; switching: boolean }) {
  const app = useApp.getState()
  const name = app.modelDisplayName(model)
  const stat = app.modelStat(model, project)
  const savings = stat ? modelStatSavingsLabel(stat) : null
  return (
    <Tooltip content="Project model">
      <button
        ref={buttonRef}
        type="button"
        className={styles.plainChip}
        aria-label={`Model ${name}` + (savings !== null ? `, ${savings}` : '')}
        onClick={() => {
          const state = useApp.getState()
          state.setModelPickerVisible(!state.modelPickerVisible)
        }}
      >
        {switching ? <BrailleSpinner size={11} tint="var(--text-secondary)" /> : <ModelLogo modelId={model} size={13} />}
        <span className={styles.modelName}>{switching ? 'Switching…' : name}</span>
        {!switching && savings !== null ? <span className={styles.savings}>{savings}</span> : null}
        <Chevron />
      </button>
    </Tooltip>
  )
}

function Chevron() {
  return (
    <span className={styles.chevron} aria-hidden="true">
      <Icon name="chevronDown" size={11} strokeWidth={2.25} />
    </span>
  )
}

interface PrimaryButtonProps {
  action: PrimaryAction
  cancelling: boolean
  canSubmit: boolean
  switching: boolean
  onSubmit: () => void
}

/// Send (32 px `accentSoft` circle with the accent arrow), a stop square
/// while this chat runs, a clock while its run is still settling. The circle
/// stays solid; only the glyph dims while inert.
function PrimaryButton({ action, cancelling, canSubmit, switching, onSubmit }: PrimaryButtonProps) {
  const isCancelling = action === 'stop' && cancelling
  const disabled = action === 'stop' ? cancelling : !canSubmit
  const label =
    action === 'send'
      ? switching
        ? 'Send (unavailable while switching the model)'
        : `Send (${RETURN_KEY})`
      : action === 'stop'
        ? cancelling
          ? 'Cancelling…'
          : `Stop the running task (${formatShortcut('mod+.')})`
        : 'Queue (this chat is still finishing its run)'
  const icon: IconName = action === 'send' ? 'send' : action === 'stop' ? 'stop' : 'queued'
  return (
    <Tooltip content={label}>
      <button
        type="button"
        className={styles.primary}
        disabled={disabled}
        aria-label={label}
        onClick={() => {
          if (action === 'stop') void useApp.getState().cancelRunningTask()
          else onSubmit()
        }}
      >
        {isCancelling ? (
          <BrailleSpinner size={12} tint="var(--accent)" />
        ) : (
          <Icon name={icon} size={PRIMARY_GLYPH[action]} strokeWidth={action === 'send' ? 2.5 : 2.25} style={{ opacity: disabled ? 0.45 : 1 }} />
        )}
      </button>
    </Tooltip>
  )
}
