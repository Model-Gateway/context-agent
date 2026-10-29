import SwiftUI
import AppKit

/// The right pane (ARCHITECTURE.md "Inspector"): tab *Run* with the Backend
/// card, the selected chat's context stats, the project's uncommitted changes, counters and task
/// state; tab *Files* with the project's file tree;
/// tab *Terminal* with a shell
/// in the selected project's root; tab *Agents* with one tab per sub-agent of
/// the selected chat; tab *Browser* with the project's embedded web page,
/// which the agent can drive without the Inspector switching to it. The icon
/// switcher slides its selection and the tab content between them.
struct InspectorView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(spacing: 0) {
            header
            ZStack {
                switch appState.inspectorTab {
                case .run:
                    RunTab()
                        .transition(.move(edge: .leading).combined(with: .opacity))
                case .files:
                    FilesTab()
                        .transition(.opacity)
                case .terminal:
                    TerminalTab()
                        .transition(.opacity)
                case .agents:
                    AgentsTab()
                        .transition(.opacity)
                case .browser:
                    BrowserTab()
                        .transition(.move(edge: .trailing).combined(with: .opacity))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .clipped()
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.canvas)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Inspector")
    }

    private var header: some View {
        HStack(spacing: 8) {
            InspectorTabs()
            Spacer(minLength: 0)
            IconButton(Icons.close, label: "Hide inspector", size: 12) {
                appState.inspectorVisible = false
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 52)
        .hairline(.bottom)
    }
}

// MARK: - Tabs

/// Icon buttons over a raised track; the selected button's card slides to
/// the new tab, the others lift on hover.
private struct InspectorTabs: View {
    @Environment(AppState.self) private var appState
    @Namespace private var selection

    var body: some View {
        HStack(spacing: 2) {
            ForEach(InspectorTab.allCases) { tab in
                InspectorTabButton(tab: tab, selected: appState.inspectorTab == tab, busy: tab == .browser && appState.browserDriven, namespace: selection) {
                    withAnimation(.spring(response: 0.34, dampingFraction: 0.82)) {
                        appState.inspectorTab = tab
                    }
                }
            }
        }
        .padding(2)
        .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Inspector tabs")
    }
}

private struct InspectorTabButton: View {
    var tab: InspectorTab
    var selected: Bool
    /// Something is happening in the tab (the agent drives the browser):
    /// a dot marks it while another tab is selected.
    var busy: Bool = false
    var namespace: Namespace.ID
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        Button(action: action) {
            Image(systemName: tab.systemImage)
                .font(.system(size: 13, weight: selected ? .semibold : .medium))
                .foregroundStyle(selected || hovering ? Theme.textPrimary : Theme.textSecondary)
                .scaleEffect(hovering && !selected ? 1.12 : 1)
                .offset(y: hovering && !selected ? -0.5 : 0)
                .frame(width: 34, height: 24)
                .overlay(alignment: .topTrailing) {
                    if busy && !selected {
                        Circle()
                            .fill(Theme.accent)
                            .frame(width: 6, height: 6)
                            .padding(.top, 4)
                            .padding(.trailing, 6)
                            .transition(.scale.combined(with: .opacity))
                            .accessibilityHidden(true)
                    }
                }
                .background {
                    if selected {
                        shape
                            .fill(Theme.surface)
                            .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
                            .shadow(color: Color.black.opacity(0.08), radius: 2, x: 0, y: 1)
                            .matchedGeometryEffect(id: "selection", in: namespace)
                    } else if hovering {
                        shape.fill(Theme.textPrimary.opacity(0.06))
                    }
                }
                .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.15)) { hovering = inside }
        }
        .help(busy && !selected ? "\(tab.title) · the agent is using it" : tab.title)
        .accessibilityLabel(busy && !selected ? "\(tab.title), in use by the agent" : tab.title)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

// MARK: - Run tab

private struct RunTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let chat = appState.selectedChat {
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 12) {
                    BackendCard()
                    TaskStateCard(chat: chat)
                    ContextStatsCard(chat: chat)
                    if chat.contextStats != nil {
                        CompactButton(chat: chat)
                    }
                    if let project = appState.selectedProject, project.isGitRepository {
                        ChangesCard(project: project)
                    }
                    CountersCard(chat: chat)
                }
                .padding(12)
            }
        } else if let project = appState.selectedProject, project.isGitRepository {
            // A fresh worktree has no chat yet but can still be committed, pushed or merged.
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 12) {
                    BackendCard()
                    ChangesCard(project: project)
                }
                .padding(12)
            }
        } else {
            VStack(spacing: 0) {
                BackendCard()
                    .padding(12)
                EmptyPlaceholder(systemImage: Icons.info, title: "No chat selected", subtitle: "Context statistics and counters of the selected chat show up here.")
            }
        }
    }
}

/// Section card used by the Run tab.
private struct InspectorCard<Content: View>: View {
    var title: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Theme.textTertiary)
                .textCase(.uppercase)
                .accessibilityAddTraits(.isHeader)
            content()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
    }
}

/// "label ····· value" row.
private struct StatRow: View {
    var label: String
    var value: String
    var tint: Color = Theme.textPrimary

    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
            Spacer(minLength: 8)
            Text(verbatim: value)
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .foregroundStyle(tint)
                .lineLimit(1)
                .selectableText()
        }
        .accessibilityElement(children: .combine)
    }
}

private struct TaskStateCard: View {
    @Environment(AppState.self) private var appState
    var chat: ChatState

    var body: some View {
        InspectorCard(title: "Task") {
            HStack(spacing: 8) {
                statusPill
                Spacer(minLength: 0)
                if let progress = chat.status.progress {
                    TimelineView(.periodic(from: .now, by: 1)) { context in
                        Text(TranscriptFormat.duration(context.date.timeIntervalSince(progress.startedAt)))
                            .font(.system(size: 12, weight: .medium, design: .monospaced))
                            .foregroundStyle(Theme.textSecondary)
                    }
                }
            }
            if let progress = chat.status.progress {
                StatRow(label: "State", value: progress.state.label)
                StatRow(label: "Step", value: "\(progress.step)")
                if chat.cancelRequested {
                    HStack(spacing: 6) {
                        BrailleSpinner(size: 10, tint: Theme.textTertiary)
                        Text("Cancelling…")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
            }
            if let pending = chat.pendingApproval {
                StatRow(label: "Approval", value: ChatState.clip(pending.summary, 36), tint: Theme.warning)
            }
            if let error = chat.lastError {
                Text(verbatim: error)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
                    .lineLimit(4)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            }
            StatRow(label: "Session model", value: appState.modelDisplayName(chat.summary.model))
        }
    }

    @ViewBuilder
    private var statusPill: some View {
        switch chat.status {
        case .idle:
            StatusPill(running: false)
        case .loadingHistory:
            StatusPill(text: "Loading history", tint: Theme.textTertiary)
        case .running:
            StatusPill(running: true)
        case .finished(let status):
            StatusPill(status)
        }
    }
}

/// The context window as one stacked bar: what the last compiled prompt is
/// made of, the autocompact buffer reserved at the end and the free space
/// between, with a legend row per slice (tokens and share of the window)
/// and the MCP tools broken down by server.
private struct ContextStatsCard: View {
    var chat: ChatState

    @State private var hovered: String?
    @State private var serversExpanded = false

    var body: some View {
        InspectorCard(title: "Context") {
            if let stats = chat.contextStats {
                let layout = ContextLayout(stats)
                header(stats, layout: layout)
                bar(layout)
                VStack(spacing: 7) {
                    ForEach(layout.slices) { slice in
                        legendRow(slice, window: layout.window)
                    }
                    if layout.buffer > 0 {
                        legendRow(.init(id: "buffer", label: "Autocompact buffer", tokens: layout.buffer, color: ContextLayout.bufferColor), window: layout.window)
                            .help("The conversation is compacted once the prompt reaches \(TranscriptFormat.tokens(layout.window - ContextLayout.bufferTokens(stats))) tokens")
                    }
                    legendRow(.init(id: "free", label: "Free space", tokens: layout.free, color: Theme.surfaceRaised), window: layout.window)
                }
                .padding(.top, 2)
                if !layout.servers.isEmpty {
                    serverList(layout.servers)
                }
                footer(stats)
            } else {
                Text(chat.status.isRunning ? "Waiting for the first context compilation…" : "No context statistics yet. They appear once a run compiles its prompt.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    /// "25% used ······ 31k / 128k"
    private func header(_ stats: ContextStats, layout: ContextLayout) -> some View {
        let fraction = layout.window > 0 ? Double(stats.estimatedPromptTokens) / Double(layout.window) : 0
        let nearCompaction = stats.estimatedPromptTokens >= layout.window - ContextLayout.bufferTokens(stats)
        return HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(verbatim: "\(Int((fraction * 100).rounded()))% used")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(nearCompaction ? Theme.warning : Theme.textPrimary)
            Spacer(minLength: 8)
            Text(verbatim: "\(TranscriptFormat.tokens(stats.estimatedPromptTokens)) / \(TranscriptFormat.tokens(stats.contextWindow))")
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .selectableText()
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Context \(Int((fraction * 100).rounded())) percent used, \(TranscriptFormat.tokens(stats.estimatedPromptTokens)) of \(TranscriptFormat.tokens(stats.contextWindow)) tokens")
    }

    /// Used slices from the left, the buffer pinned to the right, the track between.
    private func bar(_ layout: ContextLayout) -> some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let scale = layout.window > 0 ? width / CGFloat(layout.window) : 0
            ZStack(alignment: .leading) {
                Rectangle().fill(Theme.surfaceRaised)
                HStack(spacing: 1) {
                    ForEach(layout.slices) { slice in
                        Rectangle()
                            .fill(slice.color)
                            .frame(width: max(2, CGFloat(slice.tokens) * scale - 1))
                            .opacity(hovered == nil || hovered == slice.id ? 1 : 0.3)
                            .help("\(slice.label) · \(TranscriptFormat.tokens(slice.tokens))")
                    }
                }
                if layout.buffer > 0 {
                    Rectangle()
                        .fill(ContextLayout.bufferColor)
                        .frame(width: CGFloat(layout.buffer) * scale)
                        .opacity(hovered == nil || hovered == "buffer" ? 1 : 0.3)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
            .animation(.easeOut(duration: 0.12), value: hovered)
        }
        .frame(height: 10)
        .clipShape(RoundedRectangle(cornerRadius: 3, style: .continuous))
        .accessibilityHidden(true)
    }

    private func legendRow(_ slice: ContextLayout.Slice, window: Int) -> some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 2, style: .continuous)
                .fill(slice.color)
                .overlay(
                    RoundedRectangle(cornerRadius: 2, style: .continuous)
                        .strokeBorder(Theme.borderStrong, lineWidth: slice.id == "free" ? 1 : 0)
                )
                .frame(width: 9, height: 9)
            Text(slice.label)
                .font(Theme.Font.caption)
                .foregroundStyle(slice.id == "free" || slice.id == "buffer" ? Theme.textTertiary : Theme.textSecondary)
                .lineLimit(1)
            if let detail = slice.detail {
                Text(verbatim: detail)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            Spacer(minLength: 6)
            Text(verbatim: TranscriptFormat.tokens(slice.tokens))
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
                .fixedSize()
            Text(verbatim: Self.percent(slice.tokens, of: window))
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .foregroundStyle(slice.id == "free" || slice.id == "buffer" ? Theme.textTertiary : Theme.textPrimary)
                .lineLimit(1)
                .frame(minWidth: 44, alignment: .trailing)
                .fixedSize()
        }
        .contentShape(Rectangle())
        .onHover { inside in
            if inside { hovered = slice.id } else if hovered == slice.id { hovered = nil }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(slice.label)\(slice.detail.map { ", \($0)" } ?? ""): \(TranscriptFormat.tokens(slice.tokens)) tokens, \(Self.percent(slice.tokens, of: window))")
    }

    /// "MCP tools  ›  12" disclosing one row per server.
    private func serverList(_ servers: [ToolGroupStats]) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Button {
                withAnimation(.easeOut(duration: 0.15)) { serversExpanded.toggle() }
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(Theme.textTertiary)
                        .rotationEffect(.degrees(serversExpanded ? 90 : 0))
                        .frame(width: 9)
                    Text("MCP servers")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                    Spacer(minLength: 6)
                    Text(verbatim: "\(servers.reduce(0) { $0 + $1.tools }) tools")
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(Theme.textTertiary)
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .background(Theme.surfaceRaised.opacity(serversExpanded ? 1 : 0.6), in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("MCP servers, \(servers.count)")
            .accessibilityValue(serversExpanded ? "expanded" : "collapsed")
            if serversExpanded {
                ForEach(servers, id: \.self) { group in
                    HStack(spacing: 8) {
                        Text(verbatim: group.server ?? "")
                            .foregroundStyle(Theme.textSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(verbatim: "· \(group.tools) tool\(group.tools == 1 ? "" : "s")")
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                        Spacer(minLength: 6)
                        Text(verbatim: TranscriptFormat.tokens(group.tokens))
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(Theme.textTertiary)
                    }
                    .font(Theme.Font.caption)
                    .padding(.leading, 23)
                    .padding(.trailing, 8)
                    .accessibilityElement(children: .combine)
                    .transition(.opacity)
                }
            }
        }
        .padding(.top, 2)
    }

    @ViewBuilder
    private func footer(_ stats: ContextStats) -> some View {
        HStack(spacing: 8) {
            Text(verbatim: "Retrieval budget \(TranscriptFormat.tokens(stats.retrievalBudgetTokens))")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .help("The most repository source this prompt could include")
            Spacer(minLength: 8)
            if stats.compacted {
                Chip("compacted", systemImage: "arrow.down.right.and.arrow.up.left", style: .tinted(Theme.warning))
                    .help("The conversation was compacted to fit the context window")
            }
        }
        .padding(.top, 2)
    }

    /// 19.4%, <0.1% for a sliver, 0% for nothing.
    static func percent(_ tokens: Int, of window: Int) -> String {
        guard window > 0, tokens > 0 else { return "0%" }
        let value = Double(tokens) / Double(window) * 100
        return value < 0.1 ? "<0.1%" : String(format: "%.1f%%", value)
    }
}

/// Under the Context card: compacts the chat now instead of waiting for
/// the prompt to reach the autocompact buffer.
private struct CompactButton: View {
    @Environment(AppState.self) private var appState
    var chat: ChatState

    var body: some View {
        Button {
            Task { await appState.compact(chat) }
        } label: {
            HStack(spacing: 6) {
                if chat.compacting {
                    BrailleSpinner(size: 10, tint: Theme.textTertiary)
                } else {
                    Image(systemName: "arrow.down.right.and.arrow.up.left")
                }
                Text(chat.compacting ? "Compacting…" : "Compact conversation")
            }
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .disabled(chat.compacting || !appState.canRewind(chat))
        .help("Replace the older turns with a summary so the next message starts with more free space")
    }
}

/// `ContextStats` laid out on the window: the prompt's slices in legend
/// order, then the buffer and the free space, which always add up to it.
private struct ContextLayout {
    struct Slice: Identifiable {
        var id: String
        var label: String
        var detail: String? = nil
        var tokens: Int
        var color: Color
    }

    static let bufferColor = Color.adaptive(light: "#B9BCC3", dark: "#4A4C52")

    let window: Int
    let slices: [Slice]
    let buffer: Int
    let free: Int
    let servers: [ToolGroupStats]

    /// The window past the compaction threshold; the
    /// default threshold (75 %) when ctx predates reporting it.
    static func bufferTokens(_ stats: ContextStats) -> Int {
        let threshold = stats.compactionThresholdTokens > 0 ? stats.compactionThresholdTokens : stats.contextWindow * 3 / 4
        return max(0, stats.contextWindow - threshold)
    }

    init(_ stats: ContextStats) {
        let plural = { (n: Int, word: String) in "\(n) \(word)\(n == 1 ? "" : "s")" }
        var slices: [Slice] = [
            Slice(id: "conversation", label: "Conversation", detail: plural(stats.conversationMessages, "message"), tokens: stats.conversationTokens, color: .adaptive(light: "#6E76EE", dark: "#8C92FF")),
            Slice(id: "repository", label: "Repository", detail: plural(stats.repositorySnippets, "snippet"), tokens: stats.repositoryTokens, color: .adaptive(light: "#2E95D3", dark: "#5DB6EE")),
        ]
        let servers = stats.toolGroups.filter { $0.server != nil }
        var accounted = stats.conversationTokens + stats.repositoryTokens + stats.memoryTokens
        if stats.systemPromptTokens > 0 || stats.toolTokens > 0 {
            let mcpTokens = servers.reduce(0) { $0 + $1.tokens }
            let builtin = stats.toolGroups.first { $0.server == nil }
            if mcpTokens > 0 {
                slices.append(Slice(id: "mcp", label: "MCP tools", detail: plural(servers.count, "server"), tokens: mcpTokens, color: .adaptive(light: "#E08A2E", dark: "#F2AE62")))
            }
            slices.append(Slice(id: "tools", label: "Built-in tools", detail: builtin.map { plural($0.tools, "tool") }, tokens: stats.toolTokens - mcpTokens, color: .adaptive(light: "#23A266", dark: "#5BD497")))
            slices.append(Slice(id: "memory", label: "Memory", detail: plural(stats.memoryRecords, "record"), tokens: stats.memoryTokens, color: .adaptive(light: "#1C8C86", dark: "#3DB8AE")))
            slices.append(Slice(id: "instructions", label: "Instructions", tokens: stats.instructionsTokens, color: .adaptive(light: "#9B5DE5", dark: "#B98CFF")))
            slices.append(Slice(id: "system", label: "System prompt", tokens: stats.systemPromptTokens, color: .adaptive(light: "#3F6F91", dark: "#6F9DC0")))
            accounted += stats.stablePrefixTokens + stats.instructionsTokens
        } else {
            // Events from before ctx split the stable prefix.
            slices.append(Slice(id: "memory", label: "Memory", detail: plural(stats.memoryRecords, "record"), tokens: stats.memoryTokens, color: .adaptive(light: "#1C8C86", dark: "#3DB8AE")))
            slices.append(Slice(id: "system", label: "System prompt & tools", tokens: stats.stablePrefixTokens, color: .adaptive(light: "#3F6F91", dark: "#6F9DC0")))
            accounted += stats.stablePrefixTokens
        }
        // Retrieval summary, checkpoint and runtime notes around the snippets.
        let other = stats.estimatedPromptTokens - accounted
        if other > 0 {
            slices.append(Slice(id: "other", label: "Summaries & notes", tokens: other, color: .adaptive(light: "#9A9EA6", dark: "#7A7E86")))
        }
        let window = max(stats.contextWindow, stats.estimatedPromptTokens)
        let used = stats.estimatedPromptTokens
        self.window = window
        self.slices = slices.filter { $0.tokens > 0 }
        self.buffer = max(0, min(Self.bufferTokens(stats), window - used))
        self.free = max(0, window - used - buffer)
        self.servers = servers
    }
}

/// The selected project's working tree: changed files and lines, the branch
/// against its upstream, and the git step that applies next — commit when
/// dirty, push when ahead (publish without an upstream), pull when behind,
/// merge into the parent when the project is a worktree.
private struct ChangesCard: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState

    @State private var commitSheet = false
    @State private var pushSheet = false
    @State private var confirmMerge = false

    private var status: GitStatus? { project.gitStatus }
    private var fileCount: Int { project.dirtyCount }
    private var lines: (added: Int, removed: Int) { project.lineChanges }
    private var ahead: Int { status?.ahead ?? 0 }
    private var behind: Int { status?.behind ?? 0 }
    private var hasRemote: Bool { !(status?.remotes ?? []).isEmpty }
    /// Ahead of the upstream, or a branch with a remote but no upstream yet.
    private var canPush: Bool {
        guard let status, status.branch != nil, hasRemote else { return false }
        return status.upstream == nil || ahead > 0
    }
    private var parent: ProjectState? { appState.parentProject(of: project) }
    private var parentBranch: String { parent?.branch ?? parent?.name ?? "parent" }
    /// A merge touches both checkouts and needs a clean worktree.
    private var canMerge: Bool { fileCount == 0 && !project.isRunning && !(parent?.isRunning ?? false) }
    private var busy: Bool { project.gitBusy != nil }

    var body: some View {
        InspectorCard(title: "Changes") {
            summary
            branchRow
            actions
            if let error = project.gitError {
                Text(verbatim: error)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .sheet(isPresented: $commitSheet) {
            CommitChangesSheet(project: project)
                .environment(appState)
        }
        .sheet(isPresented: $pushSheet) {
            if let status {
                PushSheet(project: project, status: status)
                    .environment(appState)
            }
        }
        .confirmationDialog("Merge “\(project.worktreeLabel)” into \(parentBranch)?", isPresented: $confirmMerge, titleVisibility: .visible) {
            Button("Merge") {
                Task { await appState.mergeWorktree(project, deleteAfter: false) }
            }
            Button("Merge and Remove Worktree") {
                Task { await appState.mergeWorktree(project, deleteAfter: true) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("The branch is merged into \(parentBranch) with a merge commit; a conflict leaves \(parentBranch) untouched. “Merge and Remove Worktree” also deletes the checkout and the branch.")
        }
    }

    /// "3 files changed   +120 −4", or a clean-tree line.
    private var summary: some View {
        HStack(spacing: 8) {
            if fileCount == 0 {
                Image(systemName: Icons.check)
                    .foregroundStyle(Theme.success)
                Text("Working tree clean")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            } else {
                Text("\(fileCount) file\(fileCount == 1 ? "" : "s") changed")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
            }
            Spacer(minLength: 8)
            if busy {
                BrailleSpinner(size: 11, tint: Theme.textSecondary)
                    .help(project.gitBusy ?? "")
                    .accessibilityLabel(project.gitBusy ?? "")
            }
            if fileCount > 0 {
                HStack(spacing: 4) {
                    Text(verbatim: "+\(lines.added)").foregroundStyle(Theme.success)
                    Text(verbatim: "−\(lines.removed)").foregroundStyle(Theme.danger)
                }
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .lineLimit(1)
                .fixedSize()
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(lines.added) lines added, \(lines.removed) removed")
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var branchRow: some View {
        HStack(spacing: 6) {
            Image(systemName: Icons.branch)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            Text(verbatim: project.branch ?? "detached HEAD")
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 8)
            if let upstream = status?.upstream {
                Chip("↑\(ahead) ↓\(behind)")
                    .fixedSize()
                    .help("\(ahead) to push, \(behind) to pull (\(upstream))")
            } else if status?.branch != nil, hasRemote {
                Chip("not published")
                    .fixedSize()
                    .help("The branch has no upstream branch yet")
            }
        }
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var actions: some View {
        let buttons = HStack(spacing: 6) { actionButtons }
        ViewThatFits(in: .horizontal) {
            buttons
            VStack(alignment: .leading, spacing: 6) { actionButtons }
        }
        .controlSize(.small)
        .disabled(busy)
    }

    private var pushButton: some View {
        Button {
            pushSheet = true
        } label: {
            Label(status?.upstream == nil ? "Publish…" : "Push \(ahead)…", systemImage: Icons.push)
        }
        .help(status?.upstream == nil ? "Push the branch and set its upstream" : "Push \(ahead) commit\(ahead == 1 ? "" : "s") to \(status?.upstream ?? "the upstream")")
    }

    @ViewBuilder
    private var actionButtons: some View {
        if fileCount > 0 {
            Button {
                commitSheet = true
            } label: {
                Label("Commit…", systemImage: Icons.commit)
            }
            .buttonStyle(.borderedProminent)
            .help("Commit the staged changes, or all of them when nothing is staged")
        }
        if canPush {
            // The primary action once everything is committed.
            if fileCount > 0 {
                pushButton.buttonStyle(.bordered)
            } else {
                pushButton.buttonStyle(.borderedProminent)
            }
        }
        if behind > 0 {
            Button {
                Task { await appState.pull(rebase: false, in: project) }
            } label: {
                Label("Pull \(behind)", systemImage: Icons.pull)
            }
            .buttonStyle(.bordered)
            .help("Pull \(behind) commit\(behind == 1 ? "" : "s") from \(status?.upstream ?? "the upstream")")
        }
        if project.isWorktree {
            Button {
                confirmMerge = true
            } label: {
                Label("Merge…", systemImage: Icons.branch)
            }
            .buttonStyle(.bordered)
            .disabled(!canMerge)
            .help(fileCount > 0 ? "Commit the worktree's changes before merging it into \(parentBranch)" : "Merge this worktree's branch into \(parentBranch)")
        }
        if fileCount > 0 {
            Button("Review") {
                appState.sidebarItem = .changes
            }
            .buttonStyle(.borderless)
            .help("Open the Changes tab")
        }
    }
}

private struct CountersCard: View {
    var chat: ChatState

    /// The last outcome when the chat finished, for authoritative counts.
    private var outcome: TaskOutcome? {
        guard case .finished = chat.status, case .outcome(let item)? = chat.transcript.last else { return nil }
        return item.outcome
    }

    private var toolCalls: Int {
        chat.transcript.allToolCalls.count + (chat.liveTools?.calls.count ?? 0)
    }

    private var modelCalls: Int {
        chat.transcript.items.reduce(0) { count, item in
            if case .assistant = item { return count + 1 }
            return count
        } + (chat.live == nil ? 0 : 1)
    }

    var body: some View {
        InspectorCard(title: outcome == nil ? "Counters · session" : "Counters · last task") {
            if let outcome {
                StatRow(label: "Steps", value: "\(outcome.steps)")
                StatRow(label: "Model calls", value: "\(outcome.modelCalls)")
                StatRow(label: "Tool calls", value: "\(outcome.toolCalls)")
                StatRow(label: "Duration", value: TranscriptFormat.durationMs(outcome.durationMs))
                StatRow(label: "Tokens", value: TranscriptFormat.tokens(outcome.usage.total))
                if let cost = outcome.costUsd {
                    StatRow(label: "Cost", value: TranscriptFormat.cost(cost))
                }
            } else {
                StatRow(label: "Steps", value: "\(chat.status.progress?.step ?? 0)")
                StatRow(label: "Model calls", value: "\(modelCalls)")
                StatRow(label: "Tool calls", value: "\(toolCalls)")
                if let usage = chat.usage {
                    StatRow(label: "Tokens", value: "\(TranscriptFormat.tokens(usage.total)) (\(TranscriptFormat.tokens(usage.promptTokens)) in · \(TranscriptFormat.tokens(usage.completionTokens)) out)")
                    if let cached = usage.cachedPromptTokens, cached > 0 {
                        StatRow(label: "Cached", value: TranscriptFormat.tokens(cached))
                    }
                }
                if let cost = chat.costUsd {
                    StatRow(label: "Cost", value: TranscriptFormat.cost(cost))
                }
            }
            if !chat.changedPaths.isEmpty {
                StatRow(label: "Files changed", value: "\(chat.changedPaths.count)")
            }
        }
    }
}

// MARK: - Backend card

/// "Backend": connection dot + label, the user-level default model, the
/// selected project's approval mode; click opens Settings. While the
/// connection is `.failed` it also shows the failure summary, Retry and
/// Choose binary….
private struct BackendCard: View {
    @Environment(AppState.self) private var appState

    /// The selected project's host connection, when it is a remote project:
    /// the card then describes that backend instead of the local one.
    private var remote: RemoteConnection? {
        appState.selectedProject.flatMap { appState.remote(of: $0) }
    }

    private var connectionLabel: String {
        if let remote { return "\(remote.host.displayName) · \(remote.label)" }
        return appState.connectionLabel
    }

    private var failure: BackendFailure? {
        if let remote { return remote.state.failure }
        return appState.connection.failure
    }

    private var config: InitializeResult? {
        if let remote { return remote.client.initializeResult }
        return appState.config
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        VStack(alignment: .leading, spacing: 8) {
            Button {
                appState.openSettings(remote == nil ? nil : .remoteHosts)
            } label: {
                summary
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Open Settings")
            .accessibilityLabel("Backend: \(connectionLabel). Open Settings")

            if let failure {
                failureSection(failure)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
    }

    private var summary: some View {
        // "· mock" moves to a chip on the model line to keep the status line short.
        let isMock = config?.mock == true
        let label = isMock ? connectionLabel.replacingOccurrences(of: " · mock", with: "") : connectionLabel
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 7) {
                statusDot
                Text(verbatim: label)
                    .font(.system(size: 12.5, weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 0)
                Image(systemName: Icons.settings)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .accessibilityHidden(true)
            }
            if let model = config?.model.name, !model.isEmpty {
                HStack(spacing: 6) {
                    Text(verbatim: appState.modelDisplayName(model))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Chip("default")
                        .help("User-level default model")
                    if isMock {
                        Chip("mock")
                            .help("Mock backend: no API key, scripted replies")
                    }
                }
            }
            if let project = appState.selectedProject {
                HStack(spacing: 6) {
                    Image(systemName: Icons.approval)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                        .accessibilityHidden(true)
                    Text("\(AppState.approvalModeLabel(project.info.permissionProfile)) · \(project.name)")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
        }
    }

    private enum Dot { case spinner, ready, failed, off }

    private var dot: Dot {
        if let remote {
            switch remote.state {
            case .connecting: return .spinner
            case .ready: return .ready
            case .failed: return .failed
            case .disconnected: return .off
            }
        }
        switch appState.connection {
        case .connecting: return .spinner
        case .ready: return .ready
        case .failed: return .failed
        }
    }

    @ViewBuilder
    private var statusDot: some View {
        switch dot {
        case .spinner:
            BrailleSpinner(size: 10, tint: Theme.textSecondary)
        case .ready:
            Circle().fill(Theme.success).frame(width: 7, height: 7).accessibilityHidden(true)
        case .failed:
            Circle().fill(Theme.danger).frame(width: 7, height: 7).accessibilityHidden(true)
        case .off:
            Circle().fill(Theme.textTertiary).frame(width: 7, height: 7).accessibilityHidden(true)
        }
    }

    private func failureSection(_ failure: BackendFailure) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(verbatim: BackendFailurePanel.summary(for: failure))
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.danger)
                .lineLimit(4)
                .fixedSize(horizontal: false, vertical: true)
                .selectableText()
            if let remote, let hint = remote.hint {
                Text(verbatim: hint)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 6) {
                if let remote {
                    Button("Reconnect") {
                        Task { await appState.reconnect(remote.host) }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                } else {
                    Button {
                        Task { await appState.restartBackend() }
                    } label: {
                        HStack(spacing: 4) {
                            if appState.backendStarting {
                                BrailleSpinner(size: 10, tint: Theme.textSecondary)
                            }
                            Text("Retry")
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                    .disabled(appState.backendStarting)
                    Button("Choose binary…") {
                        Task { await appState.chooseBackendBinary() }
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        }
    }
}

// MARK: - Previews

#Preview("Inspector · Run") {
    let state = MainWindowPreview.appState()
    state.selectedSessionId = "s3"
    state.inspectorTab = .run
    return InspectorView()
        .frame(width: Theme.Metrics.inspectorWidth, height: 720)
        .environment(state)
}

#Preview("Inspector · Files") {
    let state = MainWindowPreview.appState()
    state.selectedSessionId = "s3"
    state.inspectorTab = .files
    return InspectorView()
        .frame(width: Theme.Metrics.inspectorWidth, height: 720)
        .environment(state)
}

#Preview("Inspector · Terminal") {
    let state = MainWindowPreview.appState()
    state.selectedSessionId = "s3"
    state.inspectorTab = .terminal
    return InspectorView()
        .frame(width: Theme.Metrics.inspectorWidth, height: 720)
        .environment(state)
}

#Preview("Inspector · Browser") {
    let state = MainWindowPreview.appState()
    state.selectedSessionId = "s3"
    state.inspectorTab = .browser
    return InspectorView()
        .frame(width: Theme.Metrics.inspectorWidth, height: 720)
        .environment(state)
}

#Preview("Inspector · Agents") {
    let state = MainWindowPreview.appState()
    state.selectedSessionId = "s3"
    state.inspectorTab = .agents
    return InspectorView()
        .frame(width: Theme.Metrics.inspectorWidth, height: 720)
        .environment(state)
}
