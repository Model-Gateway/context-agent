import Foundation

// MARK: - JSONValue

/// Arbitrary JSON, used for tool arguments, config patches and anything the
/// protocol leaves open. Keys are preserved verbatim (no snake/camel
/// conversion happens anywhere in the protocol layer).
enum JSONValue: Codable, Equatable, Hashable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case null
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let b = try? container.decode(Bool.self) {
            self = .bool(b)
        } else if let n = try? container.decode(Double.self) {
            self = .number(n)
        } else if let s = try? container.decode(String.self) {
            self = .string(s)
        } else if let a = try? container.decode([JSONValue].self) {
            self = .array(a)
        } else if let o = try? container.decode([String: JSONValue].self) {
            self = .object(o)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let s): try container.encode(s)
        case .number(let n):
            if n.rounded() == n, abs(n) < 9_007_199_254_740_992 {
                try container.encode(Int64(n))
            } else {
                try container.encode(n)
            }
        case .bool(let b): try container.encode(b)
        case .null: try container.encodeNil()
        case .array(let a): try container.encode(a)
        case .object(let o): try container.encode(o)
        }
    }

    init(any: Any) {
        switch any {
        case let n as NSNumber:
            // NSNumber must be tested before Bool: JSONSerialization's 0/1 bridge to Bool otherwise.
            if CFGetTypeID(n) == CFBooleanGetTypeID() { self = .bool(n.boolValue) } else { self = .number(n.doubleValue) }
        case let s as String: self = .string(s)
        case let a as [Any]: self = .array(a.map { JSONValue(any: $0) })
        case let o as [String: Any]: self = .object(o.mapValues { JSONValue(any: $0) })
        case is NSNull: self = .null
        default: self = .null
        }
    }

    var foundationObject: Any {
        switch self {
        case .string(let s): return s
        case .number(let n): return n.rounded() == n && abs(n) < 9_007_199_254_740_992 ? Int64(n) as Any : n as Any
        case .bool(let b): return b
        case .null: return NSNull()
        case .array(let a): return a.map { $0.foundationObject }
        case .object(let o): return o.mapValues { $0.foundationObject }
        }
    }

    subscript(key: String) -> JSONValue? {
        if case .object(let o) = self { return o[key] }
        return nil
    }

    subscript(index: Int) -> JSONValue? {
        if case .array(let a) = self, a.indices.contains(index) { return a[index] }
        return nil
    }

    var stringValue: String? { if case .string(let s) = self { return s } else { return nil } }
    var doubleValue: Double? { if case .number(let n) = self { return n } else { return nil } }
    var intValue: Int? { if case .number(let n) = self { return Int(n) } else { return nil } }
    var boolValue: Bool? { if case .bool(let b) = self { return b } else { return nil } }
    var arrayValue: [JSONValue]? { if case .array(let a) = self { return a } else { return nil } }
    var objectValue: [String: JSONValue]? { if case .object(let o) = self { return o } else { return nil } }
    var isNull: Bool { if case .null = self { return true } else { return false } }

    /// Pretty-printed JSON with sorted keys (for tool argument / output display).
    /// Scalars go through `compact()`: JSONSerialization raises an
    /// Objective-C exception (uncatchable) for a top-level scalar.
    func prettyPrinted() -> String {
        switch self {
        case .array, .object:
            if let data = try? JSONSerialization.data(withJSONObject: foundationObject, options: [.prettyPrinted, .sortedKeys]),
               let text = String(data: data, encoding: .utf8) {
                return text
            }
            return compact()
        default:
            return compact()
        }
    }

    func compact() -> String {
        if let data = try? JSONEncoder().encode(self), let text = String(data: data, encoding: .utf8) {
            return text
        }
        return "\(self)"
    }

    static func parse(_ text: String) -> JSONValue? {
        guard let data = text.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(JSONValue.self, from: data)
    }
}

// MARK: - Timestamps

enum RFC3339 {
    private static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    private static let plain: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// Parses the backend's `to_rfc3339()` output (any fractional precision).
    static func parse(_ text: String) -> Date? {
        if let d = plain.date(from: text) { return d }
        // Trim the fraction to three digits: ISO8601DateFormatter only accepts milliseconds.
        var trimmed = text
        if let dot = text.firstIndex(of: "."), let end = text[dot...].firstIndex(where: { $0 == "Z" || $0 == "+" || $0 == "-" }) {
            let fraction = text[text.index(after: dot)..<end]
            let kept = fraction.prefix(3)
            let padded = kept + String(repeating: "0", count: max(0, 3 - kept.count))
            trimmed = String(text[..<dot]) + "." + padded + String(text[end...])
        }
        if let d = fractional.date(from: trimmed) { return d }
        if let dot = trimmed.firstIndex(of: "."), let end = trimmed[dot...].firstIndex(where: { $0 == "Z" || $0 == "+" || $0 == "-" }) {
            return plain.date(from: String(trimmed[..<dot]) + String(trimmed[end...]))
        }
        return nil
    }
}

// MARK: - Enumerations

enum ExecutionState: String, Codable, CaseIterable {
    case idle = "IDLE"
    case preparing = "PREPARING"
    case investigating = "INVESTIGATING"
    case implementing = "IMPLEMENTING"
    case validating = "VALIDATING"
    case recovering = "RECOVERING"
    case completed = "COMPLETED"
    case blocked = "BLOCKED"
    case cancelled = "CANCELLED"
    case failed = "FAILED"

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = ExecutionState(rawValue: raw.uppercased()) ?? .idle
    }

    var isTerminal: Bool { [.completed, .blocked, .cancelled, .failed].contains(self) }

    /// Human label: "Investigating".
    var label: String { rawValue.prefix(1) + rawValue.dropFirst().lowercased() }
}

enum TaskStatus: String, Codable, CaseIterable {
    case completed, blocked, failed, cancelled

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = TaskStatus(rawValue: raw.lowercased()) ?? .failed
    }

    var label: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
}

enum RiskLevel: String, Codable {
    case readOnly = "read_only"
    case write, execute, network, destructive

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = RiskLevel(rawValue: raw) ?? .execute
    }

    var label: String {
        switch self {
        case .readOnly: return "read-only"
        default: return rawValue
        }
    }
}

enum ApprovalDecision: String, Codable {
    case approve, deny
    case approveForSession = "approve_for_session"
}

enum FileChangeKind: String, Codable {
    case created, modified, deleted

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = FileChangeKind(rawValue: raw) ?? .modified
    }
}

enum UsageSource: String, Codable {
    case reported, estimated

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = UsageSource(rawValue: raw) ?? .estimated
    }
}

// MARK: - Shared records

struct TokenUsage: Codable, Equatable {
    var promptTokens: Int
    var completionTokens: Int
    var cachedPromptTokens: Int?
    var reasoningTokens: Int?
    var source: UsageSource

    enum CodingKeys: String, CodingKey {
        case promptTokens = "prompt_tokens"
        case completionTokens = "completion_tokens"
        case cachedPromptTokens = "cached_prompt_tokens"
        case reasoningTokens = "reasoning_tokens"
        case source
    }

    var total: Int { promptTokens + completionTokens }
    static let zero = TokenUsage(promptTokens: 0, completionTokens: 0, cachedPromptTokens: nil, reasoningTokens: nil, source: .estimated)
}

struct ValidationRecord: Codable, Equatable, Hashable {
    var command: String
    var passed: Bool
    var exitCode: Int?
    var durationMs: Int
    var summary: String

    enum CodingKeys: String, CodingKey {
        case command, passed, summary
        case exitCode = "exit_code"
        case durationMs = "duration_ms"
    }
}

struct TaskOutcome: Codable, Equatable {
    var taskId: String
    var status: TaskStatus
    var summary: String
    var modifiedFiles: [String]
    var validation: [ValidationRecord]
    var unresolved: [String]
    var steps: Int
    var modelCalls: Int
    var toolCalls: Int
    var usage: TokenUsage
    var costUsd: Double?
    var durationMs: Int

    enum CodingKeys: String, CodingKey {
        case taskId = "task_id"
        case status, summary, validation, unresolved, steps, usage
        case modifiedFiles = "modified_files"
        case modelCalls = "model_calls"
        case toolCalls = "tool_calls"
        case costUsd = "cost_usd"
        case durationMs = "duration_ms"
    }
}

struct ContextStats: Codable, Equatable {
    var estimatedPromptTokens: Int
    var stablePrefixTokens: Int
    var repositorySnippets: Int
    var repositoryTokens: Int
    var memoryRecords: Int
    var memoryTokens: Int
    var conversationMessages: Int
    var conversationTokens: Int
    var retrievalBudgetTokens: Int
    var contextWindow: Int
    var compacted: Bool
    /// The split of `stablePrefixTokens`, the instructions and the compaction
    /// point; zero (empty) in events persisted before ctx reported them.
    var systemPromptTokens: Int = 0
    var toolTokens: Int = 0
    var toolGroups: [ToolGroupStats] = []
    var instructionsTokens: Int = 0
    var compactionThresholdTokens: Int = 0

    enum CodingKeys: String, CodingKey {
        case estimatedPromptTokens = "estimated_prompt_tokens"
        case stablePrefixTokens = "stable_prefix_tokens"
        case repositorySnippets = "repository_snippets"
        case repositoryTokens = "repository_tokens"
        case memoryRecords = "memory_records"
        case memoryTokens = "memory_tokens"
        case conversationMessages = "conversation_messages"
        case conversationTokens = "conversation_tokens"
        case retrievalBudgetTokens = "retrieval_budget_tokens"
        case contextWindow = "context_window"
        case compacted
        case systemPromptTokens = "system_prompt_tokens"
        case toolTokens = "tool_tokens"
        case toolGroups = "tool_groups"
        case instructionsTokens = "instructions_tokens"
        case compactionThresholdTokens = "compaction_threshold_tokens"
    }
}

extension ContextStats {
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        estimatedPromptTokens = try c.decode(Int.self, forKey: .estimatedPromptTokens)
        stablePrefixTokens = try c.decode(Int.self, forKey: .stablePrefixTokens)
        repositorySnippets = try c.decode(Int.self, forKey: .repositorySnippets)
        repositoryTokens = try c.decode(Int.self, forKey: .repositoryTokens)
        memoryRecords = try c.decode(Int.self, forKey: .memoryRecords)
        memoryTokens = try c.decode(Int.self, forKey: .memoryTokens)
        conversationMessages = try c.decode(Int.self, forKey: .conversationMessages)
        conversationTokens = try c.decode(Int.self, forKey: .conversationTokens)
        retrievalBudgetTokens = try c.decode(Int.self, forKey: .retrievalBudgetTokens)
        contextWindow = try c.decode(Int.self, forKey: .contextWindow)
        compacted = try c.decode(Bool.self, forKey: .compacted)
        systemPromptTokens = try c.decodeIfPresent(Int.self, forKey: .systemPromptTokens) ?? 0
        toolTokens = try c.decodeIfPresent(Int.self, forKey: .toolTokens) ?? 0
        toolGroups = try c.decodeIfPresent([ToolGroupStats].self, forKey: .toolGroups) ?? []
        instructionsTokens = try c.decodeIfPresent(Int.self, forKey: .instructionsTokens) ?? 0
        compactionThresholdTokens = try c.decodeIfPresent(Int.self, forKey: .compactionThresholdTokens) ?? 0
    }
}

/// One source of tool definitions in `ContextStats.toolGroups`.
struct ToolGroupStats: Codable, Equatable, Hashable {
    /// The MCP server; nil for the built-in tools.
    var server: String?
    var tools: Int
    var tokens: Int
}

/// What a sub-agent may do (`AgentMode` in the backend).
enum AgentMode: String, Codable {
    case explore, implement

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = AgentMode(rawValue: raw) ?? .explore
    }

    var label: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
}

/// A finished sub-agent's report (`agent_finished.report`).
struct AgentReport: Codable, Equatable {
    var agentId: String
    var name: String
    var mode: AgentMode
    var status: TaskStatus
    var summary: String
    var modifiedFiles: [String]
    var unresolved: [String]
    var steps: Int
    var toolCalls: Int
    var usage: TokenUsage
    var durationMs: Int

    enum CodingKeys: String, CodingKey {
        case name, mode, status, summary, unresolved, steps, usage
        case agentId = "agent_id"
        case modifiedFiles = "modified_files"
        case toolCalls = "tool_calls"
        case durationMs = "duration_ms"
    }
}

struct ApprovalRequest: Codable, Equatable, Identifiable {
    var id: String
    var tool: String
    var risk: RiskLevel
    var summary: String
    var details: String
    var reason: String

    /// `tool` of the question asked when ModelGateway would only go on at a
    /// much worse discount: deny cancels the run, approve continues it, and
    /// "always continue" also turns `spend.confirm_discount_drop` off.
    static let discountDropTool = "discount_drop"

    var isDiscountDrop: Bool { tool == Self.discountDropTool }
}

struct ToolCallInfo: Codable, Equatable, Identifiable {
    var id: String
    var name: String
    var arguments: JSONValue
}

// MARK: - Sent prompt

/// `prompt_sent`: every message of a model request and the tools it offered.
struct SentPrompt: Equatable {
    struct Message: Decodable, Equatable, Identifiable {
        var id = UUID()
        /// `policy`, `task_instructions`, `context`, `user`, `assistant` or `tool_result`.
        var kind: String
        var text: String
        var estimatedTokens: Int
        var images: Int

        enum CodingKeys: String, CodingKey {
            case kind, text, images
            case estimatedTokens = "estimated_tokens"
        }

        init(kind: String, text: String, estimatedTokens: Int = 0, images: Int = 0) {
            self.kind = kind
            self.text = text
            self.estimatedTokens = estimatedTokens
            self.images = images
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "context"
            text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
            estimatedTokens = try c.decodeIfPresent(Int.self, forKey: .estimatedTokens) ?? 0
            images = try c.decodeIfPresent(Int.self, forKey: .images) ?? 0
        }
    }

    var step: Int
    var messages: [Message]
    var tools: [String]

    var estimatedTokens: Int { messages.reduce(0) { $0 + $1.estimatedTokens } }

    /// The whole prompt as plain text (Copy).
    var plainText: String {
        var parts = messages.map { "### \($0.kind)\n\n\($0.text)" }
        if !tools.isEmpty { parts.append("### tools\n\n" + tools.joined(separator: ", ")) }
        return parts.joined(separator: "\n\n")
    }
}

// MARK: - AgentEvent

/// Mirrors `ctx_core::AgentEvent` (tag field `event`). Unknown variants
/// decode as `.unknown` so a newer backend never breaks the UI.
indirect enum AgentEvent: Equatable {
    case taskStarted(taskId: String, objective: String, continued: Bool)
    case stateChanged(taskId: String, from: ExecutionState, to: ExecutionState)
    case contextCompiled(taskId: String, stats: ContextStats)
    case modelRequestStarted(taskId: String, step: Int, estimatedPromptTokens: Int)
    /// The full prompt of the run's first model request (developer mode).
    case promptSent(taskId: String, prompt: SentPrompt)
    case modelTextDelta(taskId: String, text: String)
    case modelReasoningDelta(taskId: String, text: String)
    case modelResponse(taskId: String, text: String?, toolCallCount: Int, usage: TokenUsage?)
    case toolCallStarted(taskId: String, callId: String, name: String, arguments: JSONValue)
    case toolCallFinished(taskId: String, callId: String, name: String, success: Bool, summary: String, durationMs: Int)
    case toolCallOutput(taskId: String, callId: String, name: String, content: String, fullLength: Int, isError: Bool)
    case approvalRequested(taskId: String, request: ApprovalRequest)
    case approvalResolved(taskId: String, requestId: String, decision: ApprovalDecision)
    case validationStarted(taskId: String, command: String)
    case validationFinished(taskId: String, record: ValidationRecord)
    case fileChanged(taskId: String, path: String, kind: FileChangeKind)
    case checkpoint(taskId: String, checkpointId: String, reason: String)
    case warning(taskId: String, message: String)
    case usage(taskId: String, cumulative: TokenUsage, costUsd: Double?)
    /// A message sent to the running turn (`task/steer`) reached the model.
    case steered(taskId: String, text: String, images: Int, files: Int)
    /// A sub-agent deployed by the Ultra effort tier; `agentTaskId` is the
    /// task its own events carry.
    case agentStarted(taskId: String, agentId: String, agentTaskId: String, name: String, mode: AgentMode, objective: String)
    /// One of a sub-agent's own events, wrapped into the lead task's stream.
    case agentProgress(taskId: String, agentId: String, inner: AgentEvent)
    case agentFinished(taskId: String, agentId: String, report: AgentReport)
    case taskCompleted(taskId: String, outcome: TaskOutcome)
    case unknown(name: String, taskId: String?)

    var taskId: String? {
        switch self {
        case .taskStarted(let id, _, _), .stateChanged(let id, _, _), .contextCompiled(let id, _),
             .modelRequestStarted(let id, _, _), .promptSent(let id, _), .modelTextDelta(let id, _), .modelReasoningDelta(let id, _),
             .modelResponse(let id, _, _, _), .toolCallStarted(let id, _, _, _), .toolCallFinished(let id, _, _, _, _, _),
             .toolCallOutput(let id, _, _, _, _, _), .approvalRequested(let id, _), .approvalResolved(let id, _, _),
             .validationStarted(let id, _), .validationFinished(let id, _), .fileChanged(let id, _, _),
             .checkpoint(let id, _, _), .warning(let id, _), .usage(let id, _, _), .steered(let id, _, _, _), .taskCompleted(let id, _),
             .agentStarted(let id, _, _, _, _, _), .agentProgress(let id, _, _), .agentFinished(let id, _, _):
            return id
        case .unknown(_, let id):
            return id
        }
    }

    var name: String {
        switch self {
        case .taskStarted: return "task_started"
        case .stateChanged: return "state_changed"
        case .contextCompiled: return "context_compiled"
        case .modelRequestStarted: return "model_request_started"
        case .promptSent: return "prompt_sent"
        case .modelTextDelta: return "model_text_delta"
        case .modelReasoningDelta: return "model_reasoning_delta"
        case .modelResponse: return "model_response"
        case .toolCallStarted: return "tool_call_started"
        case .toolCallFinished: return "tool_call_finished"
        case .toolCallOutput: return "tool_call_output"
        case .approvalRequested: return "approval_requested"
        case .approvalResolved: return "approval_resolved"
        case .validationStarted: return "validation_started"
        case .validationFinished: return "validation_finished"
        case .fileChanged: return "file_changed"
        case .checkpoint: return "checkpoint"
        case .warning: return "warning"
        case .usage: return "usage"
        case .steered: return "steered"
        case .agentStarted: return "agent_started"
        case .agentProgress: return "agent_progress"
        case .agentFinished: return "agent_finished"
        case .taskCompleted: return "task_completed"
        case .unknown(let n, _): return n
        }
    }
}

extension AgentEvent: Decodable {
    private enum Keys: String, CodingKey {
        case event
        case taskId = "task_id"
        case objective, continued, from, to, stats, step, text, usage, name, arguments, success, summary, request, decision, command, record, path, kind, reason, message, outcome, content
        case estimatedPromptTokens = "estimated_prompt_tokens"
        case toolCallCount = "tool_call_count"
        case callId = "call_id"
        case durationMs = "duration_ms"
        case fullLength = "full_len"
        case isError = "is_error"
        case requestId = "request_id"
        case checkpointId = "checkpoint_id"
        case cumulative
        case costUsd = "cost_usd"
        case agentId = "agent_id"
        case agentTaskId = "agent_task_id"
        case mode, inner, report, images, files, messages, tools
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Keys.self)
        let kind = try c.decode(String.self, forKey: .event)
        let taskId = try c.decodeIfPresent(String.self, forKey: .taskId)
        func id() throws -> String {
            guard let taskId else { throw DecodingError.keyNotFound(Keys.taskId, .init(codingPath: c.codingPath, debugDescription: "task_id missing")) }
            return taskId
        }
        switch kind {
        case "task_started":
            self = .taskStarted(taskId: try id(), objective: try c.decode(String.self, forKey: .objective), continued: try c.decodeIfPresent(Bool.self, forKey: .continued) ?? false)
        case "state_changed":
            self = .stateChanged(taskId: try id(), from: try c.decode(ExecutionState.self, forKey: .from), to: try c.decode(ExecutionState.self, forKey: .to))
        case "context_compiled":
            self = .contextCompiled(taskId: try id(), stats: try c.decode(ContextStats.self, forKey: .stats))
        case "model_request_started":
            self = .modelRequestStarted(taskId: try id(), step: try c.decodeIfPresent(Int.self, forKey: .step) ?? 0, estimatedPromptTokens: try c.decodeIfPresent(Int.self, forKey: .estimatedPromptTokens) ?? 0)
        case "prompt_sent":
            let prompt = SentPrompt(step: try c.decodeIfPresent(Int.self, forKey: .step) ?? 0, messages: try c.decodeIfPresent([SentPrompt.Message].self, forKey: .messages) ?? [], tools: try c.decodeIfPresent([String].self, forKey: .tools) ?? [])
            self = .promptSent(taskId: try id(), prompt: prompt)
        case "model_text_delta":
            self = .modelTextDelta(taskId: try id(), text: try c.decodeIfPresent(String.self, forKey: .text) ?? "")
        case "model_reasoning_delta":
            self = .modelReasoningDelta(taskId: try id(), text: try c.decodeIfPresent(String.self, forKey: .text) ?? "")
        case "model_response":
            self = .modelResponse(taskId: try id(), text: try c.decodeIfPresent(String.self, forKey: .text), toolCallCount: try c.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0, usage: try c.decodeIfPresent(TokenUsage.self, forKey: .usage))
        case "tool_call_started":
            self = .toolCallStarted(taskId: try id(), callId: try c.decode(String.self, forKey: .callId), name: try c.decode(String.self, forKey: .name), arguments: try c.decodeIfPresent(JSONValue.self, forKey: .arguments) ?? .null)
        case "tool_call_finished":
            self = .toolCallFinished(taskId: try id(), callId: try c.decode(String.self, forKey: .callId), name: try c.decode(String.self, forKey: .name), success: try c.decodeIfPresent(Bool.self, forKey: .success) ?? false, summary: try c.decodeIfPresent(String.self, forKey: .summary) ?? "", durationMs: try c.decodeIfPresent(Int.self, forKey: .durationMs) ?? 0)
        case "tool_call_output":
            self = .toolCallOutput(taskId: try id(), callId: try c.decode(String.self, forKey: .callId), name: try c.decodeIfPresent(String.self, forKey: .name) ?? "", content: try c.decodeIfPresent(String.self, forKey: .content) ?? "", fullLength: try c.decodeIfPresent(Int.self, forKey: .fullLength) ?? 0, isError: try c.decodeIfPresent(Bool.self, forKey: .isError) ?? false)
        case "approval_requested":
            self = .approvalRequested(taskId: try id(), request: try c.decode(ApprovalRequest.self, forKey: .request))
        case "approval_resolved":
            self = .approvalResolved(taskId: try id(), requestId: try c.decode(String.self, forKey: .requestId), decision: try c.decode(ApprovalDecision.self, forKey: .decision))
        case "validation_started":
            self = .validationStarted(taskId: try id(), command: try c.decodeIfPresent(String.self, forKey: .command) ?? "")
        case "validation_finished":
            self = .validationFinished(taskId: try id(), record: try c.decode(ValidationRecord.self, forKey: .record))
        case "file_changed":
            self = .fileChanged(taskId: try id(), path: try c.decode(String.self, forKey: .path), kind: try c.decode(FileChangeKind.self, forKey: .kind))
        case "checkpoint":
            self = .checkpoint(taskId: try id(), checkpointId: try c.decodeIfPresent(String.self, forKey: .checkpointId) ?? "", reason: try c.decodeIfPresent(String.self, forKey: .reason) ?? "")
        case "warning":
            self = .warning(taskId: try id(), message: try c.decodeIfPresent(String.self, forKey: .message) ?? "")
        case "usage":
            self = .usage(taskId: try id(), cumulative: try c.decode(TokenUsage.self, forKey: .cumulative), costUsd: try c.decodeIfPresent(Double.self, forKey: .costUsd))
        case "steered":
            self = .steered(taskId: try id(), text: try c.decodeIfPresent(String.self, forKey: .text) ?? "", images: try c.decodeIfPresent(Int.self, forKey: .images) ?? 0, files: try c.decodeIfPresent(Int.self, forKey: .files) ?? 0)
        case "task_completed":
            self = .taskCompleted(taskId: try id(), outcome: try c.decode(TaskOutcome.self, forKey: .outcome))
        case "agent_started":
            self = .agentStarted(taskId: try id(), agentId: try c.decode(String.self, forKey: .agentId), agentTaskId: try c.decodeIfPresent(String.self, forKey: .agentTaskId) ?? "", name: try c.decodeIfPresent(String.self, forKey: .name) ?? "agent", mode: try c.decodeIfPresent(AgentMode.self, forKey: .mode) ?? .explore, objective: try c.decodeIfPresent(String.self, forKey: .objective) ?? "")
        case "agent_progress":
            self = .agentProgress(taskId: try id(), agentId: try c.decode(String.self, forKey: .agentId), inner: try c.decode(AgentEvent.self, forKey: .inner))
        case "agent_finished":
            self = .agentFinished(taskId: try id(), agentId: try c.decode(String.self, forKey: .agentId), report: try c.decode(AgentReport.self, forKey: .report))
        default:
            self = .unknown(name: kind, taskId: taskId)
        }
    }
}

// MARK: - Configuration

struct ModelSettings: Codable, Equatable {
    var name: String
    var baseUrl: String
    var apiKeyEnv: String
    var apiKeySet: Bool
    var contextWindow: Int
    var maxOutputTokens: Int
    var reasoningEffort: String?
    /// `model.credential`: the stored credential the key comes from
    /// ("modelgateway" while signed in), else nil.
    var credential: String?

    enum CodingKeys: String, CodingKey {
        case name, credential
        case baseUrl = "base_url"
        case apiKeyEnv = "api_key_env"
        case apiKeySet = "api_key_set"
        case contextWindow = "context_window"
        case maxOutputTokens = "max_output_tokens"
        case reasoningEffort = "reasoning_effort"
    }
}

struct PermissionSettings: Codable, Equatable {
    var profile: String
    var allowedCommands: [String]
    var deniedCommands: [String]
    var allowNetworkCommands: Bool

    enum CodingKeys: String, CodingKey {
        case profile
        case allowedCommands = "allowed_commands"
        case deniedCommands = "denied_commands"
        case allowNetworkCommands = "allow_network_commands"
    }
}

/// USD per million tokens for one model; cached input falls back to `input`.
struct ModelPrice: Codable, Equatable, Hashable {
    var input: Double
    var output: Double
    var cacheRead: Double?

    enum CodingKeys: String, CodingKey {
        case input, output
        case cacheRead = "cache_read"
    }
}

/// `[pricing]`: the user's own prices. `models` prices one model each; the
/// flat prices are the fallback for models with no other price.
struct PricingSettings: Codable, Equatable {
    var inputPerMillion: Double?
    var cachedInputPerMillion: Double?
    var outputPerMillion: Double?
    var models: [String: ModelPrice] = [:]

    enum CodingKeys: String, CodingKey {
        case models
        case inputPerMillion = "input_per_million"
        case cachedInputPerMillion = "cached_input_per_million"
        case outputPerMillion = "output_per_million"
    }

    init(inputPerMillion: Double? = nil, cachedInputPerMillion: Double? = nil, outputPerMillion: Double? = nil, models: [String: ModelPrice] = [:]) {
        self.inputPerMillion = inputPerMillion
        self.cachedInputPerMillion = cachedInputPerMillion
        self.outputPerMillion = outputPerMillion
        self.models = models
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        inputPerMillion = try c.decodeIfPresent(Double.self, forKey: .inputPerMillion)
        cachedInputPerMillion = try c.decodeIfPresent(Double.self, forKey: .cachedInputPerMillion)
        outputPerMillion = try c.decodeIfPresent(Double.self, forKey: .outputPerMillion)
        models = try c.decodeIfPresent([String: ModelPrice].self, forKey: .models) ?? [:]
    }

    /// The flat prices, when input and output are both set.
    var flat: ModelPrice? {
        guard let inputPerMillion, let outputPerMillion else { return nil }
        return ModelPrice(input: inputPerMillion, output: outputPerMillion, cacheRead: cachedInputPerMillion)
    }
}

/// The window spending limits count over.
enum SpendPeriod: String, Codable, CaseIterable, Identifiable {
    case day, week, month

    var id: String { rawValue }

    var title: String {
        switch self {
        case .day: "Per day"
        case .week: "Per week"
        case .month: "Per month"
        }
    }

    /// "today", "this week", "this month".
    var phrase: String {
        switch self {
        case .day: "today"
        case .week: "this week"
        case .month: "this month"
        }
    }
}

/// `[spend]`: caps on what the agent may spend per period.
struct SpendSettings: Codable, Equatable {
    var period: SpendPeriod = .month
    var maxUsd: Double?
    var models: [String: Double] = [:]
    /// Ask before ModelGateway falls back to a provider discounted at least
    /// 10 points below the model's best offer; false = always continue.
    var confirmDiscountDrop: Bool = true

    enum CodingKeys: String, CodingKey {
        case period, models
        case maxUsd = "max_usd"
        case confirmDiscountDrop = "confirm_discount_drop"
    }

    init(period: SpendPeriod = .month, maxUsd: Double? = nil, models: [String: Double] = [:], confirmDiscountDrop: Bool = true) {
        self.period = period
        self.maxUsd = maxUsd
        self.models = models
        self.confirmDiscountDrop = confirmDiscountDrop
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        period = try c.decodeIfPresent(SpendPeriod.self, forKey: .period) ?? .month
        maxUsd = try c.decodeIfPresent(Double.self, forKey: .maxUsd)
        models = try c.decodeIfPresent([String: Double].self, forKey: .models) ?? [:]
        confirmDiscountDrop = try c.decodeIfPresent(Bool.self, forKey: .confirmDiscountDrop) ?? true
    }
}

/// `spend/summary`: this period's spend against the limits.
struct SpendSummary: Codable, Equatable {
    var period: SpendPeriod
    var since: String
    var totalUsd: Double
    var maxUsd: Double?
    var models: [ModelSpend]

    enum CodingKeys: String, CodingKey {
        case period, since, models
        case totalUsd = "total_usd"
        case maxUsd = "max_usd"
    }
}

struct ModelSpend: Codable, Equatable, Identifiable {
    var model: String
    var costUsd: Double
    var requests: Int
    var unpricedRequests: Int
    var limitUsd: Double?

    var id: String { model }

    enum CodingKeys: String, CodingKey {
        case model, requests
        case costUsd = "cost_usd"
        case unpricedRequests = "unpriced_requests"
        case limitUsd = "limit_usd"
    }
}

/// One `pricing/lookup` row: the price a model is charged at, and from where.
struct PriceEntry: Codable, Equatable {
    var model: String
    var price: ModelPrice?
    /// "custom" (Settings), "catalog" (models.dev), "default" (the flat prices).
    var source: String?
}

struct PriceLookupResult: Codable, Equatable {
    var prices: [PriceEntry]
}

/// Result of `initialize` and `config/get`.
struct InitializeResult: Codable, Equatable {
    var protocolVersion: Int
    var ctxVersion: String
    var dataDir: String
    var configFile: String
    var configSources: [String]
    var mock: Bool
    var model: ModelSettings
    var permissions: PermissionSettings
    var pricing: PricingSettings?
    var spend: SpendSettings = SpendSettings()
    var mcp: McpSettings = McpSettings()
    /// The machine the server runs on (absent from older backends).
    var host: HostInfo? = nil

    enum CodingKeys: String, CodingKey {
        case protocolVersion = "protocol_version"
        case ctxVersion = "ctx_version"
        case dataDir = "data_dir"
        case configFile = "config_file"
        case configSources = "config_sources"
        case mock, model, permissions, pricing, spend, mcp, host
    }

    init(protocolVersion: Int, ctxVersion: String, dataDir: String, configFile: String, configSources: [String], mock: Bool, model: ModelSettings, permissions: PermissionSettings, pricing: PricingSettings?, spend: SpendSettings = SpendSettings(), mcp: McpSettings = McpSettings()) {
        self.protocolVersion = protocolVersion
        self.ctxVersion = ctxVersion
        self.dataDir = dataDir
        self.configFile = configFile
        self.configSources = configSources
        self.mock = mock
        self.model = model
        self.permissions = permissions
        self.pricing = pricing
        self.spend = spend
        self.mcp = mcp
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        protocolVersion = try c.decode(Int.self, forKey: .protocolVersion)
        ctxVersion = try c.decode(String.self, forKey: .ctxVersion)
        dataDir = try c.decode(String.self, forKey: .dataDir)
        configFile = try c.decode(String.self, forKey: .configFile)
        configSources = try c.decode([String].self, forKey: .configSources)
        mock = try c.decode(Bool.self, forKey: .mock)
        model = try c.decode(ModelSettings.self, forKey: .model)
        permissions = try c.decode(PermissionSettings.self, forKey: .permissions)
        pricing = try c.decodeIfPresent(PricingSettings.self, forKey: .pricing)
        // Backends older than spending limits leave it out.
        spend = try c.decodeIfPresent(SpendSettings.self, forKey: .spend) ?? SpendSettings()
        // Backends older than MCP support leave it out.
        mcp = try c.decodeIfPresent(McpSettings.self, forKey: .mcp) ?? McpSettings()
        host = try c.decodeIfPresent(HostInfo.self, forKey: .host)
    }
}

/// `initialize.host`: where the backend runs.
struct HostInfo: Codable, Equatable {
    var hostname: String
    var user: String
    var homeDir: String
    /// "macos", "linux", "windows", …
    var os: String

    enum CodingKeys: String, CodingKey {
        case hostname, user, os
        case homeDir = "home_dir"
    }
}

/// `fs/list` result: one directory of the backend's machine.
struct FsListing: Codable, Equatable {
    var path: String
    var parent: String?
    var home: String
    var entries: [FsEntry]
}

struct FsEntry: Codable, Equatable, Hashable, Identifiable {
    var name: String
    var path: String
    var isDir: Bool
    var isGit: Bool
    var isHidden: Bool

    var id: String { path }

    enum CodingKeys: String, CodingKey {
        case name, path
        case isDir = "is_dir"
        case isGit = "is_git"
        case isHidden = "is_hidden"
    }
}

/// A `task/run` image sent as bytes (a backend on another machine cannot
/// read the app's files): the base64 of a PNG, JPEG, GIF or WebP file.
struct InlineImage: Equatable, Hashable {
    var name: String
    var data: String
}

// MARK: - MCP

/// The user-level `[mcp]` table (docs/configuration.md): the master switch,
/// the timeouts and the servers by name.
struct McpSettings: Codable, Equatable {
    var enabled: Bool = true
    var startupTimeoutSecs: Int = 30
    var callTimeoutSecs: Int = 120
    var servers: [String: McpServerConfig] = [:]
    /// Default apps the user removed (the backend would otherwise add them back).
    var uninstalled: [String] = []

    enum CodingKeys: String, CodingKey {
        case enabled, servers, uninstalled
        case startupTimeoutSecs = "startup_timeout_secs"
        case callTimeoutSecs = "call_timeout_secs"
    }

    init(enabled: Bool = true, startupTimeoutSecs: Int = 30, callTimeoutSecs: Int = 120, servers: [String: McpServerConfig] = [:], uninstalled: [String] = []) {
        self.enabled = enabled
        self.startupTimeoutSecs = startupTimeoutSecs
        self.callTimeoutSecs = callTimeoutSecs
        self.servers = servers
        self.uninstalled = uninstalled
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
        startupTimeoutSecs = try c.decodeIfPresent(Int.self, forKey: .startupTimeoutSecs) ?? 30
        callTimeoutSecs = try c.decodeIfPresent(Int.self, forKey: .callTimeoutSecs) ?? 120
        servers = try c.decodeIfPresent([String: McpServerConfig].self, forKey: .servers) ?? [:]
        uninstalled = try c.decodeIfPresent([String].self, forKey: .uninstalled) ?? []
    }

    /// Server names, sorted.
    var serverNames: [String] { servers.keys.sorted() }
}

/// How an MCP server is reached.
enum McpTransport: String, Codable, CaseIterable, Identifiable {
    case stdio, http, builtin
    var id: String { rawValue }

    var title: String {
        switch self {
        case .stdio: return "Command"
        case .http: return "HTTP"
        case .builtin: return "Built in"
        }
    }
}

/// One `[mcp.servers.<name>]` entry; `config/patch` writes it whole under
/// `mcp.servers.<name>` (or `null` to remove it).
struct McpServerConfig: Codable, Equatable {
    var enabled: Bool = true
    var transport: McpTransport = .stdio
    var command: String?
    var args: [String] = []
    var env: [String: String] = [:]
    var cwd: String?
    var url: String?
    var headers: [String: String] = [:]
    var tools: [String] = []
    /// Tool patterns never offered to the model (`*` wildcards).
    var excludeTools: [String] = []
    /// builtin: which server ctx provides (`computer-use`).
    var builtin: String?
    /// The catalog app this entry was installed from.
    var app: String?
    /// Every tool runs without asking (a tool set to `ask` still asks).
    var bypassPermissions = false
    /// Per-tool overrides by the server's tool name.
    var permissions: [String: McpToolPermission] = [:]

    enum CodingKeys: String, CodingKey {
        case enabled, transport, command, args, env, cwd, url, headers, tools, builtin, app, permissions
        case excludeTools = "exclude_tools"
        case bypassPermissions = "bypass_permissions"
    }

    init(enabled: Bool = true, transport: McpTransport = .stdio, command: String? = nil, args: [String] = [], env: [String: String] = [:], cwd: String? = nil, url: String? = nil, headers: [String: String] = [:], tools: [String] = [], excludeTools: [String] = [], builtin: String? = nil, app: String? = nil) {
        self.enabled = enabled
        self.transport = transport
        self.command = command
        self.args = args
        self.env = env
        self.cwd = cwd
        self.url = url
        self.headers = headers
        self.tools = tools
        self.excludeTools = excludeTools
        self.builtin = builtin
        self.app = app
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
        transport = try c.decodeIfPresent(McpTransport.self, forKey: .transport) ?? .stdio
        command = try c.decodeIfPresent(String.self, forKey: .command)
        args = try c.decodeIfPresent([String].self, forKey: .args) ?? []
        env = try c.decodeIfPresent([String: String].self, forKey: .env) ?? [:]
        cwd = try c.decodeIfPresent(String.self, forKey: .cwd)
        url = try c.decodeIfPresent(String.self, forKey: .url)
        headers = try c.decodeIfPresent([String: String].self, forKey: .headers) ?? [:]
        tools = try c.decodeIfPresent([String].self, forKey: .tools) ?? []
        excludeTools = try c.decodeIfPresent([String].self, forKey: .excludeTools) ?? []
        builtin = try c.decodeIfPresent(String.self, forKey: .builtin)
        app = try c.decodeIfPresent(String.self, forKey: .app)
        bypassPermissions = try c.decodeIfPresent(Bool.self, forKey: .bypassPermissions) ?? false
        permissions = try c.decodeIfPresent([String: McpToolPermission].self, forKey: .permissions) ?? [:]
    }

    /// The command line or URL, as the backend lists it.
    var target: String {
        switch transport {
        case .stdio: return ([command ?? ""] + args).joined(separator: " ")
        case .http: return url ?? ""
        case .builtin: return "built into ctx (\(builtin ?? ""))"
        }
    }

    /// The `config/patch` value: only the keys the backend would write.
    var patchValue: JSONValue {
        var object: [String: JSONValue] = ["enabled": .bool(enabled), "transport": .string(transport.rawValue)]
        switch transport {
        case .stdio:
            object["command"] = .string(command ?? "")
            if !args.isEmpty { object["args"] = .array(args.map(JSONValue.string)) }
            if !env.isEmpty { object["env"] = .object(env.mapValues(JSONValue.string)) }
            if let cwd, !cwd.isEmpty { object["cwd"] = .string(cwd) }
        case .http:
            object["url"] = .string(url ?? "")
            if !headers.isEmpty { object["headers"] = .object(headers.mapValues(JSONValue.string)) }
        case .builtin:
            object["builtin"] = .string(builtin ?? "")
        }
        if !tools.isEmpty { object["tools"] = .array(tools.map(JSONValue.string)) }
        if !excludeTools.isEmpty { object["exclude_tools"] = .array(excludeTools.map(JSONValue.string)) }
        if let app { object["app"] = .string(app) }
        if bypassPermissions { object["bypass_permissions"] = .bool(true) }
        if !permissions.isEmpty { object["permissions"] = .object(permissions.mapValues { .string($0.rawValue) }) }
        return .object(object)
    }
}

/// `[mcp.servers.<name>.permissions]`: one tool's override.
enum McpToolPermission: String, Codable, Equatable {
    /// Runs without asking, even when the server marks it destructive.
    case allow
    /// Asks every time, even when the server marks it read-only.
    case ask
}

/// `mcp/catalog`: one curated app.
struct McpCatalogApp: Codable, Equatable, Identifiable {
    var id: String
    var name: String
    var tagline: String
    /// A prompt that shows what the app is for.
    var examplePrompt: String = ""
    var description: String
    var icon: String
    /// Brand logo in the asset catalog (`logo-<name>`); nil keeps the symbol.
    var logo: String?
    /// A single-colour mark, tinted for the current appearance.
    var logoMonochrome: Bool = false
    var category: String
    var vendor: String
    var homepage: String?
    var builtin: Bool
    var defaultInstalled: Bool
    var requirements: [String]
    var inputs: [McpCatalogInput]
    /// "What Context can access": tool groups for the connect screen.
    var access: [McpCatalogAccess] = []
    /// Sign-in in place of the token input (GitHub device flow).
    var oauth: McpCatalogOAuth?
    var template: McpServerConfig

    enum CodingKeys: String, CodingKey {
        case id, name, tagline, description, icon, logo, category, vendor, homepage, builtin, requirements, inputs, access, oauth, template
        case examplePrompt = "example_prompt"
        case logoMonochrome = "logo_monochrome"
        case defaultInstalled = "default_installed"
    }

    init(id: String, name: String, tagline: String, examplePrompt: String = "", description: String, icon: String, logo: String? = nil, logoMonochrome: Bool = false, category: String, vendor: String, homepage: String?, builtin: Bool, defaultInstalled: Bool, requirements: [String], inputs: [McpCatalogInput], access: [McpCatalogAccess] = [], template: McpServerConfig) {
        self.id = id
        self.name = name
        self.tagline = tagline
        self.examplePrompt = examplePrompt
        self.description = description
        self.icon = icon
        self.logo = logo
        self.logoMonochrome = logoMonochrome
        self.category = category
        self.vendor = vendor
        self.homepage = homepage
        self.builtin = builtin
        self.defaultInstalled = defaultInstalled
        self.requirements = requirements
        self.inputs = inputs
        self.access = access
        self.template = template
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        tagline = try c.decode(String.self, forKey: .tagline)
        examplePrompt = try c.decodeIfPresent(String.self, forKey: .examplePrompt) ?? ""
        description = try c.decode(String.self, forKey: .description)
        icon = try c.decode(String.self, forKey: .icon)
        logo = try c.decodeIfPresent(String.self, forKey: .logo)
        logoMonochrome = try c.decodeIfPresent(Bool.self, forKey: .logoMonochrome) ?? false
        category = try c.decode(String.self, forKey: .category)
        vendor = try c.decode(String.self, forKey: .vendor)
        homepage = try c.decodeIfPresent(String.self, forKey: .homepage)
        builtin = try c.decode(Bool.self, forKey: .builtin)
        defaultInstalled = try c.decode(Bool.self, forKey: .defaultInstalled)
        requirements = try c.decodeIfPresent([String].self, forKey: .requirements) ?? []
        inputs = try c.decodeIfPresent([McpCatalogInput].self, forKey: .inputs) ?? []
        access = try c.decodeIfPresent([McpCatalogAccess].self, forKey: .access) ?? []
        oauth = try c.decodeIfPresent(McpCatalogOAuth.self, forKey: .oauth)
        template = try c.decode(McpServerConfig.self, forKey: .template)
    }
}

/// `CatalogApp.oauth`: the app can sign in instead of taking a token.
struct McpCatalogOAuth: Codable, Equatable, Hashable {
    var provider: String
    /// Credential-store entry the token goes to.
    var credential: String
    /// The input the sign-in fills, with `${credential:<credential>}`.
    var input: String
    var scopes: [String]
    /// False when this build has no OAuth client ID: fall back to the token field.
    var available: Bool

    var reference: String { "${credential:\(credential)}" }
}

/// `mcp/oauth/start`.
struct McpDeviceCode: Codable, Equatable {
    var deviceCode: String
    var userCode: String
    var verificationUri: String
    var expiresIn: Int
    var interval: Int

    enum CodingKeys: String, CodingKey {
        case interval
        case deviceCode = "device_code"
        case userCode = "user_code"
        case verificationUri = "verification_uri"
        case expiresIn = "expires_in"
    }
}

/// `mcp/oauth/poll`.
struct McpDevicePoll: Codable, Equatable {
    /// `pending`, `approved`, `denied` or `expired`.
    var status: String
    var interval: Int?
    var account: String?
}

/// One "What Context can access" line: a group of the app's tools.
struct McpCatalogAccess: Codable, Equatable, Identifiable, Hashable {
    var title: String
    var description: String
    /// Tool-name patterns (`*` wildcards); unchecked groups go to `exclude_tools`.
    var tools: [String]
    var required: Bool
    var id: String { title }
}

struct McpCatalogInput: Codable, Equatable, Identifiable, Hashable {
    var key: String
    var label: String
    var help: String
    var secret: Bool
    var required: Bool
    /// `env` or `header`.
    var kind: String
    var id: String { key }
}

struct McpCatalogResponse: Decodable {
    var apps: [McpCatalogApp]
}

/// `mcp/status` and the `mcp/status` notification: one configured server.
struct McpServerStatus: Codable, Equatable, Identifiable {
    enum State: String, Codable {
        case disabled, connecting, connected, failed
    }

    var name: String
    var enabled: Bool
    var app: String?
    var state: State
    var error: String?
    var transport: String
    var target: String
    var serverName: String?
    var serverVersion: String?
    var protocolVersion: String?
    var instructions: String?
    var tools: [McpToolInfo] = []

    var id: String { name }

    enum CodingKeys: String, CodingKey {
        case name, enabled, app, state, error, transport, target, instructions, tools
        case serverName = "server_name"
        case serverVersion = "server_version"
        case protocolVersion = "protocol_version"
    }
}

/// One tool of a connected MCP server.
struct McpToolInfo: Codable, Equatable, Identifiable, Hashable {
    var name: String
    var qualifiedName: String
    var description: String
    var readOnly: Bool
    var destructive: Bool
    /// False for a tool the server offers but the entry's `tools` allowlist leaves out.
    var enabled: Bool = true

    var id: String { qualifiedName }

    enum CodingKeys: String, CodingKey {
        case name, description, destructive, enabled
        case qualifiedName = "qualified_name"
        case readOnly = "read_only"
    }

    init(name: String, qualifiedName: String, description: String, readOnly: Bool, destructive: Bool, enabled: Bool = true) {
        self.name = name
        self.qualifiedName = qualifiedName
        self.description = description
        self.readOnly = readOnly
        self.destructive = destructive
        self.enabled = enabled
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        qualifiedName = try c.decode(String.self, forKey: .qualifiedName)
        description = try c.decodeIfPresent(String.self, forKey: .description) ?? ""
        readOnly = try c.decodeIfPresent(Bool.self, forKey: .readOnly) ?? false
        destructive = try c.decodeIfPresent(Bool.self, forKey: .destructive) ?? false
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
    }
}

struct McpStatusNotification: Decodable, Equatable {
    var servers: [McpServerStatus]
}

/// `mcp/permissions`: the Computer Use permissions as the backend process sees them.
struct McpPermissions: Codable, Equatable {
    var screenRecording: Bool
    var accessibility: Bool
    var platform: String = "macos"

    enum CodingKeys: String, CodingKey {
        case accessibility, platform
        case screenRecording = "screen_recording"
    }
}

// MARK: - Projects

struct GitInfo: Codable, Equatable {
    var branch: String?
    var head: String?
    var dirtyFiles: Int
    /// Set when the root is a linked worktree of another checkout.
    var worktree: WorktreeInfo? = nil

    enum CodingKeys: String, CodingKey {
        case branch, head, worktree
        case dirtyFiles = "dirty_files"
    }

    var shortHead: String? { head.map { String($0.prefix(8)) } }
}

/// `GitSummary.worktree`: the parent checkout of a linked worktree.
struct WorktreeInfo: Codable, Equatable {
    var parentRoot: String
    /// Created by `worktree/create` (lives in the data dir).
    var managed: Bool

    enum CodingKeys: String, CodingKey {
        case managed
        case parentRoot = "parent_root"
    }
}

/// One entry of `worktree/list`.
struct WorktreeEntry: Codable, Equatable, Identifiable {
    var path: String
    var branch: String?
    var head: String?
    var isMain: Bool
    var managed: Bool

    var id: String { path }
    var name: String { (path as NSString).lastPathComponent }

    enum CodingKeys: String, CodingKey {
        case path, branch, head, managed
        case isMain = "is_main"
    }
}

struct WorktreeRemoveResult: Codable, Equatable {
    var removed: String
    var branch: String?
    var branchDeleted: Bool

    enum CodingKeys: String, CodingKey {
        case removed, branch
        case branchDeleted = "branch_deleted"
    }
}

/// `worktree/merge`: the parent's new HEAD (a merge commit).
struct WorktreeMergeResult: Codable, Equatable {
    var merged: String
    var subject: String
    /// The worktree (and its branch) went away with the merge (`delete_after`).
    var removed: Bool

    var shortMerged: String { String(merged.prefix(8)) }
}

struct LanguageCount: Codable, Equatable, Identifiable {
    var language: String
    var files: Int
    var id: String { language }
}

struct IndexStats: Codable, Equatable {
    var generation: Int
    var files: Int
    var symbols: Int
    var chunks: Int
    var relationships: Int
    var embeddings: Int
    var lastFullIndexAt: String?
    var languages: [LanguageCount]

    enum CodingKeys: String, CodingKey {
        case generation, files, symbols, chunks, relationships, embeddings, languages
        case lastFullIndexAt = "last_full_index_at"
    }
}

struct ValidationCommand: Codable, Equatable, Identifiable {
    var command: String
    var kind: String
    var cwd: String
    var source: String
    var id: String { kind + ":" + cwd + ":" + command }
}

struct RunningTask: Codable, Equatable {
    var sessionId: String
    var taskId: String?
    var startedAt: String

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case taskId = "task_id"
        case startedAt = "started_at"
    }
}

struct ProjectInfo: Codable, Equatable, Identifiable {
    var projectId: String
    var root: String
    var name: String
    var git: GitInfo?
    var index: IndexStats
    var indexing: Bool
    var model: String
    var permissionProfile: String
    var sandbox: String
    var validationCommands: [ValidationCommand]
    var languages: [LanguageCount]
    var buildFiles: [String]
    var fileCount: Int
    /// The oldest running task (backends that ran one task per project).
    var runningTask: RunningTask?
    /// Every session's running task, oldest first; absent from older backends.
    var runningTasks: [RunningTask]? = nil
    /// True on the `project/open` that ran `git init` for the folder (`init_git`).
    var gitInitialized: Bool? = nil

    var id: String { projectId }

    /// The sessions with a task running.
    var runningSessionIds: Set<String> {
        Set(runningTasks?.map(\.sessionId) ?? runningTask.map { [$0.sessionId] } ?? [])
    }

    enum CodingKeys: String, CodingKey {
        case projectId = "project_id"
        case root, name, git, index, indexing, model, sandbox, languages
        case gitInitialized = "git_initialized"
        case permissionProfile = "permission_profile"
        case validationCommands = "validation_commands"
        case buildFiles = "build_files"
        case fileCount = "file_count"
        case runningTask = "running_task"
        case runningTasks = "running_tasks"
    }
}

// MARK: - Sessions and history

struct SessionSummary: Codable, Equatable, Identifiable {
    var id: String
    var model: String
    var mode: String
    var title: String
    var createdAt: String
    var updatedAt: String
    var activeTaskId: String?
    var taskCount: Int
    var lastStatus: TaskStatus?

    enum CodingKeys: String, CodingKey {
        case id, model, mode, title
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case activeTaskId = "active_task_id"
        case taskCount = "task_count"
        case lastStatus = "last_status"
    }

    var updatedDate: Date { RFC3339.parse(updatedAt) ?? Date.distantPast }
    var createdDate: Date { RFC3339.parse(createdAt) ?? Date.distantPast }
}

struct TaskSummary: Codable, Equatable, Identifiable {
    var id: String
    var sessionId: String?
    var objective: String
    var category: String
    var state: ExecutionState
    var status: TaskStatus?
    var summary: String?
    var createdAt: String
    var updatedAt: String
    var completedAt: String?
    var modifiedFiles: [String]
    var unresolved: [String]

    enum CodingKeys: String, CodingKey {
        case id, objective, category, state, status, summary, unresolved
        case sessionId = "session_id"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case completedAt = "completed_at"
        case modifiedFiles = "modified_files"
    }
}

enum HistoryMessageKind: String, Codable {
    case user, assistant
    case toolResult = "tool_result"

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = HistoryMessageKind(rawValue: raw) ?? .user
    }
}

struct HistoryMessage: Codable, Equatable, Identifiable {
    var id: Int
    var kind: HistoryMessageKind
    var text: String?
    var toolCalls: [ToolCallInfo]?
    var callId: String?
    var toolName: String?
    var content: String?
    var isError: Bool?
    var runtime: Bool?
    /// User messages: the image and text files sent with it.
    var images: [String]?
    var files: [FileUpload]?
    /// Untruncated length of a tool result when the backend truncated `content`.
    var fullLength: Int?
    /// User messages: a working-tree snapshot exists for the run this
    /// message started, so `session/rewind` can restore the files.
    var snapshot: Bool?
    var createdAt: String

    enum CodingKeys: String, CodingKey {
        case id, kind, text, content, runtime, images, files, snapshot
        case toolCalls = "tool_calls"
        case callId = "call_id"
        case toolName = "tool_name"
        case isError = "is_error"
        case fullLength = "full_len"
        case createdAt = "created_at"
    }
}

struct UsageTotals: Codable, Equatable {
    var promptTokens: Int
    var completionTokens: Int
    var cachedTokens: Int
    var requests: Int
    var costUsd: Double?

    enum CodingKeys: String, CodingKey {
        case requests
        case promptTokens = "prompt_tokens"
        case completionTokens = "completion_tokens"
        case cachedTokens = "cached_tokens"
        case costUsd = "cost_usd"
    }
}

struct HistoryTurn: Codable, Equatable {
    var task: TaskSummary
    var messages: [HistoryMessage]
    var events: [AgentEvent]
    var outcome: TaskOutcome?
    var usage: UsageTotals

    enum CodingKeys: String, CodingKey { case task, messages, events, outcome, usage }

    init(task: TaskSummary, messages: [HistoryMessage], events: [AgentEvent], outcome: TaskOutcome?, usage: UsageTotals) {
        self.task = task
        self.messages = messages
        self.events = events
        self.outcome = outcome
        self.usage = usage
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        task = try c.decode(TaskSummary.self, forKey: .task)
        messages = try c.decodeIfPresent([HistoryMessage].self, forKey: .messages) ?? []
        // Decode events one by one so a single malformed event never hides a whole turn.
        var events: [AgentEvent] = []
        if var list = try? c.nestedUnkeyedContainer(forKey: .events) {
            while !list.isAtEnd {
                if let ev = try? list.decode(AgentEvent.self) {
                    events.append(ev)
                } else {
                    _ = try? list.decode(JSONValue.self)
                }
            }
        }
        self.events = events
        outcome = try c.decodeIfPresent(TaskOutcome.self, forKey: .outcome)
        usage = try c.decodeIfPresent(UsageTotals.self, forKey: .usage) ?? UsageTotals(promptTokens: 0, completionTokens: 0, cachedTokens: 0, requests: 0, costUsd: nil)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(task, forKey: .task)
        try c.encode(messages, forKey: .messages)
        try c.encodeIfPresent(outcome, forKey: .outcome)
        try c.encode(usage, forKey: .usage)
    }
}

struct SessionHistory: Codable, Equatable {
    var session: SessionSummary
    var turns: [HistoryTurn]
}

/// Files `session/rewind` put back from the snapshot.
struct RestoreReport: Codable, Equatable {
    /// Written back (added, modified or type-changed since the snapshot).
    var restored: Int
    /// Created since the snapshot, removed again.
    var deleted: Int
    /// The session's files that changed again since (another session, the
    /// user), left as they are; absent from older backends.
    var skipped: [String]? = nil

    var total: Int { restored + deleted }
}

/// `session/rewind` result.
struct RewindResult: Codable, Equatable {
    var removedTasks: Int
    var removedMessages: Int
    /// Present when files were restored.
    var files: RestoreReport?
    var session: SessionSummary

    enum CodingKeys: String, CodingKey {
        case files, session
        case removedTasks = "removed_tasks"
        case removedMessages = "removed_messages"
    }
}

// MARK: - Task run options

/// `task/run.options`; every field is optional and omitted when nil.
struct TaskRunOptions: Codable, Equatable {
    var maxSteps: Int?
    var proactiveRetrieval: Bool?
    var memoryContext: Bool?
    var stream: Bool?
    var debugPrompt: Bool?

    enum CodingKeys: String, CodingKey {
        case stream
        case maxSteps = "max_steps"
        case proactiveRetrieval = "proactive_retrieval"
        case memoryContext = "memory_context"
        case debugPrompt = "debug_prompt"
    }

    static let `default` = TaskRunOptions()
}

// MARK: - Models

struct ModelsList: Codable, Equatable {
    var models: [String]
    var current: String
    var source: String
    var error: String?
    /// ModelGateway's numbers per model id; empty for other endpoints.
    var stats: [String: ModelStat]? = nil
}

/// `models/list.stats` entry: percentages, each nil when unknown.
struct ModelStat: Codable, Equatable {
    /// Discount against the official output price.
    var savingsPercent: Double?
    /// Share of failed requests over the last 24 hours.
    var errorRate24h: Double?

    enum CodingKeys: String, CodingKey {
        case savingsPercent = "savings_percent"
        case errorRate24h = "error_rate_24h"
    }

    /// "98% off"; nil below 1%.
    var savingsLabel: String? {
        guard let savingsPercent, savingsPercent.rounded() >= 1 else { return nil }
        return "\(Int(savingsPercent.rounded()))% off"
    }

    /// "1.5%", "0%", "100%".
    var errorRateLabel: String? {
        guard let errorRate24h else { return nil }
        let rounded = (errorRate24h * 10).rounded() / 10
        return rounded == rounded.rounded() ? "\(Int(rounded))%" : String(format: "%.1f%%", rounded)
    }
}

struct CapabilitySummary: Codable, Equatable {
    var probedAt: String?
    var agentCapable: Bool
    var streaming: Bool
    var toolCalling: Bool
    var parallelToolCalls: Bool
    var reasoningControls: Bool
    var issues: [String]

    enum CodingKeys: String, CodingKey {
        case streaming, issues
        case probedAt = "probed_at"
        case agentCapable = "agent_capable"
        case toolCalling = "tool_calling"
        case parallelToolCalls = "parallel_tool_calls"
        case reasoningControls = "reasoning_controls"
    }
}

struct ModelSwitchResult: Codable, Equatable {
    var model: String
    var capability: CapabilitySummary
}

// MARK: - Git and files

struct GitFileStatus: Codable, Equatable, Identifiable {
    var path: String
    var status: String
    var staged: Bool
    /// Lines added / removed against HEAD; nil for binary files and when unknown.
    var insertions: Int? = nil
    var deletions: Int? = nil
    var id: String { path }

    /// First status letter, e.g. "M", "A", "D", "?".
    var code: String { String(status.prefix(1)) }
}

struct GitRemote: Codable, Equatable, Identifiable {
    var name: String
    var url: String
    var id: String { name }
}

struct GitStatus: Codable, Equatable {
    var branch: String?
    var head: String?
    /// Subject of the HEAD commit (shown by "Undo last commit").
    var headSubject: String? = nil
    /// `origin/main` when the branch tracks one, with the commits ahead / behind it.
    var upstream: String? = nil
    var ahead: Int? = nil
    var behind: Int? = nil
    var remotes: [GitRemote]? = nil
    var files: [GitFileStatus]

    enum CodingKeys: String, CodingKey {
        case branch, head, upstream, ahead, behind, remotes, files
        case headSubject = "head_subject"
    }

    var stagedFiles: [GitFileStatus] { files.filter { $0.staged } }
    /// Working-tree changes: unstaged, untracked, and files with both kinds (`MM`).
    var unstagedFiles: [GitFileStatus] { files.filter { !$0.staged || $0.status.count > 1 } }
}

struct GitCommitResult: Codable, Equatable {
    var commit: String
    var subject: String
    var shortHash: String { String(commit.prefix(7)) }
}

struct GitCommandOutput: Codable, Equatable {
    var output: String
}

struct GitLocalBranch: Codable, Equatable, Identifiable {
    var name: String
    var upstream: String?
    var id: String { name }
}

struct GitRemoteBranch: Codable, Equatable, Identifiable {
    /// `origin/feature`
    var name: String
    /// `feature` — what `git switch` takes to track it.
    var short: String
    var id: String { name }
}

struct GitBranches: Codable, Equatable {
    var current: String?
    var local: [GitLocalBranch]
    var remote: [GitRemoteBranch]
}

struct GitLogEntry: Codable, Equatable, Identifiable {
    var hash: String
    var short: String
    var subject: String
    var author: String
    var date: String
    var id: String { hash }
}

struct GitCommitMessage: Codable, Equatable {
    var message: String
    /// `model` or `heuristic`; `error` says why the model was not used.
    var source: String
    var error: String?
}

struct GitDiscardResult: Codable, Equatable {
    var restored: Int
    var deleted: Int
}

struct GitDiff: Codable, Equatable {
    var diff: String
    var truncated: Bool
}

struct FileContent: Codable, Equatable {
    var path: String
    var content: String
    var truncated: Bool
    var language: String?
    var size: Int
    /// `sha256:…` of the bytes returned; `file/write.expected_hash` takes it.
    var hash: String? = nil
}

/// `file/write` result.
struct FileWriteResult: Codable, Equatable {
    var path: String
    var hash: String
    var size: Int
    var language: String?
    var created: Bool
}

/// One `symbols/find` hit: a definition the index knows.
struct SymbolInfo: Codable, Equatable, Hashable, Identifiable {
    var name: String
    var kind: String
    var path: String
    var parent: String?
    /// 1-based.
    var line: Int
    var endLine: Int
    var signature: String?

    var id: String { "\(path):\(line):\(name)" }

    enum CodingKeys: String, CodingKey {
        case name, kind, path, parent, line, signature
        case endLine = "end_line"
    }
}

enum SymbolsFindMode: String {
    case prefix, exact, contains
}

// MARK: - Language servers

/// `lsp/status` and the header of `lsp/open`.
struct LspStatus: Codable, Equatable {
    var path: String
    var language: String?
    /// The executable ("rust-analyzer"); nil when none is known or installed.
    var server: String?
    var installed: Bool? = nil
    var running: Bool? = nil
    var version: Int? = nil
    var triggerCharacters: [String]? = nil
    var installHint: String? = nil

    enum CodingKeys: String, CodingKey {
        case path, language, server, installed, running, version
        case triggerCharacters = "trigger_characters"
        case installHint = "install_hint"
    }
}

/// 0-based range the server wants replaced by a completion.
struct LspRange: Codable, Equatable, Hashable {
    var startLine: Int
    var startCharacter: Int
    var endLine: Int
    var endCharacter: Int

    enum CodingKeys: String, CodingKey {
        case startLine = "start_line"
        case startCharacter = "start_character"
        case endLine = "end_line"
        case endCharacter = "end_character"
    }
}

struct LspCompletionItem: Codable, Equatable, Hashable {
    var label: String
    var kind: String
    var detail: String?
    var documentation: String?
    var insertText: String
    var filterText: String?
    var range: LspRange?
    var deprecated: Bool? = nil

    enum CodingKeys: String, CodingKey {
        case label, kind, detail, documentation, range, deprecated
        case insertText = "insert_text"
        case filterText = "filter_text"
    }
}

struct LspCompletionResult: Codable, Equatable {
    var items: [LspCompletionItem]
    var incomplete: Bool
    var server: String?
}

struct LspHover: Codable, Equatable {
    var contents: String?
}

/// 1-based.
struct LspLocation: Codable, Equatable, Hashable {
    var path: String
    var line: Int
    var column: Int
}

/// One entry of the `lsp/diagnostics` notification (0-based).
struct LspDiagnostic: Codable, Equatable, Hashable, Identifiable {
    var line: Int
    var character: Int
    var endLine: Int
    var endCharacter: Int
    var severity: String
    var message: String
    var source: String?
    var code: String?

    var id: String { "\(line):\(character):\(endLine):\(endCharacter):\(message)" }
    var isError: Bool { severity == "error" }
    var isWarning: Bool { severity == "warning" }

    enum CodingKeys: String, CodingKey {
        case line, character, severity, message, source, code
        case endLine = "end_line"
        case endCharacter = "end_character"
    }
}

struct LspDiagnostics: Codable, Equatable {
    var projectId: String
    var path: String
    var diagnostics: [LspDiagnostic]

    enum CodingKeys: String, CodingKey {
        case path, diagnostics
        case projectId = "project_id"
    }
}

// MARK: - Memory, doctor

struct MemoryEntry: Codable, Equatable, Identifiable {
    var id: String
    var category: String
    var verification: String
    var content: String
    var evidencePaths: [String]
    var createdAt: String
    var invalidated: Bool

    enum CodingKeys: String, CodingKey {
        case id, category, verification, content, invalidated
        case evidencePaths = "evidence_paths"
        case createdAt = "created_at"
    }
}

struct DoctorRow: Codable, Equatable, Identifiable {
    var label: String
    var value: String
    var ok: Bool
    var id: String { label + value }
}

struct DoctorSection: Codable, Equatable, Identifiable {
    var title: String
    var rows: [DoctorRow]
    var id: String { title }
}

struct DoctorReport: Codable, Equatable {
    var problems: Int
    var sections: [DoctorSection]
}

// MARK: - Notifications

struct EventNotification: Decodable, Equatable {
    var projectId: String
    var sessionId: String
    var event: AgentEvent

    enum CodingKeys: String, CodingKey {
        case event
        case projectId = "project_id"
        case sessionId = "session_id"
    }
}

/// `suggest/next` result.
struct NextMessageSuggestion: Codable, Equatable {
    var suggestion: String?
}

/// One `suggest/starters` prompt: `kind` is fix, explain, refactor, test,
/// docs, review, feature, cleanup or other (it picks the icon).
struct StarterSuggestion: Codable, Equatable, Hashable {
    var kind: String
    var text: String
}

/// `suggest/starters` result.
struct StarterSuggestions: Codable, Equatable {
    var suggestions: [StarterSuggestion]
}

struct TaskFinished: Codable, Equatable {
    var projectId: String
    var sessionId: String
    var taskId: String?
    var status: TaskStatus?
    var error: String?
    var errorKind: String?
    /// The conversation id the run's prompt became (the handle `session/rewind` takes).
    var messageId: Int? = nil
    /// A working-tree snapshot was taken before the run.
    var snapshot: Bool? = nil

    enum CodingKeys: String, CodingKey {
        case status, error, snapshot
        case projectId = "project_id"
        case sessionId = "session_id"
        case taskId = "task_id"
        case errorKind = "error_kind"
        case messageId = "message_id"
    }

    /// A run cancelled before it reached the runtime (index wait, probe):
    /// no task, no status, only `error_kind: "cancelled"`.
    var isEarlyCancel: Bool { status == nil && errorKind == "cancelled" }
}

struct IndexProgress: Codable, Equatable {
    var projectId: String
    var done: Int
    var total: Int

    enum CodingKeys: String, CodingKey {
        case done, total
        case projectId = "project_id"
    }
}

struct IndexReady: Codable, Equatable {
    var projectId: String
    var generation: Int
    var filesIndexed: Int
    var filesRemoved: Int
    var filesUnchanged: Int
    var durationMs: Int
    var rebuilt: Bool
    var error: String?

    enum CodingKeys: String, CodingKey {
        case generation, rebuilt, error
        case projectId = "project_id"
        case filesIndexed = "files_indexed"
        case filesRemoved = "files_removed"
        case filesUnchanged = "files_unchanged"
        case durationMs = "duration_ms"
    }
}

struct LogNotification: Codable, Equatable {
    var level: String
    var message: String
}

/// `browser/request`: the `browser` tool of a run asks the app to act in the
/// project's Inspector Browser tab (`action.action` names the step).
struct BrowserRequest: Decodable, Equatable {
    var projectId: String
    var sessionId: String
    var requestId: String
    var action: JSONValue

    enum CodingKeys: String, CodingKey {
        case action
        case projectId = "project_id"
        case sessionId = "session_id"
        case requestId = "request_id"
    }
}

/// What the app answers with `browser/respond`.
struct BrowserReply: Encodable, Equatable {
    var ok: Bool
    var content: String = ""
    var url: String? = nil
    var title: String? = nil
    var error: String? = nil
}

/// `question/request`: a run's `ask_user` tool puts questions to the user;
/// the app answers with `question/respond`.
struct QuestionRequest: Decodable, Equatable, Identifiable {
    var projectId: String
    var sessionId: String
    var requestId: String
    var questions: [AgentQuestion]

    var id: String { requestId }

    enum CodingKeys: String, CodingKey {
        case questions
        case projectId = "project_id"
        case sessionId = "session_id"
        case requestId = "request_id"
    }
}

/// One question of an `ask_user` call; the user can always answer in their own words.
struct AgentQuestion: Decodable, Equatable {
    var question: String
    var header: String?
    var options: [Option]
    var multiSelect: Bool

    struct Option: Decodable, Equatable, Hashable {
        var label: String
        var description: String?
    }

    enum CodingKeys: String, CodingKey {
        case question, header, options
        case multiSelect = "multi_select"
    }

    init(question: String, header: String? = nil, options: [Option] = [], multiSelect: Bool = false) {
        self.question = question
        self.header = header
        self.options = options
        self.multiSelect = multiSelect
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        question = try c.decode(String.self, forKey: .question)
        header = try c.decodeIfPresent(String.self, forKey: .header)
        options = try c.decodeIfPresent([Option].self, forKey: .options) ?? []
        multiSelect = try c.decodeIfPresent(Bool.self, forKey: .multiSelect) ?? false
    }
}

/// One question's answer for `question/respond`: chosen labels and/or own words.
struct QuestionAnswer: Equatable {
    var selected: [String] = []
    var other: String? = nil

    var isEmpty: Bool { selected.isEmpty && (other?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true) }
}

/// Everything the backend pushes without being asked, plus process exit.
enum BackendNotification {
    case event(EventNotification)
    case taskFinished(TaskFinished)
    case indexProgress(IndexProgress)
    case indexReady(IndexReady)
    case log(LogNotification)
    case projectChanged(JSONValue)
    case lspDiagnostics(LspDiagnostics)
    case browserRequest(BrowserRequest)
    case questionRequest(QuestionRequest)
    case mcpStatus([McpServerStatus])
    case exited(code: Int32, stderrTail: String)
}

// MARK: - Errors

enum BackendError: Error, LocalizedError, Equatable {
    /// A JSON-RPC error response. `kind` is the backend's `CtxError::code()` when present.
    case rpc(code: Int, message: String, kind: String?)
    case transport(String)
    case notRunning
    case decoding(String)
    case binaryNotFound

    var errorDescription: String? {
        switch self {
        case .rpc(_, let message, _): return message
        case .transport(let m): return "Backend connection failed: \(m)"
        case .notRunning: return "The backend is not running."
        case .decoding(let m): return "Unexpected backend response: \(m)"
        case .binaryNotFound: return "The ctx backend binary was not found. Build it with `cargo build --release` or choose it in Settings → General."
        }
    }

    static let projectNotOpen = -32001
    static let taskAlreadyRunning = -32002
    static let noTaskRunning = -32003

    var isTaskAlreadyRunning: Bool { if case .rpc(let code, _, _) = self { return code == Self.taskAlreadyRunning } else { return false } }

    /// The backend went away (pipe closed, not running): nothing was processed.
    var isConnectionLoss: Bool {
        switch self {
        case .transport, .notRunning: return true
        default: return false
        }
    }
}
