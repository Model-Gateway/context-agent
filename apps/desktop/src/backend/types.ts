// Port of apps/macos/ContextAgent/Backend/Protocol.swift (and the wire
// shapes of Models/ModelGateway.swift): every DTO of docs/desktop-protocol.md
// with the same type names, the wire's snake_case field names and the same
// optionality. Fields the Swift decoders default when the backend leaves them
// out are non-optional here too; `normalize*` functions apply those defaults
// (the Swift `init(from:)` overrides) and `client.ts` runs them on results.

// MARK: - JSONValue

/// Arbitrary JSON (Swift `JSONValue`): tool arguments, config patches and
/// anything the protocol leaves open. Keys are kept verbatim.
export type JSONValue = string | number | boolean | null | JSONValue[] | JSONObject

/** A JSON object (`JSONValue.object`). */
export type JSONObject = { [key: string]: JSONValue }

/** `JSONValue.stringValue`. */
export function jsonString(value: JSONValue | undefined): string | null {
  return typeof value === 'string' ? value : null
}

/** `JSONValue.doubleValue`. */
export function jsonNumber(value: JSONValue | undefined): number | null {
  return typeof value === 'number' ? value : null
}

/** `JSONValue.intValue` (truncates like Swift's `Int(n)`). */
export function jsonInt(value: JSONValue | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? Math.trunc(value) : null
}

/** `JSONValue.boolValue`. */
export function jsonBool(value: JSONValue | undefined): boolean | null {
  return typeof value === 'boolean' ? value : null
}

/** `JSONValue.arrayValue`. */
export function jsonArray(value: JSONValue | undefined): JSONValue[] | null {
  return Array.isArray(value) ? value : null
}

/** `JSONValue.objectValue`. */
export function jsonObject(value: JSONValue | undefined): JSONObject | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value : null
}

/** `JSONValue.isNull`. */
export function jsonIsNull(value: JSONValue | undefined): boolean {
  return value === null
}

/** `JSONValue[key]`: a member of an object, else `undefined`. */
export function jsonGet(value: JSONValue | undefined, key: string): JSONValue | undefined {
  const object = jsonObject(value)
  return object ? object[key] : undefined
}

/** `JSONValue[index]`: an element of an array, else `undefined`. */
export function jsonIndex(value: JSONValue | undefined, index: number): JSONValue | undefined {
  const array = jsonArray(value)
  return array && index >= 0 && index < array.length ? array[index] : undefined
}

/** `JSONValue.prettyPrinted()`: two-space indent, sorted keys; scalars compact. */
export function jsonPrettyPrinted(value: JSONValue): string {
  if (value !== null && typeof value === 'object') {
    try {
      return JSON.stringify(sortedKeys(value), null, 2)
    } catch {
      return jsonCompact(value)
    }
  }
  return jsonCompact(value)
}

/** `JSONValue.compact()`. */
export function jsonCompact(value: JSONValue): string {
  try {
    return JSON.stringify(value) ?? 'null'
  } catch {
    return String(value)
  }
}

/** `JSONValue.parse(_:)`: `null` for text that is not JSON. */
export function jsonParse(text: string): JSONValue | null {
  try {
    return JSON.parse(text) as JSONValue
  } catch {
    return null
  }
}

function sortedKeys(value: JSONValue): JSONValue {
  if (Array.isArray(value)) return value.map(sortedKeys)
  if (value !== null && typeof value === 'object') {
    const out: JSONObject = {}
    for (const key of Object.keys(value).sort()) {
      const member = value[key]
      if (member !== undefined) out[key] = sortedKeys(member)
    }
    return out
  }
  return value
}

// MARK: - Timestamps

/// `RFC3339.parse`: the backend's `to_rfc3339()` output (any fractional
/// precision) as a `Date`, or `null`.
export function parseRFC3339(text: string): Date | null {
  const direct = Date.parse(text)
  if (!Number.isNaN(direct)) return new Date(direct)
  // Trim the fraction to three digits: engines only promise milliseconds.
  const dot = text.indexOf('.')
  if (dot < 0) return null
  const rest = text.slice(dot + 1)
  const endOffset = rest.search(/[Z+-]/)
  if (endOffset < 0) return null
  const fraction = rest.slice(0, endOffset)
  const kept = fraction.slice(0, 3).padEnd(3, '0')
  const trimmed = `${text.slice(0, dot)}.${kept}${rest.slice(endOffset)}`
  const parsed = Date.parse(trimmed)
  if (!Number.isNaN(parsed)) return new Date(parsed)
  const plain = Date.parse(`${text.slice(0, dot)}${rest.slice(endOffset)}`)
  return Number.isNaN(plain) ? null : new Date(plain)
}

// MARK: - Enumerations

/** `ExecutionState`: the runtime's task state machine. */
export type ExecutionState =
  | 'IDLE'
  | 'PREPARING'
  | 'INVESTIGATING'
  | 'IMPLEMENTING'
  | 'VALIDATING'
  | 'RECOVERING'
  | 'COMPLETED'
  | 'BLOCKED'
  | 'CANCELLED'
  | 'FAILED'

export const EXECUTION_STATES: readonly ExecutionState[] = [
  'IDLE',
  'PREPARING',
  'INVESTIGATING',
  'IMPLEMENTING',
  'VALIDATING',
  'RECOVERING',
  'COMPLETED',
  'BLOCKED',
  'CANCELLED',
  'FAILED',
]

/** `ExecutionState(from:)`: unknown or differently cased values decode as `IDLE`. */
export function parseExecutionState(raw: unknown): ExecutionState {
  const upper = typeof raw === 'string' ? raw.toUpperCase() : ''
  return (EXECUTION_STATES as readonly string[]).includes(upper) ? (upper as ExecutionState) : 'IDLE'
}

/** `ExecutionState.isTerminal`. */
export function isTerminalExecutionState(state: ExecutionState): boolean {
  return state === 'COMPLETED' || state === 'BLOCKED' || state === 'CANCELLED' || state === 'FAILED'
}

/** `ExecutionState.label`: "Investigating". */
export function executionStateLabel(state: ExecutionState): string {
  return state.charAt(0) + state.slice(1).toLowerCase()
}

/** `TaskStatus`. */
export type TaskStatus = 'completed' | 'blocked' | 'failed' | 'cancelled'

export const TASK_STATUSES: readonly TaskStatus[] = ['completed', 'blocked', 'failed', 'cancelled']

/** `TaskStatus(from:)`: unknown values decode as `failed`. */
export function parseTaskStatus(raw: unknown): TaskStatus {
  const lower = typeof raw === 'string' ? raw.toLowerCase() : ''
  return (TASK_STATUSES as readonly string[]).includes(lower) ? (lower as TaskStatus) : 'failed'
}

/** `TaskStatus.label`: "Completed". */
export function taskStatusLabel(status: TaskStatus): string {
  return status.charAt(0).toUpperCase() + status.slice(1)
}

/** `RiskLevel` of an approval request. */
export type RiskLevel = 'read_only' | 'write' | 'execute' | 'network' | 'destructive'

/** `RiskLevel(from:)`: unknown values decode as `execute`. */
export function parseRiskLevel(raw: unknown): RiskLevel {
  switch (raw) {
    case 'read_only':
    case 'write':
    case 'execute':
    case 'network':
    case 'destructive':
      return raw
    default:
      return 'execute'
  }
}

/** `RiskLevel.label`: "read-only" for `read_only`, else the raw value. */
export function riskLevelLabel(risk: RiskLevel): string {
  return risk === 'read_only' ? 'read-only' : risk
}

/** `ApprovalDecision`. */
export type ApprovalDecision = 'approve' | 'deny' | 'approve_for_session'

/** `FileChangeKind`. */
export type FileChangeKind = 'created' | 'modified' | 'deleted'

/** `FileChangeKind(from:)`: unknown values decode as `modified`. */
export function parseFileChangeKind(raw: unknown): FileChangeKind {
  return raw === 'created' || raw === 'deleted' ? raw : 'modified'
}

/** `UsageSource`. */
export type UsageSource = 'reported' | 'estimated'

/** `UsageSource(from:)`: unknown values decode as `estimated`. */
export function parseUsageSource(raw: unknown): UsageSource {
  return raw === 'reported' ? 'reported' : 'estimated'
}

// MARK: - Shared records

/** `TokenUsage`: the two optional counters are present and `null` when unknown. */
export interface TokenUsage {
  prompt_tokens: number
  completion_tokens: number
  cached_prompt_tokens?: number | null
  reasoning_tokens?: number | null
  source: UsageSource
}

/** `TokenUsage.total`. */
export function tokenUsageTotal(usage: TokenUsage): number {
  return usage.prompt_tokens + usage.completion_tokens
}

/** `TokenUsage.zero`. */
export const TOKEN_USAGE_ZERO: TokenUsage = {
  prompt_tokens: 0,
  completion_tokens: 0,
  cached_prompt_tokens: null,
  reasoning_tokens: null,
  source: 'estimated',
}

/** `ValidationRecord`: one validation command's run. */
export interface ValidationRecord {
  command: string
  passed: boolean
  exit_code?: number | null
  duration_ms: number
  summary: string
}

/** `TaskOutcome`: `cost_usd` is absent (not null) when no request had a price. */
export interface TaskOutcome {
  task_id: string
  status: TaskStatus
  summary: string
  modified_files: string[]
  validation: ValidationRecord[]
  unresolved: string[]
  steps: number
  model_calls: number
  tool_calls: number
  usage: TokenUsage
  cost_usd?: number | null
  duration_ms: number
}

/** `ContextStats` of a `context_compiled` event. */
export interface ContextStats {
  estimated_prompt_tokens: number
  stable_prefix_tokens: number
  repository_snippets: number
  repository_tokens: number
  memory_records: number
  memory_tokens: number
  conversation_messages: number
  conversation_tokens: number
  retrieval_budget_tokens: number
  context_window: number
  compacted: boolean
}

/** `AgentMode`: what a sub-agent may do. */
export type AgentMode = 'explore' | 'implement'

/** `AgentMode(from:)`: unknown values decode as `explore`. */
export function parseAgentMode(raw: unknown): AgentMode {
  return raw === 'implement' ? 'implement' : 'explore'
}

/** `AgentMode.label`: "Explore". */
export function agentModeLabel(mode: AgentMode): string {
  return mode.charAt(0).toUpperCase() + mode.slice(1)
}

/** `AgentReport`: a finished sub-agent's report (`agent_finished.report`). */
export interface AgentReport {
  agent_id: string
  name: string
  mode: AgentMode
  status: TaskStatus
  summary: string
  modified_files: string[]
  unresolved: string[]
  steps: number
  tool_calls: number
  usage: TokenUsage
  duration_ms: number
}

/** `ApprovalRequest` of an `approval_requested` event. */
export interface ApprovalRequest {
  id: string
  tool: string
  risk: RiskLevel
  summary: string
  details: string
  reason: string
}

/// `ApprovalRequest.discountDropTool`: the question asked when ModelGateway
/// would only go on at a much worse discount.
export const DISCOUNT_DROP_TOOL = 'discount_drop'

/** `ApprovalRequest.isDiscountDrop`. */
export function isDiscountDrop(request: ApprovalRequest): boolean {
  return request.tool === DISCOUNT_DROP_TOOL
}

/** `ToolCallInfo`: one tool call of an assistant history message. */
export interface ToolCallInfo {
  id: string
  name: string
  arguments: JSONValue
}

// MARK: - Sent prompt

/** `SentPrompt.Message`: one message of a model request (developer mode). */
export interface SentPromptMessage {
  /** `policy`, `task_instructions`, `context`, `user`, `assistant` or `tool_result`. */
  kind: string
  text: string
  estimated_tokens: number
  images: number
}

/** `SentPrompt` (`prompt_sent`): every message of a model request and the tools it offered. */
export interface SentPrompt {
  step: number
  messages: SentPromptMessage[]
  tools: string[]
}

/** `SentPrompt.estimatedTokens`. */
export function sentPromptEstimatedTokens(prompt: SentPrompt): number {
  return prompt.messages.reduce((sum, message) => sum + message.estimated_tokens, 0)
}

/** `SentPrompt.plainText`: the whole prompt as plain text (Copy). */
export function sentPromptPlainText(prompt: SentPrompt): string {
  const parts = prompt.messages.map((message) => `### ${message.kind}\n\n${message.text}`)
  if (prompt.tools.length > 0) parts.push('### tools\n\n' + prompt.tools.join(', '))
  return parts.join('\n\n')
}

// MARK: - AgentEvent

/// Mirrors `ctx_core::AgentEvent` (tag field `event`), as the Swift decoder
/// leaves it: missing optional fields carry their defaults. Unknown variants
/// decode as `unknown` so a newer backend never breaks the UI.
export type AgentEvent =
  | { event: 'task_started'; task_id: string; objective: string; continued: boolean }
  | { event: 'state_changed'; task_id: string; from: ExecutionState; to: ExecutionState }
  | { event: 'context_compiled'; task_id: string; stats: ContextStats }
  | { event: 'model_request_started'; task_id: string; step: number; estimated_prompt_tokens: number }
  /** The full prompt of the run's first model request (developer mode). */
  | { event: 'prompt_sent'; task_id: string; prompt: SentPrompt }
  | { event: 'model_text_delta'; task_id: string; text: string }
  | { event: 'model_reasoning_delta'; task_id: string; text: string }
  | { event: 'model_response'; task_id: string; text: string | null; tool_call_count: number; usage: TokenUsage | null }
  | { event: 'tool_call_started'; task_id: string; call_id: string; name: string; arguments: JSONValue }
  | { event: 'tool_call_finished'; task_id: string; call_id: string; name: string; success: boolean; summary: string; duration_ms: number }
  | { event: 'tool_call_output'; task_id: string; call_id: string; name: string; content: string; full_len: number; is_error: boolean }
  | { event: 'approval_requested'; task_id: string; request: ApprovalRequest }
  | { event: 'approval_resolved'; task_id: string; request_id: string; decision: ApprovalDecision }
  | { event: 'validation_started'; task_id: string; command: string }
  | { event: 'validation_finished'; task_id: string; record: ValidationRecord }
  | { event: 'file_changed'; task_id: string; path: string; kind: FileChangeKind }
  | { event: 'checkpoint'; task_id: string; checkpoint_id: string; reason: string }
  | { event: 'warning'; task_id: string; message: string }
  | { event: 'usage'; task_id: string; cumulative: TokenUsage; cost_usd: number | null }
  /** A message sent to the running turn (`task/steer`) reached the model. */
  | { event: 'steered'; task_id: string; text: string; images: number; files: number }
  /** A sub-agent deployed by the Ultra effort tier; `agent_task_id` is the task its own events carry. */
  | { event: 'agent_started'; task_id: string; agent_id: string; agent_task_id: string; name: string; mode: AgentMode; objective: string }
  /** One of a sub-agent's own events, wrapped into the lead task's stream. */
  | { event: 'agent_progress'; task_id: string; agent_id: string; inner: AgentEvent }
  | { event: 'agent_finished'; task_id: string; agent_id: string; report: AgentReport }
  | { event: 'task_completed'; task_id: string; outcome: TaskOutcome }
  /** `AgentEvent.unknown(name:taskId:)`: a variant this app does not know. */
  | { event: 'unknown'; name: string; task_id: string | null }

/** `AgentEvent.name`: the wire tag (the raw name for `unknown`). */
export function agentEventName(event: AgentEvent): string {
  return event.event === 'unknown' ? event.name : event.event
}

/** `AgentEvent.taskId`. */
export function agentEventTaskId(event: AgentEvent): string | null {
  return event.task_id
}

/** What `decodeAgentEvent` throws: the Swift `DecodingError`, as a message. */
export class AgentEventDecodeError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AgentEventDecodeError'
  }
}

/// Port of `AgentEvent.init(from:)`: decodes one wire event, applying the
/// same defaults, and throws `AgentEventDecodeError` for a malformed one.
export function decodeAgentEvent(raw: JSONValue): AgentEvent {
  const object = jsonObject(raw)
  if (!object) throw new AgentEventDecodeError('expected an object at root')
  const kind = jsonString(object.event)
  if (kind === null) throw new AgentEventDecodeError('missing key "event" at root')
  const taskId = jsonString(object.task_id)
  const id = (): string => {
    if (taskId === null) throw new AgentEventDecodeError(`${kind}: task_id missing`)
    return taskId
  }
  const str = (key: string): string => {
    const value = jsonString(object[key])
    if (value === null) throw new AgentEventDecodeError(`${kind}: missing key "${key}"`)
    return value
  }
  const strOr = (key: string, fallback: string): string => jsonString(object[key]) ?? fallback
  const intOr = (key: string, fallback: number): number => jsonInt(object[key]) ?? fallback
  const boolOr = (key: string, fallback: boolean): boolean => jsonBool(object[key]) ?? fallback
  const obj = <T>(key: string, decode: (value: JSONObject) => T): T => {
    const value = jsonObject(object[key])
    if (!value) throw new AgentEventDecodeError(`${kind}: missing key "${key}"`)
    return decode(value)
  }
  const objOrNull = <T>(key: string, decode: (value: JSONObject) => T): T | null => {
    const value = jsonObject(object[key])
    return value ? decode(value) : null
  }

  switch (kind) {
    case 'task_started':
      return { event: kind, task_id: id(), objective: str('objective'), continued: boolOr('continued', false) }
    case 'state_changed':
      return { event: kind, task_id: id(), from: parseExecutionState(object.from), to: parseExecutionState(object.to) }
    case 'context_compiled':
      return { event: kind, task_id: id(), stats: obj('stats', decodeContextStats) }
    case 'model_request_started':
      return { event: kind, task_id: id(), step: intOr('step', 0), estimated_prompt_tokens: intOr('estimated_prompt_tokens', 0) }
    case 'prompt_sent': {
      const messages = (jsonArray(object.messages) ?? []).map(decodeSentPromptMessage)
      const tools = (jsonArray(object.tools) ?? []).flatMap((tool) => (typeof tool === 'string' ? [tool] : []))
      return { event: kind, task_id: id(), prompt: { step: intOr('step', 0), messages, tools } }
    }
    case 'model_text_delta':
      return { event: kind, task_id: id(), text: strOr('text', '') }
    case 'model_reasoning_delta':
      return { event: kind, task_id: id(), text: strOr('text', '') }
    case 'model_response':
      return {
        event: kind,
        task_id: id(),
        text: jsonString(object.text),
        tool_call_count: intOr('tool_call_count', 0),
        usage: objOrNull('usage', decodeTokenUsage),
      }
    case 'tool_call_started':
      return { event: kind, task_id: id(), call_id: str('call_id'), name: str('name'), arguments: object.arguments ?? null }
    case 'tool_call_finished':
      return {
        event: kind,
        task_id: id(),
        call_id: str('call_id'),
        name: str('name'),
        success: boolOr('success', false),
        summary: strOr('summary', ''),
        duration_ms: intOr('duration_ms', 0),
      }
    case 'tool_call_output':
      return {
        event: kind,
        task_id: id(),
        call_id: str('call_id'),
        name: strOr('name', ''),
        content: strOr('content', ''),
        full_len: intOr('full_len', 0),
        is_error: boolOr('is_error', false),
      }
    case 'approval_requested':
      return { event: kind, task_id: id(), request: obj('request', decodeApprovalRequest) }
    case 'approval_resolved':
      return { event: kind, task_id: id(), request_id: str('request_id'), decision: decodeApprovalDecision(object.decision, kind) }
    case 'validation_started':
      return { event: kind, task_id: id(), command: strOr('command', '') }
    case 'validation_finished':
      return { event: kind, task_id: id(), record: obj('record', decodeValidationRecord) }
    case 'file_changed':
      return { event: kind, task_id: id(), path: str('path'), kind: parseFileChangeKind(object.kind) }
    case 'checkpoint':
      return { event: kind, task_id: id(), checkpoint_id: strOr('checkpoint_id', ''), reason: strOr('reason', '') }
    case 'warning':
      return { event: kind, task_id: id(), message: strOr('message', '') }
    case 'usage':
      return { event: kind, task_id: id(), cumulative: obj('cumulative', decodeTokenUsage), cost_usd: jsonNumber(object.cost_usd) }
    case 'steered':
      return { event: kind, task_id: id(), text: strOr('text', ''), images: intOr('images', 0), files: intOr('files', 0) }
    case 'task_completed':
      return { event: kind, task_id: id(), outcome: obj('outcome', decodeTaskOutcome) }
    case 'agent_started':
      return {
        event: kind,
        task_id: id(),
        agent_id: str('agent_id'),
        agent_task_id: strOr('agent_task_id', ''),
        name: strOr('name', 'agent'),
        mode: parseAgentMode(object.mode),
        objective: strOr('objective', ''),
      }
    case 'agent_progress': {
      const inner = object.inner
      if (inner === undefined) throw new AgentEventDecodeError(`${kind}: missing key "inner"`)
      return { event: kind, task_id: id(), agent_id: str('agent_id'), inner: decodeAgentEvent(inner) }
    }
    case 'agent_finished':
      return { event: kind, task_id: id(), agent_id: str('agent_id'), report: obj('report', decodeAgentReport) }
    default:
      return { event: 'unknown', name: kind, task_id: taskId }
  }
}

function requireString(object: JSONObject, key: string, context: string): string {
  const value = jsonString(object[key])
  if (value === null) throw new AgentEventDecodeError(`${context}: missing key "${key}"`)
  return value
}

function requireInt(object: JSONObject, key: string, context: string): number {
  const value = jsonInt(object[key])
  if (value === null) throw new AgentEventDecodeError(`${context}: missing key "${key}"`)
  return value
}

function requireBool(object: JSONObject, key: string, context: string): boolean {
  const value = jsonBool(object[key])
  if (value === null) throw new AgentEventDecodeError(`${context}: missing key "${key}"`)
  return value
}

function stringList(value: JSONValue | undefined): string[] {
  return (jsonArray(value) ?? []).flatMap((item) => (typeof item === 'string' ? [item] : []))
}

/** Decodes a `TokenUsage` object (`cached_prompt_tokens` / `reasoning_tokens` null when absent). */
export function decodeTokenUsage(object: JSONObject): TokenUsage {
  return {
    prompt_tokens: requireInt(object, 'prompt_tokens', 'TokenUsage'),
    completion_tokens: requireInt(object, 'completion_tokens', 'TokenUsage'),
    cached_prompt_tokens: jsonInt(object.cached_prompt_tokens),
    reasoning_tokens: jsonInt(object.reasoning_tokens),
    source: parseUsageSource(object.source),
  }
}

function decodeContextStats(object: JSONObject): ContextStats {
  const int = (key: string) => requireInt(object, key, 'ContextStats')
  return {
    estimated_prompt_tokens: int('estimated_prompt_tokens'),
    stable_prefix_tokens: int('stable_prefix_tokens'),
    repository_snippets: int('repository_snippets'),
    repository_tokens: int('repository_tokens'),
    memory_records: int('memory_records'),
    memory_tokens: int('memory_tokens'),
    conversation_messages: int('conversation_messages'),
    conversation_tokens: int('conversation_tokens'),
    retrieval_budget_tokens: int('retrieval_budget_tokens'),
    context_window: int('context_window'),
    compacted: requireBool(object, 'compacted', 'ContextStats'),
  }
}

/** `SentPrompt.Message.init(from:)`: every field has a default. */
function decodeSentPromptMessage(raw: JSONValue): SentPromptMessage {
  const object = jsonObject(raw) ?? {}
  return {
    kind: jsonString(object.kind) ?? 'context',
    text: jsonString(object.text) ?? '',
    estimated_tokens: jsonInt(object.estimated_tokens) ?? 0,
    images: jsonInt(object.images) ?? 0,
  }
}

function decodeApprovalRequest(object: JSONObject): ApprovalRequest {
  const str = (key: string) => requireString(object, key, 'ApprovalRequest')
  return {
    id: str('id'),
    tool: str('tool'),
    risk: parseRiskLevel(object.risk),
    summary: str('summary'),
    details: str('details'),
    reason: str('reason'),
  }
}

function decodeApprovalDecision(raw: JSONValue | undefined, context: string): ApprovalDecision {
  if (raw === 'approve' || raw === 'deny' || raw === 'approve_for_session') return raw
  throw new AgentEventDecodeError(`${context}: unknown decision ${jsonCompact(raw ?? null)}`)
}

/** Decodes a `ValidationRecord` object. */
export function decodeValidationRecord(object: JSONObject): ValidationRecord {
  return {
    command: requireString(object, 'command', 'ValidationRecord'),
    passed: requireBool(object, 'passed', 'ValidationRecord'),
    exit_code: jsonInt(object.exit_code),
    duration_ms: requireInt(object, 'duration_ms', 'ValidationRecord'),
    summary: requireString(object, 'summary', 'ValidationRecord'),
  }
}

/** Decodes a `TaskOutcome` object. */
export function decodeTaskOutcome(object: JSONObject): TaskOutcome {
  const usage = jsonObject(object.usage)
  if (!usage) throw new AgentEventDecodeError('TaskOutcome: missing key "usage"')
  const validation = (jsonArray(object.validation) ?? []).map((record) => {
    const value = jsonObject(record)
    if (!value) throw new AgentEventDecodeError('TaskOutcome: validation entry is not an object')
    return decodeValidationRecord(value)
  })
  return {
    task_id: requireString(object, 'task_id', 'TaskOutcome'),
    status: parseTaskStatus(object.status),
    summary: requireString(object, 'summary', 'TaskOutcome'),
    modified_files: stringList(object.modified_files),
    validation,
    unresolved: stringList(object.unresolved),
    steps: requireInt(object, 'steps', 'TaskOutcome'),
    model_calls: requireInt(object, 'model_calls', 'TaskOutcome'),
    tool_calls: requireInt(object, 'tool_calls', 'TaskOutcome'),
    usage: decodeTokenUsage(usage),
    cost_usd: jsonNumber(object.cost_usd),
    duration_ms: requireInt(object, 'duration_ms', 'TaskOutcome'),
  }
}

function decodeAgentReport(object: JSONObject): AgentReport {
  const usage = jsonObject(object.usage)
  if (!usage) throw new AgentEventDecodeError('AgentReport: missing key "usage"')
  return {
    agent_id: requireString(object, 'agent_id', 'AgentReport'),
    name: requireString(object, 'name', 'AgentReport'),
    mode: parseAgentMode(object.mode),
    status: parseTaskStatus(object.status),
    summary: requireString(object, 'summary', 'AgentReport'),
    modified_files: stringList(object.modified_files),
    unresolved: stringList(object.unresolved),
    steps: requireInt(object, 'steps', 'AgentReport'),
    tool_calls: requireInt(object, 'tool_calls', 'AgentReport'),
    usage: decodeTokenUsage(usage),
    duration_ms: requireInt(object, 'duration_ms', 'AgentReport'),
  }
}

// MARK: - Configuration

/** `ModelSettings`: the user-level `[model]` table. */
export interface ModelSettings {
  name: string
  base_url: string
  api_key_env: string
  api_key_set: boolean
  context_window: number
  max_output_tokens: number
  reasoning_effort?: string | null
  /** `model.credential`: the stored credential the key comes from ("modelgateway" while signed in). */
  credential?: string | null
}

/** `PermissionSettings`: the user-level `[permissions]` table. */
export interface PermissionSettings {
  profile: string
  allowed_commands: string[]
  denied_commands: string[]
  allow_network_commands: boolean
}

/** `ModelPrice`: USD per million tokens for one model; cached input falls back to `input`. */
export interface ModelPrice {
  input: number
  output: number
  cache_read?: number | null
}

/// `PricingSettings` (`[pricing]`): the user's own prices. `models` prices
/// one model each; the flat prices are the fallback for models with no other price.
export interface PricingSettings {
  input_per_million?: number | null
  cached_input_per_million?: number | null
  output_per_million?: number | null
  models: { [model: string]: ModelPrice }
}

/** `PricingSettings.init(from:)` defaults. */
export function normalizePricingSettings(raw: Partial<PricingSettings> | null | undefined): PricingSettings {
  return {
    input_per_million: raw?.input_per_million ?? null,
    cached_input_per_million: raw?.cached_input_per_million ?? null,
    output_per_million: raw?.output_per_million ?? null,
    models: raw?.models ?? {},
  }
}

/** `PricingSettings.flat`: the flat prices, when input and output are both set. */
export function pricingFlat(pricing: PricingSettings): ModelPrice | null {
  if (pricing.input_per_million == null || pricing.output_per_million == null) return null
  return { input: pricing.input_per_million, output: pricing.output_per_million, cache_read: pricing.cached_input_per_million ?? null }
}

/** `SpendPeriod`: the window spending limits count over. */
export type SpendPeriod = 'day' | 'week' | 'month'

export const SPEND_PERIODS: readonly SpendPeriod[] = ['day', 'week', 'month']

/** `SpendPeriod.title`: "Per day". */
export function spendPeriodTitle(period: SpendPeriod): string {
  switch (period) {
    case 'day':
      return 'Per day'
    case 'week':
      return 'Per week'
    case 'month':
      return 'Per month'
  }
}

/** `SpendPeriod.phrase`: "today", "this week", "this month". */
export function spendPeriodPhrase(period: SpendPeriod): string {
  switch (period) {
    case 'day':
      return 'today'
    case 'week':
      return 'this week'
    case 'month':
      return 'this month'
  }
}

/** `SpendSettings` (`[spend]`): caps on what the agent may spend per period. */
export interface SpendSettings {
  period: SpendPeriod
  max_usd?: number | null
  models: { [model: string]: number }
  /** Ask before ModelGateway falls back to a much lower discount; false = always continue. */
  confirm_discount_drop: boolean
}

/** `SpendSettings()` / `init(from:)` defaults. */
export function normalizeSpendSettings(raw: Partial<SpendSettings> | null | undefined): SpendSettings {
  const period = raw?.period
  return {
    period: period === 'day' || period === 'week' || period === 'month' ? period : 'month',
    max_usd: raw?.max_usd ?? null,
    models: raw?.models ?? {},
    confirm_discount_drop: raw?.confirm_discount_drop ?? true,
  }
}

/** `SpendSummary` (`spend/summary`): this period's spend against the limits. */
export interface SpendSummary {
  period: SpendPeriod
  since: string
  total_usd: number
  max_usd?: number | null
  models: ModelSpend[]
}

/** `ModelSpend`: one model's spend this period. */
export interface ModelSpend {
  model: string
  cost_usd: number
  requests: number
  unpriced_requests: number
  limit_usd?: number | null
}

/** `PriceEntry`: one `pricing/lookup` row. */
export interface PriceEntry {
  model: string
  price?: ModelPrice | null
  /** "custom" (Settings), "catalog" (models.dev), "default" (the flat prices). */
  source?: string | null
}

/** `PriceLookupResult`. */
export interface PriceLookupResult {
  prices: PriceEntry[]
}

/** `InitializeResult`: result of `initialize` and `config/get`. */
export interface InitializeResult {
  protocol_version: number
  ctx_version: string
  data_dir: string
  config_file: string
  config_sources: string[]
  mock: boolean
  model: ModelSettings
  permissions: PermissionSettings
  pricing?: PricingSettings | null
  spend: SpendSettings
  mcp: McpSettings
}

/// `InitializeResult.init(from:)`: backends older than spending limits or
/// MCP support leave `spend` / `mcp` out.
export function normalizeInitializeResult(raw: InitializeResult): InitializeResult {
  return {
    ...raw,
    pricing: raw.pricing == null ? null : normalizePricingSettings(raw.pricing),
    spend: normalizeSpendSettings(raw.spend),
    mcp: normalizeMcpSettings(raw.mcp),
  }
}

// MARK: - MCP

/** `McpSettings`: the user-level `[mcp]` table. */
export interface McpSettings {
  enabled: boolean
  startup_timeout_secs: number
  call_timeout_secs: number
  servers: { [name: string]: McpServerConfig }
  /** Default apps the user removed (the backend would otherwise add them back). */
  uninstalled: string[]
}

/** `McpSettings()` / `init(from:)` defaults. */
export function normalizeMcpSettings(raw: Partial<McpSettings> | null | undefined): McpSettings {
  const servers: { [name: string]: McpServerConfig } = {}
  for (const [name, config] of Object.entries(raw?.servers ?? {})) servers[name] = normalizeMcpServerConfig(config)
  return {
    enabled: raw?.enabled ?? true,
    startup_timeout_secs: raw?.startup_timeout_secs ?? 30,
    call_timeout_secs: raw?.call_timeout_secs ?? 120,
    servers,
    uninstalled: raw?.uninstalled ?? [],
  }
}

/** `McpSettings.serverNames`: sorted. */
export function mcpServerNames(settings: McpSettings): string[] {
  return Object.keys(settings.servers).sort()
}

/** `McpTransport`: how an MCP server is reached. */
export type McpTransport = 'stdio' | 'http' | 'builtin'

export const MCP_TRANSPORTS: readonly McpTransport[] = ['stdio', 'http', 'builtin']

/** `McpTransport.title`. */
export function mcpTransportTitle(transport: McpTransport): string {
  switch (transport) {
    case 'stdio':
      return 'Command'
    case 'http':
      return 'HTTP'
    case 'builtin':
      return 'Built in'
  }
}

/** `McpToolPermission` (`[mcp.servers.<name>.permissions]`): one tool's override. */
export type McpToolPermission = 'allow' | 'ask'

/// `McpServerConfig`: one `[mcp.servers.<name>]` entry; `config/patch`
/// writes it whole under `mcp.servers.<name>` (or `null` to remove it).
export interface McpServerConfig {
  enabled: boolean
  transport: McpTransport
  command?: string | null
  args: string[]
  env: { [name: string]: string }
  cwd?: string | null
  url?: string | null
  headers: { [name: string]: string }
  tools: string[]
  /** Tool patterns never offered to the model (`*` wildcards). */
  exclude_tools: string[]
  /** builtin: which server ctx provides (`computer-use`). */
  builtin?: string | null
  /** The catalog app this entry was installed from. */
  app?: string | null
  /** Every tool runs without asking (a tool set to `ask` still asks). */
  bypass_permissions: boolean
  /** Per-tool overrides by the server's tool name. */
  permissions: { [tool: string]: McpToolPermission }
}

/** `McpServerConfig.init(from:)` defaults. */
export function normalizeMcpServerConfig(raw: Partial<McpServerConfig> | null | undefined): McpServerConfig {
  const transport = raw?.transport
  return {
    enabled: raw?.enabled ?? true,
    transport: transport === 'http' || transport === 'builtin' ? transport : 'stdio',
    command: raw?.command ?? null,
    args: raw?.args ?? [],
    env: raw?.env ?? {},
    cwd: raw?.cwd ?? null,
    url: raw?.url ?? null,
    headers: raw?.headers ?? {},
    tools: raw?.tools ?? [],
    exclude_tools: raw?.exclude_tools ?? [],
    builtin: raw?.builtin ?? null,
    app: raw?.app ?? null,
    bypass_permissions: raw?.bypass_permissions ?? false,
    permissions: raw?.permissions ?? {},
  }
}

/** `McpServerConfig.target`: the command line or URL, as the backend lists it. */
export function mcpServerTarget(config: McpServerConfig): string {
  switch (config.transport) {
    case 'stdio':
      return [config.command ?? '', ...config.args].join(' ')
    case 'http':
      return config.url ?? ''
    case 'builtin':
      return `built into ctx (${config.builtin ?? ''})`
  }
}

/** `McpServerConfig.patchValue`: the `config/patch` value, only the keys the backend would write. */
export function mcpServerPatchValue(config: McpServerConfig): JSONValue {
  const object: JSONObject = { enabled: config.enabled, transport: config.transport }
  switch (config.transport) {
    case 'stdio':
      object.command = config.command ?? ''
      if (config.args.length > 0) object.args = [...config.args]
      if (Object.keys(config.env).length > 0) object.env = { ...config.env }
      if (config.cwd) object.cwd = config.cwd
      break
    case 'http':
      object.url = config.url ?? ''
      if (Object.keys(config.headers).length > 0) object.headers = { ...config.headers }
      break
    case 'builtin':
      object.builtin = config.builtin ?? ''
      break
  }
  if (config.tools.length > 0) object.tools = [...config.tools]
  if (config.exclude_tools.length > 0) object.exclude_tools = [...config.exclude_tools]
  if (config.app != null) object.app = config.app
  if (config.bypass_permissions) object.bypass_permissions = true
  if (Object.keys(config.permissions).length > 0) object.permissions = { ...config.permissions }
  return object
}

/** `McpCatalogApp`: one curated app of `mcp/catalog`. */
export interface McpCatalogApp {
  id: string
  name: string
  tagline: string
  /** A prompt that shows what the app is for. */
  example_prompt: string
  description: string
  icon: string
  /** Brand logo in the asset catalog (`logo-<name>`); null keeps the symbol. */
  logo?: string | null
  /** A single-colour mark, tinted for the current appearance. */
  logo_monochrome: boolean
  category: string
  vendor: string
  homepage?: string | null
  builtin: boolean
  default_installed: boolean
  requirements: string[]
  inputs: McpCatalogInput[]
  /** "What Context can access": tool groups for the connect screen. */
  access: McpCatalogAccess[]
  /** Sign-in in place of the token input (GitHub device flow). */
  oauth?: McpCatalogOAuth | null
  template: McpServerConfig
}

/** `McpCatalogApp.init(from:)` defaults. */
export function normalizeMcpCatalogApp(raw: McpCatalogApp): McpCatalogApp {
  return {
    ...raw,
    example_prompt: raw.example_prompt ?? '',
    logo: raw.logo ?? null,
    logo_monochrome: raw.logo_monochrome ?? false,
    homepage: raw.homepage ?? null,
    requirements: raw.requirements ?? [],
    inputs: raw.inputs ?? [],
    access: raw.access ?? [],
    oauth: raw.oauth ?? null,
    template: normalizeMcpServerConfig(raw.template),
  }
}

/** `McpCatalogOAuth` (`CatalogApp.oauth`): the app can sign in instead of taking a token. */
export interface McpCatalogOAuth {
  provider: string
  /** Credential-store entry the token goes to. */
  credential: string
  /** The input the sign-in fills, with `${credential:<credential>}`. */
  input: string
  scopes: string[]
  /** False when this build has no OAuth client ID: fall back to the token field. */
  available: boolean
}

/** `McpCatalogOAuth.reference`: `${credential:<credential>}`. */
export function mcpOAuthReference(oauth: McpCatalogOAuth): string {
  return '${credential:' + oauth.credential + '}'
}

/** `McpDeviceCode` (`mcp/oauth/start`). */
export interface McpDeviceCode {
  device_code: string
  user_code: string
  verification_uri: string
  expires_in: number
  interval: number
}

/** `McpDevicePoll` (`mcp/oauth/poll`). */
export interface McpDevicePoll {
  /** `pending`, `approved`, `denied` or `expired`. */
  status: string
  interval?: number | null
  account?: string | null
}

/** `McpCatalogAccess`: one "What Context can access" line, a group of the app's tools. */
export interface McpCatalogAccess {
  title: string
  description: string
  /** Tool-name patterns (`*` wildcards); unchecked groups go to `exclude_tools`. */
  tools: string[]
  required: boolean
}

/** `McpCatalogInput`: one value the connect page asks for. */
export interface McpCatalogInput {
  key: string
  label: string
  help: string
  secret: boolean
  required: boolean
  /** `env` or `header`. */
  kind: string
}

/** `McpCatalogResponse` (`mcp/catalog`). */
export interface McpCatalogResponse {
  apps: McpCatalogApp[]
}

/** `McpServerStatus.State`. */
export type McpServerState = 'disabled' | 'connecting' | 'connected' | 'failed'

/** `McpServerStatus`: one configured server (`mcp/status` and the notification). */
export interface McpServerStatus {
  name: string
  enabled: boolean
  app?: string | null
  state: McpServerState
  error?: string | null
  transport: string
  target: string
  server_name?: string | null
  server_version?: string | null
  protocol_version?: string | null
  instructions?: string | null
  tools: McpToolInfo[]
}

/** `McpServerStatus` defaults (`tools` empty when absent). */
export function normalizeMcpServerStatus(raw: McpServerStatus): McpServerStatus {
  return { ...raw, tools: (raw.tools ?? []).map(normalizeMcpToolInfo) }
}

/** `McpToolInfo`: one tool of a connected MCP server. */
export interface McpToolInfo {
  name: string
  qualified_name: string
  description: string
  read_only: boolean
  destructive: boolean
  /** False for a tool the server offers but the entry's `tools` allowlist leaves out. */
  enabled: boolean
}

/** `McpToolInfo.init(from:)` defaults. */
export function normalizeMcpToolInfo(raw: McpToolInfo): McpToolInfo {
  return {
    name: raw.name,
    qualified_name: raw.qualified_name,
    description: raw.description ?? '',
    read_only: raw.read_only ?? false,
    destructive: raw.destructive ?? false,
    enabled: raw.enabled ?? true,
  }
}

/** `McpStatusNotification`: `mcp/status` result and notification. */
export interface McpStatusNotification {
  servers: McpServerStatus[]
}

/** `mcp/oauth/signout` result (the Swift client ignores it). */
export interface McpSignOutResult {
  signed_out: boolean
}

/** `McpPermissions` (`mcp/permissions`): the Computer Use permissions as the backend sees them. */
export interface McpPermissions {
  screen_recording: boolean
  accessibility: boolean
  platform: string
}

// MARK: - Projects

/** `GitInfo` (`ProjectInfo.git`). */
export interface GitInfo {
  branch?: string | null
  head?: string | null
  dirty_files: number
  /** Set when the root is a linked worktree of another checkout. */
  worktree?: WorktreeInfo | null
}

/** `GitInfo.shortHead`: the first 8 characters. */
export function gitShortHead(git: GitInfo): string | null {
  return git.head ? git.head.slice(0, 8) : null
}

/** `WorktreeInfo` (`GitSummary.worktree`): the parent checkout of a linked worktree. */
export interface WorktreeInfo {
  parent_root: string
  /** Created by `worktree/create` (lives in the data dir). */
  managed: boolean
}

/** `WorktreeEntry`: one entry of `worktree/list`. */
export interface WorktreeEntry {
  path: string
  branch?: string | null
  head?: string | null
  is_main: boolean
  managed: boolean
}

/** `WorktreeEntry.name`: the last path component. */
export function worktreeEntryName(entry: WorktreeEntry): string {
  const parts = entry.path.split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? entry.path
}

/** `WorktreeRemoveResult`. */
export interface WorktreeRemoveResult {
  removed: string
  branch?: string | null
  branch_deleted: boolean
}

/** `WorktreeMergeResult` (`worktree/merge`): the parent's new HEAD. */
export interface WorktreeMergeResult {
  merged: string
  subject: string
  /** The worktree (and its branch) went away with the merge (`delete_after`). */
  removed: boolean
}

/** `WorktreeMergeResult.shortMerged`. */
export function worktreeShortMerged(result: WorktreeMergeResult): string {
  return result.merged.slice(0, 8)
}

/** `LanguageCount`. */
export interface LanguageCount {
  language: string
  files: number
}

/** `IndexStats` (`ProjectInfo.index`). */
export interface IndexStats {
  generation: number
  files: number
  symbols: number
  chunks: number
  relationships: number
  embeddings: number
  last_full_index_at?: string | null
  languages: LanguageCount[]
}

/** `ValidationCommand`: one detected or configured validation command. */
export interface ValidationCommand {
  command: string
  kind: string
  cwd: string
  source: string
}

/** `ValidationCommand.id`. */
export function validationCommandId(command: ValidationCommand): string {
  return `${command.kind}:${command.cwd}:${command.command}`
}

/** `RunningTask`. */
export interface RunningTask {
  session_id: string
  task_id?: string | null
  started_at: string
}

/** `ProjectInfo`: result of `project/open`, `project/status`, `permissions/set`, `worktree/create`. */
export interface ProjectInfo {
  project_id: string
  root: string
  name: string
  git?: GitInfo | null
  index: IndexStats
  indexing: boolean
  model: string
  permission_profile: string
  sandbox: string
  validation_commands: ValidationCommand[]
  languages: LanguageCount[]
  build_files: string[]
  file_count: number
  /** The oldest running task (backends that ran one task per project). */
  running_task?: RunningTask | null
  /** Every session's running task, oldest first; absent from older backends. */
  running_tasks?: RunningTask[] | null
  /** True on the `project/open` that ran `git init` for the folder (`init_git`). */
  git_initialized?: boolean | null
}

/** `ProjectInfo.runningSessionIds`: the sessions with a task running. */
export function runningSessionIds(info: ProjectInfo): Set<string> {
  if (info.running_tasks) return new Set(info.running_tasks.map((task) => task.session_id))
  return new Set(info.running_task ? [info.running_task.session_id] : [])
}

// MARK: - Sessions and history

/** `SessionSummary`. */
export interface SessionSummary {
  id: string
  model: string
  mode: string
  title: string
  created_at: string
  updated_at: string
  active_task_id?: string | null
  task_count: number
  last_status?: TaskStatus | null
}

/// `SessionSummary` decoding: `last_status` goes through `TaskStatus(from:)`
/// (unknown values are `failed`), absent optionals are `null`.
export function normalizeSessionSummary(raw: SessionSummary): SessionSummary {
  return {
    ...raw,
    active_task_id: raw.active_task_id ?? null,
    last_status: raw.last_status == null ? null : parseTaskStatus(raw.last_status),
  }
}

/** `SessionSummary.updatedDate` (the distant past when unparsable). */
export function sessionUpdatedDate(session: SessionSummary): Date {
  return parseRFC3339(session.updated_at) ?? new Date(0)
}

/** `SessionSummary.createdDate`. */
export function sessionCreatedDate(session: SessionSummary): Date {
  return parseRFC3339(session.created_at) ?? new Date(0)
}

/** `TaskSummary`. */
export interface TaskSummary {
  id: string
  session_id?: string | null
  objective: string
  category: string
  state: ExecutionState
  status?: TaskStatus | null
  summary?: string | null
  created_at: string
  updated_at: string
  completed_at?: string | null
  modified_files: string[]
  unresolved: string[]
}

/// `TaskSummary` decoding: `state` through `ExecutionState(from:)` (unknown or
/// differently cased values are `IDLE`), `status` through `TaskStatus(from:)`.
export function normalizeTaskSummary(raw: TaskSummary): TaskSummary {
  return {
    ...raw,
    state: parseExecutionState(raw.state),
    status: raw.status == null ? null : parseTaskStatus(raw.status),
    modified_files: raw.modified_files ?? [],
    unresolved: raw.unresolved ?? [],
  }
}

/** `HistoryMessageKind`. */
export type HistoryMessageKind = 'user' | 'assistant' | 'tool_result'

/** `HistoryMessageKind(from:)`: unknown values decode as `user`. */
export function parseHistoryMessageKind(raw: unknown): HistoryMessageKind {
  return raw === 'assistant' || raw === 'tool_result' ? raw : 'user'
}

/** `HistoryMessage`: one persisted conversation row of a `session/history` turn. */
export interface HistoryMessage {
  id: number
  kind: HistoryMessageKind
  text?: string | null
  tool_calls?: ToolCallInfo[] | null
  call_id?: string | null
  tool_name?: string | null
  content?: string | null
  is_error?: boolean | null
  runtime?: boolean | null
  /** User messages: the image and text files sent with it. */
  images?: string[] | null
  files?: FileUpload[] | null
  /** Untruncated length of a tool result when the backend truncated `content`. */
  full_len?: number | null
  /** User messages: a working-tree snapshot exists for the run this message started. */
  snapshot?: boolean | null
  created_at: string
}

/** `HistoryMessage` decoding: `kind` through `HistoryMessageKind(from:)` (unknown values are `user`). */
export function normalizeHistoryMessage(raw: HistoryMessage): HistoryMessage {
  return { ...raw, kind: parseHistoryMessageKind(raw.kind) }
}

/** `FileUpload` (Models/AttachmentUpload.swift): a text file sent with a prompt. */
export interface FileUpload {
  path: string
  name: string
}

/** `UsageTotals` of a history turn. */
export interface UsageTotals {
  prompt_tokens: number
  completion_tokens: number
  cached_tokens: number
  requests: number
  cost_usd?: number | null
}

/** `HistoryTurn`: one task of a session with its messages, events and outcome. */
export interface HistoryTurn {
  task: TaskSummary
  messages: HistoryMessage[]
  events: AgentEvent[]
  outcome?: TaskOutcome | null
  usage: UsageTotals
}

/** A `session/history` turn as it comes off the wire, before `normalizeHistoryTurn`. */
export interface HistoryTurnWire {
  task: TaskSummary
  messages?: HistoryMessage[] | null
  events?: JSONValue[] | null
  outcome?: JSONValue | null
  usage?: UsageTotals | null
}

/// `HistoryTurn.init(from:)`: events are decoded one by one so a single
/// malformed event never hides a whole turn; `messages` / `usage` default.
/// A malformed `outcome` throws (`AgentEventDecodeError`), as the Swift
/// `decodeIfPresent` does.
export function normalizeHistoryTurn(raw: HistoryTurnWire): HistoryTurn {
  const events: AgentEvent[] = []
  for (const event of raw.events ?? []) {
    try {
      events.push(decodeAgentEvent(event))
    } catch {
      // Skipped, as Swift's per-element `try?` does.
    }
  }
  const outcome = jsonObject(raw.outcome ?? undefined)
  if (raw.outcome != null && !outcome) throw new AgentEventDecodeError('HistoryTurn: outcome is not an object')
  return {
    task: normalizeTaskSummary(raw.task),
    messages: (raw.messages ?? []).map(normalizeHistoryMessage),
    events,
    outcome: outcome ? decodeTaskOutcome(outcome) : null,
    usage: raw.usage ?? { prompt_tokens: 0, completion_tokens: 0, cached_tokens: 0, requests: 0, cost_usd: null },
  }
}

/** `SessionHistory` (`session/history`). */
export interface SessionHistory {
  session: SessionSummary
  turns: HistoryTurn[]
}

/** `session/history` as it comes off the wire. */
export interface SessionHistoryWire {
  session: SessionSummary
  turns?: HistoryTurnWire[] | null
}

/** `SessionHistory` decoding (each turn through `normalizeHistoryTurn`). */
export function normalizeSessionHistory(raw: SessionHistoryWire): SessionHistory {
  return { session: normalizeSessionSummary(raw.session), turns: (raw.turns ?? []).map(normalizeHistoryTurn) }
}

/** `RestoreReport`: files `session/rewind` put back from the snapshot. */
export interface RestoreReport {
  /** Written back (added, modified or type-changed since the snapshot). */
  restored: number
  /** Created since the snapshot, removed again. */
  deleted: number
  /** The session's files that changed again since, left as they are; absent from older backends. */
  skipped?: string[] | null
}

/** `RestoreReport.total`. */
export function restoreReportTotal(report: RestoreReport): number {
  return report.restored + report.deleted
}

/** `RewindResult` (`session/rewind`). */
export interface RewindResult {
  removed_tasks: number
  removed_messages: number
  /** Present when files were restored. */
  files?: RestoreReport | null
  session: SessionSummary
}

/** `RewindResult` decoding (`session` through `normalizeSessionSummary`). */
export function normalizeRewindResult(raw: RewindResult): RewindResult {
  return { ...raw, files: raw.files ?? null, session: normalizeSessionSummary(raw.session) }
}

// MARK: - Task run options

/** `TaskRunOptions` (`task/run.options`): every field is optional and omitted when unset. */
export interface TaskRunOptions {
  max_steps?: number | null
  proactive_retrieval?: boolean | null
  memory_context?: boolean | null
  stream?: boolean | null
  debug_prompt?: boolean | null
}

/** `TaskRunOptions.default`. */
export const DEFAULT_TASK_RUN_OPTIONS: TaskRunOptions = {}

// MARK: - Models

/** `ModelsList` (`models/list`). */
export interface ModelsList {
  models: string[]
  current: string
  source: string
  error?: string | null
  /** ModelGateway's numbers per model id; empty for other endpoints. */
  stats?: { [model: string]: ModelStat } | null
}

/** `ModelStat` (`models/list.stats` entry): percentages, each null when unknown. */
export interface ModelStat {
  /** Discount against the official output price. */
  savings_percent?: number | null
  /** Share of failed requests over the last 24 hours. */
  error_rate_24h?: number | null
}

/** `ModelStat.savingsLabel`: "98% off"; null below 1%. */
export function modelStatSavingsLabel(stat: ModelStat): string | null {
  const savings = stat.savings_percent
  if (savings == null || Math.round(savings) < 1) return null
  return `${Math.round(savings)}% off`
}

/** `ModelStat.errorRateLabel`: "1.5%", "0%", "100%". */
export function modelStatErrorRateLabel(stat: ModelStat): string | null {
  const rate = stat.error_rate_24h
  if (rate == null) return null
  const rounded = Math.round(rate * 10) / 10
  return Number.isInteger(rounded) ? `${rounded}%` : `${rounded.toFixed(1)}%`
}

/** `CapabilitySummary` (`models/switch.capability`). */
export interface CapabilitySummary {
  probed_at?: string | null
  agent_capable: boolean
  streaming: boolean
  tool_calling: boolean
  parallel_tool_calls: boolean
  reasoning_controls: boolean
  issues: string[]
}

/** `ModelSwitchResult` (`models/switch`). */
export interface ModelSwitchResult {
  model: string
  capability: CapabilitySummary
}

// MARK: - Git and files

/** `GitFileStatus`: one `git/status` file. */
export interface GitFileStatus {
  path: string
  status: string
  staged: boolean
  /** Lines added / removed against HEAD; null for binary files and when unknown. */
  insertions?: number | null
  deletions?: number | null
}

/** `GitFileStatus.code`: first status letter, e.g. "M", "A", "D", "?". */
export function gitFileCode(file: GitFileStatus): string {
  return file.status.slice(0, 1)
}

/** `GitRemote`. */
export interface GitRemote {
  name: string
  url: string
}

/** `GitStatus` (`git/status`). */
export interface GitStatus {
  branch?: string | null
  head?: string | null
  /** Subject of the HEAD commit (shown by "Undo last commit"). */
  head_subject?: string | null
  /** `origin/main` when the branch tracks one, with the commits ahead / behind it. */
  upstream?: string | null
  ahead?: number | null
  behind?: number | null
  remotes?: GitRemote[] | null
  files: GitFileStatus[]
}

/** `GitStatus.stagedFiles`. */
export function gitStagedFiles(status: GitStatus): GitFileStatus[] {
  return status.files.filter((file) => file.staged)
}

/** `GitStatus.unstagedFiles`: unstaged, untracked, and files with both kinds (`MM`). */
export function gitUnstagedFiles(status: GitStatus): GitFileStatus[] {
  return status.files.filter((file) => !file.staged || file.status.length > 1)
}

/** `GitCommitResult` (`git/commit`, `git/undo_commit`). */
export interface GitCommitResult {
  commit: string
  subject: string
}

/** `GitCommitResult.shortHash`: the first 7 characters. */
export function gitShortHash(result: GitCommitResult): string {
  return result.commit.slice(0, 7)
}

/** `GitCommandOutput` (`git/push`, `git/pull`, `git/fetch`). */
export interface GitCommandOutput {
  output: string
}

/** `GitLocalBranch`. */
export interface GitLocalBranch {
  name: string
  upstream?: string | null
}

/** `GitRemoteBranch`. */
export interface GitRemoteBranch {
  /** `origin/feature` */
  name: string
  /** `feature` — what `git switch` takes to track it. */
  short: string
}

/** `GitBranches` (`git/branches`). */
export interface GitBranches {
  current?: string | null
  local: GitLocalBranch[]
  remote: GitRemoteBranch[]
}

/** `GitLogEntry` (`git/log`). */
export interface GitLogEntry {
  hash: string
  short: string
  subject: string
  author: string
  date: string
}

/** `GitCommitMessage` (`git/commit_message`). */
export interface GitCommitMessage {
  message: string
  /** `model` or `heuristic`; `error` says why the model was not used. */
  source: string
  error?: string | null
}

/** `GitDiscardResult` (`git/discard`). */
export interface GitDiscardResult {
  restored: number
  deleted: number
}

/** `GitDiff` (`git/diff`). */
export interface GitDiff {
  diff: string
  truncated: boolean
}

/** `FileContent` (`file/read`). */
export interface FileContent {
  path: string
  content: string
  truncated: boolean
  language?: string | null
  size: number
  /** `sha256:…` of the bytes returned; `file/write.expected_hash` takes it. */
  hash?: string | null
}

/** `FileWriteResult` (`file/write`). */
export interface FileWriteResult {
  path: string
  hash: string
  size: number
  language?: string | null
  created: boolean
}

/** `SymbolInfo`: one `symbols/find` hit, a definition the index knows. */
export interface SymbolInfo {
  name: string
  kind: string
  path: string
  parent?: string | null
  /** 1-based. */
  line: number
  end_line: number
  signature?: string | null
}

/** `SymbolInfo.id`. */
export function symbolInfoId(symbol: SymbolInfo): string {
  return `${symbol.path}:${symbol.line}:${symbol.name}`
}

/** `SymbolsFindMode`. */
export type SymbolsFindMode = 'prefix' | 'exact' | 'contains'

// MARK: - Language servers

/** `LspStatus` (`lsp/status` and the header of `lsp/open`). */
export interface LspStatus {
  path: string
  language?: string | null
  /** The executable ("rust-analyzer"); null when none is known or installed. */
  server?: string | null
  installed?: boolean | null
  running?: boolean | null
  version?: number | null
  trigger_characters?: string[] | null
  install_hint?: string | null
}

/** `LspRange`: 0-based range the server wants replaced by a completion. */
export interface LspRange {
  start_line: number
  start_character: number
  end_line: number
  end_character: number
}

/** `LspCompletionItem`. */
export interface LspCompletionItem {
  label: string
  kind: string
  detail?: string | null
  documentation?: string | null
  insert_text: string
  filter_text?: string | null
  /** The server's sort key (docs/desktop-protocol.md; items already come sorted). */
  sort_text?: string | null
  range?: LspRange | null
  deprecated?: boolean | null
}

/** `LspCompletionResult` (`lsp/completion`). */
export interface LspCompletionResult {
  items: LspCompletionItem[]
  incomplete: boolean
  server?: string | null
}

/** `LspHover` (`lsp/hover`). */
export interface LspHover {
  contents?: string | null
}

/** `LspLocation`: 1-based. */
export interface LspLocation {
  path: string
  line: number
  column: number
}

/** `LspDiagnostic`: one entry of the `lsp/diagnostics` notification (0-based). */
export interface LspDiagnostic {
  line: number
  character: number
  end_line: number
  end_character: number
  severity: string
  message: string
  source?: string | null
  code?: string | null
}

/** `LspDiagnostic.id`. */
export function lspDiagnosticId(diagnostic: LspDiagnostic): string {
  return `${diagnostic.line}:${diagnostic.character}:${diagnostic.end_line}:${diagnostic.end_character}:${diagnostic.message}`
}

/** `LspDiagnostic.isError`. */
export function lspDiagnosticIsError(diagnostic: LspDiagnostic): boolean {
  return diagnostic.severity === 'error'
}

/** `LspDiagnostic.isWarning`. */
export function lspDiagnosticIsWarning(diagnostic: LspDiagnostic): boolean {
  return diagnostic.severity === 'warning'
}

/** `LspDiagnostics`: the `lsp/diagnostics` notification. */
export interface LspDiagnostics {
  project_id: string
  path: string
  diagnostics: LspDiagnostic[]
}

// MARK: - Memory, doctor

/** `MemoryEntry` (`memory/list`). */
export interface MemoryEntry {
  id: string
  category: string
  verification: string
  content: string
  evidence_paths: string[]
  created_at: string
  invalidated: boolean
}

/** `DoctorRow`. */
export interface DoctorRow {
  label: string
  value: string
  ok: boolean
}

/** `DoctorSection`. */
export interface DoctorSection {
  title: string
  rows: DoctorRow[]
}

/** `DoctorReport` (`doctor/run`). */
export interface DoctorReport {
  problems: number
  sections: DoctorSection[]
}

// MARK: - Notifications

/** `EventNotification`: the `event` notification's params. */
export interface EventNotification {
  project_id: string
  session_id: string
  event: AgentEvent
}

/** `NextMessageSuggestion` (`suggest/next`). */
export interface NextMessageSuggestion {
  suggestion?: string | null
}

/// `StarterSuggestion`: one `suggest/starters` prompt; `kind` is fix, explain,
/// refactor, test, docs, review, feature, cleanup or other (it picks the icon).
export interface StarterSuggestion {
  kind: string
  text: string
}

/** `StarterSuggestions` (`suggest/starters`). */
export interface StarterSuggestions {
  suggestions: StarterSuggestion[]
}

/** `TaskFinished`: the `task/finished` notification. */
export interface TaskFinished {
  project_id: string
  session_id: string
  task_id?: string | null
  status?: TaskStatus | null
  error?: string | null
  error_kind?: string | null
  /** The conversation id the run's prompt became (the handle `session/rewind` takes). */
  message_id?: number | null
  /** A working-tree snapshot was taken before the run. */
  snapshot?: boolean | null
}

/// `TaskFinished.isEarlyCancel`: a run cancelled before it reached the
/// runtime (index wait, probe): no task, no status, only `error_kind: "cancelled"`.
export function isEarlyCancel(finished: TaskFinished): boolean {
  return finished.status == null && finished.error_kind === 'cancelled'
}

/** `IndexProgress`: the `index/progress` notification. */
export interface IndexProgress {
  project_id: string
  done: number
  total: number
}

/** `IndexReady`: the `index/ready` notification. */
export interface IndexReady {
  project_id: string
  generation: number
  files_indexed: number
  files_removed: number
  files_unchanged: number
  duration_ms: number
  rebuilt: boolean
  error?: string | null
}

/** `LogNotification`: the `log` notification. */
export interface LogNotification {
  level: string
  message: string
}

/// `BrowserRequest` (`browser/request`): the `browser` tool of a run asks the
/// app to act in the project's Inspector Browser tab (`action.action` names the step).
export interface BrowserRequest {
  project_id: string
  session_id: string
  request_id: string
  action: JSONValue
}

/** `BrowserReply`: what the app answers with `browser/respond`. */
export interface BrowserReply {
  ok: boolean
  content?: string
  url?: string | null
  title?: string | null
  error?: string | null
}

/// `QuestionRequest` (`question/request`): a run's `ask_user` tool puts
/// questions to the user; the app answers with `question/respond`.
export interface QuestionRequest {
  project_id: string
  session_id: string
  request_id: string
  questions: AgentQuestion[]
}

/** `AgentQuestion.Option`. */
export interface AgentQuestionOption {
  label: string
  description?: string | null
}

/** `AgentQuestion`: one question of an `ask_user` call; the user can always answer in their own words. */
export interface AgentQuestion {
  question: string
  header?: string | null
  options: AgentQuestionOption[]
  multi_select: boolean
}

/** `AgentQuestion.init(from:)` defaults. */
export function normalizeAgentQuestion(raw: Partial<AgentQuestion> & { question: string }): AgentQuestion {
  return {
    question: raw.question,
    header: raw.header ?? null,
    options: raw.options ?? [],
    multi_select: raw.multi_select ?? false,
  }
}

/** `QuestionAnswer`: one question's answer for `question/respond`, chosen labels and/or own words. */
export interface QuestionAnswer {
  selected: string[]
  other?: string | null
}

/** `QuestionAnswer.isEmpty`. */
export function questionAnswerIsEmpty(answer: QuestionAnswer): boolean {
  return answer.selected.length === 0 && (answer.other ?? '').trim().length === 0
}

// MARK: - ModelGateway (Models/ModelGateway.swift wire shapes)

/** `GatewayToken`: the API key the gateway created for this device. */
export interface GatewayToken {
  id: number
  name: string
  used_quota: number
  created_time: number
}

/// `GatewayContextCredit` (`context_credit` of `gateway/account`): quota only
/// requests made with this device's key can spend. Amounts are quota units.
export interface GatewayContextCredit {
  balance: number
  granted: number
  claimed: boolean
  eligible: boolean
  offer_quota: number
  /** Top-up kinds that make the account eligible (`stripe`, `apirone`); not in the Swift struct. */
  deposit_types?: string[] | null
}

/** `GatewayContextCredit.init(from:)` defaults. */
export function normalizeGatewayContextCredit(raw: Partial<GatewayContextCredit> | null | undefined): GatewayContextCredit {
  return {
    balance: raw?.balance ?? 0,
    granted: raw?.granted ?? 0,
    claimed: raw?.claimed ?? false,
    eligible: raw?.eligible ?? false,
    offer_quota: raw?.offer_quota ?? 0,
    deposit_types: raw?.deposit_types ?? null,
  }
}

/// `GatewaySubscription`: one active ModelGateway plan of `gateway/account`.
/// Amounts are quota units (`amount_total` 0 = unlimited); times are Unix seconds.
export interface GatewaySubscription {
  id: number
  plan_id: number
  status: string
  /** `order` or `admin` (docs/desktop-protocol.md); not in the Swift struct. */
  source?: string | null
  title: string
  subtitle: string
  price_amount: number
  currency: string
  duration_unit: string
  duration_value: number
  quota_reset_period: string
  amount_total: number
  amount_used: number
  amount_reserved: number
  start_time: number
  end_time: number
  next_reset_time: number
}

/** `GatewaySubscription.init(from:)` defaults. */
export function normalizeGatewaySubscription(raw: Partial<GatewaySubscription> | null | undefined): GatewaySubscription {
  return {
    id: raw?.id ?? 0,
    plan_id: raw?.plan_id ?? 0,
    status: raw?.status ?? '',
    source: raw?.source ?? null,
    title: raw?.title ?? '',
    subtitle: raw?.subtitle ?? '',
    price_amount: raw?.price_amount ?? 0,
    currency: raw?.currency ?? 'USD',
    duration_unit: raw?.duration_unit ?? '',
    duration_value: raw?.duration_value ?? 0,
    quota_reset_period: raw?.quota_reset_period ?? 'never',
    amount_total: raw?.amount_total ?? 0,
    amount_used: raw?.amount_used ?? 0,
    amount_reserved: raw?.amount_reserved ?? 0,
    start_time: raw?.start_time ?? 0,
    end_time: raw?.end_time ?? 0,
    next_reset_time: raw?.next_reset_time ?? 0,
  }
}

/** `GatewayAccount` (`gateway/account`, also carried by the approved device poll). */
export interface GatewayAccount {
  id: number
  username: string
  display_name: string
  email: string
  group: string
  quota: number
  used_quota: number
  request_count: number
  quota_per_unit: number
  display_in_currency: boolean
  token?: GatewayToken | null
  /** Whether requests draw on the plan or the balance first; null from gateways that predate plans. */
  billing_preference?: string | null
  /** Active plans; empty means pay as you go, null that the gateway predates plans. */
  subscriptions?: GatewaySubscription[] | null
  /** Credit only Context's requests can spend; null from gateways that predate it. */
  context_credit?: GatewayContextCredit | null
}

/** `GatewayAccount.init(from:)` defaults. */
export function normalizeGatewayAccount(raw: Partial<GatewayAccount> | null | undefined): GatewayAccount {
  return {
    id: raw?.id ?? 0,
    username: raw?.username ?? '',
    display_name: raw?.display_name ?? '',
    email: raw?.email ?? '',
    group: raw?.group ?? '',
    quota: raw?.quota ?? 0,
    used_quota: raw?.used_quota ?? 0,
    request_count: raw?.request_count ?? 0,
    quota_per_unit: raw?.quota_per_unit ?? 500_000,
    display_in_currency: raw?.display_in_currency ?? true,
    token: raw?.token ?? null,
    billing_preference: raw?.billing_preference ?? null,
    subscriptions: raw?.subscriptions == null ? null : raw.subscriptions.map(normalizeGatewaySubscription),
    context_credit: raw?.context_credit == null ? null : normalizeGatewayContextCredit(raw.context_credit),
  }
}

/** `GatewayAccount.name`: display name, else the username. */
export function gatewayAccountName(account: GatewayAccount): string {
  const trimmed = account.display_name.trim()
  return trimmed.length > 0 ? trimmed : account.username
}

/** `GatewayDeviceStart` (`gateway/device/start`). */
export interface GatewayDeviceStart {
  device_code: string
  user_code: string
  verification_uri: string
  verification_uri_complete: string
  expires_in: number
  interval: number
}

/** `GatewayDevicePoll` (`gateway/device/poll`): `pending` · `approved` · `denied` · `expired`. */
export interface GatewayDevicePoll {
  status: string
  interval?: number | null
  account?: GatewayAccount | null
}

/** `GatewayStatus` (`gateway/status`): whether the backend holds a ModelGateway device key. */
export interface GatewayStatus {
  signed_in: boolean
  origin?: string | null
  account?: string | null
}

/** `GatewaySignOutResult` (`gateway/signout`). */
export interface GatewaySignOutResult {
  revoked: boolean
}

/** `GatewayUsageRow`: one hourly usage row of `gateway/usage` (`created_at` is the hour, UTC). */
export interface GatewayUsageRow {
  created_at: number
  model_name: string
  project: string
  count: number
  quota: number
  token_used: number
}

/** `GatewayUsageRow.init(from:)` defaults. */
export function normalizeGatewayUsageRow(raw: Partial<GatewayUsageRow> | null | undefined): GatewayUsageRow {
  return {
    created_at: raw?.created_at ?? 0,
    model_name: raw?.model_name ?? '',
    project: raw?.project ?? '',
    count: raw?.count ?? 0,
    quota: raw?.quota ?? 0,
    token_used: raw?.token_used ?? 0,
  }
}

/** `GatewayUsageReport` (`gateway/usage`). */
export interface GatewayUsageReport {
  start_timestamp: number
  end_timestamp: number
  quota_per_unit: number
  display_in_currency: boolean
  rows: GatewayUsageRow[]
}

/** `GatewayUsageReport.init(from:)` defaults. */
export function normalizeGatewayUsageReport(raw: Partial<GatewayUsageReport> | null | undefined): GatewayUsageReport {
  return {
    start_timestamp: raw?.start_timestamp ?? 0,
    end_timestamp: raw?.end_timestamp ?? 0,
    quota_per_unit: raw?.quota_per_unit ?? 500_000,
    display_in_currency: raw?.display_in_currency ?? true,
    rows: (raw?.rows ?? []).map(normalizeGatewayUsageRow),
  }
}

/** `GatewayBillingPreference`: how requests draw on a plan and the balance. */
export type GatewayBillingPreference = 'subscription_first' | 'wallet_first' | 'subscription_only' | 'wallet_only'

export const GATEWAY_BILLING_PREFERENCES: readonly GatewayBillingPreference[] = [
  'subscription_first',
  'wallet_first',
  'subscription_only',
  'wallet_only',
]

/** `GatewayBillingPreference(rawValue:)`. */
export function parseGatewayBillingPreference(raw: string | null | undefined): GatewayBillingPreference | null {
  return (GATEWAY_BILLING_PREFERENCES as readonly string[]).includes(raw ?? '') ? (raw as GatewayBillingPreference) : null
}

/** `GatewayTopUpOptions`: adding funds to the balance (`amount_unit` "usd" or "quota"). */
export interface GatewayTopUpOptions {
  enabled: boolean
  /** A checkout link can be made from the app (else: the wallet page). */
  in_app: boolean
  min_amount: number
  max_amount: number
  amount_options: number[]
  /** Amount (as a string) → price multiplier. */
  discounts: { [amount: string]: number }
  amount_unit: string
}

/** `GatewayTopUpOptions()` / `init(from:)` defaults. */
export function normalizeGatewayTopUpOptions(raw: Partial<GatewayTopUpOptions> | null | undefined): GatewayTopUpOptions {
  return {
    enabled: raw?.enabled ?? false,
    in_app: raw?.in_app ?? false,
    min_amount: raw?.min_amount ?? 1,
    max_amount: raw?.max_amount ?? 10_000,
    amount_options: raw?.amount_options ?? [],
    discounts: raw?.discounts ?? {},
    amount_unit: raw?.amount_unit ?? 'usd',
  }
}

/** `GatewayPlan`: a plan on sale. */
export interface GatewayPlan {
  id: number
  title: string
  subtitle: string
  price_amount: number
  currency: string
  duration_unit: string
  duration_value: number
  quota_reset_period: string
  /** Quota units; 0 = unlimited. */
  total_amount: number
  stripe: boolean
  balance: boolean
  /** docs/desktop-protocol.md lists it; not in the Swift struct. */
  max_purchase_per_user?: number | null
}

/** `GatewayPlan.init(from:)` defaults. */
export function normalizeGatewayPlan(raw: Partial<GatewayPlan> | null | undefined): GatewayPlan {
  return {
    id: raw?.id ?? 0,
    title: raw?.title ?? '',
    subtitle: raw?.subtitle ?? '',
    price_amount: raw?.price_amount ?? 0,
    currency: raw?.currency ?? 'USD',
    duration_unit: raw?.duration_unit ?? '',
    duration_value: raw?.duration_value ?? 0,
    quota_reset_period: raw?.quota_reset_period ?? 'never',
    total_amount: raw?.total_amount ?? 0,
    stripe: raw?.stripe ?? false,
    balance: raw?.balance ?? false,
    max_purchase_per_user: raw?.max_purchase_per_user ?? null,
  }
}

/** `GatewayBilling` (`gateway/billing`): what the Billing settings show. */
export interface GatewayBilling {
  billing_preference: string
  quota_per_unit: number
  display_in_currency: boolean
  wallet_url?: string | null
  topup: GatewayTopUpOptions
  plans: GatewayPlan[]
  has_active_subscription: boolean
}

/** `GatewayBilling.init(from:)` defaults. */
export function normalizeGatewayBilling(raw: Partial<GatewayBilling> | null | undefined): GatewayBilling {
  return {
    billing_preference: raw?.billing_preference ?? 'subscription_first',
    quota_per_unit: raw?.quota_per_unit ?? 500_000,
    display_in_currency: raw?.display_in_currency ?? true,
    wallet_url: raw?.wallet_url ?? null,
    topup: normalizeGatewayTopUpOptions(raw?.topup),
    plans: (raw?.plans ?? []).map(normalizeGatewayPlan),
    has_active_subscription: raw?.has_active_subscription ?? false,
  }
}

/** `GatewayTopUpQuote` (`gateway/billing/quote`). */
export interface GatewayTopUpQuote {
  amount: number
  pay_money: number
  currency?: string | null
}

/** `GatewayCheckout` (`gateway/billing/topup` and `/subscribe`): a page to pay on, or done. */
export interface GatewayCheckout {
  checkout_url?: string | null
  subscribed?: boolean | null
  /** The gateway's order number of a top-up (docs/desktop-protocol.md); not in the Swift struct. */
  trade_no?: string | null
}

/** `GatewayPreferenceResult` (`gateway/billing/preference`). */
export interface GatewayPreferenceResult {
  billing_preference: string
}

/// `GatewayPriceRule`: one price rule of the device key. Requests only go to
/// providers charging at most `100 − discount_percent` percent of the model's
/// standard output-token price, and at most `max_price_per_m` dollars per million.
export interface GatewayPriceRule {
  discount_percent?: number | null
  max_price_per_m?: number | null
}

/** `GatewayPriceRule.isEmpty`. */
export function gatewayPriceRuleIsEmpty(rule: GatewayPriceRule): boolean {
  return rule.discount_percent == null && rule.max_price_per_m == null
}

/** `GatewayPriceRule.json`: the wire object without absent fields. */
export function gatewayPriceRuleJSON(rule: GatewayPriceRule): JSONObject {
  const object: JSONObject = {}
  if (rule.discount_percent != null) object.discount_percent = rule.discount_percent
  if (rule.max_price_per_m != null) object.max_price_per_m = rule.max_price_per_m
  return object
}

/** `GatewayPriceLimits`: the key-wide rule and the per-model rules that replace it. */
export interface GatewayPriceLimits {
  default?: GatewayPriceRule | null
  models: { [model: string]: GatewayPriceRule }
}

/** `GatewayPriceLimits()` / `init(from:)` defaults. */
export function normalizeGatewayPriceLimits(raw: Partial<GatewayPriceLimits> | null | undefined): GatewayPriceLimits {
  return { default: raw?.default ?? null, models: raw?.models ?? {} }
}

/** `GatewayPriceLimits.json`: the `price_limits` body of `gateway/key/update` (`{}` clears). */
export function gatewayPriceLimitsJSON(limits: GatewayPriceLimits): JSONObject {
  const object: JSONObject = {}
  if (limits.default && !gatewayPriceRuleIsEmpty(limits.default)) object.default = gatewayPriceRuleJSON(limits.default)
  const models: JSONObject = {}
  for (const [model, rule] of Object.entries(limits.models)) {
    if (!gatewayPriceRuleIsEmpty(rule)) models[model] = gatewayPriceRuleJSON(rule)
  }
  if (Object.keys(models).length > 0) object.models = models
  return object
}

/** `GatewayKeySettings` (`gateway/key`): the device key's price limits and spending cap. */
export interface GatewayKeySettings {
  /** The key's id on the gateway (docs/desktop-protocol.md); not in the Swift struct. */
  id?: number | null
  name: string
  exhausted: boolean
  unlimited_quota: boolean
  remain_quota: number
  used_quota: number
  quota_per_unit: number
  display_in_currency: boolean
  price_limits: GatewayPriceLimits
}

/** `GatewayKeySettings.init(from:)` defaults. */
export function normalizeGatewayKeySettings(raw: Partial<GatewayKeySettings> | null | undefined): GatewayKeySettings {
  return {
    id: raw?.id ?? null,
    name: raw?.name ?? '',
    exhausted: raw?.exhausted ?? false,
    unlimited_quota: raw?.unlimited_quota ?? true,
    remain_quota: raw?.remain_quota ?? 0,
    used_quota: raw?.used_quota ?? 0,
    quota_per_unit: raw?.quota_per_unit ?? 500_000,
    display_in_currency: raw?.display_in_currency ?? true,
    price_limits: normalizeGatewayPriceLimits(raw?.price_limits),
  }
}
