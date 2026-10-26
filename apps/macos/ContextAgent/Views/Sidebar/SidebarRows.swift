import SwiftUI
import AppKit

// The sidebar's rows (ARCHITECTURE.md "Sidebar"): 30 pt tall, 13.5 pt text,
// 16 pt symbols in `textSecondary`, an 8 pt `surfaceRaised` background on
// hover and selection. `NavRow` is a fixed item, `ProjectRow` an open
// project, `PinnedRecentRow` a pinned recent that is not open and
// `SessionRow` one chat with inline rename and delete confirmation.

// MARK: - Shared row chrome

/// The 30 pt row background used by every sidebar row.
private struct SidebarRowChrome: ViewModifier {
    var isSelected: Bool
    var isHovering: Bool

    func body(content: Content) -> some View {
        content
            .padding(.horizontal, 8)
            .frame(height: 30)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .fill(isSelected || isHovering ? Theme.surfaceRaised : Color.clear)
            )
            .contentShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
    }
}

private extension View {
    func sidebarRow(selected: Bool, hovering: Bool) -> some View {
        modifier(SidebarRowChrome(isSelected: selected, isHovering: hovering))
    }
}

/// A 16 pt sidebar symbol in `textSecondary` (or `accent` on a selected row).
private struct RowIcon: View {
    var systemImage: String
    var tint: Color = Theme.textSecondary

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: 16, weight: .medium))
            .foregroundStyle(tint)
            .frame(width: 22, height: 22)
            .accessibilityHidden(true)
    }
}

// MARK: - NavRow

/// Welcome · New chat · Projects · Changes · Project.
struct NavRow<Trailing: View>: View {
    var systemImage: String
    var title: String
    var isSelected: Bool
    var isEnabled: Bool
    var action: () -> Void
    @ViewBuilder var trailing: () -> Trailing

    @State private var hovering = false

    init(systemImage: String, title: String, isSelected: Bool = false, isEnabled: Bool = true, action: @escaping () -> Void, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.systemImage = systemImage
        self.title = title
        self.isSelected = isSelected
        self.isEnabled = isEnabled
        self.action = action
        self.trailing = trailing
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                RowIcon(systemImage: systemImage)
                Text(title)
                    .font(Theme.Font.sidebar)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                Spacer(minLength: 4)
                trailing()
            }
            .sidebarRow(selected: isSelected, hovering: hovering && isEnabled)
        }
        .buttonStyle(.plain)
        .disabled(!isEnabled)
        .opacity(isEnabled ? 1 : 0.45)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .accessibilityLabel(title)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

extension NavRow where Trailing == EmptyView {
    init(systemImage: String, title: String, isSelected: Bool = false, isEnabled: Bool = true, action: @escaping () -> Void) {
        self.init(systemImage: systemImage, title: title, isSelected: isSelected, isEnabled: isEnabled, action: action, trailing: { EmptyView() })
    }
}

// MARK: - ProjectRow

/// One open project: folder icon, name, branch chip, a collapse chevron when
/// worktrees are listed below it, a spinner while it runs unselected, an
/// unseen-chats badge, a close × on hover and a context menu. Collapsed, the
/// hidden worktrees' spinner and badge roll up into this row.
struct ProjectRow: View {
    var project: ProjectState

    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var hovering = false
    /// The × and the context menu's Close ask before closing.
    @State private var confirmClose = false

    private var isSelected: Bool { appState.selectedProjectId == project.id }

    private var isPinned: Bool {
        appState.recents.item(for: project.root)?.pinned == true
    }

    private var openRoots: Set<String> { Set(appState.projects.map(\.root)) }

    private var hasWorktrees: Bool { !appState.worktrees(of: project).isEmpty }
    private var isCollapsed: Bool { appState.isCollapsed(project) }
    private var hidden: [ProjectState] { appState.hiddenWorktrees(of: project) }
    private var showsSpinner: Bool {
        (project.isRunning && !isSelected) || hidden.contains(where: \.isRunning)
    }
    private var unseenCount: Int { project.unseenCount + hidden.reduce(0) { $0 + $1.unseenCount } }

    private func toggleCollapsed() {
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) {
            appState.setCollapsed(!isCollapsed, project)
        }
    }

    private var collapseButton: some View {
        Button(action: toggleCollapsed) {
            Image(systemName: Icons.chevronRight)
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(Theme.textTertiary)
                .rotationEffect(.degrees(isCollapsed ? 0 : 90))
                .frame(width: 16, height: 16)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(isCollapsed ? "Show worktrees" : "Hide worktrees")
        .accessibilityLabel(isCollapsed ? "Show worktrees" : "Hide worktrees")
    }

    private var nameLabel: some View {
        Text(verbatim: project.name)
            .font(Theme.Font.sidebar)
            .foregroundStyle(Theme.textPrimary)
            .lineLimit(1)
            .truncationMode(.tail)
    }

    var body: some View {
        Button {
            appState.select(project: project.id)
        } label: {
            HStack(spacing: 10) {
                RowIcon(systemImage: Icons.folder, tint: isSelected ? Theme.accent : Theme.textSecondary)
                // Name + branch chip when both fit; otherwise the name alone
                // (truncated at the end) rather than "cont…gent" next to a chip.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 10) {
                        nameLabel
                            .fixedSize()
                        if let branch = project.branch, !branch.isEmpty {
                            Chip(branch, systemImage: Icons.branch)
                                .fixedSize()
                                .help("Branch \(branch)")
                        }
                    }
                    nameLabel
                }
                .layoutPriority(1)
                if hasWorktrees {
                    collapseButton
                        .padding(.leading, -4)
                }
                Spacer(minLength: 4)
                // One trailing slot, always as wide as the close button: hover
                // swaps the spinner / badge for it without changing the layout,
                // so the name-and-branch arrangement above never flips.
                ZStack(alignment: .trailing) {
                    HStack(spacing: 10) {
                        if showsSpinner {
                            BrailleSpinner(size: 11)
                                .help("A task is running")
                        }
                        if unseenCount > 0 {
                            Chip("\(unseenCount)", style: .badge)
                                .fixedSize()
                                .help("\(unseenCount) chat\(unseenCount == 1 ? "" : "s") with news")
                                .accessibilityLabel("\(unseenCount) unseen chats")
                        }
                    }
                    .opacity(hovering ? 0 : 1)
                    IconButton(Icons.close, label: "Close project", size: 11) {
                        confirmClose = true
                    }
                    .opacity(hovering ? 1 : 0)
                    .allowsHitTesting(hovering)
                    .accessibilityHidden(!hovering)
                }
            }
            .sidebarRow(selected: isSelected, hovering: hovering)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .contextMenu {
            Button("New Chat in a Worktree") {
                Task { await appState.newWorktree(in: project) }
            }
            .disabled(!project.isGitRepository || project.creatingWorktree)
            let closedWorktrees = project.worktrees.filter { $0.managed && !openRoots.contains($0.path) }
            if !closedWorktrees.isEmpty {
                Menu("Open Worktree") {
                    ForEach(closedWorktrees) { entry in
                        Button {
                            Task { await appState.openWorktree(entry) }
                        } label: {
                            Text(verbatim: entry.branch.map { $0.hasPrefix("ctx/") ? String($0.dropFirst(4)) : $0 } ?? entry.name)
                        }
                    }
                }
            }
            if hasWorktrees {
                Button(isCollapsed ? "Show Worktrees" : "Hide Worktrees", action: toggleCollapsed)
            }
            Divider()
            Button(isPinned ? "Unpin" : "Pin") {
                if appState.recents.item(for: project.root) == nil {
                    appState.recents.touch(path: project.root, name: project.name)
                }
                appState.recents.togglePin(path: project.root)
            }
            Button("Reveal in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([project.rootURL])
            }
            Button("Copy Path") {
                appState.copyToPasteboard(project.root)
            }
            Divider()
            Button("Close…") {
                confirmClose = true
            }
        }
        .confirmationDialog("Close “\(project.name)”?", isPresented: $confirmClose, titleVisibility: .visible) {
            Button(project.isRunning ? (project.runningSessionIds.count > 1 ? "Stop Tasks and Close" : "Stop Task and Close") : "Close Project", role: .destructive) {
                Task { await appState.closeProject(project.id) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(project.isRunning
                 ? "Its running \(project.runningSessionIds.count > 1 ? "tasks" : "task") will be cancelled. Chats are kept; reopen the project any time from Projects."
                 : "Chats are kept; reopen the project any time from Projects.")
        }
        .accessibilityLabel("Project \(project.name)")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - WorktreeRow

/// A worktree of the project above it, indented: branch icon, the branch
/// name without its `ctx/` prefix, a spinner while it runs unselected, the
/// unseen badge, a remove × on hover and a context menu. Removing asks
/// whether the branch goes too.
struct WorktreeRow: View {
    var project: ProjectState

    @Environment(AppState.self) private var appState
    @State private var hovering = false
    @State private var confirmRemove = false
    @State private var confirmMerge = false

    private var isSelected: Bool { appState.selectedProjectId == project.id }
    private var parent: ProjectState? { appState.parentProject(of: project) }
    /// The branch the merge lands on; the parent's folder name until its git status is known.
    private var parentBranch: String { parent?.branch ?? parent?.name ?? "parent" }
    /// A merge touches both checkouts: neither may be running a task.
    private var canMerge: Bool { !project.isRunning && !(parent?.isRunning ?? false) }

    var body: some View {
        Button {
            appState.select(project: project.id)
        } label: {
            HStack(spacing: 10) {
                RowIcon(systemImage: Icons.worktree, tint: isSelected ? Theme.accent : Theme.textSecondary)
                Text(verbatim: project.worktreeLabel)
                    .font(Theme.Font.sidebar)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .layoutPriority(1)
                if project.dirtyCount > 0 {
                    Chip("\(project.dirtyCount)", systemImage: Icons.edit)
                        .fixedSize()
                        .help("\(project.dirtyCount) changed file\(project.dirtyCount == 1 ? "" : "s")")
                }
                Spacer(minLength: 4)
                ZStack(alignment: .trailing) {
                    HStack(spacing: 10) {
                        if project.isRunning, !isSelected {
                            BrailleSpinner(size: 11)
                                .help("A task is running")
                        }
                        if project.unseenCount > 0 {
                            Chip("\(project.unseenCount)", style: .badge)
                                .fixedSize()
                                .accessibilityLabel("\(project.unseenCount) unseen chats")
                        }
                    }
                    .opacity(hovering ? 0 : 1)
                    IconButton(Icons.close, label: "Remove worktree", size: 11) {
                        confirmRemove = true
                    }
                    .opacity(hovering ? 1 : 0)
                    .allowsHitTesting(hovering)
                    .accessibilityHidden(!hovering)
                }
            }
            .padding(.leading, 18)
            .sidebarRow(selected: isSelected, hovering: hovering)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help("Worktree on \(project.branch ?? project.name)\n\(project.root)")
        .contextMenu {
            Button("Reveal in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([project.rootURL])
            }
            Button("Copy Path") {
                appState.copyToPasteboard(project.root)
            }
            if let branch = project.branch {
                Button("Copy Branch Name") {
                    appState.copyToPasteboard(branch)
                }
            }
            Divider()
            Button("Merge into \(parentBranch)…") {
                confirmMerge = true
            }
            .disabled(!canMerge)
            Divider()
            Button("Close (Keep Worktree)") {
                Task { await appState.closeProject(project.id) }
            }
            Button("Remove Worktree…", role: .destructive) {
                confirmRemove = true
            }
            .disabled(project.isRunning)
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
            Text(project.dirtyCount > 0
                 ? "The worktree has \(project.dirtyCount) uncommitted change\(project.dirtyCount == 1 ? "" : "s"); commit them in its Changes tab first. The branch is merged into \(parentBranch) with a merge commit; a conflict leaves \(parentBranch) untouched."
                 : "The branch is merged into \(parentBranch) with a merge commit; a conflict leaves \(parentBranch) untouched. “Merge and Remove Worktree” also deletes the checkout and the branch.")
        }
        .confirmationDialog("Remove worktree “\(project.worktreeLabel)”?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove, Keep Branch") {
                Task { await appState.removeWorktree(project, deleteBranch: false) }
            }
            Button("Remove and Delete Branch", role: .destructive) {
                Task { await appState.removeWorktree(project, deleteBranch: true) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(project.dirtyCount > 0
                 ? "The checkout and its \(project.dirtyCount) uncommitted change\(project.dirtyCount == 1 ? "" : "s") are deleted. Commits on the branch are kept unless the branch is deleted too."
                 : "The checkout is deleted. Commits on the branch are kept unless the branch is deleted too.")
        }
        .accessibilityLabel("Worktree \(project.worktreeLabel)")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - Loading rows

/// A folder whose `project/open` has not returned yet (the remembered
/// projects at launch, or one just opened): its name muted with a spinner
/// where the badge sits. Not clickable; the real `ProjectRow` replaces it.
struct OpeningProjectRow: View {
    var path: String

    @Environment(AppState.self) private var appState

    private var name: String {
        appState.recents.item(for: path)?.name ?? URL(fileURLWithPath: path).lastPathComponent
    }

    var body: some View {
        HStack(spacing: 10) {
            RowIcon(systemImage: Icons.folder, tint: Theme.textTertiary)
            Text(verbatim: name)
                .font(Theme.Font.sidebar)
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 4)
            BrailleSpinner(size: 11, tint: Theme.textTertiary)
        }
        .sidebarRow(selected: false, hovering: false)
        .help("Opening \(path)")
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Opening project \(name)")
    }
}

/// Placeholder chat rows while the chat list loads: bars of varied width in
/// the 30 pt row, pulsing together.
struct SessionRowsSkeleton: View {
    var count = 5

    private static let widths: [CGFloat] = [0.78, 0.6, 0.86, 0.5, 0.7, 0.64]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(0..<count, id: \.self) { index in
                GeometryReader { geometry in
                    SkeletonBar(width: geometry.size.width * Self.widths[index % Self.widths.count], height: 9)
                        .frame(maxHeight: .infinity)
                }
                .sidebarRow(selected: false, hovering: false)
            }
        }
        .skeletonPulse("Loading chats")
    }
}

// MARK: - PinnedRecentRow

/// A pinned recent project that is not open: muted; click opens it.
struct PinnedRecentRow: View {
    var recent: RecentProject

    @Environment(AppState.self) private var appState
    @State private var hovering = false

    var body: some View {
        Button {
            Task { await appState.openProject(url: recent.url) }
        } label: {
            HStack(spacing: 10) {
                RowIcon(systemImage: Icons.folder, tint: Theme.textTertiary)
                Text(verbatim: recent.name)
                    .font(Theme.Font.sidebar)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 4)
                Image(systemName: Icons.pinned)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .accessibilityHidden(true)
            }
            .sidebarRow(selected: false, hovering: hovering)
        }
        .buttonStyle(.plain)
        .disabled(!appState.connection.isReady)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(recent.abbreviatedPath)
        .contextMenu {
            Button("Open") {
                Task { await appState.openProject(url: recent.url) }
            }
            Button("Unpin") {
                appState.recents.setPinned(false, path: recent.path)
            }
            Button("Reveal in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([recent.url])
            }
            Divider()
            Button("Remove from Recents") {
                appState.recents.remove(path: recent.path)
            }
        }
        .accessibilityLabel("Pinned project \(recent.name), not open")
    }
}

// MARK: - SessionRow

/// One chat of the selected project: truncated title, relative time on hover,
/// spinner while running, an orange dot while it needs the user (an approval,
/// the agent's questions, or it ended blocked), a blue dot when it finished
/// and has not been opened yet, a red one after an error, inline rename (Return commits, Esc
/// cancels, double-click starts it) and a destructive delete confirmation.
struct SessionRow: View {
    var session: SessionSummary
    var project: ProjectState

    @Environment(AppState.self) private var appState
    @State private var hovering = false
    @State private var renaming = false
    @State private var draftTitle = ""
    @State private var confirmDelete = false
    @FocusState private var renameFocused: Bool

    private var isSelected: Bool { appState.selectedSessionId == session.id }
    private var isRunning: Bool { project.isRunning(session: session.id) }
    private var unseen: Unseen? { project.existingChat(for: session.id)?.unseen }
    private var needsHelp: Bool { project.existingChat(for: session.id)?.needsHelp ?? false }

    var body: some View {
        Group {
            if renaming {
                // No Button while editing, so clicks reach the text field.
                renameField
                    .sidebarRow(selected: isSelected, hovering: hovering)
            } else {
                Button {
                    Task { await appState.select(session: session.id) }
                } label: {
                    rowContent
                        .sidebarRow(selected: isSelected, hovering: hovering)
                }
                .buttonStyle(.plain)
                .simultaneousGesture(
                    TapGesture(count: 2).onEnded {
                        startRename()
                    }
                )
            }
        }
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .contextMenu {
            Button("Rename") { startRename() }
            Button("Copy Title") { appState.copyToPasteboard(session.title) }
            Divider()
            Button("Delete…", role: .destructive) { confirmDelete = true }
                .disabled(isRunning)
                .help(isRunning ? "Finish or cancel the running task first" : "Delete this chat")
        }
        .confirmationDialog("Delete “\(session.title)”?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task { await appState.deleteSession(session.id) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Its tasks and history are removed.")
        }
        .help(isRunning ? "Running · \(session.title)" : session.title)
        .accessibilityLabel(accessibilityText)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private var rowContent: some View {
        HStack(spacing: 8) {
            Text(verbatim: session.title)
                .font(Theme.Font.sidebar)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 4)
            // A run waiting on the user is still running; the dot says why it stopped.
            if needsHelp {
                dot(Theme.warning)
                    .help(helpLabel)
            } else if isRunning {
                BrailleSpinner(size: 11)
                    .help("Running")
            } else if let unseen {
                dot(dotColor(unseen))
                    .help(dotLabel(unseen))
            }
            if hovering, !isRunning {
                Text(TranscriptFormat.relative(session.updatedDate))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .fixedSize()
                    .transition(.opacity)
            }
        }
    }

    private var renameField: some View {
        HStack(spacing: 8) {
            TextField("Chat title", text: $draftTitle)
                .textFieldStyle(.plain)
                .font(Theme.Font.sidebar)
                .foregroundStyle(Theme.textPrimary)
                .focused($renameFocused)
                .onSubmit(commitRename)
                .onExitCommand(perform: cancelRename)
                .onChange(of: renameFocused) { _, focused in
                    if !focused, renaming { commitRename() }
                }
                .accessibilityLabel("Rename chat")
            Text("↩")
                .kbd()
                .accessibilityHidden(true)
        }
    }

    private var accessibilityText: String {
        var parts = [session.title]
        if isRunning { parts.append("running") }
        if needsHelp {
            parts.append(helpLabel)
        } else if let unseen {
            parts.append(dotLabel(unseen))
        }
        return parts.joined(separator: ", ")
    }

    private func dot(_ color: Color) -> some View {
        Circle()
            .fill(color)
            .frame(width: 7, height: 7)
            .frame(width: 11, height: 11)
    }

    /// Why the orange dot is up.
    private var helpLabel: String {
        let chat = project.existingChat(for: session.id)
        if chat?.pendingQuestion != nil { return "needs your answer" }
        if chat?.pendingApproval != nil || unseen == .approval { return "approval waiting" }
        return "blocked, needs your help"
    }

    private func dotColor(_ unseen: Unseen) -> Color {
        switch unseen {
        case .finished(.failed): return Theme.danger
        case .finished(.blocked), .approval: return Theme.warning
        case .finished: return Theme.accent
        case .error: return Theme.danger
        }
    }

    private func dotLabel(_ unseen: Unseen) -> String {
        switch unseen {
        case .finished(.completed): return "finished, not opened yet"
        case .finished(let status): return "task \(status.rawValue), not opened yet"
        case .approval: return "approval waiting"
        case .error: return "error, not opened yet"
        }
    }

    // MARK: Rename

    private func startRename() {
        draftTitle = session.title == "New chat" ? "" : session.title
        renaming = true
        Task { @MainActor in
            // The field is attached a beat after `renaming` flips; focus then sticks.
            try? await Task.sleep(nanoseconds: 40_000_000)
            renameFocused = true
        }
    }

    private func commitRename() {
        guard renaming else { return }
        renaming = false
        let title = draftTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard title != session.title else { return }
        Task { await appState.renameSession(session.id, title: title) }
    }

    private func cancelRename() {
        renaming = false
        draftTitle = ""
    }
}

// MARK: - Section header

/// "Projects" / "Chats": 12 pt `textTertiary`.
struct SidebarSectionHeader<Trailing: View>: View {
    var title: String
    @ViewBuilder var trailing: () -> Trailing

    init(_ title: String, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.trailing = trailing
    }

    var body: some View {
        HStack(spacing: 6) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            Spacer(minLength: 4)
            trailing()
        }
        .padding(.horizontal, 8)
        .padding(.top, 14)
        .padding(.bottom, 4)
        .accessibilityAddTraits(.isHeader)
    }
}

extension SidebarSectionHeader where Trailing == EmptyView {
    init(_ title: String) {
        self.init(title, trailing: { EmptyView() })
    }
}

// MARK: - Preview

#Preview("Sidebar rows") {
    let state = MainWindowPreview.appState()
    let project = state.projects[0]
    return VStack(alignment: .leading, spacing: 2) {
        NavRow(systemImage: Icons.welcome, title: "Welcome", isSelected: true) {}
        NavRow(systemImage: Icons.changes, title: "Changes", action: {}) { Chip("3") }
        NavRow(systemImage: Icons.project, title: "Project", isEnabled: false) {}
        SidebarSectionHeader("Projects")
        ForEach(state.projects) { ProjectRow(project: $0) }
        PinnedRecentRow(recent: RecentProject(path: "/Users/me/Projects/content-engine", name: "content-engine", lastOpened: Date(), pinned: true))
        OpeningProjectRow(path: "/Users/me/Projects/api-gateway")
        SidebarSectionHeader("Chats")
        ForEach(project.sessions) { SessionRow(session: $0, project: project) }
        SessionRowsSkeleton(count: 3)
    }
    .padding(10)
    .frame(width: Theme.Metrics.sidebarWidth)
    .background(Theme.sidebar)
    .environment(state)
}
