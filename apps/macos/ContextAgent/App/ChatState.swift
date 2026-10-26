import SwiftUI
import AppKit
import Foundation

// Per-session live state: the committed transcript, the streaming assistant
// turn, the open tool group, status, queue and the Inspector log lines. The
// reducer (`apply`) is the only writer of transcript items during a run;
// `loadHistory` rebuilds everything from `session/history`.

/// A message on its way to `task/run`: the text and the image and text
/// files sent with it (`AttachmentUpload.prompt`).
struct OutgoingPrompt: Equatable {
    var text: String
    var images: [String] = []
    var files: [FileUpload] = []
}

struct RunProgress: Equatable {
    var state: ExecutionState
    var step: Int
    var startedAt: Date

    var elapsed: TimeInterval { Date().timeIntervalSince(startedAt) }
}

enum ChatStatus: Equatable {
    case idle
    case loadingHistory
    case running(RunProgress)
    case finished(TaskStatus)

    var isRunning: Bool {
        if case .running = self { return true }
        return false
    }

    var progress: RunProgress? {
        if case .running(let p) = self { return p }
        return nil
    }

    var finishedStatus: TaskStatus? {
        if case .finished(let s) = self { return s }
        return nil
    }
}

/// Why a chat wants attention (sidebar dot, dock badge).
enum Unseen: Equatable {
    case finished(TaskStatus)
    case approval
    case error
}

/// One Inspector Log line (formatted like the TUI's `State::log`).
struct LogLine: Identifiable {
    let id: Int
    let date: Date
    let text: String
    /// "info", "warn" or "error".
    let level: String
}

@MainActor
@Observable
final class ChatState: Identifiable {
    static let maxLogLines = 2000
    static let deltaFlushInterval: UInt64 = 33_000_000   // 33 ms

    let sessionId: String
    let projectId: String
    nonisolated var id: String { sessionId }

    var summary: SessionSummary
    /// Committed items.
    var transcript = Transcript()
    /// The streaming assistant turn (not yet in `transcript`).
    var live: AssistantTurn?
    /// The open tool group of the current run (not yet in `transcript`).
    var liveTools: ToolGroup?
    /// The sub-agents of the current run that are still running (not yet in
    /// `transcript`); committed once every agent in the group has reported.
    var liveAgents: AgentGroup?
    var status: ChatStatus = .idle
    var usage: TokenUsage?
    var costUsd: Double?
    var contextStats: ContextStats?
    var pendingApproval: ApprovalRequest?
    /// True while `approval/respond` is in flight.
    var approvalInFlight = false
    /// Inline failure of the last `approval/respond`.
    var approvalError: String?
    /// Questions of the run's `ask_user` tool, waiting for the user
    /// (`question/request`); answered from the card above the composer.
    var pendingQuestion: QuestionRequest?
    /// True while `question/respond` is in flight.
    var questionInFlight = false
    /// Inline failure of the last `question/respond`.
    var questionError: String?
    /// True from `task/cancel` until `task/finished` (status bar spinner).
    var cancelRequested = false
    var draft = ""
    /// Files and folders attached to `draft` (composer chips).
    var attachments: [ComposerAttachment] = []
    /// `suggest/next` after a completed run: ghost text in the empty composer,
    /// → accepts it. Cleared when the user types, sends or a run starts.
    var suggestion: String?
    var queued: [OutgoingPrompt] = []
    /// Sent to the running turn (`task/steer`) and not read by the model
    /// yet; each `steered` event moves the first into the transcript.
    var steering: [OutgoingPrompt] = []
    /// Set when the queue became non-empty (dequeue order across chats).
    var queuedSince: Date?
    var changedPaths: Set<String> = []
    var unseen: Unseen?
    var lastError: String?
    var lastErrorKind: String?
    var log: [LogLine] = []
    /// The optimistic user item appended by `send()`.
    var pendingUserItemId: String?
    /// Inline error when `session/history` could not be loaded.
    var historyError: String?
    /// The user item being edited in place (`UserMessageView` shows an
    /// editor instead of the bubble) and its text.
    var editingTurnId: String?
    var editingText = ""
    /// The sub-agent shown in the Inspector Agents tab; nil follows the
    /// latest running (else latest) agent.
    var inspectedAgentId: String?
    /// The agent `inspectedAgentId == nil` follows. Stored (refreshed when an
    /// agent starts or finishes) so that what reads it, every agent row in
    /// the transcript, does not depend on each streamed agent event.
    private(set) var followedAgentId: String?

    /// The run is waiting on the user (an approval or questions), or it
    /// ended blocked and nobody has looked yet: the sidebar's orange dot.
    var needsHelp: Bool {
        pendingApproval != nil || pendingQuestion != nil || unseen == .approval || unseen == .finished(.blocked)
    }

    /// Whether this chat is on screen in the active app (AppState sets it).
    @ObservationIgnored var isVisible: () -> Bool = { true }

    @ObservationIgnored private var pendingText = ""
    @ObservationIgnored private var pendingReasoning = ""
    /// Sub-agents' streamed text by agent id, flushed with the lead's.
    @ObservationIgnored private var pendingAgentText: [String: String] = [:]
    @ObservationIgnored private var flushTask: Task<Void, Never>?
    @ObservationIgnored private var nextLogId = 0
    @ObservationIgnored private var replaying = false
    /// The first transcript item of the history turn being replayed.
    @ObservationIgnored private var replayTurnStart = 0
    /// The task of the run in progress (set by `task_started`).
    @ObservationIgnored private(set) var currentTaskId: String?

    init(sessionId: String, projectId: String, summary: SessionSummary) {
        self.sessionId = sessionId
        self.projectId = projectId
        self.summary = summary
    }

    convenience init(summary: SessionSummary, projectId: String) {
        self.init(sessionId: summary.id, projectId: projectId, summary: summary)
    }

    var title: String { summary.title }
    var isRunning: Bool { status.isRunning }
    var hasContent: Bool { !transcript.isEmpty || live != nil || liveTools != nil || liveAgents != nil }

    /// Every sub-agent of the session in deployment order, live ones last.
    var allAgents: [AgentRun] {
        var agents: [AgentRun] = []
        for item in transcript.items {
            if case .agents(let group) = item { agents.append(contentsOf: group.agents) }
        }
        agents.append(contentsOf: liveAgents?.agents ?? [])
        return agents
    }

    /// The agent the Inspector Agents tab shows.
    var inspectedAgent: AgentRun? {
        if let inspectedAgentId, let agent = agent(inspectedAgentId) { return agent }
        return followedAgentId.flatMap(agent)
    }

    /// The id of the agent the Inspector shows (or would show when it is
    /// open), without reading any agent's progress.
    var shownAgentId: String? { inspectedAgentId ?? followedAgentId }

    private func agent(_ id: String) -> AgentRun? {
        if let agent = liveAgents?.agents.last(where: { $0.id == id }) { return agent }
        guard let location = transcript.locate(agentId: id), case .agents(let group) = transcript[location.item] else { return nil }
        return group.agents[location.agent]
    }

    private func refreshFollowedAgent() {
        var latest = liveAgents?.agents.last { $0.status.isRunning }
        if latest == nil {
            // An agent on a later turn runs in an earlier, committed card.
            for item in transcript.items.reversed() {
                if case .agents(let group) = item, let running = group.agents.last(where: { $0.status.isRunning }) {
                    latest = running
                    break
                }
            }
        }
        if latest == nil { latest = liveAgents?.agents.last }
        if latest == nil {
            for item in transcript.items.reversed() {
                if case .agents(let group) = item, let last = group.agents.last {
                    latest = last
                    break
                }
            }
        }
        if followedAgentId != latest?.id { followedAgentId = latest?.id }
    }

    // MARK: - Queue

    func enqueue(_ prompt: OutgoingPrompt) {
        if queued.isEmpty { queuedSince = Date() }
        queued.append(prompt)
    }

    @discardableResult
    func dequeueFirst() -> OutgoingPrompt? {
        guard !queued.isEmpty else { return nil }
        let first = queued.removeFirst()
        if queued.isEmpty { queuedSince = nil }
        return first
    }

    func removeQueued(at index: Int) {
        guard queued.indices.contains(index) else { return }
        queued.remove(at: index)
        if queued.isEmpty { queuedSince = nil }
    }

    // MARK: - History

    /// Rebuilds the transcript from `session/history`: user messages with
    /// their real timestamps, events replayed through the reducer, tool
    /// outputs filled from `tool_result` messages, outcome from the turn.
    func loadHistory(_ history: SessionHistory) {
        cancelFlush()
        summary = history.session
        transcript.removeAll()
        live = nil
        liveTools = nil
        liveAgents = nil
        pendingApproval = nil
        approvalError = nil
        pendingQuestion = nil
        questionError = nil
        changedPaths.removeAll()
        lastError = nil
        lastErrorKind = nil
        historyError = nil
        pendingUserItemId = nil
        contextStats = nil

        var totalPrompt = 0
        var totalCompletion = 0
        var totalCached = 0
        var totalCost: Double?

        replaying = true
        for turn in history.turns {
            replayTurnStart = transcript.count
            let taskDate = RFC3339.parse(turn.task.createdAt) ?? Date()
            let completedDate = turn.task.completedAt.flatMap(RFC3339.parse) ?? RFC3339.parse(turn.task.updatedAt) ?? taskDate

            // User messages (the objective plus runtime-injected ones) are
            // interleaved into the event replay by their created_at.
            typealias HistoryUser = (text: String, images: [String], files: [FileUpload], date: Date, runtime: Bool, id: Int?, snapshot: Bool)
            var pendingUsers: [HistoryUser] = turn.messages
                .filter { $0.kind == .user && !($0.text ?? "").isEmpty }
                .map { ($0.text ?? "", $0.images ?? [], $0.files ?? [], RFC3339.parse($0.createdAt) ?? taskDate, $0.runtime == true, $0.id, $0.snapshot == true) }
                .sorted { $0.date < $1.date }
            func appendUser(_ user: HistoryUser) {
                commitLive(date: user.date)
                commitLiveTools(date: user.date)
                transcript.append(.user(UserTurn(text: user.text, images: user.images, files: user.files, date: user.date, isRuntime: user.runtime, messageId: user.id, hasSnapshot: user.snapshot)))
            }
            func flushUsers(upTo date: Date) {
                while let next = pendingUsers.first, next.date <= date {
                    pendingUsers.removeFirst()
                    appendUser(next)
                }
            }
            if let first = pendingUsers.first {
                pendingUsers.removeFirst()
                appendUser(first)
            } else if !turn.task.objective.isEmpty {
                appendUser((turn.task.objective, [], [], taskDate, false, nil, false))
            }

            // Dates for events come from the surrounding records.
            let assistantDates = turn.messages.filter { $0.kind == .assistant }.compactMap { RFC3339.parse($0.createdAt) }
            var toolResultDates: [String: Date] = [:]
            for message in turn.messages where message.kind == .toolResult {
                if let callId = message.callId, let date = RFC3339.parse(message.createdAt) {
                    toolResultDates[callId] = date
                }
            }
            var cursor = taskDate
            var responseIndex = 0
            var sawOutcome = false
            for event in turn.events {
                switch event {
                case .taskStarted:
                    cursor = taskDate
                case .modelResponse:
                    if assistantDates.indices.contains(responseIndex) { cursor = assistantDates[responseIndex] }
                    responseIndex += 1
                case .toolCallFinished(_, let callId, _, _, _, _), .toolCallOutput(_, let callId, _, _, _, _):
                    if let date = toolResultDates[callId] { cursor = date }
                case .taskCompleted:
                    cursor = completedDate
                    sawOutcome = true
                default:
                    break
                }
                flushUsers(upTo: cursor)
                apply(event, date: cursor)
            }
            for user in pendingUsers { appendUser(user) }
            pendingUsers.removeAll()

            // Tool outputs from the persisted conversation (the live stream's
            // synthetic `tool_call_output` is not persisted).
            for message in turn.messages where message.kind == .toolResult {
                guard let callId = message.callId else { continue }
                let content = message.content ?? ""
                let output = ToolOutput(content: content, fullLength: message.fullLength ?? content.count, isError: message.isError ?? false, tool: message.toolName ?? "")
                attach(output: output, callId: callId, toolName: message.toolName)
            }

            if !sawOutcome, let outcome = turn.outcome {
                commitLive(date: completedDate)
                closeLiveTools(terminal: Self.terminal(for: outcome.status), date: completedDate)
                closeLiveAgents(date: completedDate)
                transcript.append(.outcome(OutcomeItem(outcome: outcome, date: completedDate, summaryRepeatsReply: transcript.repeatsLastReply(outcome.summary))))
            } else if !sawOutcome {
                // A turn without a completion (crashed or still running elsewhere): keep what it had.
                commitLive(date: completedDate)
                closeLiveTools(terminal: turn.task.state.isTerminal ? Self.terminal(for: turn.task.status ?? .failed) : nil, date: completedDate)
                closeLiveAgents(date: completedDate)
            }

            totalPrompt += turn.usage.promptTokens
            totalCompletion += turn.usage.completionTokens
            totalCached += turn.usage.cachedTokens
            if let cost = turn.usage.costUsd { totalCost = (totalCost ?? 0) + cost }
        }
        replaying = false
        refreshFollowedAgent()
        // A persisted approval_requested without its approval_resolved (the
        // backend was killed or quit while the card was pending) must not
        // survive the replay as a live request: nothing could answer it.
        for index in 0..<transcript.count {
            transcript.update(approvalAt: index) { if $0.decision == nil { $0.decision = .deny } }
        }
        pendingApproval = nil
        approvalInFlight = false

        if !history.turns.isEmpty {
            usage = TokenUsage(promptTokens: totalPrompt, completionTokens: totalCompletion, cachedPromptTokens: totalCached, reasoningTokens: nil, source: .reported)
            costUsd = totalCost
        } else {
            usage = nil
            costUsd = nil
        }
        status = .idle
    }

    /// History that arrived after a run began in this chat (the user pressed
    /// Return while `session/history` was in flight): the earlier turns go in
    /// front of what the run has shown so far. Replayed through a scratch
    /// reducer so the live state is untouched; the turn in progress is
    /// already on screen and is skipped.
    func prependHistory(_ history: SessionHistory) {
        let scratch = ChatState(sessionId: sessionId, projectId: projectId, summary: history.session)
        var earlier = history
        earlier.turns = history.turns.filter { turn in
            if let currentTaskId, turn.task.id == currentTaskId { return false }
            return turn.task.state.isTerminal || !status.isRunning
        }
        scratch.loadHistory(earlier)
        transcript.prepend(scratch.transcript.items)
        if usage == nil {
            usage = scratch.usage
            costUsd = scratch.costUsd
        }
        changedPaths.formUnion(scratch.changedPaths)
        historyError = nil
    }

    // MARK: - Reducer

    /// `TranscriptReducer`: applies one runtime event. `date` stamps the
    /// items it creates (history replay passes the persisted timestamps).
    func apply(_ event: AgentEvent, date: Date = Date()) {
        switch event {
        case .taskStarted(let taskId, let objective, let continued):
            cancelFlush()
            currentTaskId = taskId
            if pendingUserItemId == nil {
                if !replaying { transcript.append(.user(UserTurn(text: objective, date: date))) }
            }
            pendingUserItemId = nil
            live = nil
            liveTools = nil
            liveAgents = nil
            pendingApproval = nil
            approvalError = nil
            pendingQuestion = nil
            questionError = nil
            cancelRequested = false
            lastError = nil
            lastErrorKind = nil
            status = .running(RunProgress(state: .preparing, step: 0, startedAt: date))
            addLog("task \(continued ? "continued" : "started"): \(Self.clip(objective, 60))", date: date)

        case .stateChanged(_, _, let to):
            if var progress = status.progress {
                progress.state = to
                status = .running(progress)
            }
            addLog("state: \(to.rawValue.lowercased())", date: date)

        case .modelRequestStarted(_, let step, let estimated):
            flushPending()
            if var progress = status.progress {
                progress.step = step
                status = .running(progress)
            }
            commitLive(date: date)
            live = AssistantTurn(step: step, date: date)
            addLog("model request · step \(step) · ~\(estimated) tok", date: date)

        case .promptSent(_, let prompt):
            // Developer mode: shown above the message that started the run.
            if let index = transcript.lastUserIndex(where: { !$0.isRuntime && !$0.isSteering }) {
                transcript.update(userAt: index) { $0.sentPrompt = prompt }
            }

        case .modelReasoningDelta(_, let text):
            guard !text.isEmpty else { return }
            ensureLive(date: date)
            if live?.thinkingStarted == nil {
                live?.thinkingStarted = date
                live?.isThinking = true
            }
            pendingReasoning += text
            scheduleFlush()

        case .modelTextDelta(_, let text):
            guard !text.isEmpty else { return }
            ensureLive(date: date)
            if live?.isThinking == true || (live?.thinkingStarted != nil && live?.thinkingDuration == nil) {
                live?.isThinking = false
                if let started = live?.thinkingStarted, live?.thinkingDuration == nil {
                    live?.thinkingDuration = max(0, date.timeIntervalSince(started))
                }
            }
            pendingText += text
            scheduleFlush()

        case .modelResponse(_, let text, let toolCallCount, _):
            flushPending()
            ensureLive(date: date)
            if let text, !text.isEmpty {
                live?.text = text
            }
            // History has no deltas: the response record's date is the turn's date.
            if replaying { live?.date = date }
            live?.isStreaming = false
            if live?.isThinking == true {
                live?.isThinking = false
                if let started = live?.thinkingStarted, live?.thinkingDuration == nil {
                    live?.thinkingDuration = max(0, date.timeIntervalSince(started))
                }
            }
            commitLive(date: date)
            live = nil
            addLog("model response · \(toolCallCount) tool call\(toolCallCount == 1 ? "" : "s")", date: date)

        case .toolCallStarted(_, let callId, let name, let arguments):
            flushPending()
            commitLive(date: date)
            if liveTools == nil {
                liveTools = ToolGroup(startedAt: date)
            }
            var call = ToolCallItem(callId: callId, name: name, arguments: arguments)
            call.startedAt = date
            liveTools?.calls.append(call)
            if name == "execute_command" || name == "run_validation" {
                liveTools?.commandsRun += 1
            }
            addLog("→ \(name) \(Self.clip(arguments.compact(), 90))", date: date)

        case .toolCallFinished(_, let callId, _, let success, let summary, let durationMs):
            updateCall(callId) { call in
                call.status = success ? .succeeded : .failed
                call.summary = summary
                call.durationMs = durationMs
            }
            closeCommittedGroupIfIdle(callId: callId, date: date)
            addLog("  \(success ? "✓" : "✗") \(Self.clip(summary, 90)) (\(durationMs) ms)", date: date, level: success ? "info" : "warn")

        case .toolCallOutput(_, let callId, let name, let content, let fullLength, let isError):
            let output = ToolOutput(content: content, fullLength: fullLength, isError: isError, tool: name)
            attach(output: output, callId: callId, toolName: name)

        case .approvalRequested(_, let request):
            flushPending()
            commitLive(date: date)
            commitLiveTools(date: date)
            pendingApproval = request
            approvalError = nil
            if let (callId, agentId) = Self.callId(forApproval: request.id) {
                if let agentId {
                    updateAgent(agentId) { agent in
                        if let i = agent.calls.lastIndex(where: { $0.callId == callId && $0.status == .running }) ?? agent.calls.lastIndex(where: { $0.callId == callId }) {
                            agent.calls[i].approvalRequestId = request.id
                        }
                    }
                } else {
                    updateCall(callId) { $0.approvalRequestId = request.id }
                }
            }
            transcript.append(.approval(ApprovalItem(request: request, date: date)))
            if !replaying, !isVisible() { unseen = .approval }
            addLog("approval requested: \(Self.clip(request.summary, 90))", date: date, level: "warn")

        case .approvalResolved(_, let requestId, let decision):
            if let index = transcript.locate(approvalId: requestId) {
                transcript.update(approvalAt: index) { $0.decision = decision }
            }
            if pendingApproval?.id == requestId { pendingApproval = nil }
            if unseen == .approval { unseen = nil }
            addLog("approval \(decision.rawValue): \(requestId)", date: date)

        case .validationStarted(_, let command):
            addLog("validation started: \(Self.clip(command, 90))", date: date)

        case .validationFinished(_, let record):
            // The run_validation call already shows in the tool timeline.
            addLog("validation \(record.passed ? "PASSED" : "FAILED"): \(Self.clip(record.command, 90))", date: date, level: record.passed ? "info" : "warn")

        case .fileChanged(_, let path, let kind):
            if liveTools != nil {
                liveTools?.filesChanged.insert(path)
            } else if let index = transcript.currentToolGroupIndex {
                // The group may already be closed: file_changed follows the
                // tool_call_finished that closed an approval-split group.
                transcript.update(toolsAt: index) { $0.filesChanged.insert(path) }
            }
            changedPaths.insert(path)
            addLog("  file \(kind.rawValue): \(path)", date: date)

        case .checkpoint(_, _, let reason):
            appendNotice(line: "checkpoint: \(reason)", detail: true, date: date)
            addLog("checkpoint: \(reason)", date: date)

        case .warning(_, let message):
            let detail = message.lowercased().hasPrefix("task boundary")
            appendNotice(line: message, detail: detail, date: date)
            addLog("note: \(message)", date: date, level: "warn")

        case .usage(_, let cumulative, let cost):
            usage = cumulative
            costUsd = cost

        case .steered(_, let text, _, _):
            // A history replay has the message itself (`turn.messages`).
            guard !replaying else { return }
            flushPending()
            commitLive(date: date)
            commitLiveTools(date: date)
            let sent = steering.isEmpty ? nil : steering.removeFirst()
            transcript.append(.user(UserTurn(text: sent?.text ?? text, images: sent?.images ?? [], files: sent?.files ?? [], date: date, isSteering: true)))
            addLog("steered: \(Self.clip(text, 60))", date: date)

        case .contextCompiled(_, let stats):
            contextStats = stats
            addLog("context ~\(stats.estimatedPromptTokens) tok · \(stats.repositorySnippets) snippets · \(stats.memoryRecords) memories\(stats.compacted ? " · compacted" : "")", date: date)

        case .agentStarted(_, let agentId, let agentTaskId, let name, let mode, let objective):
            flushPending()
            commitLive(date: date)
            commitLiveTools(date: date)
            // Every turn of an agent announces it again under the same id
            // (the lead messaged it after it reported): its row runs again.
            if let index = liveAgents?.agents.lastIndex(where: { $0.id == agentId }) {
                liveAgents?.agents[index].resume(agentTaskId: agentTaskId)
            } else if let location = transcript.locate(agentId: agentId) {
                transcript.update(agentsAt: location.item) { group in
                    group.agents[location.agent].resume(agentTaskId: agentTaskId)
                    group.endedAt = nil
                }
            } else {
                var run = AgentRun(id: agentId, agentTaskId: agentTaskId, name: name, mode: mode, objective: objective)
                run.startedAt = date
                if liveAgents == nil {
                    liveAgents = AgentGroup(startedAt: date)
                }
                liveAgents?.agents.append(run)
            }
            refreshFollowedAgent()
            addLog("◈ agent \(name) (\(mode.rawValue)) started: \(Self.clip(objective, 60))", date: date)

        case .agentProgress(_, let agentId, let inner):
            applyAgentEvent(inner, agentId: agentId, date: date)

        case .agentFinished(_, let agentId, let report):
            updateAgent(agentId) { agent in
                agent.status = .finished(report.status)
                agent.report = report
                agent.endedAt = date
                agent.liveText = ""
                agent.filesChanged.formUnion(report.modifiedFiles)
                for i in agent.calls.indices where agent.calls[i].status == .running {
                    agent.calls[i].status = report.status == .completed ? .succeeded : .failed
                    if agent.calls[i].summary.isEmpty { agent.calls[i].summary = "did not finish" }
                }
            }
            pendingAgentText[agentId] = nil
            changedPaths.formUnion(report.modifiedFiles)
            addLog("◈ agent \(report.name) \(report.status.rawValue): \(report.steps) steps, \(report.toolCalls) tool calls, \(report.durationMs) ms", date: date, level: report.status == .failed ? "warn" : "info")
            if liveAgents?.agents.contains(where: { $0.id == agentId }) != true, let location = transcript.locate(agentId: agentId) {
                // A later turn of an agent in an already committed card.
                transcript.update(agentsAt: location.item) { if !$0.isOpen { $0.endedAt = date } }
            }
            commitLiveAgentsIfDone(date: date)
            refreshFollowedAgent()

        case .taskCompleted(_, let outcome):
            flushPending()
            commitLive(date: date)
            closeLiveTools(terminal: Self.terminal(for: outcome.status), date: date)
            closeOpenGroups(terminal: Self.terminal(for: outcome.status), date: date)
            closeLiveAgents(date: date)
            transcript.append(.outcome(OutcomeItem(outcome: outcome, date: date, summaryRepeatsReply: transcript.repeatsLastReply(outcome.summary))))
            status = .finished(outcome.status)
            pendingApproval = nil
            pendingUserItemId = nil
            if outcome.costUsd != nil { costUsd = outcome.costUsd }
            addLog("task \(outcome.status.rawValue): \(Self.clip(outcome.summary, 90))", date: date, level: outcome.status == .failed ? "error" : "info")

        case .unknown(let name, _):
            addLog("event \(name)", date: date)
        }
    }

    /// `task/finished`: closes everything the run left open. `terminal`
    /// overrides how an open tool group is closed (backend exit → `.stopped`).
    func finish(_ f: TaskFinished, terminal: ToolGroupTerminal? = nil, date: Date = Date()) {
        flushPending()
        cancelFlush()
        commitLive(date: date)
        // A run cancelled before it reached the runtime (index wait, probe)
        // reports no status, only error_kind "cancelled": a cancel, not a
        // failure, so no error notice, no red dot, no "Task failed".
        let cancelledEarly = f.isEarlyCancel
        let finalStatus = f.status ?? (cancelledEarly ? .cancelled : status.finishedStatus ?? .failed)
        let groupTerminal = terminal ?? (f.error != nil && !cancelledEarly ? .failed : Self.terminal(for: finalStatus))
        closeLiveTools(terminal: groupTerminal, date: date)
        closeOpenGroups(terminal: groupTerminal, date: date)
        closeLiveAgents(date: date)

        if cancelledEarly {
            addLog("task cancelled before it started", date: date)
        } else if let error = f.error, !error.isEmpty {
            transcript.append(.notice(NoticeItem(lines: [error], isError: true, date: date)))
            lastError = error
            lastErrorKind = f.errorKind
            addLog("error: \(error)", date: date, level: "error")
        }
        if let pending = pendingApproval, let index = transcript.locate(approvalId: pending.id) {
            // The runtime dropped the request with the run: the tool never ran.
            transcript.update(approvalAt: index) { if $0.decision == nil { $0.decision = .deny } }
        }
        pendingApproval = nil
        approvalInFlight = false
        pendingQuestion = nil
        questionInFlight = false
        questionError = nil
        cancelRequested = false
        pendingUserItemId = nil
        // Steering the run never read (cancelled, failed, out of steps)
        // runs next instead of being lost.
        if !steering.isEmpty {
            if queued.isEmpty { queuedSince = date }
            queued.insert(contentsOf: steering, at: 0)
            steering = []
        }
        status = .finished(finalStatus)
        if !isVisible() {
            unseen = f.error != nil && !cancelledEarly ? .error : .finished(finalStatus)
        }
        summary.lastStatus = finalStatus
        summary.activeTaskId = nil
        // The prompt's persisted id makes the run's message editable /
        // undoable (the newest user item that has none yet).
        if let messageId = f.messageId, let index = transcript.lastUserIndex(where: { $0.messageId == nil && !$0.isRuntime && !$0.isSteering }) {
            transcript.update(userAt: index) {
                $0.messageId = messageId
                $0.hasSnapshot = f.snapshot ?? false
            }
        }
    }

    // MARK: - Sub-agents

    /// A sub-agent's own event: its tool rows, outputs and streamed text.
    /// Deltas are buffered and flushed with the lead's (several agents
    /// stream at once), everything else is applied like the lead's events.
    private func applyAgentEvent(_ event: AgentEvent, agentId: String, date: Date) {
        switch event {
        case .modelRequestStarted:
            pendingAgentText[agentId] = nil
            updateAgent(agentId) { $0.liveText = "" }
        case .modelTextDelta(_, let text):
            guard !text.isEmpty else { return }
            pendingAgentText[agentId, default: ""] += text
            scheduleFlush()
        case .modelResponse(_, let text, _, _):
            if let text, !text.isEmpty {
                pendingAgentText[agentId] = nil
                updateAgent(agentId) { $0.liveText = text }
            }
        case .toolCallStarted(_, let callId, let name, let arguments):
            var call = ToolCallItem(callId: callId, name: name, arguments: arguments)
            call.startedAt = date
            updateAgent(agentId) { agent in
                agent.calls.append(call)
                if name == "execute_command" || name == "run_validation" { agent.commandsRun += 1 }
            }
            addLog("  [\(agentName(agentId))] → \(name) \(Self.clip(arguments.compact(), 70))", date: date)
        case .toolCallFinished(_, let callId, _, let success, let summary, let durationMs):
            updateAgent(agentId) { agent in
                if let i = agent.calls.lastIndex(where: { $0.callId == callId && $0.status == .running }) ?? agent.calls.lastIndex(where: { $0.callId == callId }) {
                    agent.calls[i].status = success ? .succeeded : .failed
                    agent.calls[i].summary = summary
                    agent.calls[i].durationMs = durationMs
                }
            }
            addLog("  [\(agentName(agentId))]   \(success ? "✓" : "✗") \(Self.clip(summary, 70)) (\(durationMs) ms)", date: date, level: success ? "info" : "warn")
        case .toolCallOutput(_, let callId, let name, let content, let fullLength, let isError):
            let output = ToolOutput(content: content, fullLength: fullLength, isError: isError, tool: name)
            updateAgent(agentId) { agent in
                if let i = agent.calls.lastIndex(where: { $0.callId == callId && $0.output == nil }) ?? agent.calls.lastIndex(where: { $0.callId == callId }) {
                    agent.calls[i].output = output
                }
            }
        case .fileChanged(_, let path, _):
            updateAgent(agentId) { $0.filesChanged.insert(path) }
            changedPaths.insert(path)
        case .approvalRequested(_, let request):
            // The lead's reducer owns the approval card; it routes the
            // request id (suffixed `@agent`) back to this agent's row.
            apply(.approvalRequested(taskId: currentTaskId ?? "", request: request), date: date)
        case .approvalResolved(_, let requestId, let decision):
            apply(.approvalResolved(taskId: currentTaskId ?? "", requestId: requestId, decision: decision), date: date)
        case .warning(_, let message):
            addLog("  [\(agentName(agentId))]   note: \(message)", date: date, level: "warn")
        default:
            break
        }
    }

    private func agentName(_ agentId: String) -> String {
        if let agent = liveAgents?.agents.first(where: { $0.id == agentId }) { return agent.name }
        if let location = transcript.locate(agentId: agentId), case .agents(let g) = transcript[location.item] { return g.agents[location.agent].name }
        return agentId
    }

    /// Resolves against the live group first, then committed groups.
    private func updateAgent(_ agentId: String, _ change: (inout AgentRun) -> Void) {
        if var group = liveAgents, let index = group.agents.lastIndex(where: { $0.id == agentId }) {
            change(&group.agents[index])
            liveAgents = group
            return
        }
        if let location = transcript.locate(agentId: agentId) {
            transcript.update(agentsAt: location.item) { group in
                change(&group.agents[location.agent])
            }
        }
    }

    /// The live agent group is committed once every agent has reported, so
    /// the lead's next message lands below the card.
    private func commitLiveAgentsIfDone(date: Date) {
        guard var group = liveAgents, !group.isOpen else { return }
        group.endedAt = date
        transcript.append(.agents(group))
        liveAgents = nil
    }

    /// The run ended: agents still running are marked stopped and the group
    /// is committed as it stands.
    private func closeLiveAgents(date: Date) {
        // An agent messaged for a later turn runs again in its committed card.
        for i in transcript.items.indices {
            guard case .agents(let g) = transcript[i], g.isOpen else { continue }
            transcript.update(agentsAt: i) { group in
                for a in group.agents.indices { group.agents[a].stop(at: date) }
                group.endedAt = date
            }
        }
        guard var group = liveAgents else { return }
        liveAgents = nil
        guard !group.agents.isEmpty else { return }
        for i in group.agents.indices { group.agents[i].stop(at: date) }
        group.endedAt = date
        transcript.append(.agents(group))
        refreshFollowedAgent()
    }

    // MARK: - Log

    func addLog(_ text: String, date: Date = Date(), level: String = "info") {
        guard !replaying else { return }
        nextLogId += 1
        log.append(LogLine(id: nextLogId, date: date, text: text, level: level))
        if log.count > Self.maxLogLines {
            log.removeFirst(log.count - Self.maxLogLines)
        }
    }

    func clearLog() {
        log.removeAll()
    }

    // MARK: - Delta coalescing

    private func ensureLive(date: Date) {
        if live == nil {
            live = AssistantTurn(step: status.progress?.step ?? 0, date: date)
        }
    }

    private func scheduleFlush() {
        guard flushTask == nil else { return }
        flushTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: Self.deltaFlushInterval)
            guard let self, !Task.isCancelled else { return }
            self.flushTask = nil
            self.flushPending()
        }
    }

    private func cancelFlush() {
        flushTask?.cancel()
        flushTask = nil
        pendingText = ""
        pendingReasoning = ""
        pendingAgentText.removeAll()
    }

    /// Moves the buffered deltas into `live` (synchronous; also called before any commit).
    func flushPending() {
        if !pendingAgentText.isEmpty {
            let texts = pendingAgentText
            pendingAgentText.removeAll()
            for (agentId, text) in texts {
                updateAgent(agentId) { $0.liveText += text }
            }
        }
        guard !pendingText.isEmpty || !pendingReasoning.isEmpty else { return }
        if live == nil { live = AssistantTurn(step: status.progress?.step ?? 0) }
        if !pendingReasoning.isEmpty {
            live?.reasoning += pendingReasoning
            pendingReasoning = ""
        }
        if !pendingText.isEmpty {
            live?.text += pendingText
            pendingText = ""
        }
    }

    // MARK: - Commit helpers

    /// Commits a non-empty `live` turn (preceded by the open tool group so order is preserved).
    private func commitLive(date: Date) {
        guard var turn = live else { return }
        flushPending()
        if let flushed = live { turn = flushed }
        guard !turn.isEmpty else {
            live = nil
            return
        }
        commitLiveTools(date: date)
        turn.isStreaming = false
        if turn.isThinking {
            turn.isThinking = false
            if let started = turn.thinkingStarted, turn.thinkingDuration == nil {
                turn.thinkingDuration = max(0, date.timeIntervalSince(started))
            }
        }
        transcript.append(.assistant(turn))
        live = nil
    }

    /// Moves a non-empty `liveTools` into the transcript. A group with no
    /// running call closes as `.done`; one still waiting (approval in
    /// flight) stays open and is closed when its last call finishes.
    private func commitLiveTools(date: Date) {
        guard var group = liveTools, !group.calls.isEmpty else {
            liveTools = nil
            return
        }
        if group.runningCount == 0 {
            group.terminal = .done
            group.endedAt = date
        }
        transcript.append(.tools(group))
        liveTools = nil
    }

    private func closeLiveTools(terminal: ToolGroupTerminal?, date: Date) {
        guard var group = liveTools else { return }
        liveTools = nil
        guard !group.calls.isEmpty else { return }
        for i in group.calls.indices where group.calls[i].status == .running {
            group.calls[i].status = terminal == .done ? .succeeded : .failed
            if group.calls[i].summary.isEmpty {
                group.calls[i].summary = terminal == .stopped ? "stopped" : (terminal == .done ? "" : "did not finish")
            }
        }
        group.terminal = terminal
        group.endedAt = date
        transcript.append(.tools(group))
    }

    /// Closes committed groups that are still open (approval case) at the end of a run.
    private func closeOpenGroups(terminal: ToolGroupTerminal?, date: Date) {
        while let index = transcript.openToolGroupIndex {
            transcript.update(toolsAt: index) { group in
                for i in group.calls.indices where group.calls[i].status == .running {
                    group.calls[i].status = terminal == .done ? .succeeded : .failed
                    if group.calls[i].summary.isEmpty { group.calls[i].summary = terminal == .stopped ? "stopped" : "did not finish" }
                }
                group.terminal = terminal ?? .done
                group.endedAt = date
            }
        }
    }

    /// Resolves against the most recent row for the id, preferring one that
    /// is still running (ids such as `call_0` repeat across steps).
    private func updateCall(_ callId: String, _ change: (inout ToolCallItem) -> Void) {
        if var group = liveTools,
           let index = group.calls.lastIndex(where: { $0.callId == callId && $0.status == .running }) ?? group.calls.lastIndex(where: { $0.callId == callId }) {
            change(&group.calls[index])
            liveTools = group
            return
        }
        if let location = transcript.locate(callId: callId) {
            transcript.update(toolsAt: location.item) { group in
                change(&group.calls[location.call])
            }
        }
    }

    /// A committed-but-open group (split by an approval) closes once nothing in it runs.
    private func closeCommittedGroupIfIdle(callId: String, date: Date) {
        guard liveTools?.calls.contains(where: { $0.callId == callId }) != true,
              let location = transcript.locate(callId: callId) else { return }
        transcript.update(toolsAt: location.item) { group in
            if group.terminal == nil, group.runningCount == 0 {
                group.terminal = .done
                group.endedAt = date
            }
        }
    }

    /// Outputs pair with rows positionally (call ids repeat across steps):
    /// the row for the id that has no output yet takes it. History is
    /// replayed oldest-first, the live stream newest-first.
    private func attach(output: ToolOutput, callId: String, toolName: String?) {
        var out = output
        if out.format == .text, let name = toolName, !name.isEmpty {
            out.format = ToolOutput.inferFormat(tool: name, content: out.content)
        }
        let matches: (ToolCallItem) -> Bool = { $0.callId == callId && $0.output == nil }
        if replaying {
            // Outputs belong to the turn being replayed: scanning from its
            // first item keeps a long history linear to load.
            for i in min(replayTurnStart, transcript.count)..<transcript.count {
                if case .tools(let g) = transcript[i], let c = g.calls.firstIndex(where: matches) {
                    transcript.update(toolsAt: i) { $0.calls[c].output = out }
                    return
                }
            }
            if var group = liveTools, let index = group.calls.firstIndex(where: matches) {
                group.calls[index].output = out
                liveTools = group
                return
            }
        } else {
            if var group = liveTools, let index = group.calls.lastIndex(where: matches) {
                group.calls[index].output = out
                liveTools = group
                return
            }
            for i in stride(from: transcript.count - 1, through: 0, by: -1) {
                if case .tools(let g) = transcript[i], let c = g.calls.lastIndex(where: matches) {
                    transcript.update(toolsAt: i) { $0.calls[c].output = out }
                    return
                }
            }
        }
        updateCall(callId) { $0.output = out }
    }

    /// Consecutive notices coalesce into one item.
    private func appendNotice(line: String, detail: Bool, date: Date) {
        if case .notice(let last)? = transcript.last, !last.isError {
            transcript.update(noticeAt: transcript.count - 1) { notice in
                if detail { notice.detailLines.append(line) } else { notice.lines.append(line) }
            }
            return
        }
        var notice = NoticeItem(date: date)
        if detail { notice.detailLines = [line] } else { notice.lines = [line] }
        transcript.append(.notice(notice))
    }

    // MARK: - Static helpers

    static func terminal(for status: TaskStatus) -> ToolGroupTerminal {
        switch status {
        case .completed: return .done
        case .cancelled: return .stopped
        case .failed: return .failed
        case .blocked: return .blocked
        }
    }

    /// `approval_call_1` → (`call_1`, nil); a sub-agent's
    /// `approval_call_1@agent-1a2b3c4d` → (`call_1`, `agent-1a2b3c4d`).
    static func callId(forApproval requestId: String) -> (callId: String, agentId: String?)? {
        let prefix = "approval_"
        guard requestId.hasPrefix(prefix) else { return nil }
        let rest = String(requestId.dropFirst(prefix.count))
        guard !rest.isEmpty else { return nil }
        if let at = rest.lastIndex(of: "@") {
            let callId = String(rest[..<at])
            let agentId = String(rest[rest.index(after: at)...])
            return callId.isEmpty ? nil : (callId, agentId.isEmpty ? nil : agentId)
        }
        return (rest, nil)
    }

    static func clip(_ text: String, _ limit: Int) -> String {
        let collapsed = text.split(whereSeparator: \.isNewline).joined(separator: " ")
        return collapsed.count <= limit ? collapsed : String(collapsed.prefix(limit)) + "…"
    }
}
