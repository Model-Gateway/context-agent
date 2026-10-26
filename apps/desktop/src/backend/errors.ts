// Port of `BackendError` (Backend/Protocol.swift) and `BackendFailure`
// (App/AppState.swift): what a protocol call throws, and why the backend
// could not be started, in the shape the failure panel wants.

/// `BackendError`'s cases. `rpc.kind` is the backend's `CtxError::code()`
/// (`data.kind`) when present; `incompatible` is the `rpc` error `start`
/// throws when the binary does not speak protocol 1.
export type BackendErrorDetail =
  | { type: 'rpc'; code: number; message: string; kind: string | null }
  | { type: 'transport'; message: string }
  | { type: 'notRunning' }
  | { type: 'decoding'; message: string }
  | { type: 'binaryNotFound' }

/** `BackendError`: thrown by every `BackendClient` method. `message` is Swift's `errorDescription`. */
export class BackendError extends Error {
  /** JSON-RPC error codes the app distinguishes (`BackendError.projectNotOpen` …). */
  static readonly PROJECT_NOT_OPEN = -32001
  static readonly TASK_ALREADY_RUNNING = -32002
  static readonly NO_TASK_RUNNING = -32003
  /** `BackendError.incompatibleKind` / `incompatibleCode`. */
  static readonly INCOMPATIBLE_KIND = 'incompatible'
  static readonly INCOMPATIBLE_CODE = -1

  readonly detail: BackendErrorDetail

  constructor(detail: BackendErrorDetail) {
    super(BackendError.describe(detail))
    this.name = 'BackendError'
    this.detail = detail
  }

  /** `.rpc(code:message:kind:)`: a JSON-RPC error response. */
  static rpc(code: number, message: string, kind: string | null = null): BackendError {
    return new BackendError({ type: 'rpc', code, message, kind })
  }

  /** `.transport(_:)`: the pipe or the child went away. */
  static transport(message: string): BackendError {
    return new BackendError({ type: 'transport', message })
  }

  /** `.notRunning`. */
  static notRunning(): BackendError {
    return new BackendError({ type: 'notRunning' })
  }

  /** `.decoding(_:)`: a result or params subtree had an unexpected shape. */
  static decoding(message: string): BackendError {
    return new BackendError({ type: 'decoding', message })
  }

  /** `.binaryNotFound`. */
  static binaryNotFound(): BackendError {
    return new BackendError({ type: 'binaryNotFound' })
  }

  /** `BackendError.incompatible(_:)`: the binary speaks another protocol version. */
  static incompatible(detail: string): BackendError {
    return BackendError.rpc(BackendError.INCOMPATIBLE_CODE, detail, BackendError.INCOMPATIBLE_KIND)
  }

  /// `code` of the errors the Rust shell raises itself (`RpcError::LOCAL_CODE`
  /// in src-tauri/src/backend/rpc.rs); their `kind` names the Swift case.
  static readonly LOCAL_CODE = -1
  /// The shell's local kinds (`RpcError::KIND_*`).
  static readonly LOCAL_KIND_TRANSPORT = 'transport'
  static readonly LOCAL_KIND_NOT_RUNNING = 'not_running'
  static readonly LOCAL_KIND_DECODING = 'decoding'

  /// Converts whatever `invoke('rpc_request')` rejected with: the
  /// `{ code, message, kind? }` object (or that JSON as a string). Errors the
  /// shell raised itself (`code` -1 with kind `transport` / `not_running` /
  /// `decoding`) become those cases, so `isConnectionLoss` holds for a child
  /// that went away; every other code is a backend `.rpc` error. Anything
  /// that is not such an object is `.transport`. A `BackendError` passes through.
  static from(error: unknown): BackendError {
    if (error instanceof BackendError) return error
    const rpc = parseRpcRejection(error)
    if (rpc) {
      if (rpc.code === BackendError.LOCAL_CODE) {
        switch (rpc.kind) {
          case BackendError.LOCAL_KIND_TRANSPORT:
            return BackendError.transport(rpc.message)
          case BackendError.LOCAL_KIND_NOT_RUNNING:
            return BackendError.notRunning()
          case BackendError.LOCAL_KIND_DECODING:
            return BackendError.decoding(rpc.message)
          default:
            break
        }
      }
      return BackendError.rpc(rpc.code, rpc.message, rpc.kind)
    }
    return BackendError.transport(errorText(error))
  }

  /** `errorDescription`. */
  static describe(detail: BackendErrorDetail): string {
    switch (detail.type) {
      case 'rpc':
        return detail.message
      case 'transport':
        return `Backend connection failed: ${detail.message}`
      case 'notRunning':
        return 'The backend is not running.'
      case 'decoding':
        return `Unexpected backend response: ${detail.message}`
      case 'binaryNotFound':
        return 'The ctx backend binary was not found. Build it with `cargo build --release` or choose it in Settings → General.'
    }
  }

  /** The JSON-RPC code of an `.rpc` error, else `null`. */
  get code(): number | null {
    return this.detail.type === 'rpc' ? this.detail.code : null
  }

  /** `data.kind` of an `.rpc` error, else `null`. */
  get kind(): string | null {
    return this.detail.type === 'rpc' ? this.detail.kind : null
  }

  /** `isIncompatible`. */
  get isIncompatible(): boolean {
    return this.detail.type === 'rpc' && this.detail.kind === BackendError.INCOMPATIBLE_KIND
  }

  /** `case .rpc`: a JSON-RPC error response of the backend. */
  get isRpc(): boolean {
    return this.detail.type === 'rpc'
  }

  /** `isTaskAlreadyRunning` (`-32002`). */
  get isTaskAlreadyRunning(): boolean {
    return this.code === BackendError.TASK_ALREADY_RUNNING
  }

  /** `code == BackendError.noTaskRunning` (`-32003`). */
  get isNoTaskRunning(): boolean {
    return this.code === BackendError.NO_TASK_RUNNING
  }

  /** `code == BackendError.projectNotOpen` (`-32001`). */
  get isProjectNotOpen(): boolean {
    return this.code === BackendError.PROJECT_NOT_OPEN
  }

  /** `isConnectionLoss`: the backend went away (pipe closed, not running); nothing was processed. */
  get isConnectionLoss(): boolean {
    return this.detail.type === 'transport' || this.detail.type === 'notRunning'
  }
}

/** The `{ code, message, kind? }` rejection of `rpc_request`, parsed. */
function parseRpcRejection(error: unknown): { code: number; message: string; kind: string | null } | null {
  let value: unknown = error
  if (typeof value === 'string') {
    const trimmed = value.trim()
    if (!trimmed.startsWith('{')) return null
    try {
      value = JSON.parse(trimmed)
    } catch {
      return null
    }
  }
  if (value === null || typeof value !== 'object') return null
  const record = value as Record<string, unknown>
  if (typeof record.code !== 'number' || typeof record.message !== 'string') return null
  let kind: string | null = typeof record.kind === 'string' ? record.kind : null
  if (kind === null && record.data !== null && typeof record.data === 'object') {
    const data = record.data as Record<string, unknown>
    if (typeof data.kind === 'string') kind = data.kind
  }
  return { code: record.code, message: record.message, kind }
}

/** A user-presentable line for any thrown value. */
export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message
  // A shell command's `{ code, message, kind }` rejection may arrive as its JSON text.
  if (typeof error === 'string') return parseRpcRejection(error)?.message ?? error
  if (error !== null && typeof error === 'object') {
    const record = error as Record<string, unknown>
    if (typeof record.message === 'string') return record.message
    try {
      return JSON.stringify(error)
    } catch {
      return String(error)
    }
  }
  return String(error)
}

// MARK: - BackendFailure

/// `BackendFailure` (App/AppState.swift): why the backend is not running,
/// for the Welcome / inspector failure panel.
export type BackendFailure =
  | { kind: 'notFound'; searched: string[] }
  | { kind: 'incompatible'; found: string; detail: string }
  | { kind: 'exited'; code: number; tail: string }
  | { kind: 'transport'; message: string }

/** Constructors for `BackendFailure` (the Swift cases). */
export const BackendFailure = {
  notFound(searched: string[]): BackendFailure {
    return { kind: 'notFound', searched }
  },
  incompatible(found: string, detail: string): BackendFailure {
    return { kind: 'incompatible', found, detail }
  },
  exited(code: number, tail: string): BackendFailure {
    return { kind: 'exited', code, tail }
  },
  transport(message: string): BackendFailure {
    return { kind: 'transport', message }
  },
  /// The BackendFailure-shaped error JSON `backend_start` rejects with
  /// (`{ kind: "not_found", searched } | { kind: "exited", code, tail } |
  /// { kind: "transport", message }`), as a string or an object; anything
  /// else is `transport`.
  fromInvoke(error: unknown): BackendFailure {
    if (error instanceof BackendFailureError) return error.failure
    if (error instanceof BackendError) return BackendFailure.transport(error.message)
    let value: unknown = error
    if (typeof value === 'string') {
      const trimmed = value.trim()
      if (trimmed.startsWith('{')) {
        try {
          value = JSON.parse(trimmed)
        } catch {
          value = error
        }
      }
    }
    if (value !== null && typeof value === 'object') {
      const record = value as Record<string, unknown>
      switch (record.kind) {
        case 'not_found':
        case 'notFound':
          return BackendFailure.notFound(stringArray(record.searched))
        case 'incompatible':
          return BackendFailure.incompatible(stringOr(record.found, ''), stringOr(record.detail, errorText(error)))
        case 'exited':
          return BackendFailure.exited(typeof record.code === 'number' ? record.code : -1, stringOr(record.tail, ''))
        case 'transport':
          return BackendFailure.transport(stringOr(record.message, errorText(error)))
        default:
          break
      }
    }
    return BackendFailure.transport(errorText(error))
  },
}

/// What the failure panel shows for a `BackendFailure`: `title` and
/// `detail` are `BackendFailure.title` / `.detail` (App/AppState.swift);
/// `explanation` and `summary` are `BackendFailurePanel.explanation(for:)` /
/// `.summary(for:)` (Views/Welcome/WelcomeView.swift), the paragraph under the
/// title and the short form for the sidebar card.
export interface FailureDescription {
  title: string
  detail: string
  explanation: string
  summary: string
}

/// Describes `failure` for the panel. `home` (the user's home directory)
/// abbreviates paths in `summary` to `~/…` like `abbreviatingWithTildeInPath`.
export function describe(failure: BackendFailure, home?: string | null): FailureDescription {
  switch (failure.kind) {
    case 'notFound': {
      const count = failure.searched.length
      return {
        title: 'ctx not found',
        detail: count === 0 ? 'No ctx binary was found.' : 'Searched:\n' + failure.searched.join('\n'),
        explanation:
          'The ctx backend binary was not found. Build it with the command below, or point the app at an existing binary.',
        summary: count === 0 ? 'No ctx binary was found.' : `No ctx binary in ${count} searched location${count === 1 ? '' : 's'}.`,
      }
    }
    case 'incompatible':
      return {
        title: 'ctx is too old',
        detail: `${failure.found}\n${failure.detail}`,
        explanation: 'This ctx binary does not speak the desktop protocol. Rebuild it from this checkout or choose a newer binary.',
        summary: `Too old: ${abbreviateHome(failure.found, home)}`,
      }
    case 'exited': {
      const lines = failure.tail.split(/\r?\n/).filter((line) => line.length > 0)
      const last = lines[lines.length - 1] ?? ''
      return {
        title: 'Backend stopped',
        detail: failure.tail.length === 0 ? `ctx exited with code ${failure.code}.` : `ctx exited with code ${failure.code}.\n${failure.tail}`,
        explanation: `The backend process exited with code ${failure.code}. Retry, or check the log below for the cause.`,
        summary: last.length === 0 ? `ctx exited with code ${failure.code}.` : `Exited (${failure.code}): ${last}`,
      }
    }
    case 'transport':
      return {
        title: 'Backend unavailable',
        detail: failure.message,
        explanation: 'The app could not talk to the backend process.',
        summary: failure.message.length === 0 ? 'Backend unavailable.' : failure.message,
      }
  }
}

/// `(path as NSString).abbreviatingWithTildeInPath`: `home` (with either
/// separator) at the start of `path` becomes `~`.
export function abbreviateHome(path: string, home?: string | null): string {
  if (!home) return path
  const trimmed = home.replace(/[\\/]+$/, '')
  if (trimmed.length === 0) return path
  if (path === trimmed) return '~'
  if (path.startsWith(trimmed + '/') || path.startsWith(trimmed + '\\')) return '~' + path.slice(trimmed.length)
  return path
}

/** `BackendFailure.searchedPaths`. */
export function failureSearchedPaths(failure: BackendFailure): string[] {
  return failure.kind === 'notFound' ? failure.searched : []
}

/** `BackendFailure.foundPath`. */
export function failureFoundPath(failure: BackendFailure): string | null {
  return failure.kind === 'incompatible' ? failure.found : null
}

/** `BackendFailure.stderrTail`. */
export function failureStderrTail(failure: BackendFailure): string | null {
  return failure.kind === 'exited' && failure.tail.length > 0 ? failure.tail : null
}

/// What `BackendClient.start` throws: the failure it recorded as
/// `lastStartFailure`, as an `Error` so it can travel through `await`.
export class BackendFailureError extends Error {
  readonly failure: BackendFailure

  constructor(failure: BackendFailure) {
    const described = describe(failure)
    super(`${described.title}: ${described.detail}`)
    this.name = 'BackendFailureError'
    this.failure = failure
  }
}

/// `AppState.failure(from:error:)`: the `BackendFailure` for whatever
/// `start()` threw (a `BackendFailureError` carries one; anything else is
/// `transport` with its text).
export function failureOf(error: unknown): BackendFailure {
  return BackendFailure.fromInvoke(error)
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}

function stringOr(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value : fallback
}
