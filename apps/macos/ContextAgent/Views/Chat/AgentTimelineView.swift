import SwiftUI
import AppKit

/// Sub-agents deployed by the Ultra effort tier (one `spawn_agents` call):
/// a header "◈ 3 agents, 2 running ⌄" with the elapsed time at the right,
/// then one row per agent (24 pt icon with a spinner while it runs, the
/// agent's name, a mode chip, its status and time). Clicking a row shows the
/// agent in the Inspector Agents tab (`AgentsTab`).
struct AgentTimelineView: View {
    var group: AgentGroup

    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var isExpanded: Bool

    private static let columnInset: CGFloat = 6
    private static let iconSize: CGFloat = 24
    private static let rowPadding: CGFloat = 6
    private static let recentWindow: TimeInterval = 120

    init(group: AgentGroup) {
        self.group = group
        _isExpanded = State(initialValue: Self.startsExpanded(group))
    }

    private static func startsExpanded(_ group: AgentGroup) -> Bool {
        if group.isOpen || group.failedCount > 0 { return true }
        guard let ended = group.endedAt else { return true }
        return Date().timeIntervalSince(ended) < recentWindow
    }

    /// The agent the inspector currently shows, when it is on screen. Only
    /// ids are read: a committed group must not redraw on every event of
    /// the agents still running.
    private var inspectedAgentId: String? {
        guard appState.inspectorVisible, appState.inspectorTab == .agents, let chat = appState.selectedChat else { return nil }
        return chat.shownAgentId
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if isExpanded {
                rows
                    .padding(.top, 2)
                    .transition(.opacity)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Agents, \(group.headline)")
    }

    // MARK: - Header

    private var header: some View {
        Button {
            withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { isExpanded.toggle() }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: Icons.agents)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(group.isOpen ? Theme.accent : Theme.textPrimary)
                    .frame(width: Self.iconSize, height: Self.iconSize)
                    .accessibilityHidden(true)
                Text(group.headline)
                    .font(Theme.Font.body)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
                    .rotationEffect(.degrees(isExpanded ? 0 : -90))
                    .accessibilityHidden(true)
                Spacer(minLength: 12)
                elapsed
            }
            .padding(.horizontal, Self.columnInset)
            .padding(.vertical, Self.rowPadding)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .hoverHighlight()
        .accessibilityValue(isExpanded ? "Expanded" : "Collapsed")
        .accessibilityHint("Shows or hides the agents")
    }

    @ViewBuilder
    private var elapsed: some View {
        Group {
            if group.isOpen {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(TranscriptFormat.duration(max(0, context.date.timeIntervalSince(group.startedAt))))
                }
            } else {
                Text(TranscriptFormat.duration(max(0, group.elapsed)))
            }
        }
        .font(Theme.Font.secondary)
        .foregroundStyle(Theme.textSecondary)
        .monospacedDigit()
        .accessibilityLabel("Elapsed")
    }

    // MARK: - Rows

    private var rows: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(group.agents) { agent in
                AgentRow(agent: agent, isSelected: agent.id == inspectedAgentId) {
                    guard let chat = appState.selectedChat else { return }
                    appState.showAgent(agent.id, in: chat)
                }
            }
        }
        .background(alignment: .topLeading) {
            connector
        }
    }

    /// The 1 px line behind the icon column, like the tool timeline's.
    private var connector: some View {
        let iconCentre = Self.columnInset + Self.iconSize / 2
        return Rectangle()
            .fill(Theme.border)
            .frame(width: 1)
            .padding(.leading, iconCentre - 0.5)
            .padding(.top, Self.rowPadding + Self.iconSize / 2)
            .padding(.bottom, Self.rowPadding + Self.iconSize / 2)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

// MARK: - Agent row

private struct AgentRow: View {
    let agent: AgentRun
    let isSelected: Bool
    let open: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: open) {
            HStack(alignment: .center, spacing: 12) {
                AgentStatusIcon(agent: agent)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(agent.name)
                            .font(Theme.Font.body)
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                        Chip(agent.mode.label, systemImage: agent.modeIcon)
                    }
                    AgentDetailLine(agent: agent)
                }
                Spacer(minLength: 12)
                AgentElapsed(agent: agent)
                Image(systemName: Icons.chevronRight)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
                    .opacity(hovering || isSelected ? 1 : 0)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 6)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .fill(hovering || isSelected ? Theme.surfaceRaised.opacity(hovering ? 1 : 0.6) : Color.clear)
        )
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(agent.objective)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Agent \(agent.name), \(agent.mode.rawValue), \(agent.detailLine)")
        .accessibilityHint("Shows the agent in the inspector")
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    }
}

// MARK: - Shared agent visuals

/// The status square shown next to an agent: a spinner while it runs, else
/// its verdict icon.
struct AgentStatusIcon: View {
    let agent: AgentRun
    var size: CGFloat = 24

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size / 4, style: .continuous)
                .fill(agent.statusBackground)
            if agent.status.isRunning {
                BrailleSpinner(size: size * 0.46, tint: agent.waitingForApproval ? Theme.warning : Theme.accent)
            } else {
                Image(systemName: agent.statusIcon)
                    .font(.system(size: size / 2, weight: .medium))
                    .foregroundStyle(agent.statusTint)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// "Reading src/auth.rs · 4 tool calls": what the agent is doing, then its
/// tool count. A running agent changes activity many times a second (a file
/// read takes milliseconds between two model calls), so the activity is a
/// `SteadyText` that holds each value briefly and cross-fades, and the count
/// ticks on its own instead of restarting the fade.
struct AgentDetailLine: View {
    let agent: AgentRun

    var body: some View {
        HStack(spacing: 0) {
            // A verdict lands at once; the inspector reuses this view when
            // it switches agents, so each agent gets its own text state.
            SteadyText(agent.activityLabel, minimumDwell: agent.status.isRunning ? 0.8 : 0)
                .truncationMode(.tail)
                .id(agent.id)
            Text(" · \(agent.toolCountLabel)")
                .contentTransition(.numericText())
                .animation(.easeOut(duration: 0.2), value: agent.calls.count)
                .layoutPriority(1)
            if let edited = agent.editedLabel {
                Text(" · \(edited)")
                    .layoutPriority(1)
            }
        }
        .font(Theme.Font.caption)
        .foregroundStyle(agent.detailTint)
        .lineLimit(1)
        .animation(.easeInOut(duration: 0.2), value: agent.detailTint)
    }
}

/// Text that holds each value on screen for at least `minimumDwell` and
/// cross-fades to the next. Values that come and go within the dwell are
/// skipped; the latest one always lands.
struct SteadyText: View {
    let text: String
    var minimumDwell: TimeInterval = 0.8

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var shown: String
    @State private var shownAt = Date.distantPast
    @State private var pending: Task<Void, Never>?

    init(_ text: String, minimumDwell: TimeInterval = 0.8) {
        self.text = text
        self.minimumDwell = minimumDwell
        _shown = State(initialValue: text)
    }

    var body: some View {
        Text(shown)
            .contentTransition(.opacity)
            .onChange(of: text) { _, next in schedule(next) }
            .onDisappear { pending?.cancel() }
    }

    private func schedule(_ next: String) {
        pending?.cancel()
        let wait = minimumDwell - Date().timeIntervalSince(shownAt)
        guard wait > 0 else { return show(next) }
        pending = Task { @MainActor in
            try? await Task.sleep(for: .seconds(wait))
            guard !Task.isCancelled else { return }
            show(next)
        }
    }

    private func show(_ next: String) {
        guard next != shown else { return }
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.25)) { shown = next }
        shownAt = Date()
    }
}

/// The agent's running clock, or its final duration.
struct AgentElapsed: View {
    let agent: AgentRun

    var body: some View {
        Group {
            if agent.status.isRunning {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(TranscriptFormat.duration(max(0, context.date.timeIntervalSince(agent.startedAt))))
                }
            } else {
                Text(TranscriptFormat.duration(max(0, agent.elapsed)))
            }
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.textTertiary)
        .monospacedDigit()
    }
}

extension AgentRun {
    var modeIcon: String { mode == .explore ? Icons.agentExplore : Icons.agentImplement }

    var waitingForApproval: Bool {
        status.isRunning && calls.contains { $0.status == .running && $0.approvalRequestId != nil }
    }

    /// The running agent's current step, or its verdict.
    var activityLabel: String {
        switch status {
        case .running:
            if waitingForApproval { return "Waiting for approval" }
            if let current = calls.last(where: { $0.status == .running }) {
                return ToolMetadata.rowLabel(for: current)
            }
            return liveText.isEmpty ? "Thinking" : "Writing"
        case .finished(let status): return status.label
        case .stopped: return "Stopped"
        }
    }

    var toolCountLabel: String { "\(calls.count) tool call\(calls.count == 1 ? "" : "s")" }

    /// "edited 2 files" once a finished agent reports edits.
    var editedLabel: String? {
        guard case .finished = status, let report, !report.modifiedFiles.isEmpty else { return nil }
        return "edited \(report.modifiedFiles.count) file\(report.modifiedFiles.count == 1 ? "" : "s")"
    }

    /// The running agent's tool count and current step, or its verdict.
    var detailLine: String {
        ([activityLabel, toolCountLabel] + [editedLabel].compactMap { $0 }).joined(separator: " · ")
    }

    var detailTint: Color {
        switch status {
        case .running: return waitingForApproval ? Theme.warning : Theme.textSecondary
        case .finished(let status):
            switch status {
            case .completed: return Theme.textSecondary
            case .blocked: return Theme.warning
            case .failed: return Theme.danger
            case .cancelled: return Theme.textSecondary
            }
        case .stopped: return Theme.textSecondary
        }
    }

    var statusIcon: String {
        switch status {
        case .running: return Icons.agents
        case .finished(let status):
            switch status {
            case .completed: return Icons.check
            case .blocked: return Icons.approval
            case .failed: return Icons.failed
            case .cancelled: return Icons.stopped
            }
        case .stopped: return Icons.stopped
        }
    }

    var statusTint: Color {
        switch status {
        case .running: return Theme.accent
        case .finished(let status):
            switch status {
            case .completed: return Theme.success
            case .blocked: return Theme.warning
            case .failed: return Theme.danger
            case .cancelled: return Theme.textSecondary
            }
        case .stopped: return Theme.textSecondary
        }
    }

    var statusBackground: Color {
        switch status {
        case .running: return Theme.accentSoft
        case .finished(let status) where status == .failed: return Theme.danger.opacity(0.12)
        default: return Theme.surfaceRaised
        }
    }
}

// MARK: - Previews

#Preview("Agent timeline") {
    var explorer = AgentRun(id: "agent-1", agentTaskId: "t1", name: "auth-explorer", mode: .explore, objective: "Find where sessions are created and how the login flow validates tokens.")
    explorer.startedAt = Date().addingTimeInterval(-24)
    var read = ToolCallItem(callId: "call_0", name: "read_file", arguments: .object(["path": .string("src/auth.rs")]))
    read.status = .succeeded
    read.summary = "read 120 lines"
    read.durationMs = 4
    var search = ToolCallItem(callId: "call_1", name: "search_code", arguments: .object(["query": .string("fn login")]))
    search.status = .running
    explorer.calls = [read, search]
    explorer.liveText = "The token check lives in validate_session; looking for its callers."

    var fixer = AgentRun(id: "agent-2", agentTaskId: "t2", name: "api-implementer", mode: .implement, objective: "Add the /sessions endpoint.")
    fixer.startedAt = Date().addingTimeInterval(-62)
    fixer.endedAt = Date().addingTimeInterval(-3)
    fixer.status = .finished(.completed)
    var patch = ToolCallItem(callId: "call_0", name: "apply_patch", arguments: .object(["path": .string("src/api.rs")]))
    patch.status = .succeeded
    patch.summary = "1 edit applied"
    patch.durationMs = 6
    fixer.calls = [patch]
    fixer.filesChanged = ["src/api.rs"]
    fixer.report = AgentReport(agentId: "agent-2", name: "api-implementer", mode: .implement, status: .completed, summary: "Added `POST /sessions` in `src/api.rs`; the handler reuses `validate_session`.\n\nNot verified: no test covers the new route yet.", modifiedFiles: ["src/api.rs"], unresolved: ["no test covers the new route"], steps: 5, toolCalls: 1, usage: TokenUsage(promptTokens: 4200, completionTokens: 380, cachedPromptTokens: nil, reasoningTokens: nil, source: .reported), durationMs: 59_000)

    var running = AgentGroup(startedAt: Date().addingTimeInterval(-62))
    running.agents = [explorer, fixer]

    var done = running
    done.id = "done"
    var finishedExplorer = explorer
    finishedExplorer.status = .finished(.completed)
    finishedExplorer.endedAt = Date()
    finishedExplorer.calls[1].status = .succeeded
    finishedExplorer.report = AgentReport(agentId: "agent-1", name: "auth-explorer", mode: .explore, status: .completed, summary: "Sessions are created in `src/auth.rs:42`.", modifiedFiles: [], unresolved: [], steps: 3, toolCalls: 2, usage: .zero, durationMs: 24_000)
    done.agents = [finishedExplorer, fixer]
    done.endedAt = Date()

    return ScrollView {
        VStack(alignment: .leading, spacing: 32) {
            AgentTimelineView(group: running)
            AgentTimelineView(group: done)
        }
        .padding(24)
        .frame(width: 680)
    }
    .frame(height: 760)
    .background(Theme.canvas)
    .environment(AppState())
}
