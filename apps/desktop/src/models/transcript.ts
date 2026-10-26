// Port of apps/macos/ContextAgent/Models/Transcript.swift: the rendered
// conversation of one chat session. `TranscriptReducer` (models/reducer.ts)
// turns `AgentEvent`s into these items; the views in views/Chat render them.
// Everything here is plain data plus the `Transcript` container with the
// indexes the reducer needs.
//
// Swift's enum-with-payload `TranscriptItem` is a discriminated union on
// `kind` whose members *are* the payload records (so `item.id` works on
// every variant and views switch on `item.kind`). Updates through
// `Transcript.update*` replace the item object (value semantics), so memoised
// rows re-render only when their own item changed.

import {
  agentModeLabel,
  jsonArray,
  jsonObject,
  jsonParse,
  jsonString,
  taskStatusLabel,
  type AgentMode,
  type AgentReport,
  type ApprovalDecision,
  type ApprovalRequest,
  type FileUpload,
  type JSONValue,
  type SentPrompt,
  type TaskOutcome,
  type TaskStatus,
} from '../backend/types'

// MARK: - Ids

/** `UUID().uuidString`: a fresh row identity. */
export function newItemId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID().toUpperCase()
  } catch {
    // Fall through to the arithmetic id.
  }
  const hex = () => Math.floor(Math.random() * 0x10000).toString(16).padStart(4, '0')
  return `${hex()}${hex()}-${hex()}-${hex()}-${hex()}-${hex()}${hex()}${hex()}`.toUpperCase()
}

// MARK: - Outgoing prompt

/// `OutgoingPrompt` (App/ChatState.swift): a message on its way to
/// `task/run`: the text and the image and text files sent with it
/// (`prepareAttachmentPrompt` in models/attachments.ts).
export interface OutgoingPrompt {
  text: string
  images: string[]
  files: FileUpload[]
}

/** `OutgoingPrompt(text:)` with no attachments. */
export function outgoingPrompt(text: string, images: string[] = [], files: FileUpload[] = []): OutgoingPrompt {
  return { text, images, files }
}

// MARK: - Items

/** `UserTurn`: a message the user (or the runtime) sent. */
export interface UserTurn {
  kind: 'user'
  id: string
  text: string
  /** Image and text files sent with the message (they may since be gone). */
  images: string[]
  files: FileUpload[]
  date: Date
  /** Injected by the runtime (`[Context runtime] …`), rendered muted. */
  isRuntime: boolean
  /// The persisted conversation id (`session/history`, `task/finished`);
  /// null while the run that stores it is in flight. The handle for
  /// `session/rewind` (edit this message / undo from here).
  messageId: number | null
  /// A working-tree snapshot exists for the run this message started, so
  /// a rewind can revert the files.
  hasSnapshot: boolean
  /// Sent to a running turn (`task/steer`); its `messageId` comes with
  /// the next `session/history`, not with `task/finished`.
  isSteering: boolean
  /** Developer mode: the full prompt of the run's first model request. */
  sentPrompt: SentPrompt | null
}

/** `UserTurn(text:…)` with the Swift defaults. */
export function makeUserTurn(fields: Partial<Omit<UserTurn, 'kind'>> & { text: string }): UserTurn {
  return {
    kind: 'user',
    id: fields.id ?? newItemId(),
    text: fields.text,
    images: fields.images ?? [],
    files: fields.files ?? [],
    date: fields.date ?? new Date(),
    isRuntime: fields.isRuntime ?? false,
    messageId: fields.messageId ?? null,
    hasSnapshot: fields.hasSnapshot ?? false,
    isSteering: fields.isSteering ?? false,
    sentPrompt: fields.sentPrompt ?? null,
  }
}

/** `UserTurn.isRewindable`: editing and undo need the persisted id of a message the user typed. */
export function userTurnIsRewindable(turn: UserTurn): boolean {
  return turn.messageId !== null && !turn.isRuntime
}

/** `AssistantTurn`: one model reply (streamed, then final). */
export interface AssistantTurn {
  kind: 'assistant'
  id: string
  /** Markdown; grows while streaming, replaced by the final `model_response` text. */
  text: string
  /** Reasoning / "thinking" text streamed before the answer. Empty when the model exposes none. */
  reasoning: string
  isStreaming: boolean
  /** True while reasoning deltas arrive and no text delta has arrived yet. */
  isThinking: boolean
  step: number
  thinkingStarted: Date | null
  /** Seconds (Swift `TimeInterval`). */
  thinkingDuration: number | null
  date: Date
}

/** `AssistantTurn(step:date:)` with the Swift defaults. */
export function makeAssistantTurn(fields: Partial<Omit<AssistantTurn, 'kind'>> = {}): AssistantTurn {
  return {
    kind: 'assistant',
    id: fields.id ?? newItemId(),
    text: fields.text ?? '',
    reasoning: fields.reasoning ?? '',
    isStreaming: fields.isStreaming ?? true,
    isThinking: fields.isThinking ?? false,
    step: fields.step ?? 0,
    thinkingStarted: fields.thinkingStarted ?? null,
    thinkingDuration: fields.thinkingDuration ?? null,
    date: fields.date ?? new Date(),
  }
}

/** `AssistantTurn.isEmpty`. */
export function assistantTurnIsEmpty(turn: AssistantTurn): boolean {
  return turn.text.length === 0 && turn.reasoning.length === 0
}

/** `ToolCallStatus`. */
export type ToolCallStatus = 'running' | 'succeeded' | 'failed'

/** `ToolOutputFormat` (raw values are the labels the output view shows). */
export type ToolOutputFormat = 'JSON' | 'stdout' | 'diff' | 'text'

/** `ToolOutput`: what a tool returned, as the model saw it. */
export interface ToolOutput {
  content: string
  fullLength: number
  isError: boolean
  format: ToolOutputFormat
}

/** `ToolOutput.init(content:fullLength:isError:tool:)`: infers the format. */
export function makeToolOutput(content: string, fullLength: number, isError: boolean, tool: string): ToolOutput {
  return { content, fullLength, isError, format: inferToolOutputFormat(tool, content) }
}

/// `ToolOutput.omittedCharacters`: characters the model never saw
/// (truncated by the runtime). `full_len` is the runtime's byte count, so it
/// is compared with bytes, not with characters (any non-ASCII output would
/// otherwise look truncated).
export function toolOutputOmittedCharacters(output: ToolOutput): number {
  return Math.max(0, output.fullLength - utf8Length(output.content))
}

/** `ToolOutput.inferFormat(tool:content:)`. */
export function inferToolOutputFormat(tool: string, content: string): ToolOutputFormat {
  const trimmed = content.trim()
  if (tool === 'git_diff' || tool === 'apply_patch') return 'diff'
  if (trimmed.startsWith('diff --git') || trimmed.startsWith('@@') || trimmed.startsWith('--- ')) return 'diff'
  if (
    ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) &&
    jsonParse(trimmed) !== null
  ) {
    return 'JSON'
  }
  if (tool === 'execute_command' || tool === 'run_validation') return 'stdout'
  return 'text'
}

/** `String.utf8.count`. */
export function utf8Length(text: string): number {
  let bytes = 0
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code < 0x80) bytes += 1
    else if (code < 0x800) bytes += 2
    else if (code >= 0xd800 && code <= 0xdbff) {
      // A surrogate pair encodes as four bytes.
      bytes += 4
      i += 1
    } else bytes += 3
  }
  return bytes
}

/** `ToolCallItem`: one row of a tool timeline. */
export interface ToolCallItem {
  /// Row identity (`ForEach`, expansion state). Unique per row: call ids
  /// such as `call_0` repeat across steps and runs.
  id: string
  /** The runtime's `call_id`. */
  callId: string
  name: string
  arguments: JSONValue
  status: ToolCallStatus
  summary: string
  durationMs: number | null
  output: ToolOutput | null
  startedAt: Date
  /** Approval request raised by this call, when the policy asked for one. */
  approvalRequestId: string | null
}

/** `ToolCallItem(callId:name:arguments:)` with the Swift defaults. */
export function makeToolCallItem(fields: Partial<ToolCallItem> & { callId: string; name: string; arguments: JSONValue }): ToolCallItem {
  return {
    id: fields.id ?? newItemId(),
    callId: fields.callId,
    name: fields.name,
    arguments: fields.arguments,
    status: fields.status ?? 'running',
    summary: fields.summary ?? '',
    durationMs: fields.durationMs ?? null,
    output: fields.output ?? null,
    startedAt: fields.startedAt ?? new Date(),
    approvalRequestId: fields.approvalRequestId ?? null,
  }
}

const TARGET_KEYS = ['path', 'command', 'query', 'name', 'symbol', 'pattern', 'kind', 'directory', 'glob']

/** `ToolCallItem.target`: best-effort primary argument for the row label ("src/app.rs", "cargo test"). */
export function toolCallTarget(call: ToolCallItem): string | null {
  const object = jsonObject(call.arguments)
  if (!object) return null
  for (const key of TARGET_KEYS) {
    const value = jsonString(object[key])
    if (value !== null && value.length > 0) return value
  }
  for (const value of Object.values(object)) {
    if (typeof value === 'string') return value
  }
  return null
}

/** `ToolGroupTerminal`: how a tool group ended. */
export type ToolGroupTerminal = 'done' | 'stopped' | 'failed' | 'blocked'

/** `ToolGroupTerminal(status:)` / `ChatState.terminal(for:)`. */
export function terminalForStatus(status: TaskStatus): ToolGroupTerminal {
  switch (status) {
    case 'completed':
      return 'done'
    case 'cancelled':
      return 'stopped'
    case 'failed':
      return 'failed'
    case 'blocked':
      return 'blocked'
  }
}

/** `ToolGroup`: consecutive tool calls of one run, one timeline card. */
export interface ToolGroup {
  kind: 'tools'
  id: string
  calls: ToolCallItem[]
  filesChanged: Set<string>
  commandsRun: number
  startedAt: Date
  endedAt: Date | null
  /** Set when the task finished; the view renders the "Done" row. */
  terminal: ToolGroupTerminal | null
}

/** `ToolGroup(startedAt:)` with the Swift defaults. */
export function makeToolGroup(fields: Partial<Omit<ToolGroup, 'kind'>> = {}): ToolGroup {
  return {
    kind: 'tools',
    id: fields.id ?? newItemId(),
    calls: fields.calls ?? [],
    filesChanged: fields.filesChanged ?? new Set(),
    commandsRun: fields.commandsRun ?? 0,
    startedAt: fields.startedAt ?? new Date(),
    endedAt: fields.endedAt ?? null,
    terminal: fields.terminal ?? null,
  }
}

/** `ToolGroup.isOpen`. */
export function toolGroupIsOpen(group: ToolGroup): boolean {
  return group.terminal === null
}

/** `ToolGroup.runningCount`. */
export function toolGroupRunningCount(group: ToolGroup): number {
  return group.calls.filter((call) => call.status === 'running').length
}

/** `ToolGroup.failedCount`. */
export function toolGroupFailedCount(group: ToolGroup): number {
  return group.calls.filter((call) => call.status === 'failed').length
}

/** `ToolGroup.headline`: "18 tools, edited 5 files, ran 2 commands". */
export function toolGroupHeadline(group: ToolGroup): string {
  const parts = [`${group.calls.length} tool${group.calls.length === 1 ? '' : 's'}`]
  if (group.filesChanged.size > 0) parts.push(`edited ${group.filesChanged.size} file${group.filesChanged.size === 1 ? '' : 's'}`)
  if (group.commandsRun > 0) parts.push(`ran ${group.commandsRun} command${group.commandsRun === 1 ? '' : 's'}`)
  return parts.join(', ')
}

/** `ToolGroup.elapsed`, in seconds. */
export function toolGroupElapsed(group: ToolGroup, now: Date = new Date()): number {
  return ((group.endedAt ?? now).getTime() - group.startedAt.getTime()) / 1000
}

/** `AgentRunStatus`. */
export type AgentRunStatus =
  | { kind: 'running' }
  | { kind: 'finished'; status: TaskStatus }
  /** The lead task ended (or the backend went away) before the agent reported. */
  | { kind: 'stopped' }

/** `AgentRunStatus.isRunning`. */
export function agentStatusIsRunning(status: AgentRunStatus): boolean {
  return status.kind === 'running'
}

/// `AgentRun`: one sub-agent deployed by the Ultra effort tier: its own
/// tool calls, its latest streamed text while it runs, and its report once
/// done.
export interface AgentRun {
  /** The runtime's agent id (`agent-1a2b3c4d`), unique per run. */
  id: string
  agentTaskId: string
  name: string
  mode: AgentMode
  objective: string
  status: AgentRunStatus
  calls: ToolCallItem[]
  filesChanged: Set<string>
  commandsRun: number
  /** What the agent is currently saying (its streamed text, replaced each step). */
  liveText: string
  report: AgentReport | null
  startedAt: Date
  endedAt: Date | null
}

/** `AgentRun(id:agentTaskId:name:mode:objective:)` with the Swift defaults. */
export function makeAgentRun(
  fields: Partial<AgentRun> & { id: string; agentTaskId: string; name: string; mode: AgentMode; objective: string },
): AgentRun {
  return {
    id: fields.id,
    agentTaskId: fields.agentTaskId,
    name: fields.name,
    mode: fields.mode,
    objective: fields.objective,
    status: fields.status ?? { kind: 'running' },
    calls: fields.calls ?? [],
    filesChanged: fields.filesChanged ?? new Set(),
    commandsRun: fields.commandsRun ?? 0,
    liveText: fields.liveText ?? '',
    report: fields.report ?? null,
    startedAt: fields.startedAt ?? new Date(),
    endedAt: fields.endedAt ?? null,
  }
}

/** `AgentRun.runningCalls`. */
export function agentRunRunningCalls(agent: AgentRun): number {
  return agent.calls.filter((call) => call.status === 'running').length
}

/** `AgentRun.elapsed`, in seconds. */
export function agentRunElapsed(agent: AgentRun, now: Date = new Date()): number {
  return ((agent.endedAt ?? now).getTime() - agent.startedAt.getTime()) / 1000
}

/** `AgentRun.toolGroup`: the agent's tool calls as a tool group, for the shared timeline view. */
export function agentRunToolGroup(agent: AgentRun): ToolGroup {
  let terminal: ToolGroupTerminal | null
  switch (agent.status.kind) {
    case 'running':
      terminal = null
      break
    case 'finished':
      terminal = terminalForStatus(agent.status.status)
      break
    case 'stopped':
      terminal = 'stopped'
      break
  }
  return {
    kind: 'tools',
    id: agent.id + '.tools',
    calls: agent.calls,
    filesChanged: agent.filesChanged,
    commandsRun: agent.commandsRun,
    startedAt: agent.startedAt,
    endedAt: agent.endedAt,
    terminal,
  }
}

/// `AgentGroup`: sub-agents deployed together (one `spawn_agents` call):
/// shown as one card with a row per agent.
export interface AgentGroup {
  kind: 'agents'
  id: string
  agents: AgentRun[]
  startedAt: Date
  endedAt: Date | null
}

/** `AgentGroup(startedAt:)` with the Swift defaults. */
export function makeAgentGroup(fields: Partial<Omit<AgentGroup, 'kind'>> = {}): AgentGroup {
  return {
    kind: 'agents',
    id: fields.id ?? newItemId(),
    agents: fields.agents ?? [],
    startedAt: fields.startedAt ?? new Date(),
    endedAt: fields.endedAt ?? null,
  }
}

/** `AgentGroup.isOpen`. */
export function agentGroupIsOpen(group: AgentGroup): boolean {
  return group.agents.some((agent) => agentStatusIsRunning(agent.status))
}

/** `AgentGroup.runningCount`. */
export function agentGroupRunningCount(group: AgentGroup): number {
  return group.agents.filter((agent) => agentStatusIsRunning(agent.status)).length
}

/** `AgentGroup.failedCount`: finished failed, or stopped. */
export function agentGroupFailedCount(group: AgentGroup): number {
  return group.agents.filter((agent) =>
    agent.status.kind === 'finished' ? agent.status.status === 'failed' : agent.status.kind === 'stopped',
  ).length
}

/** `AgentGroup.elapsed`, in seconds. */
export function agentGroupElapsed(group: AgentGroup, now: Date = new Date()): number {
  return ((group.endedAt ?? now).getTime() - group.startedAt.getTime()) / 1000
}

/** `AgentGroup.headline`: "3 agents, 2 running" / "3 agents, done" / "3 agents, 1 failed". */
export function agentGroupHeadline(group: AgentGroup): string {
  const parts = [`${group.agents.length} agent${group.agents.length === 1 ? '' : 's'}`]
  const running = agentGroupRunningCount(group)
  const failed = agentGroupFailedCount(group)
  if (running > 0) parts.push(`${running} running`)
  else if (failed > 0) parts.push(`${failed} failed`)
  else parts.push('done')
  const files = new Set<string>()
  for (const agent of group.agents) for (const path of agent.filesChanged) files.add(path)
  if (files.size > 0) parts.push(`edited ${files.size} file${files.size === 1 ? '' : 's'}`)
  return parts.join(', ')
}

/** `ApprovalItem`: an approval card, pending until `decision` is set. */
export interface ApprovalItem {
  kind: 'approval'
  /** `request.id`. */
  id: string
  request: ApprovalRequest
  decision: ApprovalDecision | null
  date: Date
}

/** `ApprovalItem(request:date:)`. */
export function makeApprovalItem(request: ApprovalRequest, date: Date = new Date(), decision: ApprovalDecision | null = null): ApprovalItem {
  return { kind: 'approval', id: request.id, request, decision, date }
}

/** `ApprovalItem.isPending`. */
export function approvalIsPending(item: ApprovalItem): boolean {
  return item.decision === null
}

/** `OutcomeItem`: the run's outcome card. */
export interface OutcomeItem {
  kind: 'outcome'
  id: string
  outcome: TaskOutcome
  date: Date
  /// The summary repeats the task's last assistant reply (the backend often
  /// uses the final message as the summary); the card then leaves it out.
  summaryRepeatsReply: boolean
}

/** `OutcomeItem(outcome:date:summaryRepeatsReply:)`. */
export function makeOutcomeItem(outcome: TaskOutcome, date: Date = new Date(), summaryRepeatsReply = false, id: string = newItemId()): OutcomeItem {
  return { kind: 'outcome', id, outcome, date, summaryRepeatsReply }
}

/** `NoticeItem`: checkpoint / warning / error lines. */
export interface NoticeItem {
  kind: 'notice'
  id: string
  /** Lines shown by default. */
  lines: string[]
  /** Low-value lines (task boundary notes, checkpoints) behind a disclosure. */
  detailLines: string[]
  isError: boolean
  date: Date
}

/** `NoticeItem(lines:detailLines:isError:date:)` with the Swift defaults. */
export function makeNoticeItem(fields: Partial<Omit<NoticeItem, 'kind'>> = {}): NoticeItem {
  return {
    kind: 'notice',
    id: fields.id ?? newItemId(),
    lines: fields.lines ?? [],
    detailLines: fields.detailLines ?? [],
    isError: fields.isError ?? false,
    date: fields.date ?? new Date(),
  }
}

/** `NoticeItem.isEmpty`. */
export function noticeIsEmpty(notice: NoticeItem): boolean {
  return notice.lines.length === 0 && notice.detailLines.length === 0
}

/** `TranscriptItem`: one row of the conversation. */
export type TranscriptItem = UserTurn | AssistantTurn | ToolGroup | AgentGroup | ApprovalItem | OutcomeItem | NoticeItem

/** `TranscriptItem.kind`'s values. */
export type TranscriptItemKind = TranscriptItem['kind']

/** `TranscriptItem.date`: `startedAt` for tool and agent groups. */
export function transcriptItemDate(item: TranscriptItem): Date {
  switch (item.kind) {
    case 'tools':
    case 'agents':
      return item.startedAt
    default:
      return item.date
  }
}

// MARK: - Transcript

/** `(item: Int, call: Int)` of `Transcript.locate(callId:)`. */
export interface CallLocation {
  item: number
  call: number
}

/** `(item: Int, agent: Int)` of `Transcript.locate(agentId:)`. */
export interface AgentLocation {
  item: number
  agent: number
}

/** `Transcript.rewindScope(from:)`'s result. */
export interface RewindScope {
  laterMessages: number
  changedFiles: string[]
}

/// `Transcript`: ordered items plus the indexes the reducer needs. Mutated
/// in place by the reducer; `items` is the live array (do not mutate it from
/// outside), every update replaces the item object it changes.
export class Transcript {
  private _items: TranscriptItem[] = []

  /** `items` (read-only view; the array is replaced on `prepend`/`removeAll`, else mutated). */
  get items(): readonly TranscriptItem[] {
    return this._items
  }

  get isEmpty(): boolean {
    return this._items.length === 0
  }

  get count(): number {
    return this._items.length
  }

  append(item: TranscriptItem): void {
    this._items.push(item)
  }

  removeLast(): void {
    if (this._items.length === 0) return
    this._items.pop()
  }

  removeAll(): void {
    this._items = []
  }

  /** Inserts earlier items (history that arrived after a run began) in front. */
  prepend(earlier: readonly TranscriptItem[]): void {
    if (earlier.length === 0) return
    this._items = [...earlier, ...this._items]
  }

  /** `transcript[index]` (getter). */
  at(index: number): TranscriptItem | undefined {
    return this._items[index]
  }

  /** `transcript[index] = item` (setter). */
  set(index: number, item: TranscriptItem): void {
    if (index < 0 || index >= this._items.length) return
    this._items[index] = item
  }

  get last(): TranscriptItem | null {
    return this._items[this._items.length - 1] ?? null
  }

  /// True when `summary` is (or is contained in) the last assistant reply
  /// since the most recent user turn, ignoring whitespace differences.
  repeatsLastReply(summary: string): boolean {
    const wanted = normalizedWhitespace(summary)
    if (wanted.length === 0) return false
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item) continue
      if (item.kind === 'assistant') {
        const reply = normalizedWhitespace(item.text)
        if (reply.length === 0) continue
        return reply === wanted || reply.includes(wanted)
      }
      if (item.kind === 'user' && !item.isRuntime) return false
    }
    return false
  }

  /** Index of the most recent assistant item, if it is the last item. */
  get trailingAssistantIndex(): number | null {
    const last = this.last
    if (!last || last.kind !== 'assistant') return null
    return this._items.length - 1
  }

  /** Index of the most recent assistant item anywhere after the last user turn. */
  get currentAssistantIndex(): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item) continue
      if (item.kind === 'assistant') return i
      if (item.kind === 'user') return null
    }
    return null
  }

  /** The open tool group of the current run, if any. */
  get openToolGroupIndex(): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item) continue
      if (item.kind === 'tools') return toolGroupIsOpen(item) ? i : null
      if (item.kind === 'user') return null
    }
    return null
  }

  /// The most recent tool group of the current run, open or already closed
  /// (a `file_changed` follows the `tool_call_finished` that closed an
  /// approval-split group).
  get currentToolGroupIndex(): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item) continue
      if (item.kind === 'tools') return i
      if (item.kind === 'user') return null
    }
    return null
  }

  /// Where a tool call lives: (item index, call index). Call ids repeat
  /// across steps, so the most recent row wins, preferring one that is
  /// still running (a finished row's updates have already arrived).
  locateCall(callId: string): CallLocation | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item || item.kind !== 'tools') continue
      const call = lastIndexWhere(item.calls, (c) => c.callId === callId && c.status === 'running') ?? lastIndexWhere(item.calls, (c) => c.callId === callId)
      if (call !== null) return { item: i, call }
    }
    return null
  }

  /** The open agent group of the current run, if any. */
  get openAgentGroupIndex(): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item) continue
      if (item.kind === 'agents') return agentGroupIsOpen(item) ? i : null
      if (item.kind === 'user') return null
    }
    return null
  }

  /** Where a sub-agent lives: (item index, agent index). */
  locateAgent(agentId: string): AgentLocation | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (!item || item.kind !== 'agents') continue
      const agent = lastIndexWhere(item.agents, (a) => a.id === agentId)
      if (agent !== null) return { item: i, agent }
    }
    return null
  }

  locateApproval(approvalId: string): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (item && item.kind === 'approval' && item.id === approvalId) return i
    }
    return null
  }

  locateUser(userId: string): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (item && item.kind === 'user' && item.id === userId) return i
    }
    return null
  }

  /** The last user item (newest first) matching `predicate`. */
  lastUserIndex(predicate: (turn: UserTurn) => boolean): number | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (item && item.kind === 'user' && predicate(item)) return i
    }
    return null
  }

  /** `update(userAt:_:)`: `change` edits a copy that replaces the item. */
  updateUser(index: number, change: (turn: UserTurn) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'user') return
    const copy: UserTurn = { ...item }
    change(copy)
    this._items[index] = copy
  }

  /** The newest message the user typed that can be rewound to (Undo Last Turn). */
  get lastRewindableUser(): UserTurn | null {
    const index = this.lastUserIndex(userTurnIsRewindable)
    if (index === null) return null
    const item = this._items[index]
    return item && item.kind === 'user' ? item : null
  }

  /// What a rewind to the user item at `index` removes: the later messages
  /// the user typed and the files changed from that item on (tool groups,
  /// sub-agents and outcomes), for the keep-or-revert sheet.
  rewindScope(index: number): RewindScope {
    const start = this._items[index]
    if (!start) return { laterMessages: 0, changedFiles: [] }
    let later = 0
    const files: string[] = []
    const seen = new Set<string>()
    const add = (paths: Iterable<string>) => {
      for (const path of paths) {
        if (!seen.has(path)) {
          seen.add(path)
          files.push(path)
        }
      }
    }
    for (let i = index; i < this._items.length; i += 1) {
      const item = this._items[i]
      if (!item) continue
      switch (item.kind) {
        case 'user':
          if (item.id !== start.id && !item.isRuntime) later += 1
          break
        case 'tools':
          add([...item.filesChanged].sort())
          break
        case 'agents':
          for (const agent of item.agents) add([...agent.filesChanged].sort())
          break
        case 'outcome':
          add(item.outcome.modified_files)
          break
        default:
          break
      }
    }
    return { laterMessages: later, changedFiles: files }
  }

  /** `update(assistantAt:_:)`. */
  updateAssistant(index: number, change: (turn: AssistantTurn) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'assistant') return
    const copy: AssistantTurn = { ...item }
    change(copy)
    this._items[index] = copy
  }

  /// `update(toolsAt:_:)`: the group and its `calls` array are copied so
  /// `change` may push to or replace entries of `group.calls`; replace a call
  /// object (`group.calls[i] = { ...group.calls[i], … }`) to change one.
  updateTools(index: number, change: (group: ToolGroup) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'tools') return
    const copy: ToolGroup = { ...item, calls: [...item.calls], filesChanged: new Set(item.filesChanged) }
    change(copy)
    this._items[index] = copy
  }

  /** `update(agentsAt:_:)`: the group and its `agents` array are copied. */
  updateAgents(index: number, change: (group: AgentGroup) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'agents') return
    const copy: AgentGroup = { ...item, agents: [...item.agents] }
    change(copy)
    this._items[index] = copy
  }

  /** `update(approvalAt:_:)`. */
  updateApproval(index: number, change: (item: ApprovalItem) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'approval') return
    const copy: ApprovalItem = { ...item }
    change(copy)
    this._items[index] = copy
  }

  /** `update(noticeAt:_:)`: the line arrays are copied. */
  updateNotice(index: number, change: (notice: NoticeItem) => void): void {
    const item = this._items[index]
    if (!item || item.kind !== 'notice') return
    const copy: NoticeItem = { ...item, lines: [...item.lines], detailLines: [...item.detailLines] }
    change(copy)
    this._items[index] = copy
  }

  /// All tool calls in order (for counts in the sidebar / outcome),
  /// including the ones sub-agents made.
  get allToolCalls(): ToolCallItem[] {
    const out: ToolCallItem[] = []
    for (const item of this._items) {
      if (item.kind === 'tools') out.push(...item.calls)
      else if (item.kind === 'agents') for (const agent of item.agents) out.push(...agent.calls)
    }
    return out
  }

  /** Text of the last assistant message, for previews. */
  get lastAssistantText(): string | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (item && item.kind === 'assistant' && item.text.length > 0) return item.text
    }
    return null
  }

  /** Text of the last user message (for Retry). */
  get lastUserText(): string | null {
    return this.lastUserPrompt?.text ?? null
  }

  /** The newest message the user sent, with its attachments (Retry). */
  get lastUserPrompt(): OutgoingPrompt | null {
    for (let i = this._items.length - 1; i >= 0; i -= 1) {
      const item = this._items[i]
      if (item && item.kind === 'user' && !item.isRuntime) return { text: item.text, images: item.images, files: item.files }
    }
    return null
  }

  /** Markdown export of the whole conversation (Copy Transcript / Export Chat). */
  markdown(title: string | null = null): string {
    let out = ''
    if (title !== null) out += `# ${title}\n\n`
    for (const item of this._items) {
      switch (item.kind) {
        case 'user':
          out += (item.isRuntime ? '> _runtime:_ ' : '**You:** ') + item.text + '\n\n'
          break
        case 'assistant':
          if (item.text.length > 0) out += item.text + '\n\n'
          break
        case 'tools':
          out += `<details><summary>${toolGroupHeadline(item)}</summary>\n\n`
          for (const call of item.calls) {
            const mark = call.status === 'failed' ? '✗' : call.status === 'running' ? '…' : '✓'
            out += `- ${mark} \`${call.name}\` ${toolCallTarget(call) ?? ''} — ${call.summary}\n`
          }
          out += '\n</details>\n\n'
          break
        case 'agents':
          out += `<details><summary>${agentGroupHeadline(item)}</summary>\n\n`
          for (const agent of item.agents) {
            let status: string
            switch (agent.status.kind) {
              case 'running':
                status = 'running'
                break
              case 'finished':
                status = agent.status.status
                break
              case 'stopped':
                status = 'stopped'
                break
            }
            out += `- **${agent.name}** (${agent.mode}) — ${status}, ${agent.calls.length} tool call${agent.calls.length === 1 ? '' : 's'}\n`
            if (agent.report && agent.report.summary.length > 0) {
              out += '\n  ' + agent.report.summary.replace(/\n/g, '\n  ') + '\n'
            }
          }
          out += '\n</details>\n\n'
          break
        case 'approval': {
          const decision = item.decision ?? 'pending'
          out += `_Approval ${decision}: ${item.request.summary}_\n\n`
          break
        }
        case 'outcome':
          out += `**${taskStatusLabel(item.outcome.status)}** — ${item.outcome.summary}\n`
          if (item.outcome.modified_files.length > 0) out += '\nModified: ' + item.outcome.modified_files.join(', ') + '\n'
          if (item.outcome.unresolved.length > 0) out += '\nUnresolved:\n' + item.outcome.unresolved.map((line) => `- ${line}`).join('\n') + '\n'
          out += '\n'
          break
        case 'notice':
          for (const line of item.lines) out += `_${line}_\n`
          if (item.lines.length > 0) out += '\n'
          break
      }
    }
    return out
  }
}

/** `Transcript.normalized`: whitespace runs collapsed to one space. */
function normalizedWhitespace(text: string): string {
  return text.split(/\s+/).filter((part) => part.length > 0).join(' ')
}

/** `Array.lastIndex(where:)`: null when nothing matches. */
export function lastIndexWhere<T>(array: readonly T[], predicate: (item: T) => boolean): number | null {
  for (let i = array.length - 1; i >= 0; i -= 1) {
    const item = array[i]
    if (item !== undefined && predicate(item)) return i
  }
  return null
}

/** `Array.firstIndex(where:)`: null when nothing matches. */
export function firstIndexWhere<T>(array: readonly T[], predicate: (item: T) => boolean): number | null {
  for (let i = 0; i < array.length; i += 1) {
    const item = array[i]
    if (item !== undefined && predicate(item)) return i
  }
  return null
}

/** `AgentMode.label` for a sub-agent row ("Explore"). */
export function agentRunModeLabel(agent: AgentRun): string {
  return agentModeLabel(agent.mode)
}

/** The `questions` of an `ask_user` call's arguments, for row labels and cards. */
export function toolCallQuestions(call: ToolCallItem): string[] {
  const object = jsonObject(call.arguments)
  const questions = jsonArray(object?.questions) ?? []
  return questions.flatMap((entry) => {
    const text = jsonString(jsonObject(entry)?.question)
    return text === null ? [] : [text]
  })
}

// MARK: - Formatting helpers shared by views

/** `TranscriptFormat`: the number / duration / date formats the chat views share. */
export const TranscriptFormat = {
  /** 42_100 → "42.1k", 1_200_000 → "1.2M" */
  tokens(n: number): string {
    if (n < 1000) return `${n}`
    if (n < 1_000_000) return n % 1000 < 50 ? `${Math.round(n / 1000)}k` : `${(n / 1000).toFixed(1)}k`
    return `${(n / 1_000_000).toFixed(1)}M`
  },

  /** 372 → "6m 12s", 5 → "5s", 0.8 → "<1s" ("0.0s" reads like a bug). `seconds` is a `TimeInterval`. */
  duration(seconds: number): string {
    if (seconds < 1) return '<1s'
    if (seconds < 60) return `${Math.trunc(seconds)}s`
    const whole = Math.trunc(seconds)
    const m = Math.trunc(whole / 60)
    const s = whole % 60
    if (m < 60) return `${m}m ${s}s`
    return `${Math.trunc(m / 60)}h ${m % 60}m`
  },

  /** 0 → "<1 ms", 12 → "12 ms", 2140 → "2s" */
  durationMs(ms: number): string {
    if (ms < 1) return '<1 ms'
    return ms < 1000 ? `${ms} ms` : TranscriptFormat.duration(ms / 1000)
  },

  cost(usd: number): string {
    return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`
  },

  /** "just now", "4m ago", "Yesterday", "Oct 12" */
  relative(date: Date, now: Date = new Date()): string {
    const delta = (now.getTime() - date.getTime()) / 1000
    if (delta < 60) return 'just now'
    if (delta < 3600) return `${Math.trunc(delta / 60)}m ago`
    if (delta < 86_400) return `${Math.trunc(delta / 3600)}h ago`
    if (delta < 172_800) return 'Yesterday'
    const sameYear = date.getFullYear() === now.getFullYear()
    const month = MONTHS[date.getMonth()] ?? ''
    return sameYear ? `${month} ${date.getDate()}` : `${month} ${date.getDate()}, ${date.getFullYear()}`
  },
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
