import SwiftUI
import AppKit

/// One tool group of a run (reference-tools.png): a header "✳ 18 tools,
/// edited 5 files, ran 2 commands ⌄" with the elapsed time at the right,
/// then one row per call (24 pt rounded-square icon, verb + target label,
/// category chip) joined by a 1 px connector, and a last "Done" / "Stopped"
/// / "Failed" row once the task finished. Clicking a row toggles its
/// `ToolOutputView` card. Rows whose target is a path get a context menu.
/// A group of one call is just that call's row ("Listing .", no header and
/// no "Done"); a Stopped / Failed / Blocked ending still shows below it.
struct ToolTimelineView: View {
    var group: ToolGroup
    /// False shows the rows without the collapsible "N tools" header (the
    /// Inspector's agent view, which shows a slice of the agent's calls).
    var showsHeader = true

    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var isExpanded: Bool
    @State private var expandedCalls: Set<String> = []

    /// Left inset of the icon column; the connector sits at its centre.
    private static let columnInset: CGFloat = 6
    private static let iconSize: CGFloat = 24
    private static let rowPadding: CGFloat = 6

    /// Groups that ended within this window start expanded, so a run the user
    /// just watched keeps showing its rows and the "Done" row once it is
    /// committed; older history starts collapsed.
    private static let recentWindow: TimeInterval = 120

    init(group: ToolGroup, showsHeader: Bool = true) {
        self.group = group
        self.showsHeader = showsHeader
        _isExpanded = State(initialValue: Self.startsExpanded(group))
    }

    /// Open groups, groups with a failed call and recently finished groups start expanded.
    private static func startsExpanded(_ group: ToolGroup) -> Bool {
        if group.isOpen || group.failedCount > 0 { return true }
        guard let ended = group.endedAt else { return true }
        return Date().timeIntervalSince(ended) < recentWindow
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if isSingleCall || !showsHeader {
                rows
            } else {
                header
                if isExpanded {
                    rows
                        .padding(.top, 2)
                        .transition(.opacity)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Tool timeline, \(group.headline)")
    }

    private var isSingleCall: Bool { group.calls.count == 1 }

    /// The ending row, only when the group ended other than normally
    /// (stopped, failed, blocked); a finished group shows no "Done" row.
    private var shownTerminal: ToolGroupTerminal? {
        guard let terminal = group.terminal, terminal != .done else { return nil }
        return terminal
    }

    // MARK: - Header

    private var header: some View {
        Button {
            withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { isExpanded.toggle() }
        } label: {
            HStack(spacing: 10) {
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
        .accessibilityHint("Shows or hides the tool calls")
    }

    @ViewBuilder
    private var elapsed: some View {
        Group {
            if group.isOpen {
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(TranscriptFormat.duration(max(0, context.date.timeIntervalSince(group.startedAt))))
                }
            } else {
                // Tool-only groups can finish in milliseconds: `duration` floors at "<1s".
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
            ForEach(group.calls) { call in
                ToolRow(call: call, isExpanded: expandedCalls.contains(call.id)) {
                    toggle(call.id)
                } reveal: { path, action in
                    reveal(path: path, action: action)
                }
                .equatable()
                if expandedCalls.contains(call.id) {
                    ToolOutputView(call: call)
                        .padding(.vertical, 6)
                        .transition(.opacity)
                }
            }
            if let terminal = shownTerminal {
                TerminalRow(terminal: terminal)
            }
        }
        .background(alignment: .topLeading) {
            // A lone call with no ending row has nothing to connect.
            if !(isSingleCall && shownTerminal == nil) {
                connector
            }
        }
    }

    /// The 1 px line behind the icon column, from the first icon to the last row's icon.
    private var connector: some View {
        let iconCentre = Self.columnInset + Self.iconSize / 2
        let bottom = shownTerminal == nil ? Self.rowPadding + Self.iconSize / 2 : Self.rowPadding + Self.iconSize
        return Rectangle()
            .fill(Theme.border)
            .frame(width: 1)
            .padding(.leading, iconCentre - 0.5)
            .padding(.top, Self.rowPadding + Self.iconSize / 2)
            .padding(.bottom, bottom)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }

    private func toggle(_ callId: String) {
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) {
            if expandedCalls.contains(callId) {
                expandedCalls.remove(callId)
            } else {
                expandedCalls.insert(callId)
            }
        }
    }

    private func reveal(path: String, action: PathAction) {
        guard let project = appState.selectedProject else { return }
        appState.reveal(path: path, in: project, action: action)
    }
}

// MARK: - Call row

private struct ToolRow: View, Equatable {
    let call: ToolCallItem
    let isExpanded: Bool
    let toggle: () -> Void
    let reveal: (String, PathAction) -> Void

    @State private var hovering = false

    /// A group re-renders on every streamed event and whenever a sibling
    /// toggles; rows whose call did not change keep their body (and the
    /// line-count parse of their arguments).
    static func == (lhs: ToolRow, rhs: ToolRow) -> Bool {
        lhs.call == rhs.call && lhs.isExpanded == rhs.isExpanded
    }

    private var meta: ToolMetadata { ToolMetadata.info(for: call.name) }
    private var isRunning: Bool { call.status == .running }
    private var isFailed: Bool { call.status == .failed }
    private var waitingForApproval: Bool { isRunning && call.approvalRequestId != nil }

    var body: some View {
        Button(action: toggle) {
            HStack(alignment: .center, spacing: 12) {
                iconSlot
                VStack(alignment: .leading, spacing: 2) {
                    Text(ToolMetadata.rowLabel(for: call))
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if let detail {
                        Text(detail.text)
                            .font(Theme.Font.caption)
                            .foregroundStyle(detail.tint)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 12)
                if hovering, !isRunning, let ms = call.durationMs {
                    Text(TranscriptFormat.durationMs(ms))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .monospacedDigit()
                }
                if let changes {
                    LineChangeCounts(added: changes.added, removed: changes.removed)
                }
                Chip(meta.category.rawValue)
            }
            .padding(.horizontal, 6)
            .padding(.vertical, 6)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .fill(hovering || isExpanded ? Theme.surfaceRaised.opacity(hovering ? 1 : 0.6) : Color.clear)
        )
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .timelinePathContextMenu(path: ToolMetadata.pathArgument(for: call), perform: reveal)
        .help(call.summary.isEmpty ? ToolMetadata.rowLabel(for: call) : call.summary)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilityText)
        .accessibilityValue(isExpanded ? "Expanded" : "Collapsed")
        .accessibilityAddTraits(.isButton)
    }

    /// "+12 −3" for writes and edits; a failed call changed nothing.
    private var changes: (added: Int, removed: Int)? {
        isFailed ? nil : ToolMetadata.lineChanges(for: call)
    }

    /// Second line: "failed · <summary>" for failures, "Waiting for approval" while an approval is pending.
    private var detail: (text: String, tint: Color)? {
        if isFailed {
            let summary = ToolMetadata.abbreviate(call.summary, limit: 160)
            return (summary.isEmpty ? "failed" : "failed · \(summary)", Theme.danger)
        }
        if waitingForApproval {
            return ("Waiting for approval", Theme.warning)
        }
        return nil
    }

    private var iconSlot: some View {
        ZStack {
            // A plain `surfaceRaised` square (reference-tools.png); no border,
            // which competed with the connector line.
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(isFailed ? Theme.danger.opacity(0.12) : Theme.surfaceRaised)
            if isRunning {
                BrailleSpinner(size: 11, tint: waitingForApproval ? Theme.warning : Theme.textSecondary)
            } else {
                Image(systemName: meta.icon)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(isFailed ? Theme.danger : Theme.textSecondary)
            }
        }
        .frame(width: 24, height: 24)
        .accessibilityHidden(true)
    }

    private var accessibilityText: String {
        let status: String
        switch call.status {
        case .running: status = waitingForApproval ? "waiting for approval" : "running"
        case .succeeded: status = "succeeded"
        case .failed: status = "failed"
        }
        let lines = changes.map { ", \($0.added) lines added, \($0.removed) removed" } ?? ""
        return "\(ToolMetadata.rowLabel(for: call))\(lines), \(meta.category.rawValue), \(status)"
    }
}

// MARK: - Line change counts

/// "+12 −3" in the diff colours; a zero side is left out ("+40" for a new file).
private struct LineChangeCounts: View {
    let added: Int
    let removed: Int

    var body: some View {
        HStack(spacing: 4) {
            if added > 0 || removed == 0 {
                Text("+\(added)").foregroundStyle(Theme.success)
            }
            if removed > 0 {
                Text("−\(removed)").foregroundStyle(Theme.danger)
            }
        }
        .font(Theme.Font.caption)
        .monospacedDigit()
        .accessibilityHidden(true)
    }
}

// MARK: - Terminal row

private struct TerminalRow: View {
    let terminal: ToolGroupTerminal

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(tint)
                .frame(width: 24, height: 24)
                .accessibilityHidden(true)
            Text(label)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.textPrimary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 6)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Tools \(label.lowercased())")
    }

    private var label: String {
        switch terminal {
        case .done: return "Done"
        case .stopped: return "Stopped"
        case .failed: return "Failed"
        case .blocked: return "Blocked"
        }
    }

    private var icon: String {
        switch terminal {
        case .done: return Icons.check
        case .stopped: return Icons.stopped
        case .failed: return Icons.failed
        case .blocked: return Icons.approval
        }
    }

    private var tint: Color {
        switch terminal {
        case .done: return Theme.textSecondary
        case .stopped: return Theme.textSecondary
        case .failed: return Theme.danger
        case .blocked: return Theme.warning
        }
    }
}

// MARK: - Path context menu

private extension View {
    @ViewBuilder
    func timelinePathContextMenu(path: String?, perform: @escaping (String, PathAction) -> Void) -> some View {
        if let path {
            contextMenu {
                Button("Show in Changes") { perform(path, .showInChanges) }
                Button("Open in Editor") { perform(path, .openInEditor) }
                Button("Reveal in Finder") { perform(path, .revealInFinder) }
                Button("Open with Default Editor") { perform(path, .openWithEditor) }
                Divider()
                Button("Copy Path") { perform(path, .copyPath) }
            }
        } else {
            self
        }
    }
}

// MARK: - Previews

#Preview("Tool timeline") {
    let json = "{\n  \"taskId\": \"11\",\n  \"status\": \"completed\"\n}"
    var calls: [ToolCallItem] = []

    var command = ToolCallItem(callId: "call_1", name: "execute_command", arguments: .object(["command": .string("cargo test")]))
    command.status = .succeeded
    command.summary = "exit 0"
    command.durationMs = 2_140
    command.output = ToolOutput(content: json, fullLength: json.count, isError: false, tool: "execute_command")
    calls.append(command)

    var read = ToolCallItem(callId: "call_2", name: "read_file", arguments: .object(["path": .string("src/app.rs")]))
    read.status = .succeeded
    read.summary = "read 120 lines"
    read.durationMs = 4
    read.output = ToolOutput(content: "1: fn main() {\n2:     println!(\"hi\");\n3: }", fullLength: 40, isError: false, tool: "read_file")
    calls.append(read)

    var patch = ToolCallItem(callId: "call_3", name: "apply_patch", arguments: .object([
        "path": .string("src/app.rs"),
        "edits": .array([.object(["old_text": .string("let x = 1;"), "new_text": .string("let x = 2;")])]),
    ]))
    patch.status = .failed
    patch.summary = "old_text was not found in the current file"
    patch.durationMs = 3
    patch.output = ToolOutput(content: "apply_patch on src/app.rs failed: edit 1: old_text was not found in the current file.", fullLength: 90, isError: true, tool: "apply_patch")
    calls.append(patch)

    var write = ToolCallItem(callId: "call_5", name: "write_file", arguments: .object([
        "path": .string("docs/notes.md"),
        "content": .string("# Notes\n\nFirst line.\nSecond line.\n"),
    ]))
    write.status = .succeeded
    write.summary = "created docs/notes.md"
    write.durationMs = 2
    calls.append(write)

    var search = ToolCallItem(callId: "call_4", name: "search_code", arguments: .object(["query": .string("fn handle_event")]))
    search.status = .running
    calls.append(search)

    var finished = ToolGroup(startedAt: Date().addingTimeInterval(-372))
    finished.calls = calls.map { call in
        var done = call
        if done.status == .running { done.status = .succeeded; done.summary = "3 matches"; done.durationMs = 12 }
        return done
    }
    finished.filesChanged = ["src/app.rs", "src/lib.rs", "README.md", "Cargo.toml", "docs/cli.md"]
    finished.commandsRun = 2
    finished.terminal = .done
    finished.endedAt = Date()

    var running = ToolGroup(startedAt: Date().addingTimeInterval(-42))
    running.calls = calls
    running.commandsRun = 1

    return ScrollView {
        VStack(alignment: .leading, spacing: 32) {
            ToolTimelineView(group: finished)
            ToolTimelineView(group: running)
        }
        .padding(24)
        .frame(width: 640)
    }
    .frame(height: 760)
    .background(Theme.canvas)
    .environment(AppState())
}
