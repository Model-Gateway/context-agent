// Port of the `TranscriptReducer` part of apps/macos/ContextAgent/App/ChatState.swift
// (apps/macos/ARCHITECTURE.md "TranscriptReducer rules"): turns `AgentEvent`s
// into transcript items, the streaming assistant turn (`live`), the open tool
// group (`liveTools`) and the running sub-agents (`liveAgents`); rebuilds a
// chat from `session/history`; and closes a run on `task/finished`.
//
// The Swift reducer is part of the `ChatState` class. Here it is a class of
// its own that writes into a host object (`ReducerTarget`), so
// state/project.ts's `ChatState` hosts it:
//
//   class ChatState implements ReducerTarget { …; readonly reducer = new TranscriptReducer(this, { onFlush: () => touch(this) }) }
//
// Everything the reducer changes is replaced, never mutated in place
// (`live`, `liveTools`, `liveAgents`, `status`, `changedPaths`, `log`,
// `queued`, `steering`, and every transcript item through
// `Transcript.update*`), so memoised views re-render only for what changed.
// Text and reasoning deltas are buffered and flushed into `live` at most
// every 33 ms (`deltaFlushIntervalMs`); a flush that fires on its own calls
// `onFlush` so the host can bump its version.

import {
  isEarlyCancel,
  isTerminalExecutionState,
  jsonCompact,
  parseRFC3339,
  type AgentEvent,
  type ApprovalRequest,
  type ContextStats,
  type ExecutionState,
  type FileUpload,
  type QuestionRequest,
  type SessionHistory,
  type SessionSummary,
  type TaskFinished,
  type TaskStatus,
  type TokenUsage,
} from '../backend/types'
import {
  agentGroupIsOpen,
  agentStatusIsRunning,
  assistantTurnIsEmpty,
  firstIndexWhere,
  inferToolOutputFormat,
  lastIndexWhere,
  makeAgentGroup,
  makeAgentRun,
  makeApprovalItem,
  makeAssistantTurn,
  makeNoticeItem,
  makeOutcomeItem,
  makeToolCallItem,
  makeToolGroup,
  makeToolOutput,
  makeUserTurn,
  terminalForStatus,
  toolGroupRunningCount,
  Transcript,
  type AgentGroup,
  type AgentRun,
  type AssistantTurn,
  type OutgoingPrompt,
  type ToolCallItem,
  type ToolGroup,
  type ToolGroupTerminal,
  type ToolOutput,
} from './transcript'

// MARK: - Run status

/** `RunProgress` (ChatState.swift): the running task's state, step and start. */
export interface RunProgress {
  state: ExecutionState
  step: number
  startedAt: Date
}

/** `RunProgress.elapsed`, in seconds. */
export function runProgressElapsed(progress: RunProgress, now: Date = new Date()): number {
  return (now.getTime() - progress.startedAt.getTime()) / 1000
}

/** `ChatStatus` (ChatState.swift). */
export type ChatStatus =
  | { kind: 'idle' }
  | { kind: 'loadingHistory' }
  | { kind: 'running'; progress: RunProgress }
  | { kind: 'finished'; status: TaskStatus }

/** `ChatStatus.isRunning`. */
export function chatStatusIsRunning(status: ChatStatus): boolean {
  return status.kind === 'running'
}

/** `ChatStatus.progress`. */
export function chatStatusProgress(status: ChatStatus): RunProgress | null {
  return status.kind === 'running' ? status.progress : null
}

/** `ChatStatus.finishedStatus`. */
export function chatStatusFinished(status: ChatStatus): TaskStatus | null {
  return status.kind === 'finished' ? status.status : null
}

/** `ChatStatus ==`. */
export function chatStatusEquals(a: ChatStatus, b: ChatStatus): boolean {
  if (a.kind !== b.kind) return false
  if (a.kind === 'running' && b.kind === 'running') {
    return a.progress.state === b.progress.state && a.progress.step === b.progress.step && a.progress.startedAt.getTime() === b.progress.startedAt.getTime()
  }
  if (a.kind === 'finished' && b.kind === 'finished') return a.status === b.status
  return true
}

/** `Unseen` (ChatState.swift): why a chat wants attention (sidebar dot, badge). */
export type Unseen = { kind: 'finished'; status: TaskStatus } | { kind: 'approval' } | { kind: 'error' }

/** `Unseen ==` (with `nil` on either side). */
export function unseenEquals(a: Unseen | null | undefined, b: Unseen | null | undefined): boolean {
  if (!a || !b) return !a && !b
  if (a.kind === 'finished' && b.kind === 'finished') return a.status === b.status
  return a.kind === b.kind
}

/** `LogLine` (ChatState.swift): one Inspector Log line (formatted like the TUI's `State::log`). */
export interface LogLine {
  id: number
  date: Date
  text: string
  /** "info", "warn" or "error". */
  level: string
}

// MARK: - Target

/// The `ChatState` fields the reducer reads and writes. The required ones are
/// the run's visible state; the optional ones are the rest of what the Swift
/// reducer touches (written when the host declares them, read as empty when it
/// does not), so a host can grow into them.
export interface ReducerTarget {
  /** Committed items. */
  transcript: Transcript
  /** The streaming assistant turn (not yet in `transcript`). */
  live: AssistantTurn | null
  /** The open tool group of the current run (not yet in `transcript`). */
  liveTools: ToolGroup | null
  /// The sub-agents of the current run that are still running (not yet in
  /// `transcript`); committed once every agent in the group has reported.
  liveAgents: AgentGroup | null
  status: ChatStatus
  usage: TokenUsage | null
  costUsd: number | null
  contextStats: ContextStats | null
  pendingApproval: ApprovalRequest | null
  /** Questions of the run's `ask_user` tool, waiting for the user (`question/request`). */
  pendingQuestion: QuestionRequest | null
  changedPaths: Set<string>
  lastError: string | null
  lastErrorKind: string | null
  log: LogLine[]
  /** The optimistic user item appended by `send()`. */
  pendingUserItemId: string | null
  unseen: Unseen | null

  /** `ChatState.summary`: replaced by `loadHistory`, stamped by `finish`. */
  summary?: SessionSummary
  /** True while `approval/respond` is in flight. */
  approvalInFlight?: boolean
  /** Inline failure of the last `approval/respond`. */
  approvalError?: string | null
  /** True while `question/respond` is in flight. */
  questionInFlight?: boolean
  /** Inline failure of the last `question/respond`. */
  questionError?: string | null
  /** True from `task/cancel` until `task/finished` (status bar spinner). */
  cancelRequested?: boolean
  /** Inline error when `session/history` could not be loaded. */
  historyError?: string | null
  /// Sent to the running turn (`task/steer`) and not read by the model yet;
  /// each `steered` event moves the first into the transcript.
  steering?: OutgoingPrompt[]
  /** Prompts waiting for the run to end. */
  queued?: OutgoingPrompt[]
  /** Set when the queue became non-empty (dequeue order across chats). */
  queuedSince?: Date | null
  /// The agent the Inspector follows when nothing is picked: the latest
  /// running (else latest) sub-agent. Refreshed when an agent starts or finishes.
  followedAgentId?: string | null
  /** Whether this chat is on screen in the active app (`ChatState.isVisible`); visible when absent. */
  isVisible?: () => boolean
}

/** A fresh target with `ChatState`'s initial values (scratch replays, tests, hosts that spread it). */
export function makeReducerTarget(summary?: SessionSummary): ReducerTarget {
  const target: ReducerTarget = {
    transcript: new Transcript(),
    live: null,
    liveTools: null,
    liveAgents: null,
    status: { kind: 'idle' },
    usage: null,
    costUsd: null,
    contextStats: null,
    pendingApproval: null,
    pendingQuestion: null,
    changedPaths: new Set(),
    lastError: null,
    lastErrorKind: null,
    log: [],
    pendingUserItemId: null,
    unseen: null,
    approvalInFlight: false,
    approvalError: null,
    questionInFlight: false,
    questionError: null,
    cancelRequested: false,
    historyError: null,
    steering: [],
    queued: [],
    queuedSince: null,
    followedAgentId: null,
  }
  if (summary) target.summary = summary
  return target
}

/** `TranscriptReducer`'s options. */
export interface TranscriptReducerOptions {
  /** Called after a delta flush the timer ran (not one an event forced), so the host can redraw. */
  onFlush?: () => void
}

/** One user message of a history turn (`HistoryUser` in `loadHistory`). */
interface HistoryUser {
  text: string
  images: string[]
  files: FileUpload[]
  date: Date
  runtime: boolean
  id: number | null
  snapshot: boolean
}

/** `(callId: String, agentId: String?)` of `ChatState.callId(forApproval:)`. */
export interface ApprovalCallTarget {
  callId: string
  agentId: string | null
}

// MARK: - Reducer

/// `TranscriptReducer` (ChatState.apply / finish / loadHistory / prependHistory
/// and their helpers): the only writer of transcript items during a run.
export class TranscriptReducer {
  /** `ChatState.maxLogLines`. */
  static readonly maxLogLines = 2000
  /** `ChatState.deltaFlushInterval` (33 ms). */
  static readonly deltaFlushIntervalMs = 33

  readonly target: ReducerTarget
  /** See `TranscriptReducerOptions.onFlush`. */
  onFlush: (() => void) | null

  private pendingText = ''
  private pendingReasoning = ''
  /** Sub-agents' streamed text by agent id, flushed with the lead's. */
  private pendingAgentText = new Map<string, string>()
  private flushTimer: ReturnType<typeof setTimeout> | null = null
  private nextLogId = 0
  private replaying = false
  /** The first transcript item of the history turn being replayed. */
  private replayTurnStart = 0
  private _currentTaskId: string | null = null

  constructor(target: ReducerTarget, options: TranscriptReducerOptions = {}) {
    this.target = target
    this.onFlush = options.onFlush ?? null
  }

  /** The task of the run in progress (set by `task_started`). */
  get currentTaskId(): string | null {
    return this._currentTaskId
  }

  /** Stops the pending delta flush (the chat is going away). */
  dispose(): void {
    this.cancelFlush()
  }

  // MARK: - History

  /// `ChatState.loadHistory(_:)`: rebuilds the transcript from
  /// `session/history`: user messages with their real timestamps, events
  /// replayed through the reducer, tool outputs filled from `tool_result`
  /// messages, outcome from the turn.
  loadHistory(history: SessionHistory): void {
    const t = this.target
    this.cancelFlush()
    t.summary = history.session
    t.transcript.removeAll()
    t.live = null
    t.liveTools = null
    t.liveAgents = null
    t.pendingApproval = null
    t.approvalError = null
    t.pendingQuestion = null
    t.questionError = null
    t.changedPaths = new Set()
    t.lastError = null
    t.lastErrorKind = null
    t.historyError = null
    t.pendingUserItemId = null
    t.contextStats = null

    let totalPrompt = 0
    let totalCompletion = 0
    let totalCached = 0
    let totalCost: number | null = null

    this.replaying = true
    try {
      for (const turn of history.turns) {
        this.replayTurnStart = t.transcript.count
        const taskDate = parseRFC3339(turn.task.created_at) ?? new Date()
        const completedDate =
          (turn.task.completed_at != null ? parseRFC3339(turn.task.completed_at) : null) ?? parseRFC3339(turn.task.updated_at) ?? taskDate

        // User messages (the objective plus runtime-injected ones) are
        // interleaved into the event replay by their created_at.
        const pendingUsers: HistoryUser[] = turn.messages
          .filter((message) => message.kind === 'user' && (message.text ?? '').length > 0)
          .map((message) => ({
            text: message.text ?? '',
            images: message.images ?? [],
            files: message.files ?? [],
            date: parseRFC3339(message.created_at) ?? taskDate,
            runtime: message.runtime === true,
            id: message.id,
            snapshot: message.snapshot === true,
          }))
          .sort((a, b) => a.date.getTime() - b.date.getTime())
        const appendUser = (user: HistoryUser) => {
          this.commitLive(user.date)
          this.commitLiveTools(user.date)
          t.transcript.append(
            makeUserTurn({
              text: user.text,
              images: user.images,
              files: user.files,
              date: user.date,
              isRuntime: user.runtime,
              messageId: user.id,
              hasSnapshot: user.snapshot,
            }),
          )
        }
        const flushUsers = (date: Date) => {
          for (;;) {
            const next = pendingUsers[0]
            if (!next || next.date.getTime() > date.getTime()) break
            pendingUsers.shift()
            appendUser(next)
          }
        }
        const first = pendingUsers.shift()
        if (first) {
          appendUser(first)
        } else if (turn.task.objective.length > 0) {
          appendUser({ text: turn.task.objective, images: [], files: [], date: taskDate, runtime: false, id: null, snapshot: false })
        }

        // Dates for events come from the surrounding records.
        const assistantDates: Date[] = []
        const toolResultDates = new Map<string, Date>()
        for (const message of turn.messages) {
          if (message.kind === 'assistant') {
            const date = parseRFC3339(message.created_at)
            if (date) assistantDates.push(date)
          } else if (message.kind === 'tool_result' && message.call_id != null) {
            const date = parseRFC3339(message.created_at)
            if (date) toolResultDates.set(message.call_id, date)
          }
        }
        let cursor = taskDate
        let responseIndex = 0
        let sawOutcome = false
        for (const event of turn.events) {
          switch (event.event) {
            case 'task_started':
              cursor = taskDate
              break
            case 'model_response': {
              const date = assistantDates[responseIndex]
              if (date) cursor = date
              responseIndex += 1
              break
            }
            case 'tool_call_finished':
            case 'tool_call_output': {
              const date = toolResultDates.get(event.call_id)
              if (date) cursor = date
              break
            }
            case 'task_completed':
              cursor = completedDate
              sawOutcome = true
              break
            default:
              break
          }
          flushUsers(cursor)
          this.apply(event, cursor)
        }
        for (const user of pendingUsers) appendUser(user)
        pendingUsers.length = 0

        // Tool outputs from the persisted conversation (the live stream's
        // synthetic `tool_call_output` is not persisted).
        for (const message of turn.messages) {
          if (message.kind !== 'tool_result' || message.call_id == null) continue
          const content = message.content ?? ''
          const output = makeToolOutput(content, message.full_len ?? Array.from(content).length, message.is_error ?? false, message.tool_name ?? '')
          this.attach(output, message.call_id, message.tool_name ?? null)
        }

        const outcome = turn.outcome
        if (!sawOutcome && outcome) {
          this.commitLive(completedDate)
          this.closeLiveTools(terminalForStatus(outcome.status), completedDate)
          this.closeLiveAgents(completedDate)
          t.transcript.append(makeOutcomeItem(outcome, completedDate, t.transcript.repeatsLastReply(outcome.summary)))
        } else if (!sawOutcome) {
          // A turn without a completion (crashed or still running elsewhere): keep what it had.
          this.commitLive(completedDate)
          this.closeLiveTools(isTerminalExecutionState(turn.task.state) ? terminalForStatus(turn.task.status ?? 'failed') : null, completedDate)
          this.closeLiveAgents(completedDate)
        }

        totalPrompt += turn.usage.prompt_tokens
        totalCompletion += turn.usage.completion_tokens
        totalCached += turn.usage.cached_tokens
        if (turn.usage.cost_usd != null) totalCost = (totalCost ?? 0) + turn.usage.cost_usd
      }
    } finally {
      this.replaying = false
    }
    this.refreshFollowedAgent()
    // A persisted approval_requested without its approval_resolved (the
    // backend was killed or quit while the card was pending) must not
    // survive the replay as a live request: nothing could answer it.
    for (let index = 0; index < t.transcript.count; index += 1) {
      const item = t.transcript.at(index)
      if (item && item.kind === 'approval' && item.decision === null) {
        t.transcript.updateApproval(index, (approval) => {
          approval.decision = 'deny'
        })
      }
    }
    t.pendingApproval = null
    t.approvalInFlight = false

    if (history.turns.length > 0) {
      t.usage = {
        prompt_tokens: totalPrompt,
        completion_tokens: totalCompletion,
        cached_prompt_tokens: totalCached,
        reasoning_tokens: null,
        source: 'reported',
      }
      t.costUsd = totalCost
    } else {
      t.usage = null
      t.costUsd = null
    }
    t.status = { kind: 'idle' }
  }

  /// `ChatState.prependHistory(_:)`: history that arrived after a run began
  /// in this chat (the user pressed Return while `session/history` was in
  /// flight): the earlier turns go in front of what the run has shown so far.
  /// Replayed through a scratch reducer so the live state is untouched; the
  /// turn in progress is already on screen and is skipped.
  prependHistory(history: SessionHistory): void {
    const t = this.target
    const scratchTarget = makeReducerTarget(history.session)
    const scratch = new TranscriptReducer(scratchTarget)
    const running = chatStatusIsRunning(t.status)
    const currentTaskId = this._currentTaskId
    const turns = history.turns.filter((turn) => {
      if (currentTaskId !== null && turn.task.id === currentTaskId) return false
      return isTerminalExecutionState(turn.task.state) || !running
    })
    scratch.loadHistory({ ...history, turns })
    scratch.dispose()
    t.transcript.prepend(scratchTarget.transcript.items)
    if (t.usage === null) {
      t.usage = scratchTarget.usage
      t.costUsd = scratchTarget.costUsd
    }
    this.insertChangedPaths(scratchTarget.changedPaths)
    t.historyError = null
  }

  // MARK: - Reducer

  /// `ChatState.apply(_:date:)`: applies one runtime event. `date` stamps the
  /// items it creates (history replay passes the persisted timestamps).
  /// Returns false when the event was only buffered (a text or reasoning
  /// delta that changed nothing on screen yet): the host may skip its redraw,
  /// the 33 ms flush calls `onFlush`.
  apply(event: AgentEvent, date: Date = new Date()): boolean {
    const t = this.target
    switch (event.event) {
      case 'task_started': {
        this.cancelFlush()
        this._currentTaskId = event.task_id
        if (t.pendingUserItemId === null) {
          if (!this.replaying) t.transcript.append(makeUserTurn({ text: event.objective, date }))
        }
        t.pendingUserItemId = null
        t.live = null
        t.liveTools = null
        t.liveAgents = null
        t.pendingApproval = null
        t.approvalError = null
        t.pendingQuestion = null
        t.questionError = null
        t.cancelRequested = false
        t.lastError = null
        t.lastErrorKind = null
        t.status = { kind: 'running', progress: { state: 'PREPARING', step: 0, startedAt: date } }
        this.addLog(`task ${event.continued ? 'continued' : 'started'}: ${TranscriptReducer.clip(event.objective, 60)}`, date)
        return true
      }

      case 'state_changed': {
        const progress = chatStatusProgress(t.status)
        if (progress) t.status = { kind: 'running', progress: { ...progress, state: event.to } }
        this.addLog(`state: ${event.to.toLowerCase()}`, date)
        return true
      }

      case 'model_request_started': {
        this.flushPending()
        const progress = chatStatusProgress(t.status)
        if (progress) t.status = { kind: 'running', progress: { ...progress, step: event.step } }
        this.commitLive(date)
        t.live = makeAssistantTurn({ step: event.step, date })
        this.addLog(`model request · step ${event.step} · ~${event.estimated_prompt_tokens} tok`, date)
        return true
      }

      case 'prompt_sent': {
        // Developer mode: shown above the message that started the run.
        const index = t.transcript.lastUserIndex((turn) => !turn.isRuntime && !turn.isSteering)
        if (index !== null) {
          t.transcript.updateUser(index, (turn) => {
            turn.sentPrompt = event.prompt
          })
        }
        return true
      }

      case 'model_reasoning_delta': {
        if (event.text.length === 0) return false
        let changed = this.ensureLive(date)
        if (t.live && t.live.thinkingStarted === null) {
          this.mutateLive((turn) => {
            turn.thinkingStarted = date
            turn.isThinking = true
          })
          changed = true
        }
        this.pendingReasoning += event.text
        this.scheduleFlush()
        return changed
      }

      case 'model_text_delta': {
        if (event.text.length === 0) return false
        let changed = this.ensureLive(date)
        const live = t.live
        if (live && (live.isThinking || (live.thinkingStarted !== null && live.thinkingDuration === null))) {
          this.mutateLive((turn) => {
            turn.isThinking = false
            if (turn.thinkingStarted !== null && turn.thinkingDuration === null) {
              turn.thinkingDuration = Math.max(0, secondsBetween(turn.thinkingStarted, date))
            }
          })
          changed = true
        }
        this.pendingText += event.text
        this.scheduleFlush()
        return changed
      }

      case 'model_response': {
        this.flushPending()
        this.ensureLive(date)
        const text = event.text
        const replaying = this.replaying
        this.mutateLive((turn) => {
          if (text !== null && text.length > 0) turn.text = text
          // History has no deltas: the response record's date is the turn's date.
          if (replaying) turn.date = date
          turn.isStreaming = false
          if (turn.isThinking) {
            turn.isThinking = false
            if (turn.thinkingStarted !== null && turn.thinkingDuration === null) {
              turn.thinkingDuration = Math.max(0, secondsBetween(turn.thinkingStarted, date))
            }
          }
        })
        this.commitLive(date)
        t.live = null
        this.addLog(`model response · ${event.tool_call_count} tool call${event.tool_call_count === 1 ? '' : 's'}`, date)
        return true
      }

      case 'tool_call_started': {
        this.flushPending()
        this.commitLive(date)
        if (t.liveTools === null) t.liveTools = makeToolGroup({ startedAt: date })
        const call = makeToolCallItem({ callId: event.call_id, name: event.name, arguments: event.arguments, startedAt: date })
        const isCommand = event.name === 'execute_command' || event.name === 'run_validation'
        this.mutateLiveTools((group) => {
          group.calls.push(call)
          if (isCommand) group.commandsRun += 1
        })
        this.addLog(`→ ${event.name} ${TranscriptReducer.clip(jsonCompact(event.arguments), 90)}`, date)
        return true
      }

      case 'tool_call_finished': {
        this.updateCall(event.call_id, (call) => {
          call.status = event.success ? 'succeeded' : 'failed'
          call.summary = event.summary
          call.durationMs = event.duration_ms
        })
        this.closeCommittedGroupIfIdle(event.call_id, date)
        this.addLog(
          `  ${event.success ? '✓' : '✗'} ${TranscriptReducer.clip(event.summary, 90)} (${event.duration_ms} ms)`,
          date,
          event.success ? 'info' : 'warn',
        )
        return true
      }

      case 'tool_call_output': {
        const output = makeToolOutput(event.content, event.full_len, event.is_error, event.name)
        this.attach(output, event.call_id, event.name)
        return true
      }

      case 'approval_requested': {
        const request = event.request
        this.flushPending()
        this.commitLive(date)
        this.commitLiveTools(date)
        t.pendingApproval = request
        t.approvalError = null
        const target = TranscriptReducer.callIdForApproval(request.id)
        if (target) {
          if (target.agentId !== null) {
            this.updateAgent(target.agentId, (agent) => {
              const index =
                lastIndexWhere(agent.calls, (call) => call.callId === target.callId && call.status === 'running') ??
                lastIndexWhere(agent.calls, (call) => call.callId === target.callId)
              if (index !== null) {
                editCall(agent.calls, index, (call) => {
                  call.approvalRequestId = request.id
                })
              }
            })
          } else {
            this.updateCall(target.callId, (call) => {
              call.approvalRequestId = request.id
            })
          }
        }
        t.transcript.append(makeApprovalItem(request, date))
        if (!this.replaying && !this.isVisible()) t.unseen = { kind: 'approval' }
        this.addLog(`approval requested: ${TranscriptReducer.clip(request.summary, 90)}`, date, 'warn')
        return true
      }

      case 'approval_resolved': {
        const index = t.transcript.locateApproval(event.request_id)
        if (index !== null) {
          t.transcript.updateApproval(index, (approval) => {
            approval.decision = event.decision
          })
        }
        if (t.pendingApproval?.id === event.request_id) t.pendingApproval = null
        if (t.unseen?.kind === 'approval') t.unseen = null
        this.addLog(`approval ${event.decision}: ${event.request_id}`, date)
        return true
      }

      case 'validation_started':
        this.addLog(`validation started: ${TranscriptReducer.clip(event.command, 90)}`, date)
        return true

      case 'validation_finished':
        // The run_validation call already shows in the tool timeline.
        this.addLog(
          `validation ${event.record.passed ? 'PASSED' : 'FAILED'}: ${TranscriptReducer.clip(event.record.command, 90)}`,
          date,
          event.record.passed ? 'info' : 'warn',
        )
        return true

      case 'file_changed': {
        const path = event.path
        if (t.liveTools !== null) {
          this.mutateLiveTools((group) => {
            group.filesChanged.add(path)
          })
        } else {
          // The group may already be closed: file_changed follows the
          // tool_call_finished that closed an approval-split group.
          const index = t.transcript.currentToolGroupIndex
          if (index !== null) {
            t.transcript.updateTools(index, (group) => {
              group.filesChanged.add(path)
            })
          }
        }
        this.insertChangedPaths([path])
        this.addLog(`  file ${event.kind}: ${path}`, date)
        return true
      }

      case 'checkpoint':
        this.appendNotice(`checkpoint: ${event.reason}`, true, date)
        this.addLog(`checkpoint: ${event.reason}`, date)
        return true

      case 'warning': {
        const detail = event.message.toLowerCase().startsWith('task boundary')
        this.appendNotice(event.message, detail, date)
        this.addLog(`note: ${event.message}`, date, 'warn')
        return true
      }

      case 'usage':
        t.usage = event.cumulative
        t.costUsd = event.cost_usd
        return true

      case 'steered': {
        // A history replay has the message itself (`turn.messages`).
        if (this.replaying) return false
        this.flushPending()
        this.commitLive(date)
        this.commitLiveTools(date)
        const steering = t.steering ?? []
        const sent = steering[0] ?? null
        if (steering.length > 0) t.steering = steering.slice(1)
        t.transcript.append(
          makeUserTurn({
            text: sent?.text ?? event.text,
            images: sent?.images ?? [],
            files: sent?.files ?? [],
            date,
            isSteering: true,
          }),
        )
        this.addLog(`steered: ${TranscriptReducer.clip(event.text, 60)}`, date)
        return true
      }

      case 'context_compiled': {
        const stats = event.stats
        t.contextStats = stats
        this.addLog(
          `context ~${stats.estimated_prompt_tokens} tok · ${stats.repository_snippets} snippets · ${stats.memory_records} memories${stats.compacted ? ' · compacted' : ''}`,
          date,
        )
        return true
      }

      case 'agent_started': {
        this.flushPending()
        this.commitLive(date)
        this.commitLiveTools(date)
        const run = makeAgentRun({
          id: event.agent_id,
          agentTaskId: event.agent_task_id,
          name: event.name,
          mode: event.mode,
          objective: event.objective,
          startedAt: date,
        })
        if (t.liveAgents === null) t.liveAgents = makeAgentGroup({ startedAt: date })
        this.mutateLiveAgents((group) => {
          group.agents.push(run)
        })
        this.refreshFollowedAgent()
        this.addLog(`◈ agent ${event.name} (${event.mode}) started: ${TranscriptReducer.clip(event.objective, 60)}`, date)
        return true
      }

      case 'agent_progress':
        return this.applyAgentEvent(event.inner, event.agent_id, date)

      case 'agent_finished': {
        const report = event.report
        this.updateAgent(event.agent_id, (agent) => {
          agent.status = { kind: 'finished', status: report.status }
          agent.report = report
          agent.endedAt = date
          agent.liveText = ''
          for (const path of report.modified_files) agent.filesChanged.add(path)
          agent.calls = agent.calls.map((call) =>
            call.status !== 'running'
              ? call
              : {
                  ...call,
                  status: report.status === 'completed' ? 'succeeded' : 'failed',
                  summary: call.summary.length === 0 ? 'did not finish' : call.summary,
                },
          )
        })
        this.pendingAgentText.delete(event.agent_id)
        this.insertChangedPaths(report.modified_files)
        this.addLog(
          `◈ agent ${report.name} ${report.status}: ${report.steps} steps, ${report.tool_calls} tool calls, ${report.duration_ms} ms`,
          date,
          report.status === 'failed' ? 'warn' : 'info',
        )
        this.commitLiveAgentsIfDone(date)
        this.refreshFollowedAgent()
        return true
      }

      case 'task_completed': {
        const outcome = event.outcome
        this.flushPending()
        this.commitLive(date)
        const terminal = terminalForStatus(outcome.status)
        this.closeLiveTools(terminal, date)
        this.closeOpenGroups(terminal, date)
        this.closeLiveAgents(date)
        t.transcript.append(makeOutcomeItem(outcome, date, t.transcript.repeatsLastReply(outcome.summary)))
        t.status = { kind: 'finished', status: outcome.status }
        t.pendingApproval = null
        t.pendingUserItemId = null
        if (outcome.cost_usd != null) t.costUsd = outcome.cost_usd
        this.addLog(`task ${outcome.status}: ${TranscriptReducer.clip(outcome.summary, 90)}`, date, outcome.status === 'failed' ? 'error' : 'info')
        return true
      }

      case 'unknown':
        this.addLog(`event ${event.name}`, date)
        return true
    }
  }

  /// `ChatState.finish(_:terminal:date:)`: `task/finished` closes everything
  /// the run left open. `terminal` overrides how an open tool group is closed
  /// (backend exit → `stopped`).
  finish(f: TaskFinished, terminal: ToolGroupTerminal | null = null, date: Date = new Date()): void {
    const t = this.target
    this.flushPending()
    this.cancelFlush()
    this.commitLive(date)
    // A run cancelled before it reached the runtime (index wait, probe)
    // reports no status, only error_kind "cancelled": a cancel, not a
    // failure, so no error notice, no red dot, no "Task failed".
    const cancelledEarly = isEarlyCancel(f)
    const finalStatus: TaskStatus = f.status ?? (cancelledEarly ? 'cancelled' : (chatStatusFinished(t.status) ?? 'failed'))
    const groupTerminal: ToolGroupTerminal = terminal ?? (f.error != null && !cancelledEarly ? 'failed' : terminalForStatus(finalStatus))
    this.closeLiveTools(groupTerminal, date)
    this.closeOpenGroups(groupTerminal, date)
    this.closeLiveAgents(date)

    if (cancelledEarly) {
      this.addLog('task cancelled before it started', date)
    } else if (f.error != null && f.error.length > 0) {
      t.transcript.append(makeNoticeItem({ lines: [f.error], isError: true, date }))
      t.lastError = f.error
      t.lastErrorKind = f.error_kind ?? null
      this.addLog(`error: ${f.error}`, date, 'error')
    }
    const pending = t.pendingApproval
    if (pending) {
      const index = t.transcript.locateApproval(pending.id)
      if (index !== null) {
        // The runtime dropped the request with the run: the tool never ran.
        t.transcript.updateApproval(index, (approval) => {
          if (approval.decision === null) approval.decision = 'deny'
        })
      }
    }
    t.pendingApproval = null
    t.approvalInFlight = false
    t.pendingQuestion = null
    t.questionInFlight = false
    t.questionError = null
    t.cancelRequested = false
    t.pendingUserItemId = null
    // Steering the run never read (cancelled, failed, out of steps) runs
    // next instead of being lost.
    const steering = t.steering ?? []
    if (steering.length > 0) {
      const queued = t.queued ?? []
      if (queued.length === 0) t.queuedSince = date
      t.queued = [...steering, ...queued]
      t.steering = []
    }
    t.status = { kind: 'finished', status: finalStatus }
    if (!this.isVisible()) {
      t.unseen = f.error != null && !cancelledEarly ? { kind: 'error' } : { kind: 'finished', status: finalStatus }
    }
    if (t.summary) t.summary = { ...t.summary, last_status: finalStatus, active_task_id: null }
    // The prompt's persisted id makes the run's message editable / undoable
    // (the newest user item that has none yet).
    const messageId = f.message_id
    if (messageId != null) {
      const index = t.transcript.lastUserIndex((turn) => turn.messageId === null && !turn.isRuntime && !turn.isSteering)
      if (index !== null) {
        t.transcript.updateUser(index, (turn) => {
          turn.messageId = messageId
          turn.hasSnapshot = f.snapshot ?? false
        })
      }
    }
  }

  // MARK: - Sub-agents

  /// `ChatState.applyAgentEvent(_:agentId:date:)`: a sub-agent's own event:
  /// its tool rows, outputs and streamed text. Deltas are buffered and
  /// flushed with the lead's (several agents stream at once), everything
  /// else is applied like the lead's events.
  private applyAgentEvent(event: AgentEvent, agentId: string, date: Date): boolean {
    switch (event.event) {
      case 'model_request_started':
        this.pendingAgentText.delete(agentId)
        this.updateAgent(agentId, (agent) => {
          agent.liveText = ''
        })
        return true
      case 'model_text_delta':
        if (event.text.length === 0) return false
        this.pendingAgentText.set(agentId, (this.pendingAgentText.get(agentId) ?? '') + event.text)
        this.scheduleFlush()
        return false
      case 'model_response': {
        const text = event.text
        if (text !== null && text.length > 0) {
          this.pendingAgentText.delete(agentId)
          this.updateAgent(agentId, (agent) => {
            agent.liveText = text
          })
        }
        return true
      }
      case 'tool_call_started': {
        const call = makeToolCallItem({ callId: event.call_id, name: event.name, arguments: event.arguments, startedAt: date })
        const isCommand = event.name === 'execute_command' || event.name === 'run_validation'
        this.updateAgent(agentId, (agent) => {
          agent.calls.push(call)
          if (isCommand) agent.commandsRun += 1
        })
        this.addLog(`  [${this.agentName(agentId)}] → ${event.name} ${TranscriptReducer.clip(jsonCompact(event.arguments), 70)}`, date)
        return true
      }
      case 'tool_call_finished': {
        this.updateAgent(agentId, (agent) => {
          const index =
            lastIndexWhere(agent.calls, (call) => call.callId === event.call_id && call.status === 'running') ??
            lastIndexWhere(agent.calls, (call) => call.callId === event.call_id)
          if (index !== null) {
            editCall(agent.calls, index, (call) => {
              call.status = event.success ? 'succeeded' : 'failed'
              call.summary = event.summary
              call.durationMs = event.duration_ms
            })
          }
        })
        this.addLog(
          `  [${this.agentName(agentId)}]   ${event.success ? '✓' : '✗'} ${TranscriptReducer.clip(event.summary, 70)} (${event.duration_ms} ms)`,
          date,
          event.success ? 'info' : 'warn',
        )
        return true
      }
      case 'tool_call_output': {
        const output = makeToolOutput(event.content, event.full_len, event.is_error, event.name)
        this.updateAgent(agentId, (agent) => {
          const index =
            lastIndexWhere(agent.calls, (call) => call.callId === event.call_id && call.output === null) ??
            lastIndexWhere(agent.calls, (call) => call.callId === event.call_id)
          if (index !== null) {
            editCall(agent.calls, index, (call) => {
              call.output = output
            })
          }
        })
        return true
      }
      case 'file_changed': {
        const path = event.path
        this.updateAgent(agentId, (agent) => {
          agent.filesChanged.add(path)
        })
        this.insertChangedPaths([path])
        return true
      }
      case 'approval_requested':
        // The lead's reducer owns the approval card; it routes the request
        // id (suffixed `@agent`) back to this agent's row.
        return this.apply({ event: 'approval_requested', task_id: this._currentTaskId ?? '', request: event.request }, date)
      case 'approval_resolved':
        return this.apply(
          { event: 'approval_resolved', task_id: this._currentTaskId ?? '', request_id: event.request_id, decision: event.decision },
          date,
        )
      case 'warning':
        this.addLog(`  [${this.agentName(agentId)}]   note: ${event.message}`, date, 'warn')
        return true
      default:
        return false
    }
  }

  private agentName(agentId: string): string {
    const live = this.target.liveAgents?.agents.find((agent) => agent.id === agentId)
    if (live) return live.name
    const location = this.target.transcript.locateAgent(agentId)
    if (location) {
      const item = this.target.transcript.at(location.item)
      if (item && item.kind === 'agents') {
        const agent = item.agents[location.agent]
        if (agent) return agent.name
      }
    }
    return agentId
  }

  /** `ChatState.updateAgent(_:_:)`: resolves against the live group first, then committed groups. */
  private updateAgent(agentId: string, change: (agent: AgentRun) => void): void {
    const live = this.target.liveAgents
    if (live) {
      const index = lastIndexWhere(live.agents, (agent) => agent.id === agentId)
      if (index !== null) {
        this.mutateLiveAgents((group) => {
          const agent = group.agents[index]
          if (!agent) return
          const copy = copyAgent(agent)
          change(copy)
          group.agents[index] = copy
        })
        return
      }
    }
    const location = this.target.transcript.locateAgent(agentId)
    if (location) {
      this.target.transcript.updateAgents(location.item, (group) => {
        const agent = group.agents[location.agent]
        if (!agent) return
        const copy = copyAgent(agent)
        change(copy)
        group.agents[location.agent] = copy
      })
    }
  }

  /// `ChatState.commitLiveAgentsIfDone(date:)`: the live agent group is
  /// committed once every agent has reported, so the lead's next message
  /// lands below the card.
  private commitLiveAgentsIfDone(date: Date): void {
    const group = this.target.liveAgents
    if (!group || agentGroupIsOpen(group)) return
    this.target.transcript.append({ ...group, agents: [...group.agents], endedAt: date })
    this.target.liveAgents = null
  }

  /// `ChatState.closeLiveAgents(date:)`: the run ended: agents still running
  /// are marked stopped and the group is committed as it stands.
  private closeLiveAgents(date: Date): void {
    const group = this.target.liveAgents
    if (!group) return
    this.target.liveAgents = null
    if (group.agents.length === 0) return
    const agents = group.agents.map((agent): AgentRun => {
      if (!agentStatusIsRunning(agent.status)) return agent
      return {
        ...agent,
        status: { kind: 'stopped' },
        endedAt: date,
        liveText: '',
        calls: agent.calls.map((call) =>
          call.status !== 'running' ? call : { ...call, status: 'failed', summary: call.summary.length === 0 ? 'stopped' : call.summary },
        ),
      }
    })
    this.target.transcript.append({ ...group, agents, endedAt: date })
    this.refreshFollowedAgent()
  }

  /** `ChatState.refreshFollowedAgent()`: the latest running (else latest) live agent, else the newest committed one. */
  private refreshFollowedAgent(): void {
    const t = this.target
    const liveAgents = t.liveAgents?.agents ?? []
    let latest: AgentRun | null = null
    const running = lastIndexWhere(liveAgents, (agent) => agentStatusIsRunning(agent.status))
    if (running !== null) latest = liveAgents[running] ?? null
    else latest = liveAgents[liveAgents.length - 1] ?? null
    if (latest === null) {
      const items = t.transcript.items
      for (let i = items.length - 1; i >= 0; i -= 1) {
        const item = items[i]
        if (item && item.kind === 'agents') {
          const last = item.agents[item.agents.length - 1]
          if (last) {
            latest = last
            break
          }
        }
      }
    }
    const id = latest?.id ?? null
    if ((t.followedAgentId ?? null) !== id) t.followedAgentId = id
  }

  // MARK: - Log

  /** `ChatState.addLog(_:date:level:)`: one Inspector Log line (none while replaying history). */
  addLog(text: string, date: Date = new Date(), level = 'info'): void {
    if (this.replaying) return
    this.nextLogId += 1
    const next = [...this.target.log, { id: this.nextLogId, date, text, level }]
    if (next.length > TranscriptReducer.maxLogLines) next.splice(0, next.length - TranscriptReducer.maxLogLines)
    this.target.log = next
  }

  /** `ChatState.clearLog()`. */
  clearLog(): void {
    this.target.log = []
  }

  // MARK: - Delta coalescing

  /** Creates `live` when there is none; true when it did. */
  private ensureLive(date: Date): boolean {
    if (this.target.live !== null) return false
    this.target.live = makeAssistantTurn({ step: chatStatusProgress(this.target.status)?.step ?? 0, date })
    return true
  }

  private scheduleFlush(): void {
    if (this.flushTimer !== null) return
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null
      this.flushPending()
      this.onFlush?.()
    }, TranscriptReducer.deltaFlushIntervalMs)
  }

  private cancelFlush(): void {
    if (this.flushTimer !== null) clearTimeout(this.flushTimer)
    this.flushTimer = null
    this.pendingText = ''
    this.pendingReasoning = ''
    this.pendingAgentText.clear()
  }

  /** `ChatState.flushPending()`: moves the buffered deltas into `live` (synchronous; also called before any commit). */
  flushPending(): void {
    if (this.pendingAgentText.size > 0) {
      const texts = [...this.pendingAgentText]
      this.pendingAgentText.clear()
      for (const [agentId, text] of texts) {
        this.updateAgent(agentId, (agent) => {
          agent.liveText += text
        })
      }
    }
    if (this.pendingText.length === 0 && this.pendingReasoning.length === 0) return
    if (this.target.live === null) this.target.live = makeAssistantTurn({ step: chatStatusProgress(this.target.status)?.step ?? 0 })
    const reasoning = this.pendingReasoning
    const text = this.pendingText
    this.pendingReasoning = ''
    this.pendingText = ''
    this.mutateLive((turn) => {
      if (reasoning.length > 0) turn.reasoning += reasoning
      if (text.length > 0) turn.text += text
    })
  }

  // MARK: - Commit helpers

  /** `ChatState.commitLive(date:)`: commits a non-empty `live` turn (preceded by the open tool group so order is preserved). */
  private commitLive(date: Date): void {
    if (this.target.live === null) return
    this.flushPending()
    const turn = this.target.live
    if (turn === null) return
    if (assistantTurnIsEmpty(turn)) {
      this.target.live = null
      return
    }
    this.commitLiveTools(date)
    const committed: AssistantTurn = { ...turn, isStreaming: false }
    if (committed.isThinking) {
      committed.isThinking = false
      if (committed.thinkingStarted !== null && committed.thinkingDuration === null) {
        committed.thinkingDuration = Math.max(0, secondsBetween(committed.thinkingStarted, date))
      }
    }
    this.target.transcript.append(committed)
    this.target.live = null
  }

  /// `ChatState.commitLiveTools(date:)`: moves a non-empty `liveTools` into
  /// the transcript. A group with no running call closes as `done`; one
  /// still waiting (approval in flight) stays open and is closed when its
  /// last call finishes.
  private commitLiveTools(date: Date): void {
    const group = this.target.liveTools
    if (!group || group.calls.length === 0) {
      this.target.liveTools = null
      return
    }
    const committed: ToolGroup = { ...group, calls: [...group.calls], filesChanged: new Set(group.filesChanged) }
    if (toolGroupRunningCount(committed) === 0) {
      committed.terminal = 'done'
      committed.endedAt = date
    }
    this.target.transcript.append(committed)
    this.target.liveTools = null
  }

  /** `ChatState.closeLiveTools(terminal:date:)`. */
  private closeLiveTools(terminal: ToolGroupTerminal | null, date: Date): void {
    const group = this.target.liveTools
    if (!group) return
    this.target.liveTools = null
    if (group.calls.length === 0) return
    const calls = group.calls.map((call): ToolCallItem => {
      if (call.status !== 'running') return call
      return {
        ...call,
        status: terminal === 'done' ? 'succeeded' : 'failed',
        summary: call.summary.length > 0 ? call.summary : terminal === 'stopped' ? 'stopped' : terminal === 'done' ? '' : 'did not finish',
      }
    })
    this.target.transcript.append({ ...group, calls, filesChanged: new Set(group.filesChanged), terminal, endedAt: date })
  }

  /** `ChatState.closeOpenGroups(terminal:date:)`: closes committed groups that are still open (approval case) at the end of a run. */
  private closeOpenGroups(terminal: ToolGroupTerminal | null, date: Date): void {
    const transcript = this.target.transcript
    for (;;) {
      const index = transcript.openToolGroupIndex
      if (index === null) break
      transcript.updateTools(index, (group) => {
        group.calls = group.calls.map((call) =>
          call.status !== 'running'
            ? call
            : {
                ...call,
                status: terminal === 'done' ? 'succeeded' : 'failed',
                summary: call.summary.length > 0 ? call.summary : terminal === 'stopped' ? 'stopped' : 'did not finish',
              },
        )
        group.terminal = terminal ?? 'done'
        group.endedAt = date
      })
    }
  }

  /// `ChatState.updateCall(_:_:)`: resolves against the most recent row for
  /// the id, preferring one that is still running (ids such as `call_0`
  /// repeat across steps).
  private updateCall(callId: string, change: (call: ToolCallItem) => void): void {
    const live = this.target.liveTools
    if (live) {
      const index =
        lastIndexWhere(live.calls, (call) => call.callId === callId && call.status === 'running') ??
        lastIndexWhere(live.calls, (call) => call.callId === callId)
      if (index !== null) {
        this.mutateLiveTools((group) => editCall(group.calls, index, change))
        return
      }
    }
    const location = this.target.transcript.locateCall(callId)
    if (location) this.target.transcript.updateTools(location.item, (group) => editCall(group.calls, location.call, change))
  }

  /** `ChatState.closeCommittedGroupIfIdle(callId:date:)`: a committed-but-open group (split by an approval) closes once nothing in it runs. */
  private closeCommittedGroupIfIdle(callId: string, date: Date): void {
    if (this.target.liveTools?.calls.some((call) => call.callId === callId) === true) return
    const location = this.target.transcript.locateCall(callId)
    if (!location) return
    const item = this.target.transcript.at(location.item)
    if (!item || item.kind !== 'tools' || item.terminal !== null || toolGroupRunningCount(item) !== 0) return
    this.target.transcript.updateTools(location.item, (group) => {
      group.terminal = 'done'
      group.endedAt = date
    })
  }

  /// `ChatState.attach(output:callId:toolName:)`: outputs pair with rows
  /// positionally (call ids repeat across steps): the row for the id that
  /// has no output yet takes it. History is replayed oldest-first, the live
  /// stream newest-first.
  private attach(output: ToolOutput, callId: string, toolName: string | null): void {
    let out = output
    if (out.format === 'text' && toolName !== null && toolName.length > 0) {
      out = { ...out, format: inferToolOutputFormat(toolName, out.content) }
    }
    const setOutput = (call: ToolCallItem) => {
      call.output = out
    }
    const matches = (call: ToolCallItem) => call.callId === callId && call.output === null
    const transcript = this.target.transcript
    if (this.replaying) {
      // Outputs belong to the turn being replayed: scanning from its first
      // item keeps a long history linear to load.
      for (let i = Math.min(this.replayTurnStart, transcript.count); i < transcript.count; i += 1) {
        const item = transcript.at(i)
        if (!item || item.kind !== 'tools') continue
        const c = firstIndexWhere(item.calls, matches)
        if (c !== null) {
          transcript.updateTools(i, (group) => editCall(group.calls, c, setOutput))
          return
        }
      }
      const live = this.target.liveTools
      if (live) {
        const index = firstIndexWhere(live.calls, matches)
        if (index !== null) {
          this.mutateLiveTools((group) => editCall(group.calls, index, setOutput))
          return
        }
      }
    } else {
      const live = this.target.liveTools
      if (live) {
        const index = lastIndexWhere(live.calls, matches)
        if (index !== null) {
          this.mutateLiveTools((group) => editCall(group.calls, index, setOutput))
          return
        }
      }
      for (let i = transcript.count - 1; i >= 0; i -= 1) {
        const item = transcript.at(i)
        if (!item || item.kind !== 'tools') continue
        const c = lastIndexWhere(item.calls, matches)
        if (c !== null) {
          transcript.updateTools(i, (group) => editCall(group.calls, c, setOutput))
          return
        }
      }
    }
    this.updateCall(callId, setOutput)
  }

  /** `ChatState.appendNotice(line:detail:date:)`: consecutive notices coalesce into one item. */
  private appendNotice(line: string, detail: boolean, date: Date): void {
    const transcript = this.target.transcript
    const last = transcript.last
    if (last && last.kind === 'notice' && !last.isError) {
      transcript.updateNotice(transcript.count - 1, (notice) => {
        if (detail) notice.detailLines.push(line)
        else notice.lines.push(line)
      })
      return
    }
    transcript.append(makeNoticeItem({ date, lines: detail ? [] : [line], detailLines: detail ? [line] : [] }))
  }

  // MARK: - Value-semantics helpers

  private mutateLive(change: (turn: AssistantTurn) => void): void {
    const live = this.target.live
    if (!live) return
    const copy: AssistantTurn = { ...live }
    change(copy)
    this.target.live = copy
  }

  private mutateLiveTools(change: (group: ToolGroup) => void): void {
    const group = this.target.liveTools
    if (!group) return
    const copy: ToolGroup = { ...group, calls: [...group.calls], filesChanged: new Set(group.filesChanged) }
    change(copy)
    this.target.liveTools = copy
  }

  private mutateLiveAgents(change: (group: AgentGroup) => void): void {
    const group = this.target.liveAgents
    if (!group) return
    const copy: AgentGroup = { ...group, agents: [...group.agents] }
    change(copy)
    this.target.liveAgents = copy
  }

  private insertChangedPaths(paths: Iterable<string>): void {
    let next: Set<string> | null = null
    for (const path of paths) {
      if ((next ?? this.target.changedPaths).has(path)) continue
      next ??= new Set(this.target.changedPaths)
      next.add(path)
    }
    if (next) this.target.changedPaths = next
  }

  private isVisible(): boolean {
    return this.target.isVisible ? this.target.isVisible() : true
  }

  // MARK: - Static helpers

  /** `ChatState.terminal(for:)`. */
  static terminal(status: TaskStatus): ToolGroupTerminal {
    return terminalForStatus(status)
  }

  /// `ChatState.callId(forApproval:)`: `approval_call_1` → (`call_1`, null);
  /// a sub-agent's `approval_call_1@agent-1a2b3c4d` → (`call_1`, `agent-1a2b3c4d`).
  static callIdForApproval(requestId: string): ApprovalCallTarget | null {
    const prefix = 'approval_'
    if (!requestId.startsWith(prefix)) return null
    const rest = requestId.slice(prefix.length)
    if (rest.length === 0) return null
    const at = rest.lastIndexOf('@')
    if (at >= 0) {
      const callId = rest.slice(0, at)
      const agentId = rest.slice(at + 1)
      return callId.length === 0 ? null : { callId, agentId: agentId.length === 0 ? null : agentId }
    }
    return { callId: rest, agentId: null }
  }

  /** `ChatState.clip(_:_:)`: newlines folded to spaces, capped at `limit` characters with an ellipsis. */
  static clip(text: string, limit: number): string {
    const collapsed = text
      .split(/[\n\r\v\f\u0085\u2028\u2029]+/)
      .filter((part) => part.length > 0)
      .join(' ')
    const chars = Array.from(collapsed)
    return chars.length <= limit ? collapsed : chars.slice(0, limit).join('') + '…'
  }
}

// MARK: - Module helpers

/** Seconds from `start` to `end` (Swift `end.timeIntervalSince(start)`). */
function secondsBetween(start: Date, end: Date): number {
  return (end.getTime() - start.getTime()) / 1000
}

/** A copy of `agent` whose `calls` and `filesChanged` may be changed in place. */
function copyAgent(agent: AgentRun): AgentRun {
  return { ...agent, calls: [...agent.calls], filesChanged: new Set(agent.filesChanged) }
}

/** Replaces `calls[index]` with a changed copy. */
function editCall(calls: ToolCallItem[], index: number, change: (call: ToolCallItem) => void): void {
  const call = calls[index]
  if (!call) return
  const copy: ToolCallItem = { ...call }
  change(copy)
  calls[index] = copy
}
