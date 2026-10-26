import SwiftUI
import AppKit

/// The Inspector Agents tab: a horizontally scrolling strip with one tab per
/// sub-agent of the selected chat, over the selected agent's view — its
/// header, then (scrolling, opened at the bottom) its brief, tool timeline,
/// the line it is writing while it runs and its report.
/// Clicking an agent row in the transcript selects it here
/// (`AppState.showAgent`).
struct AgentsTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let chat = appState.selectedChat {
            let agents = chat.allAgents
            if agents.isEmpty {
                EmptyPlaceholder(systemImage: Icons.agents, title: "No agents", subtitle: "Sub-agents deployed by the Ultra effort tier show up here, one tab each.")
            } else {
                VStack(spacing: 0) {
                    AgentTabStrip(agents: agents, selectedId: chat.inspectedAgent?.id) { id in
                        chat.inspectedAgentId = id
                    }
                    if let agent = chat.inspectedAgent {
                        AgentPane(agent: agent)
                            .id(agent.id)
                    }
                }
                .background(Theme.canvas)
            }
        } else {
            EmptyPlaceholder(systemImage: Icons.agents, title: "No chat selected", subtitle: "Sub-agents of the selected chat show up here.")
        }
    }
}

// MARK: - Tab strip

private struct AgentTabStrip: View {
    var agents: [AgentRun]
    var selectedId: String?
    var select: (String) -> Void

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                HStack(spacing: 2) {
                    ForEach(agents) { agent in
                        AgentTabChip(agent: agent, selected: agent.id == selectedId) {
                            select(agent.id)
                        }
                        .id(agent.id)
                        .transition(.scale(scale: 0.85).combined(with: .opacity))
                    }
                }
                .padding(.horizontal, 8)
                .frame(maxHeight: .infinity)
            }
            .scrollIndicators(.never)
            .onAppear {
                if let selectedId { proxy.scrollTo(selectedId) }
            }
            .onChange(of: selectedId) { _, id in
                guard let id else { return }
                withAnimation { proxy.scrollTo(id) }
            }
        }
        .frame(height: 36)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Agent tabs")
    }
}

private struct AgentTabChip: View {
    var agent: AgentRun
    var selected: Bool
    var select: () -> Void

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        HStack(spacing: 5) {
            if agent.status.isRunning {
                BrailleSpinner(size: 10, tint: agent.waitingForApproval ? Theme.warning : Theme.accent)
            } else {
                Image(systemName: agent.statusIcon)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(agent.statusTint)
                    .accessibilityHidden(true)
            }
            Text(verbatim: agent.name)
                .font(.system(size: 12, weight: selected ? .semibold : .medium))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 140, alignment: .leading)
        }
        .foregroundStyle(selected || hovering ? Theme.textPrimary : Theme.textSecondary)
        .padding(.horizontal, 8)
        .frame(height: 24)
        .background(shape.fill(selected ? Theme.surfaceRaised : (hovering ? Theme.surfaceRaised.opacity(0.55) : Color.clear)))
        .contentShape(shape)
        .onTapGesture(perform: select)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(agent.objective)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Agent \(agent.name), \(agent.detailLine)")
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : [.isButton])
        .accessibilityAction { select() }
    }
}

// MARK: - Agent view

/// One agent: a pinned header with its status over a scroll view that opens
/// at the bottom — its brief, tool timeline, the line it is writing while it
/// runs, and its report once finished. Only the newest tool calls are loaded
/// (`TailWindow`), the others stand as empty space at their measured height;
/// scrolling up loads earlier ones and unloads the newest past the limit,
/// scrolling back down loads them again.
private struct AgentPane: View {
    let agent: AgentRun

    @State private var briefExpanded = true
    @State private var window: TailWindow
    @State private var rows = MeasuredRows()

    init(agent: AgentRun) {
        self.agent = agent
        _window = State(initialValue: TailWindow(count: agent.calls.count))
    }

    var body: some View {
        VStack(spacing: 0) {
            header
                .padding(12)
                .hairline(.bottom)
            ScrollView(.vertical) {
                timeline
                    .padding(12)
            }
            .defaultScrollAnchor(.bottom, for: .initialOffset)
            .tailWindow($window, count: agent.calls.count, rows: rows)
        }
    }

    private var timeline: some View {
        let range = window.range(count: agent.calls.count)
        let atNewest = range.upperBound == agent.calls.count
        return VStack(alignment: .leading, spacing: 12) {
            brief
            TailWindowSpacer(height: window.spacerAbove, rowsEdge: .bottom, placeholderRowHeight: 36) { index in
                ToolCallSkeleton(label: Self.skeletonWidths[index % Self.skeletonWidths.count])
            }
            if !range.isEmpty {
                ToolTimelineView(group: Self.slice(agent.toolGroup, range, atNewest: atNewest), showsHeader: false)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { rows = MeasuredRows(height: $0, count: range.count) }
            }
            TailWindowSpacer(height: window.spacerBelow(count: agent.calls.count), rowsEdge: .top, placeholderRowHeight: 36) { index in
                ToolCallSkeleton(label: Self.skeletonWidths[index % Self.skeletonWidths.count])
            }
            if atNewest {
                newest
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// Label widths of the tool-call skeleton rows standing for unloaded calls.
    private static let skeletonWidths: [CGFloat] = [0.55, 0.4, 0.7, 0.48, 0.62, 0.35]

    /// The loaded calls; the ending row only with the newest call.
    private static func slice(_ group: ToolGroup, _ range: Range<Int>, atNewest: Bool) -> ToolGroup {
        var slice = group
        slice.calls = Array(group.calls[range])
        if !atNewest { slice.terminal = nil }
        return slice
    }

    private var brief: some View {
        DisclosureRow(isExpanded: $briefExpanded) {
            Text("Brief")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
        } content: {
            Text(agent.objective)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .selectableText()
                .padding(.leading, 6)
        }
        .padding(.leading, -6)
    }

    /// What follows the newest call: the line being written, the report.
    @ViewBuilder
    private var newest: some View {
        if agent.status.isRunning, !agent.liveText.isEmpty {
            HStack(alignment: .top, spacing: 6) {
                BrailleSpinner(size: 11, tint: Theme.textTertiary)
                    .padding(.top, 3)
                Text(agent.liveText)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }

        if let report = agent.report {
            if !report.summary.isEmpty {
                MarkdownView(report.summary)
            }
            if !report.unresolved.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(report.unresolved.enumerated()), id: \.offset) { _, line in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text("•").accessibilityHidden(true)
                            Text(line)
                                .fixedSize(horizontal: false, vertical: true)
                                .selectableText()
                        }
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.warning)
                    }
                }
            }
            Text("\(report.steps) step\(report.steps == 1 ? "" : "s") · \(report.toolCalls) tool call\(report.toolCalls == 1 ? "" : "s") · \(TranscriptFormat.tokens(report.usage.total)) tokens · \(TranscriptFormat.durationMs(report.durationMs))")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .monospacedDigit()
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 10) {
            AgentStatusIcon(agent: agent, size: 28)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(agent.name)
                        .font(Theme.Font.body.weight(.semibold))
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Chip(agent.mode.label, systemImage: agent.modeIcon)
                }
                AgentDetailLine(agent: agent)
            }
            Spacer(minLength: 8)
            AgentElapsed(agent: agent)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}
