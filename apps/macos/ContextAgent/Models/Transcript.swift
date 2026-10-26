import Foundation

// The rendered conversation of one chat session. `TranscriptReducer`
// (App/ChatState.swift) turns `AgentEvent`s into these items; the views in
// Views/Chat render them. Everything here is plain data.

enum TranscriptItem: Identifiable, Equatable {
    case user(UserTurn)
    case assistant(AssistantTurn)
    case tools(ToolGroup)
    case agents(AgentGroup)
    case approval(ApprovalItem)
    case outcome(OutcomeItem)
    case notice(NoticeItem)

    var id: String {
        switch self {
        case .user(let v): return v.id
        case .assistant(let v): return v.id
        case .tools(let v): return v.id
        case .agents(let v): return v.id
        case .approval(let v): return v.id
        case .outcome(let v): return v.id
        case .notice(let v): return v.id
        }
    }

    var date: Date {
        switch self {
        case .user(let v): return v.date
        case .assistant(let v): return v.date
        case .tools(let v): return v.startedAt
        case .agents(let v): return v.startedAt
        case .approval(let v): return v.date
        case .outcome(let v): return v.date
        case .notice(let v): return v.date
        }
    }
}

struct UserTurn: Identifiable, Equatable {
    var id: String = UUID().uuidString
    var text: String
    /// Image and text files sent with the message (they may since be gone).
    var images: [String] = []
    var files: [FileUpload] = []
    var date: Date = Date()
    /// Injected by the runtime (`[Context runtime] …`), rendered muted.
    var isRuntime: Bool = false
    /// The persisted conversation id (`session/history`, `task/finished`);
    /// nil while the run that stores it is in flight. The handle for
    /// `session/rewind` (edit this message / undo from here).
    var messageId: Int? = nil
    /// A working-tree snapshot exists for the run this message started, so
    /// a rewind can revert the files.
    var hasSnapshot: Bool = false
    /// Sent to a running turn (`task/steer`); its `messageId` comes with
    /// the next `session/history`, not with `task/finished`.
    var isSteering: Bool = false
    /// Developer mode: the full prompt of the run's first model request.
    var sentPrompt: SentPrompt? = nil

    /// Editing and undo need the persisted id of a message the user typed.
    var isRewindable: Bool { messageId != nil && !isRuntime }
}

struct AssistantTurn: Identifiable, Equatable {
    var id: String = UUID().uuidString
    /// Markdown; grows while streaming, replaced by the final `model_response` text.
    var text: String = ""
    /// Reasoning / "thinking" text streamed before the answer. Empty when the model exposes none.
    var reasoning: String = ""
    var isStreaming: Bool = true
    /// True while reasoning deltas arrive and no text delta has arrived yet.
    var isThinking: Bool = false
    var step: Int = 0
    var thinkingStarted: Date?
    var thinkingDuration: TimeInterval?
    var date: Date = Date()

    var isEmpty: Bool { text.isEmpty && reasoning.isEmpty }
}

enum ToolCallStatus: Equatable {
    case running, succeeded, failed
}

enum ToolOutputFormat: String, Equatable {
    case json = "JSON"
    case stdout = "stdout"
    case diff = "diff"
    case text = "text"
}

struct ToolOutput: Equatable {
    var content: String
    var fullLength: Int
    var isError: Bool
    var format: ToolOutputFormat

    /// Characters the model never saw (truncated by the runtime). `full_len`
    /// is the runtime's byte count, so it is compared with bytes, not with
    /// `Character`s (any non-ASCII output would otherwise look truncated).
    var omittedCharacters: Int { max(0, fullLength - content.utf8.count) }

    init(content: String, fullLength: Int, isError: Bool, tool: String) {
        self.content = content
        self.fullLength = fullLength
        self.isError = isError
        self.format = ToolOutput.inferFormat(tool: tool, content: content)
    }

    static func inferFormat(tool: String, content: String) -> ToolOutputFormat {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        if tool == "git_diff" || tool == "apply_patch" { return .diff }
        if trimmed.hasPrefix("diff --git") || trimmed.hasPrefix("@@") || trimmed.hasPrefix("--- ") { return .diff }
        if (trimmed.hasPrefix("{") && trimmed.hasSuffix("}")) || (trimmed.hasPrefix("[") && trimmed.hasSuffix("]")), JSONValue.parse(trimmed) != nil { return .json }
        if tool == "execute_command" || tool == "run_validation" { return .stdout }
        return .text
    }
}

struct ToolCallItem: Identifiable, Equatable {
    /// Row identity (`ForEach`, expansion state). Unique per row: call ids
    /// such as `call_0` repeat across steps and runs.
    var id: String = UUID().uuidString
    /// The runtime's `call_id`.
    var callId: String
    var name: String
    var arguments: JSONValue
    var status: ToolCallStatus = .running
    var summary: String = ""
    var durationMs: Int?
    var output: ToolOutput?
    var startedAt: Date = Date()
    /// Approval request raised by this call, when the policy asked for one.
    var approvalRequestId: String?

    /// Best-effort primary argument for the row label ("src/app.rs", "cargo test").
    var target: String? {
        for key in ["path", "command", "query", "name", "symbol", "pattern", "kind", "directory", "glob"] {
            if let s = arguments[key]?.stringValue, !s.isEmpty { return s }
        }
        if let first = arguments.objectValue?.values.compactMap({ $0.stringValue }).first { return first }
        return nil
    }
}

enum ToolGroupTerminal: Equatable {
    case done, stopped, failed, blocked

    init(status: TaskStatus) {
        switch status {
        case .completed: self = .done
        case .cancelled: self = .stopped
        case .failed: self = .failed
        case .blocked: self = .blocked
        }
    }
}

struct ToolGroup: Identifiable, Equatable {
    var id: String = UUID().uuidString
    var calls: [ToolCallItem] = []
    var filesChanged: Set<String> = []
    var commandsRun: Int = 0
    var startedAt: Date = Date()
    var endedAt: Date?
    /// Set when the task finished; the view renders the "Done" row.
    var terminal: ToolGroupTerminal?

    var isOpen: Bool { terminal == nil }
    var runningCount: Int { calls.filter { $0.status == .running }.count }
    var failedCount: Int { calls.filter { $0.status == .failed }.count }

    /// "18 tools, edited 5 files, ran 2 commands"
    var headline: String {
        var parts: [String] = ["\(calls.count) tool\(calls.count == 1 ? "" : "s")"]
        if !filesChanged.isEmpty { parts.append("edited \(filesChanged.count) file\(filesChanged.count == 1 ? "" : "s")") }
        if commandsRun > 0 { parts.append("ran \(commandsRun) command\(commandsRun == 1 ? "" : "s")") }
        return parts.joined(separator: ", ")
    }

    var elapsed: TimeInterval { (endedAt ?? Date()).timeIntervalSince(startedAt) }
}

enum AgentRunStatus: Equatable {
    case running
    case finished(TaskStatus)
    /// The lead task ended (or the backend went away) before the agent reported.
    case stopped

    var isRunning: Bool { self == .running }
}

/// One sub-agent deployed by the Ultra effort tier: its own tool calls,
/// its latest streamed text while it runs, and its report once done.
struct AgentRun: Identifiable, Equatable {
    /// The runtime's agent id (`agent-1a2b3c4d`), unique per run.
    var id: String
    var agentTaskId: String
    var name: String
    var mode: AgentMode
    var objective: String
    var status: AgentRunStatus = .running
    var calls: [ToolCallItem] = []
    var filesChanged: Set<String> = []
    var commandsRun: Int = 0
    /// What the agent is currently saying (its streamed text, replaced each step).
    var liveText: String = ""
    var report: AgentReport?
    var startedAt: Date = Date()
    var endedAt: Date?

    var runningCalls: Int { calls.filter { $0.status == .running }.count }
    var elapsed: TimeInterval { (endedAt ?? Date()).timeIntervalSince(startedAt) }

    /// The agent's tool calls as a tool group, for the shared timeline view.
    var toolGroup: ToolGroup {
        var group = ToolGroup(id: id + ".tools", calls: calls, filesChanged: filesChanged, commandsRun: commandsRun, startedAt: startedAt, endedAt: endedAt)
        switch status {
        case .running: group.terminal = nil
        case .finished(let s): group.terminal = ToolGroupTerminal(status: s)
        case .stopped: group.terminal = .stopped
        }
        return group
    }
}

/// Sub-agents deployed together (one `spawn_agents` call): shown as one
/// card with a row per agent.
struct AgentGroup: Identifiable, Equatable {
    var id: String = UUID().uuidString
    var agents: [AgentRun] = []
    var startedAt: Date = Date()
    var endedAt: Date?

    var isOpen: Bool { agents.contains { $0.status.isRunning } }
    var runningCount: Int { agents.filter { $0.status.isRunning }.count }
    var failedCount: Int { agents.filter { if case .finished(let s) = $0.status { return s == .failed } else { return $0.status == .stopped } }.count }
    var elapsed: TimeInterval { (endedAt ?? Date()).timeIntervalSince(startedAt) }

    /// "3 agents, 2 running" / "3 agents, done" / "3 agents, 1 failed"
    var headline: String {
        var parts = ["\(agents.count) agent\(agents.count == 1 ? "" : "s")"]
        if runningCount > 0 {
            parts.append("\(runningCount) running")
        } else if failedCount > 0 {
            parts.append("\(failedCount) failed")
        } else {
            parts.append("done")
        }
        let files = agents.reduce(into: Set<String>()) { $0.formUnion($1.filesChanged) }
        if !files.isEmpty { parts.append("edited \(files.count) file\(files.count == 1 ? "" : "s")") }
        return parts.joined(separator: ", ")
    }
}

struct ApprovalItem: Identifiable, Equatable {
    var request: ApprovalRequest
    var decision: ApprovalDecision?
    var date: Date = Date()
    var id: String { request.id }
    var isPending: Bool { decision == nil }
}

struct OutcomeItem: Identifiable, Equatable {
    var id: String = UUID().uuidString
    var outcome: TaskOutcome
    var date: Date = Date()
    /// The summary repeats the task's last assistant reply (the backend often
    /// uses the final message as the summary); the card then leaves it out.
    var summaryRepeatsReply = false
}

struct NoticeItem: Identifiable, Equatable {
    var id: String = UUID().uuidString
    /// Lines shown by default.
    var lines: [String] = []
    /// Low-value lines (task boundary notes, checkpoints) behind a disclosure.
    var detailLines: [String] = []
    var isError: Bool = false
    var date: Date = Date()

    var isEmpty: Bool { lines.isEmpty && detailLines.isEmpty }
}

/// Ordered items plus the indexes the reducer needs.
struct Transcript: Equatable {
    private(set) var items: [TranscriptItem] = []

    var isEmpty: Bool { items.isEmpty }
    var count: Int { items.count }

    mutating func append(_ item: TranscriptItem) {
        items.append(item)
    }

    mutating func removeLast() {
        guard !items.isEmpty else { return }
        items.removeLast()
    }

    mutating func removeAll() {
        items.removeAll()
    }

    /// Inserts earlier items (history that arrived after a run began) in front.
    mutating func prepend(_ earlier: [TranscriptItem]) {
        guard !earlier.isEmpty else { return }
        items.insert(contentsOf: earlier, at: 0)
    }

    subscript(index: Int) -> TranscriptItem {
        get { items[index] }
        set { items[index] = newValue }
    }

    var last: TranscriptItem? { items.last }

    /// True when `summary` is (or is contained in) the last assistant reply
    /// since the most recent user turn, ignoring whitespace differences.
    func repeatsLastReply(_ summary: String) -> Bool {
        let wanted = Self.normalized(summary)
        guard !wanted.isEmpty else { return false }
        for item in items.reversed() {
            switch item {
            case .assistant(let turn):
                let reply = Self.normalized(turn.text)
                if reply.isEmpty { continue }
                return reply == wanted || reply.contains(wanted)
            case .user(let turn) where !turn.isRuntime:
                return false
            default:
                continue
            }
        }
        return false
    }

    private static func normalized(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    /// Index of the most recent assistant item, if it is the last item.
    var trailingAssistantIndex: Int? {
        guard let last = items.last, case .assistant = last else { return nil }
        return items.count - 1
    }

    /// Index of the most recent assistant item anywhere after the last user turn.
    var currentAssistantIndex: Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            switch items[i] {
            case .assistant: return i
            case .user: return nil
            default: continue
            }
        }
        return nil
    }

    /// The open tool group of the current run, if any.
    var openToolGroupIndex: Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            switch items[i] {
            case .tools(let g): return g.isOpen ? i : nil
            case .user: return nil
            default: continue
            }
        }
        return nil
    }

    /// The most recent tool group of the current run, open or already closed
    /// (a `file_changed` follows the `tool_call_finished` that closed an
    /// approval-split group).
    var currentToolGroupIndex: Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            switch items[i] {
            case .tools: return i
            case .user: return nil
            default: continue
            }
        }
        return nil
    }

    /// Where a tool call lives: (item index, call index). Call ids repeat
    /// across steps, so the most recent row wins, preferring one that is
    /// still running (a finished row's updates have already arrived).
    func locate(callId: String) -> (item: Int, call: Int)? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            if case .tools(let g) = items[i],
               let c = g.calls.lastIndex(where: { $0.callId == callId && $0.status == .running }) ?? g.calls.lastIndex(where: { $0.callId == callId }) {
                return (i, c)
            }
        }
        return nil
    }

    /// The open agent group of the current run, if any.
    var openAgentGroupIndex: Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            switch items[i] {
            case .agents(let g): return g.isOpen ? i : nil
            case .user: return nil
            default: continue
            }
        }
        return nil
    }

    /// Where a sub-agent lives: (item index, agent index).
    func locate(agentId: String) -> (item: Int, agent: Int)? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            if case .agents(let g) = items[i], let a = g.agents.lastIndex(where: { $0.id == agentId }) {
                return (i, a)
            }
        }
        return nil
    }

    func locate(approvalId: String) -> Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            if case .approval(let a) = items[i], a.id == approvalId { return i }
        }
        return nil
    }

    func locate(userId: String) -> Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            if case .user(let u) = items[i], u.id == userId { return i }
        }
        return nil
    }

    /// The last user item (newest first) matching `predicate`.
    func lastUserIndex(where predicate: (UserTurn) -> Bool) -> Int? {
        for i in stride(from: items.count - 1, through: 0, by: -1) {
            if case .user(let u) = items[i], predicate(u) { return i }
        }
        return nil
    }

    mutating func update(userAt index: Int, _ change: (inout UserTurn) -> Void) {
        guard items.indices.contains(index), case .user(var u) = items[index] else { return }
        change(&u)
        items[index] = .user(u)
    }

    /// The newest message the user typed that can be rewound to (Undo Last Turn).
    var lastRewindableUser: UserTurn? {
        guard let index = lastUserIndex(where: { $0.isRewindable }), case .user(let u) = items[index] else { return nil }
        return u
    }

    /// What a rewind to the user item at `index` removes: the later messages
    /// the user typed and the files changed from that item on (tool groups,
    /// sub-agents and outcomes), for the keep-or-revert sheet.
    func rewindScope(from index: Int) -> (laterMessages: Int, changedFiles: [String]) {
        guard items.indices.contains(index) else { return (0, []) }
        var later = 0
        var files: [String] = []
        var seen: Set<String> = []
        func add(_ paths: some Sequence<String>) {
            for path in paths where seen.insert(path).inserted { files.append(path) }
        }
        for item in items[index...] {
            switch item {
            case .user(let u):
                if u.id != items[index].id, !u.isRuntime { later += 1 }
            case .tools(let group):
                add(group.filesChanged.sorted())
            case .agents(let group):
                for agent in group.agents { add(agent.filesChanged.sorted()) }
            case .outcome(let outcome):
                add(outcome.outcome.modifiedFiles)
            default:
                break
            }
        }
        return (later, files)
    }

    mutating func update(assistantAt index: Int, _ change: (inout AssistantTurn) -> Void) {
        guard items.indices.contains(index), case .assistant(var turn) = items[index] else { return }
        change(&turn)
        items[index] = .assistant(turn)
    }

    mutating func update(toolsAt index: Int, _ change: (inout ToolGroup) -> Void) {
        guard items.indices.contains(index), case .tools(var group) = items[index] else { return }
        change(&group)
        items[index] = .tools(group)
    }

    mutating func update(agentsAt index: Int, _ change: (inout AgentGroup) -> Void) {
        guard items.indices.contains(index), case .agents(var group) = items[index] else { return }
        change(&group)
        items[index] = .agents(group)
    }

    mutating func update(approvalAt index: Int, _ change: (inout ApprovalItem) -> Void) {
        guard items.indices.contains(index), case .approval(var a) = items[index] else { return }
        change(&a)
        items[index] = .approval(a)
    }

    mutating func update(noticeAt index: Int, _ change: (inout NoticeItem) -> Void) {
        guard items.indices.contains(index), case .notice(var n) = items[index] else { return }
        change(&n)
        items[index] = .notice(n)
    }

    /// All tool calls in order (for counts in the sidebar / outcome),
    /// including the ones sub-agents made.
    var allToolCalls: [ToolCallItem] {
        items.flatMap { item -> [ToolCallItem] in
            switch item {
            case .tools(let g): return g.calls
            case .agents(let g): return g.agents.flatMap(\.calls)
            default: return []
            }
        }
    }

    /// Text of the last assistant message, for previews.
    var lastAssistantText: String? {
        for item in items.reversed() {
            if case .assistant(let a) = item, !a.text.isEmpty { return a.text }
        }
        return nil
    }

    /// Text of the last user message (for Retry).
    var lastUserText: String? { lastUserPrompt?.text }

    /// The newest message the user sent, with its attachments (Retry).
    var lastUserPrompt: OutgoingPrompt? {
        for item in items.reversed() {
            if case .user(let u) = item, !u.isRuntime { return OutgoingPrompt(text: u.text, images: u.images, files: u.files) }
        }
        return nil
    }

    /// Markdown export of the whole conversation (Copy Transcript / Export Chat).
    func markdown(title: String? = nil) -> String {
        var out = ""
        if let title { out += "# \(title)\n\n" }
        for item in items {
            switch item {
            case .user(let u):
                out += (u.isRuntime ? "> _runtime:_ " : "**You:** ") + u.text + "\n\n"
            case .assistant(let a):
                if !a.text.isEmpty { out += a.text + "\n\n" }
            case .tools(let g):
                out += "<details><summary>\(g.headline)</summary>\n\n"
                for c in g.calls {
                    let mark = c.status == .failed ? "✗" : (c.status == .running ? "…" : "✓")
                    out += "- \(mark) `\(c.name)` \(c.target ?? "") — \(c.summary)\n"
                }
                out += "\n</details>\n\n"
            case .agents(let g):
                out += "<details><summary>\(g.headline)</summary>\n\n"
                for a in g.agents {
                    let status: String
                    switch a.status {
                    case .running: status = "running"
                    case .finished(let s): status = s.rawValue
                    case .stopped: status = "stopped"
                    }
                    out += "- **\(a.name)** (\(a.mode.rawValue)) — \(status), \(a.calls.count) tool call\(a.calls.count == 1 ? "" : "s")\n"
                    if let report = a.report, !report.summary.isEmpty {
                        out += "\n  " + report.summary.replacingOccurrences(of: "\n", with: "\n  ") + "\n"
                    }
                }
                out += "\n</details>\n\n"
            case .approval(let a):
                let d = a.decision.map { $0.rawValue } ?? "pending"
                out += "_Approval \(d): \(a.request.summary)_\n\n"
            case .outcome(let o):
                out += "**\(o.outcome.status.label)** — \(o.outcome.summary)\n"
                if !o.outcome.modifiedFiles.isEmpty { out += "\nModified: " + o.outcome.modifiedFiles.joined(separator: ", ") + "\n" }
                if !o.outcome.unresolved.isEmpty { out += "\nUnresolved:\n" + o.outcome.unresolved.map { "- \($0)" }.joined(separator: "\n") + "\n" }
                out += "\n"
            case .notice(let n):
                for line in n.lines { out += "_\(line)_\n" }
                if !n.lines.isEmpty { out += "\n" }
            }
        }
        return out
    }
}

// MARK: - Formatting helpers shared by views

enum TranscriptFormat {
    /// 42_100 → "42.1k", 1_200_000 → "1.2M"
    static func tokens(_ n: Int) -> String {
        switch n {
        case ..<1000: return "\(n)"
        case ..<1_000_000: return String(format: n % 1000 < 50 ? "%.0fk" : "%.1fk", Double(n) / 1000)
        default: return String(format: "%.1fM", Double(n) / 1_000_000)
        }
    }

    /// 372 → "6m 12s", 5 → "5s", 0.8 → "<1s" ("0.0s" reads like a bug)
    static func duration(_ seconds: TimeInterval) -> String {
        if seconds < 1 { return "<1s" }
        if seconds < 60 { return "\(Int(seconds))s" }
        let m = Int(seconds) / 60
        let s = Int(seconds) % 60
        if m < 60 { return "\(m)m \(s)s" }
        return "\(m / 60)h \(m % 60)m"
    }

    /// 0 → "<1 ms", 12 → "12 ms", 2140 → "2s"
    static func durationMs(_ ms: Int) -> String {
        if ms < 1 { return "<1 ms" }
        return ms < 1000 ? "\(ms) ms" : duration(Double(ms) / 1000)
    }

    static func cost(_ usd: Double) -> String {
        usd < 0.01 ? String(format: "$%.4f", usd) : String(format: "$%.2f", usd)
    }

    /// "just now", "4m ago", "Yesterday", "Oct 12"
    static func relative(_ date: Date, now: Date = Date()) -> String {
        let delta = now.timeIntervalSince(date)
        if delta < 60 { return "just now" }
        if delta < 3600 { return "\(Int(delta / 60))m ago" }
        if delta < 86_400 { return "\(Int(delta / 3600))h ago" }
        if delta < 172_800 { return "Yesterday" }
        let f = DateFormatter()
        f.dateFormat = Calendar.current.isDate(date, equalTo: now, toGranularity: .year) ? "MMM d" : "MMM d, yyyy"
        return f.string(from: date)
    }
}
