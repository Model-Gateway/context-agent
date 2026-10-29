// Port of the state half of apps/macos/ContextAgent/App/ChatState.swift: one
// chat session's live state. The transcript itself is written by the
// `TranscriptReducer` (models/reducer.ts, the reducer half of ChatState.swift)
// that this class hosts: `apply`, `finish`, `loadHistory` and
// `prependHistory` run the reducer and then `touch()` the chat so the views
// subscribed to it re-render (apps/desktop/ARCHITECTURE.md "Stores").
//
// The class is mutated in place, like the Swift `@Observable` class. Every
// change ends with `touch()`, which bumps `version` and tells the app store
// (state/app.ts wires `ChatState.changeSink`), which bumps the owning
// project's version and the store's. Code outside the class changes fields
// through `update(change)` (or the small setters), which touches for it.

import type { ApprovalRequest, CompactResult, ContextStats, QuestionRequest, SessionSummary, TaskFinished, TokenUsage, AgentEvent, SessionHistory } from '../backend/types'
import type { ComposerAttachment } from '../models/attachments'
import {
  TranscriptReducer,
  chatStatusIsRunning,
  type ChatStatus,
  type LogLine,
  type ReducerTarget,
  type Unseen,
} from '../models/reducer'
import { Transcript, type AgentGroup, type AgentRun, type AssistantTurn, type OutgoingPrompt, type ToolGroup, type ToolGroupTerminal } from '../models/transcript'

export type { ChatStatus, LogLine, Unseen, RunProgress } from '../models/reducer'
export type { OutgoingPrompt } from '../models/transcript'

/// `ChatState` (App/ChatState.swift): per-session live state: the committed
/// transcript, the streaming assistant turn, the open tool group, status,
/// queue, steering, the composer draft and the Inspector log lines. One
/// instance per session, created by `ProjectState.chat(sessionId)` and never
/// replaced.
export class ChatState implements ReducerTarget {
  /** `ChatState.maxLogLines`. */
  static readonly maxLogLines = TranscriptReducer.maxLogLines
  /** `ChatState.deltaFlushInterval` (33 ms), applied by the reducer. */
  static readonly deltaFlushIntervalMs = TranscriptReducer.deltaFlushIntervalMs

  /// Called after every `touch()` of any chat; state/app.ts sets it to bump
  /// the chat's project and the store. Null outside the app (tests).
  static changeSink: ((chat: ChatState) => void) | null = null

  readonly sessionId: string
  readonly projectId: string

  summary: SessionSummary
  /** Committed items. */
  transcript = new Transcript()
  /** The streaming assistant turn (not yet in `transcript`). */
  live: AssistantTurn | null = null
  /** The open tool group of the current run (not yet in `transcript`). */
  liveTools: ToolGroup | null = null
  /// The sub-agents of the current run that are still running (not yet in
  /// `transcript`); committed once every agent in the group has reported.
  liveAgents: AgentGroup | null = null
  status: ChatStatus = { kind: 'idle' }
  usage: TokenUsage | null = null
  costUsd: number | null = null
  contextStats: ContextStats | null = null
  /** True while `session/compact` is in flight. */
  compacting = false
  pendingApproval: ApprovalRequest | null = null
  /** True while `approval/respond` is in flight. */
  approvalInFlight = false
  /** Inline failure of the last `approval/respond`. */
  approvalError: string | null = null
  /// Questions of the run's `ask_user` tool, waiting for the user
  /// (`question/request`); answered from the card above the composer.
  pendingQuestion: QuestionRequest | null = null
  /** True while `question/respond` is in flight. */
  questionInFlight = false
  /** Inline failure of the last `question/respond`. */
  questionError: string | null = null
  /** True from `task/cancel` until `task/finished` (status bar spinner). */
  cancelRequested = false
  draft = ''
  /** Files and folders attached to `draft` (composer chips). */
  attachments: ComposerAttachment[] = []
  /// `suggest/next` after a completed run: ghost text in the empty composer,
  /// → accepts it. Cleared when the user types, sends or a run starts.
  suggestion: string | null = null
  queued: OutgoingPrompt[] = []
  /// Sent to the running turn (`task/steer`) and not read by the model yet;
  /// each `steered` event moves the first into the transcript.
  steering: OutgoingPrompt[] = []
  /** Set when the queue became non-empty (dequeue order across chats). */
  queuedSince: Date | null = null
  changedPaths: Set<string> = new Set()
  unseen: Unseen | null = null
  lastError: string | null = null
  lastErrorKind: string | null = null
  log: LogLine[] = []
  /** The optimistic user item appended by `send()`. */
  pendingUserItemId: string | null = null
  /** Inline error when `session/history` could not be loaded. */
  historyError: string | null = null
  /// The user item being edited in place (`UserMessageView` shows an editor
  /// instead of the bubble) and its text.
  editingTurnId: string | null = null
  editingText = ''
  /// The sub-agent shown in the Inspector Agents tab; null follows the
  /// latest running (else latest) agent.
  inspectedAgentId: string | null = null
  /// The agent `inspectedAgentId == null` follows. Written by the reducer
  /// (refreshed when an agent starts or finishes) so that what reads it,
  /// every agent row in the transcript, does not depend on each streamed
  /// agent event.
  followedAgentId: string | null = null

  /** Whether this chat is on screen in the active app (AppState sets it). */
  isVisible: () => boolean = () => true

  /** Bumped by every `touch()`; views subscribe to it (`useChat`). */
  version = 0

  /// The reducer writing into this chat. Its 33 ms delta flush touches the
  /// chat so the streaming text redraws about 30 times a second.
  readonly reducer: TranscriptReducer

  constructor(summary: SessionSummary, projectId: string, sessionId: string = summary.id) {
    this.sessionId = sessionId
    this.projectId = projectId
    this.summary = summary
    this.reducer = new TranscriptReducer(this, { onFlush: () => this.touch() })
  }

  // MARK: - Observation

  /** `id` (`Identifiable`): the session id. */
  get id(): string {
    return this.sessionId
  }

  /** Marks the chat changed: bumps `version` and tells the store. */
  touch(): void {
    this.version += 1
    ChatState.changeSink?.(this)
  }

  /** Runs `change` on the chat, then touches it. */
  update(change: (chat: ChatState) => void): void {
    change(this)
    this.touch()
  }

  /** The composer's text binding. */
  setDraft(text: string): void {
    if (text === this.draft) return
    this.draft = text
    this.touch()
  }

  /** The composer's chips. */
  setAttachments(attachments: ComposerAttachment[]): void {
    this.attachments = attachments
    this.touch()
  }

  /** The ghost text (null clears it). */
  setSuggestion(text: string | null): void {
    if (text === this.suggestion) return
    this.suggestion = text
    this.touch()
  }

  /** The inline editor's text binding. */
  setEditingText(text: string): void {
    if (text === this.editingText) return
    this.editingText = text
    this.touch()
  }

  /** The Inspector Agents tab's selection (null follows the latest agent). */
  setInspectedAgent(agentId: string | null): void {
    if (agentId === this.inspectedAgentId) return
    this.inspectedAgentId = agentId
    this.touch()
  }

  // MARK: - Derived

  get title(): string {
    return this.summary.title
  }

  get isRunning(): boolean {
    return chatStatusIsRunning(this.status)
  }

  get hasContent(): boolean {
    return !this.transcript.isEmpty || this.live !== null || this.liveTools !== null || this.liveAgents !== null
  }

  /** The task of the run in progress (set by `task_started`). */
  get currentTaskId(): string | null {
    return this.reducer.currentTaskId
  }

  /// The run is waiting on the user (an approval or questions), or it ended
  /// blocked and nobody has looked yet: the sidebar's orange dot.
  get needsHelp(): boolean {
    const unseen = this.unseen
    return (
      this.pendingApproval !== null ||
      this.pendingQuestion !== null ||
      unseen?.kind === 'approval' ||
      (unseen?.kind === 'finished' && unseen.status === 'blocked')
    )
  }

  /** Every sub-agent of the session in deployment order, live ones last. */
  get allAgents(): AgentRun[] {
    const agents: AgentRun[] = []
    for (const item of this.transcript.items) {
      if (item.kind === 'agents') agents.push(...item.agents)
    }
    if (this.liveAgents) agents.push(...this.liveAgents.agents)
    return agents
  }

  /** The agent the Inspector Agents tab shows. */
  get inspectedAgent(): AgentRun | null {
    if (this.inspectedAgentId !== null) {
      const agent = this.agent(this.inspectedAgentId)
      if (agent) return agent
    }
    return this.followedAgentId !== null ? this.agent(this.followedAgentId) : null
  }

  /// The id of the agent the Inspector shows (or would show when it is
  /// open), without reading any agent's progress.
  get shownAgentId(): string | null {
    return this.inspectedAgentId ?? this.followedAgentId
  }

  private agent(id: string): AgentRun | null {
    const live = this.liveAgents?.agents
    if (live) {
      for (let i = live.length - 1; i >= 0; i -= 1) {
        const agent = live[i]
        if (agent && agent.id === id) return agent
      }
    }
    const location = this.transcript.locateAgent(id)
    if (!location) return null
    const item = this.transcript.at(location.item)
    return item && item.kind === 'agents' ? (item.agents[location.agent] ?? null) : null
  }

  // MARK: - Queue

  enqueue(prompt: OutgoingPrompt): void {
    if (this.queued.length === 0) this.queuedSince = new Date()
    this.queued = [...this.queued, prompt]
    this.touch()
  }

  /** Removes and returns the first queued prompt. */
  dequeueFirst(): OutgoingPrompt | null {
    const [first, ...rest] = this.queued
    if (first === undefined) return null
    this.queued = rest
    if (rest.length === 0) this.queuedSince = null
    this.touch()
    return first
  }

  removeQueued(index: number): void {
    if (index < 0 || index >= this.queued.length) return
    this.queued = this.queued.filter((_, i) => i !== index)
    if (this.queued.length === 0) this.queuedSince = null
    this.touch()
  }

  /** `queued.insert(prompt, at: 0)` with `queuedSince` set when it was empty. */
  requeueFirst(prompt: OutgoingPrompt): void {
    this.queued = [prompt, ...this.queued]
    if (this.queuedSince === null) this.queuedSince = new Date()
    this.touch()
  }

  // MARK: - Reducer

  /// `apply(_:date:)`: one runtime event through the reducer. Returns false
  /// when the event was only buffered (a text delta); the 33 ms flush
  /// touches the chat then.
  apply(event: AgentEvent, date: Date = new Date()): boolean {
    const changed = this.reducer.apply(event, date)
    if (changed) this.touch()
    return changed
  }

  /// `finish(_:terminal:date:)`: `task/finished` closes everything the run
  /// left open. `terminal` overrides how an open tool group is closed
  /// (backend exit → `stopped`).
  finish(finished: TaskFinished, terminal: ToolGroupTerminal | null = null, date: Date = new Date()): void {
    this.reducer.finish(finished, terminal, date)
    this.touch()
  }

  /// `loadHistory(_:)`: rebuilds the transcript from `session/history`.
  loadHistory(history: SessionHistory): void {
    this.reducer.loadHistory(history)
    this.touch()
  }

  /// `prependHistory(_:)`: history that arrived after a run began in this
  /// chat goes in front of what the run has shown so far.
  prependHistory(history: SessionHistory): void {
    this.reducer.prependHistory(history)
    this.touch()
  }

  /** Moves the buffered deltas into `live` (also done before any commit). */
  flushPending(): void {
    this.reducer.flushPending()
    this.touch()
  }

  // MARK: - Log

  /** `addLog(_:date:level:)`: one Inspector Log line. */
  addLog(text: string, date: Date = new Date(), level = 'info'): void {
    this.reducer.addLog(text, date, level)
    this.touch()
  }

  /** `applyCompaction(_:)`: see `TranscriptReducer.applyCompaction`. */
  applyCompaction(result: CompactResult): void {
    this.reducer.applyCompaction(result)
    this.touch()
  }

  clearLog(): void {
    this.reducer.clearLog()
    this.touch()
  }

  /** Stops the pending delta flush (the chat is going away). */
  dispose(): void {
    this.reducer.dispose()
  }

  // MARK: - Static helpers

  /** `ChatState.clip(_:_:)`: newlines folded to spaces, capped at `limit` characters with an ellipsis. */
  static clip(text: string, limit: number): string {
    return TranscriptReducer.clip(text, limit)
  }
}
