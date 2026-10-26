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

private struct ContextStatsCard: View {
    var chat: ChatState

    var body: some View {
        InspectorCard(title: "Context") {
            if let stats = chat.contextStats {
                usageBar(stats)
                StatRow(label: "Prompt estimate", value: "\(TranscriptFormat.tokens(stats.estimatedPromptTokens)) / \(TranscriptFormat.tokens(stats.contextWindow))")
                StatRow(label: "Stable prefix", value: TranscriptFormat.tokens(stats.stablePrefixTokens))
                StatRow(label: "Repository", value: "\(stats.repositorySnippets) snippet\(stats.repositorySnippets == 1 ? "" : "s") · \(TranscriptFormat.tokens(stats.repositoryTokens))")
                StatRow(label: "Memory", value: "\(stats.memoryRecords) record\(stats.memoryRecords == 1 ? "" : "s") · \(TranscriptFormat.tokens(stats.memoryTokens))")
                StatRow(label: "Conversation", value: "\(stats.conversationMessages) msg · \(TranscriptFormat.tokens(stats.conversationTokens))")
                StatRow(label: "Retrieval budget", value: TranscriptFormat.tokens(stats.retrievalBudgetTokens))
                if stats.compacted {
                    Chip("compacted", systemImage: "arrow.down.right.and.arrow.up.left", style: .tinted(Theme.warning))
                        .help("The conversation was compacted to fit the context window")
                }
            } else {
                Text(chat.status.isRunning ? "Waiting for the first context compilation…" : "No context statistics yet. They appear once a run compiles its prompt.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func usageBar(_ stats: ContextStats) -> some View {
        let fraction = stats.contextWindow > 0 ? min(1, Double(stats.estimatedPromptTokens) / Double(stats.contextWindow)) : 0
        let tint: Color = fraction > 0.9 ? Theme.danger : (fraction > 0.7 ? Theme.warning : Theme.accent)
        return GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.surfaceRaised)
                Capsule().fill(tint).frame(width: max(4, geometry.size.width * fraction))
            }
        }
        .frame(height: 6)
        .accessibilityLabel("Context window usage \(Int(fraction * 100)) percent")
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

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        VStack(alignment: .leading, spacing: 8) {
            Button {
                appState.openSettings()
            } label: {
                summary
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Open Settings")
            .accessibilityLabel("Backend: \(appState.connectionLabel). Open Settings")

            if let failure = appState.connection.failure {
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
        let isMock = appState.config?.mock == true
        let label = isMock ? appState.connectionLabel.replacingOccurrences(of: " · mock", with: "") : appState.connectionLabel
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
            if let model = appState.config?.model.name, !model.isEmpty {
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

    @ViewBuilder
    private var statusDot: some View {
        switch appState.connection {
        case .connecting:
            BrailleSpinner(size: 10, tint: Theme.textSecondary)
        case .ready:
            Circle().fill(Theme.success).frame(width: 7, height: 7).accessibilityHidden(true)
        case .failed:
            Circle().fill(Theme.danger).frame(width: 7, height: 7).accessibilityHidden(true)
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
            HStack(spacing: 6) {
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
