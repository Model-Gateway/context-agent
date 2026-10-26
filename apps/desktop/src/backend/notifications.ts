// Port of `BackendNotification` (Backend/Protocol.swift) and
// `BackendClient.decodeNotification` (Backend/BackendClient.swift): every
// notification frame the backend pushes, decoded into typed shapes, plus the
// child's exit. Unknown methods and undecodable params are dropped (logged
// in debug), exactly as the Swift app does.

import { debug } from './log'
import {
  decodeAgentEvent,
  jsonArray,
  jsonBool,
  jsonInt,
  jsonObject,
  jsonString,
  normalizeAgentQuestion,
  normalizeMcpServerStatus,
  parseTaskStatus,
  type AgentEvent,
  type AgentQuestion,
  type BrowserRequest,
  type IndexProgress,
  type IndexReady,
  type JSONObject,
  type JSONValue,
  type LspDiagnostic,
  type LspDiagnostics,
  type McpServerStatus,
  type QuestionRequest,
  type TaskFinished,
} from './types'

/// `BackendNotification`: everything the backend pushes without being
/// asked, plus process exit. `exited` is only sent for exits that were not
/// requested by `stop()` and did not happen while `start()` was initialising.
export type BackendNotification =
  | { kind: 'event'; projectId: string; sessionId: string; event: AgentEvent }
  | { kind: 'taskFinished'; finished: TaskFinished }
  | { kind: 'browserRequest'; request: BrowserRequest }
  | { kind: 'questionRequest'; request: QuestionRequest }
  | { kind: 'lspDiagnostics'; diagnostics: LspDiagnostics }
  | { kind: 'mcpStatus'; servers: McpServerStatus[] }
  | { kind: 'indexProgress'; progress: IndexProgress }
  | { kind: 'indexReady'; ready: IndexReady }
  | { kind: 'projectChanged'; params: JSONValue }
  | { kind: 'log'; level: string; message: string }
  | { kind: 'exited'; code: number; tail: string }

/** Thrown inside the decoders below; `decodeNotification` turns it into a debug line. */
class NotificationDecodeError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'NotificationDecodeError'
  }
}

/// `BackendClient.decodeNotification`: maps one `{ method, params }` frame
/// onto `BackendNotification`, or `null` for an unknown method or
/// undecodable params.
export function decodeNotification(method: string, params: JSONValue | undefined): BackendNotification | null {
  try {
    switch (method) {
      case 'event': {
        const object = requireObject(params, method)
        const event = object.event
        if (event === undefined) throw new NotificationDecodeError('missing key "event"')
        return {
          kind: 'event',
          projectId: requireString(object, 'project_id'),
          sessionId: requireString(object, 'session_id'),
          event: decodeAgentEvent(event),
        }
      }
      case 'task/finished':
        return { kind: 'taskFinished', finished: decodeTaskFinished(requireObject(params, method)) }
      case 'index/progress':
        return { kind: 'indexProgress', progress: decodeIndexProgress(requireObject(params, method)) }
      case 'index/ready':
        return { kind: 'indexReady', ready: decodeIndexReady(requireObject(params, method)) }
      case 'log': {
        const object = requireObject(params, method)
        return { kind: 'log', level: requireString(object, 'level'), message: requireString(object, 'message') }
      }
      case 'project/changed':
        return { kind: 'projectChanged', params: params ?? null }
      case 'lsp/diagnostics':
        return { kind: 'lspDiagnostics', diagnostics: decodeLspDiagnostics(requireObject(params, method)) }
      case 'browser/request':
        return { kind: 'browserRequest', request: decodeBrowserRequest(requireObject(params, method)) }
      case 'question/request':
        return { kind: 'questionRequest', request: decodeQuestionRequest(requireObject(params, method)) }
      case 'mcp/status': {
        const object = requireObject(params, method)
        const servers = jsonArray(object.servers)
        if (!servers) throw new NotificationDecodeError('missing key "servers"')
        return { kind: 'mcpStatus', servers: servers.map(decodeMcpServerStatus) }
      }
      default:
        debug(() => `ignored notification ${method}`)
        return null
    }
  } catch (error) {
    debug(() => `undecodable ${method} params: ${error instanceof Error ? error.message : String(error)}`)
    return null
  }
}

function requireObject(value: JSONValue | undefined, method: string): JSONObject {
  const object = jsonObject(value)
  if (!object) throw new NotificationDecodeError(`${method}: params is not an object`)
  return object
}

function requireString(object: JSONObject, key: string): string {
  const value = jsonString(object[key])
  if (value === null) throw new NotificationDecodeError(`missing key "${key}"`)
  return value
}

function requireInt(object: JSONObject, key: string): number {
  const value = jsonInt(object[key])
  if (value === null) throw new NotificationDecodeError(`missing key "${key}"`)
  return value
}

function requireBool(object: JSONObject, key: string): boolean {
  const value = jsonBool(object[key])
  if (value === null) throw new NotificationDecodeError(`missing key "${key}"`)
  return value
}

/** `TaskFinished` (optional fields `null` when absent). */
function decodeTaskFinished(object: JSONObject): TaskFinished {
  const status = jsonString(object.status)
  return {
    project_id: requireString(object, 'project_id'),
    session_id: requireString(object, 'session_id'),
    task_id: jsonString(object.task_id),
    status: status === null ? null : parseTaskStatus(status),
    error: jsonString(object.error),
    error_kind: jsonString(object.error_kind),
    message_id: jsonInt(object.message_id),
    snapshot: jsonBool(object.snapshot),
  }
}

function decodeIndexProgress(object: JSONObject): IndexProgress {
  return {
    project_id: requireString(object, 'project_id'),
    done: requireInt(object, 'done'),
    total: requireInt(object, 'total'),
  }
}

function decodeIndexReady(object: JSONObject): IndexReady {
  return {
    project_id: requireString(object, 'project_id'),
    generation: requireInt(object, 'generation'),
    files_indexed: requireInt(object, 'files_indexed'),
    files_removed: requireInt(object, 'files_removed'),
    files_unchanged: requireInt(object, 'files_unchanged'),
    duration_ms: requireInt(object, 'duration_ms'),
    rebuilt: requireBool(object, 'rebuilt'),
    error: jsonString(object.error),
  }
}

function decodeLspDiagnostics(object: JSONObject): LspDiagnostics {
  const list = jsonArray(object.diagnostics)
  if (!list) throw new NotificationDecodeError('missing key "diagnostics"')
  return {
    project_id: requireString(object, 'project_id'),
    path: requireString(object, 'path'),
    diagnostics: list.map(decodeLspDiagnostic),
  }
}

function decodeLspDiagnostic(raw: JSONValue): LspDiagnostic {
  const object = jsonObject(raw)
  if (!object) throw new NotificationDecodeError('diagnostic is not an object')
  return {
    line: requireInt(object, 'line'),
    character: requireInt(object, 'character'),
    end_line: requireInt(object, 'end_line'),
    end_character: requireInt(object, 'end_character'),
    severity: requireString(object, 'severity'),
    message: requireString(object, 'message'),
    source: jsonString(object.source),
    code: jsonString(object.code),
  }
}

function decodeBrowserRequest(object: JSONObject): BrowserRequest {
  const action = object.action
  if (action === undefined) throw new NotificationDecodeError('missing key "action"')
  return {
    project_id: requireString(object, 'project_id'),
    session_id: requireString(object, 'session_id'),
    request_id: requireString(object, 'request_id'),
    action,
  }
}

function decodeQuestionRequest(object: JSONObject): QuestionRequest {
  const list = jsonArray(object.questions)
  if (!list) throw new NotificationDecodeError('missing key "questions"')
  return {
    project_id: requireString(object, 'project_id'),
    session_id: requireString(object, 'session_id'),
    request_id: requireString(object, 'request_id'),
    questions: list.map(decodeAgentQuestion),
  }
}

/** `AgentQuestion.init(from:)`: `options` and `multi_select` default. */
function decodeAgentQuestion(raw: JSONValue): AgentQuestion {
  const object = jsonObject(raw)
  if (!object) throw new NotificationDecodeError('question is not an object')
  const options = (jsonArray(object.options) ?? []).map((item) => {
    const option = jsonObject(item)
    if (!option) throw new NotificationDecodeError('option is not an object')
    return { label: requireString(option, 'label'), description: jsonString(option.description) }
  })
  return normalizeAgentQuestion({
    question: requireString(object, 'question'),
    header: jsonString(object.header),
    options,
    multi_select: jsonBool(object.multi_select) ?? false,
  })
}

/** `McpServerStatus`: the required fields checked, the rest passed through with defaults. */
function decodeMcpServerStatus(raw: JSONValue): McpServerStatus {
  const object = jsonObject(raw)
  if (!object) throw new NotificationDecodeError('server status is not an object')
  const state = requireString(object, 'state')
  if (state !== 'disabled' && state !== 'connecting' && state !== 'connected' && state !== 'failed') {
    throw new NotificationDecodeError(`unknown server state "${state}"`)
  }
  requireString(object, 'name')
  requireBool(object, 'enabled')
  requireString(object, 'transport')
  requireString(object, 'target')
  return normalizeMcpServerStatus(object as unknown as McpServerStatus)
}
