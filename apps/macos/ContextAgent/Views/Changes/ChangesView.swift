import SwiftUI
import AppKit

/// The Changes tab: a 240 pt list of changed files (status letter, staged
/// toggle, refresh) and the diff of the selected file. Reads
/// `appState.selectedProject`; selection goes through `appState.selectChange`.
/// AppState refreshes the status on `file_changed` / `task/finished`.
struct ChangesView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        Group {
            if let project = appState.selectedProject {
                ChangesContent(project: project)
            } else {
                EmptyPlaceholder(
                    systemImage: Icons.changes,
                    title: "No project open",
                    subtitle: "Open a repository to review its working tree changes.",
                    actionTitle: "Open a project…"
                ) {
                    Task { await appState.chooseProject() }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
    }
}

// MARK: - Content

private struct ChangesContent: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    var body: some View {
        Group {
            if !project.isGitRepository {
                EmptyPlaceholder(
                    systemImage: Icons.changes,
                    title: "Not a git repository",
                    subtitle: "Open a folder with a .git directory to review changes here."
                )
            } else if let status = project.gitStatus {
                // A clean tree still shows the branch, sync and commit controls.
                split(status)
            } else if let error = project.gitError {
                EmptyPlaceholder(
                    systemImage: Icons.warning,
                    title: "Could not read git status",
                    subtitle: error,
                    actionTitle: "Retry"
                ) {
                    Task { await appState.refreshGit(project: project) }
                }
            } else {
                VStack(spacing: 10) {
                    BrailleSpinner(size: 16)
                    Text("Reading git status…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: project.id) { await initialLoad() }
    }

    /// 300 pt list when the pane has room; narrower (down to 220 pt) when it
    /// doesn't, e.g. the 960 pt window with the sidebar and inspector open
    /// leaves ~390 pt, so the list and diff never overflow the pane.
    private static let listWidth: CGFloat = 300
    private static let listMinimum: CGFloat = 220

    @State private var paneWidth: CGFloat = 0

    private var listWidth: CGFloat {
        guard paneWidth > 0 else { return Self.listWidth }
        return min(Self.listWidth, max(Self.listMinimum, (paneWidth * 0.45).rounded()))
    }

    private func split(_ status: GitStatus) -> some View {
        HStack(spacing: 0) {
            ChangeListPane(project: project, status: status)
                .frame(width: listWidth)
                .frame(maxHeight: .infinity)
                .background(Theme.canvas)
                .hairline(.trailing)
            ChangeDiffPane(project: project, file: status.files.first { $0.path == project.selectedChangePath })
                .frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
                .background(Theme.surface)
                .clipped()
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { paneWidth = $0 }
    }

    private func initialLoad() async {
        if project.gitStatus == nil, project.gitError == nil {
            await appState.refreshGit(project: project)
        } else if let path = project.selectedChangePath, project.diff == nil, project.diffError == nil, !project.diffLoading {
            await appState.selectChange(path: path, in: project)
        }
    }
}

// MARK: - File list

/// Which section a row sits in; a file with both staged and working-tree
/// changes (`MM`) appears in both, with the diff following the section.
private enum ChangeSection: Hashable {
    case staged, changes
}

/// A path presented in a sheet (`.sheet(item:)`).
private struct PathItem: Identifiable {
    let id: String
}

/// What a discard confirmation covers.
private enum DiscardTarget: Identifiable {
    case paths([String])
    case all(Int)

    var id: String {
        switch self {
        case .paths(let p): return p.joined(separator: "\u{0}")
        case .all: return "*"
        }
    }
}

/// The list pane: header, branch / sync row, the commit box, then the
/// Staged Changes and Changes sections (hover + / − / discard per row,
/// Stage all / Unstage all / Discard all per section) and the recent
/// commits. Sheets: push, new branch, commit one file.
private struct ChangeListPane: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState
    let status: GitStatus

    @FocusState private var listFocused: Bool
    @State private var pushSheet = false
    @State private var newBranchSheet = false
    @State private var commitFile: PathItem?
    @State private var discardTarget: DiscardTarget?
    @State private var confirmUndoCommit = false
    @State private var hoveringBranch = false

    private var staged: [GitFileStatus] { status.stagedFiles }
    private var unstaged: [GitFileStatus] { status.unstagedFiles }
    /// Keyboard order: staged rows, then working-tree rows.
    private var rows: [(file: GitFileStatus, section: ChangeSection)] {
        staged.map { ($0, .staged) } + unstaged.map { ($0, .changes) }
    }

    var body: some View {
        @Bindable var project = project
        VStack(spacing: 0) {
            header
            branchRow
            CommitBox(project: project, status: status, onUndo: { confirmUndoCommit = true })
            if let error = project.gitError {
                errorBanner(error)
            }
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 2) {
                        if status.files.isEmpty {
                            cleanRow
                        }
                        if !staged.isEmpty {
                            SectionHeader(title: "Staged Changes", count: staged.count) {
                                IconButton(Icons.unstage, label: "Unstage all changes", size: 11) {
                                    Task { await appState.unstage([], in: project) }
                                }
                                .help("Unstage all")
                            }
                            ForEach(staged) { file in
                                row(file, section: .staged)
                            }
                        }
                        if !unstaged.isEmpty {
                            SectionHeader(title: "Changes", count: unstaged.count) {
                                IconButton(Icons.discard, label: "Discard all changes", size: 11) {
                                    discardTarget = .all(unstaged.count)
                                }
                                .help("Discard all changes…")
                                IconButton(Icons.stage, label: "Stage all changes", size: 11) {
                                    Task { await appState.stage([], in: project) }
                                }
                                .help("Stage all")
                            }
                            ForEach(unstaged) { file in
                                row(file, section: .changes)
                            }
                        }
                        if !project.commitLog.isEmpty {
                            SectionHeader(title: "Recent Commits", count: nil) {
                                IconButton(Icons.push, label: "Push…", size: 11) { pushSheet = true }
                                    .help("Push…")
                            }
                            ForEach(Array(project.commitLog.prefix(8).enumerated()), id: \.element.id) { entry in
                                CommitRow(entry: entry.element, isHead: entry.offset == 0, onUndo: { confirmUndoCommit = true })
                            }
                        }
                    }
                    .padding(8)
                }
                .focusable()
                .focused($listFocused)
                .focusEffectDisabled()
                .onMoveCommand { direction in
                    move(direction, proxy: proxy)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Changed files")
        .task(id: project.id) {
            await appState.loadCommitLog(project)
            await appState.loadBranches(project)
        }
        .sheet(isPresented: $pushSheet) {
            PushSheet(project: project, status: status)
                .environment(appState)
        }
        .sheet(isPresented: $newBranchSheet) {
            NewBranchSheet(project: project)
                .environment(appState)
        }
        .sheet(item: $commitFile) { item in
            CommitFileSheet(project: project, path: item.id)
                .environment(appState)
        }
        .confirmationDialog(discardTitle, isPresented: Binding(get: { discardTarget != nil }, set: { if !$0 { discardTarget = nil } }), titleVisibility: .visible, presenting: discardTarget) { target in
            Button("Discard Changes", role: .destructive) {
                switch target {
                case .paths(let paths): Task { await appState.discard(paths, in: project) }
                case .all: Task { await appState.discard([], in: project) }
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Modified files go back to their last staged or committed contents; untracked files are deleted. This cannot be undone.")
        }
        .confirmationDialog("Undo the last commit?", isPresented: $confirmUndoCommit, titleVisibility: .visible) {
            Button("Undo Last Commit") {
                Task { await appState.undoLastCommit(in: project) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("“\(ChatState.clip(status.headSubject ?? "", 60))” is removed from the branch; its changes stay staged. Already pushed commits are not undone on the remote.")
        }
        .confirmationDialog("Nothing is staged.", isPresented: $project.askCommitAll, titleVisibility: .visible) {
            Button("Stage All and Commit") {
                Task { await appState.commitFromBox(in: project, all: true) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Stage all \(unstaged.count) changed file\(unstaged.count == 1 ? "" : "s") and commit them?")
        }
    }

    private var discardTitle: String {
        switch discardTarget {
        case .paths(let paths)? where paths.count == 1: return "Discard changes to \((paths[0] as NSString).lastPathComponent)?"
        case .paths(let paths)?: return "Discard changes to \(paths.count) files?"
        case .all(let n)?: return "Discard all \(n) changes?"
        case nil: return "Discard changes?"
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("Changes")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            Chip("\(status.files.count)")
            if let busy = project.gitBusy {
                BrailleSpinner(size: 11, tint: Theme.textSecondary)
                    .help(busy)
                    .accessibilityLabel(busy)
            }
            Spacer(minLength: 4)
            IconButton(Icons.refresh, label: "Refresh", size: 14) {
                Task {
                    await appState.refreshGit(project: project)
                    await appState.loadCommitLog(project)
                    await appState.loadBranches(project)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 12)
        .padding(.bottom, 8)
    }

    /// Branch menu (switch / create), ahead-behind chip, fetch / pull / push.
    /// A long branch name truncates so the buttons always fit.
    private var branchRow: some View {
        HStack(spacing: 6) {
            BranchLeadLayout(spacing: 6) {
                branchMenu
                if status.upstream != nil {
                    // Offered no width (dropped) when the list is too narrow for it.
                    ViewThatFits(in: .horizontal) {
                        Chip("↑\(status.ahead ?? 0) ↓\(status.behind ?? 0)")
                            .help("\(status.ahead ?? 0) to push, \(status.behind ?? 0) to pull (\(status.upstream ?? ""))")
                        EmptyView()
                    }
                }
            }
            // Sized before the spacer, which would otherwise take half the row.
            .layoutPriority(1)
            Spacer(minLength: 4)
            IconButton(Icons.fetch, label: "Fetch", size: 13) {
                Task { await appState.fetch(in: project) }
            }
            .help("Fetch all remotes")
            IconButton(Icons.pull, label: "Pull", size: 13) {
                Task { await appState.pull(rebase: false, in: project) }
            }
            .help("Pull (right-click: rebase)")
            .contextMenu {
                Button("Pull") { Task { await appState.pull(rebase: false, in: project) } }
                Button("Pull (Rebase)") { Task { await appState.pull(rebase: true, in: project) } }
            }
            IconButton(Icons.push, label: "Push…", size: 13) { pushSheet = true }
                .help("Push…")
        }
        .disabled(project.gitBusy != nil)
        .padding(.horizontal, 12)
        .padding(.bottom, 8)
    }

    /// "⎇ main ⌄": a SwiftUI label rather than a borderless menu's, so the
    /// name can truncate (`BranchLeadLayout` never offers more than it needs).
    private var branchMenu: some View {
        Menu {
            if let branches = project.branches {
                Section("Branches") {
                    ForEach(branches.local) { branch in
                        Button {
                            Task { await appState.checkout(branch.name, create: false, in: project) }
                        } label: {
                            if branch.name == branches.current {
                                Label(branch.name, systemImage: "checkmark")
                            } else {
                                Text(branch.name)
                            }
                        }
                        .disabled(branch.name == branches.current)
                    }
                }
                let remoteOnly = branches.remote.filter { remote in !branches.local.contains { $0.name == remote.short } }
                if !remoteOnly.isEmpty {
                    Section("Remote") {
                        ForEach(remoteOnly) { remote in
                            Button(remote.name) {
                                Task { await appState.checkout(remote.short, create: false, in: project) }
                            }
                        }
                    }
                }
            }
            Divider()
            Button("New Branch…") { newBranchSheet = true }
            Button("Refresh Branches") {
                Task { await appState.loadBranches(project) }
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: Icons.branch)
                Text(status.branch ?? "detached HEAD")
                    .lineLimit(1)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textSecondary)
            }
            .font(Theme.Font.secondary)
            .foregroundStyle(Theme.textPrimary)
            .padding(.horizontal, 4)
            .frame(height: 22)
            .background(
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(hoveringBranch ? Theme.surfaceRaised : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .menuIndicator(.hidden)
        .onHover { hoveringBranch = $0 }
        // The highlight's inset: the icon lines up with the rows below.
        .padding(.leading, -4)
        .help("\(status.branch ?? "detached HEAD") · \(status.upstream.map { "tracks \($0)" } ?? "no upstream branch")")
    }

    private var cleanRow: some View {
        HStack(spacing: 8) {
            Image(systemName: Icons.check)
                .foregroundStyle(Theme.success)
            Text("Working tree clean")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 10)
    }

    private func row(_ file: GitFileStatus, section: ChangeSection) -> some View {
        let actions: [RowAction]
        switch section {
        case .staged:
            actions = [RowAction(id: "unstage", icon: Icons.unstage, label: "Unstage") { Task { await appState.unstage([file.path], in: project) } }]
        case .changes:
            actions = [
                RowAction(id: "discard", icon: Icons.discard, label: "Discard changes") { discardTarget = .paths([file.path]) },
                RowAction(id: "stage", icon: Icons.stage, label: "Stage") { Task { await appState.stage([file.path], in: project) } },
            ]
        }
        return ChangeRow(
            file: file,
            isSelected: file.path == project.selectedChangePath && project.showStaged == (section == .staged),
            actions: project.gitBusy == nil ? actions : [],
            select: { select(file.path, section: section) },
            contextAction: { action in appState.reveal(path: file.path, in: project, action: action) }
        ) {
            if section == .changes {
                Button("Stage") { Task { await appState.stage([file.path], in: project) } }
                Button("Discard Changes…") { discardTarget = .paths([file.path]) }
            } else {
                Button("Unstage") { Task { await appState.unstage([file.path], in: project) } }
            }
            Button("Commit This File…") { commitFile = PathItem(id: file.path) }
            Divider()
        }
        .id("\(section)|\(file.path)")
    }

    private func errorBanner(_ error: String) -> some View {
        HStack(alignment: .top, spacing: 6) {
            Image(systemName: Icons.warning)
                .font(.system(size: 11, weight: .medium))
            Text(error)
                .font(Theme.Font.caption)
                .lineLimit(3)
            Spacer(minLength: 0)
            Button("Retry") {
                Task { await appState.refreshGit(project: project) }
            }
            .buttonStyle(.plain)
            .font(Theme.Font.caption)
            .foregroundStyle(Theme.accent)
        }
        .foregroundStyle(Theme.danger)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Theme.danger.opacity(0.08))
        .hairline(.bottom)
    }

    private func select(_ path: String, section: ChangeSection) {
        listFocused = true
        Task { await appState.selectChange(path: path, staged: section == .staged, in: project) }
    }

    private func move(_ direction: MoveCommandDirection, proxy: ScrollViewProxy) {
        let rows = rows
        guard !rows.isEmpty else { return }
        let current = rows.firstIndex { $0.file.path == project.selectedChangePath && project.showStaged == ($0.section == .staged) }
        let next: Int
        switch direction {
        case .down:
            next = current.map { min($0 + 1, rows.count - 1) } ?? 0
        case .up:
            next = current.map { max($0 - 1, 0) } ?? 0
        default:
            return
        }
        guard next != current else { return }
        let target = rows[next]
        withAnimation(.easeOut(duration: 0.12)) { proxy.scrollTo("\(target.section)|\(target.file.path)", anchor: nil) }
        select(target.file.path, section: target.section)
    }
}

/// The branch menu, then the ahead/behind chip. The branch name truncates
/// first, down to `floor` points of menu; narrower than that the chip is
/// offered no width (its `ViewThatFits` drops it) and the name gets it all.
private struct BranchLeadLayout: Layout {
    var spacing: CGFloat
    /// About ten characters of branch name.
    var floor: CGFloat = 100

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let (menu, chip) = widths(proposal.width, subviews)
        let height = subviews.map { $0.sizeThatFits(.unspecified).height }.max() ?? 0
        return CGSize(width: chip > 0 ? menu + spacing + chip : menu, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let (menu, chip) = widths(bounds.width, subviews)
        var x = bounds.minX
        for (subview, width) in zip(subviews, [menu, chip]) {
            subview.place(at: CGPoint(x: x, y: bounds.midY), anchor: .leading, proposal: ProposedViewSize(width: width, height: bounds.height))
            x += width + spacing
        }
    }

    /// Menu and chip widths in `width`; a chip width of 0 drops the chip.
    private func widths(_ width: CGFloat?, _ subviews: Subviews) -> (menu: CGFloat, chip: CGFloat) {
        let ideal = subviews.map { $0.sizeThatFits(.unspecified).width }
        let menu = ideal.first ?? 0
        let chip = ideal.count > 1 ? ideal[1] : 0
        guard let width else { return (menu, chip) }
        if chip > 0, width - spacing - chip >= min(menu, floor) {
            return (min(menu, width - spacing - chip), chip)
        }
        return (min(menu, width), 0)
    }
}

/// "STAGED CHANGES · 3" with the section's actions at the right.
private struct SectionHeader<Actions: View>: View {
    let title: String
    let count: Int?
    @ViewBuilder let actions: Actions

    var body: some View {
        HStack(spacing: 6) {
            Text(title.uppercased())
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(Theme.textTertiary)
                .tracking(0.4)
            if let count {
                Text(verbatim: "\(count)")
                    .font(.system(size: 10.5, weight: .medium, design: .monospaced))
                    .foregroundStyle(Theme.textTertiary)
            }
            Spacer(minLength: 4)
            actions
        }
        .padding(.horizontal, 6)
        .padding(.top, 8)
        .padding(.bottom, 2)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(count.map { "\(title), \($0)" } ?? title)
    }
}

/// A hover action on a changed-file row.
private struct RowAction: Identifiable {
    let id: String
    let icon: String
    let label: String
    let action: () -> Void

    init(id: String, icon: String, label: String, action: @escaping () -> Void) {
        self.id = id
        self.icon = icon
        self.label = label
        self.action = action
    }
}

/// The commit box: message editor (placeholder names the branch), ✨ model
/// generation, and Commit with a menu for Amend / Commit All / Undo.
private struct CommitBox: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState
    let status: GitStatus
    var onUndo: () -> Void

    var body: some View {
        @Bindable var project = project
        VStack(spacing: 6) {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $project.commitMessage)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                    .scrollContentBackground(.hidden)
                    .frame(height: 64)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 4)
                if project.commitMessage.isEmpty {
                    Text("Message (⌘⏎ to commit on \(status.branch ?? "HEAD"))")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(.horizontal, 11)
                        .padding(.vertical, 8)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
            }
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .strokeBorder(Theme.border, lineWidth: 1)
            )
            HStack(spacing: 6) {
                IconButton(Icons.generate, label: "Generate commit message", size: 13) {
                    Task { await appState.generateCommitMessage(in: project) }
                }
                .disabled(project.generatingMessage || status.files.isEmpty)
                .help(status.files.isEmpty ? "No changes to describe" : "Write the message from the staged changes with the model")
                if project.generatingMessage {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                }
                Spacer(minLength: 4)
                Button("Commit") {
                    Task { await appState.commitFromBox(in: project) }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .disabled(!canCommit)
                .help(canCommit ? "Commit the staged changes (⌘⏎)" : "Type a message and stage changes to commit")
                Menu {
                    Button("Commit") { Task { await appState.commitFromBox(in: project) } }
                        .disabled(!canCommit)
                    Button("Commit All (Stage Everything)") { Task { await appState.commitFromBox(in: project, all: true) } }
                        .disabled(messageEmpty || status.files.isEmpty)
                    Button("Commit (Amend Last)") { Task { await appState.commitFromBox(in: project, amend: true) } }
                        .disabled(messageEmpty || status.headSubject == nil)
                    Divider()
                    Button("Undo Last Commit…", action: onUndo)
                        .disabled(status.headSubject == nil)
                } label: {
                    Image(systemName: Icons.chevronDown)
                        .font(.system(size: 10, weight: .semibold))
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .controlSize(.small)
                .frame(width: 22)
                .help("More commit options")
                .accessibilityLabel("Commit options")
            }
            .disabled(project.gitBusy != nil)
        }
        .padding(.horizontal, 12)
        .padding(.bottom, 10)
        .hairline(.bottom)
    }

    private var messageEmpty: Bool {
        project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Enabled with a message and any change (nothing staged asks "stage all?").
    private var canCommit: Bool {
        !messageEmpty && !status.files.isEmpty
    }
}

/// One recent commit: short hash and subject; the HEAD row can be undone.
private struct CommitRow: View {
    @Environment(AppState.self) private var appState
    let entry: GitLogEntry
    let isHead: Bool
    var onUndo: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(verbatim: entry.short)
                .font(Theme.Font.monoSmall)
                .foregroundStyle(Theme.textTertiary)
            Text(verbatim: entry.subject)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .help("\(entry.author) · \(entry.date)")
        .contextMenu {
            Button("Copy Hash") { appState.copyToPasteboard(entry.hash) }
            Button("Copy Subject") { appState.copyToPasteboard(entry.subject) }
            if isHead {
                Divider()
                Button("Undo Last Commit…", action: onUndo)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Commit \(entry.short), \(entry.subject)")
    }
}

private struct ChangeRow<ExtraMenu: View>: View {
    let file: GitFileStatus
    let isSelected: Bool
    /// Shown instead of the line counts while hovered (stage / unstage / discard).
    let actions: [RowAction]
    let select: () -> Void
    let contextAction: (PathAction) -> Void
    @ViewBuilder let extraMenu: ExtraMenu

    @State private var hovering = false

    var body: some View {
        Button(action: select) {
            HStack(spacing: 8) {
                StatusLetter(code: file.code)
                VStack(alignment: .leading, spacing: 1) {
                    Text(verbatim: fileName)
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if !directory.isEmpty {
                        Text(verbatim: directory)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.head)
                    }
                }
                Spacer(minLength: 4)
                if hovering, !actions.isEmpty {
                    HStack(spacing: 2) {
                        ForEach(actions) { action in
                            IconButton(action.icon, label: action.label, size: 11, action: action.action)
                                .help(action.label)
                        }
                    }
                    // Taller than a text line; let the buttons spill into the
                    // row padding so hovering doesn't resize the row.
                    .frame(height: 16)
                } else if file.insertions != nil || file.deletions != nil {
                    HStack(spacing: 4) {
                        if let added = file.insertions, added > 0 || file.deletions == 0 {
                            Text(verbatim: "+\(added)")
                                .foregroundStyle(Theme.success)
                        }
                        if let removed = file.deletions, removed > 0 {
                            Text(verbatim: "−\(removed)")
                                .foregroundStyle(Theme.danger)
                        }
                    }
                    .font(Theme.Font.monoSmall)
                    .monospacedDigit()
                    .lineLimit(1)
                    .fixedSize()
                    .accessibilityLabel(lineChangesLabel)
                }
                if file.staged {
                    Circle()
                        .fill(Theme.success)
                        .frame(width: 6, height: 6)
                        .help("Staged")
                        .accessibilityLabel("staged")
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .frame(maxWidth: .infinity, alignment: .leading)
            // One background for selection and hover (one hover handler per
            // row): the path is in the row itself, so no tooltip either.
            .background(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .fill(isSelected || hovering ? Theme.surfaceRaised : Color.clear)
            )
            .contentShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { inside in hovering = inside }
        .accessibilityLabel("\(ChangesStatus.label(for: file.code)) \(file.path)\(file.staged ? ", staged" : "")\(lineChangesLabel.isEmpty ? "" : ", " + lineChangesLabel)")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
        .contextMenu {
            extraMenu
            Button("Open in Editor") { contextAction(.openInEditor) }
            Button("Reveal in Finder") { contextAction(.revealInFinder) }
            Button("Open with Default Editor") { contextAction(.openWithEditor) }
            Button("Copy Path") { contextAction(.copyPath) }
        }
    }

    private var fileName: String {
        (file.path as NSString).lastPathComponent
    }

    /// "12 lines added, 3 removed" for VoiceOver; empty when unknown.
    private var lineChangesLabel: String {
        var parts: [String] = []
        if let added = file.insertions { parts.append("\(added) line\(added == 1 ? "" : "s") added") }
        if let removed = file.deletions, removed > 0 { parts.append("\(removed) removed") }
        return parts.joined(separator: ", ")
    }

    private var directory: String {
        let dir = (file.path as NSString).deletingLastPathComponent
        return dir == "." ? "" : dir
    }
}

/// The git status letter in a small tinted square.
private struct StatusLetter: View {
    let code: String

    var body: some View {
        Text(verbatim: code)
            .font(.system(size: 11, weight: .semibold, design: .monospaced))
            .foregroundStyle(ChangesStatus.tint(for: code))
            .frame(width: 18, height: 18)
            .background(ChangesStatus.tint(for: code).opacity(0.14), in: RoundedRectangle(cornerRadius: 4, style: .continuous))
            .accessibilityHidden(true)
    }
}

/// Status-letter presentation shared by the list, the diff header and the
/// Inspector Files tab.
enum ChangesStatus {
    static func label(for code: String) -> String {
        switch code {
        case "M": return "Modified"
        case "A": return "Added"
        case "D": return "Deleted"
        case "R": return "Renamed"
        case "C": return "Copied"
        case "T": return "Type changed"
        case "U": return "Unmerged"
        case "?": return "Untracked"
        case "!": return "Ignored"
        default: return "Changed"
        }
    }

    static func tint(for code: String) -> Color {
        switch code {
        case "M", "T": return Theme.warning
        case "A": return Theme.success
        case "D", "U": return Theme.danger
        case "R", "C": return Theme.accent
        default: return Theme.textTertiary
        }
    }
}

// MARK: - Diff pane

/// +N / −N of a single-file diff for the pane header.
private struct DiffCounts: Equatable {
    var added = 0
    var removed = 0

    static func count(_ diff: String) -> DiffCounts {
        var counts = DiffCounts()
        for line in diff.utf8.split(separator: 0x0A, omittingEmptySubsequences: true) {
            guard let first = line.first else { continue }
            if first == UInt8(ascii: "+") {
                if line.starts(with: "+++".utf8) { continue }
                counts.added += 1
            } else if first == UInt8(ascii: "-") {
                if line.starts(with: "---".utf8) { continue }
                counts.removed += 1
            }
        }
        return counts
    }
}

private struct ChangeDiffPane: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState
    let file: GitFileStatus?

    @State private var counts: DiffCounts?
    @State private var width: CGFloat = 0

    var body: some View {
        VStack(spacing: 0) {
            if let file {
                header(file)
                body(for: file)
            } else {
                EmptyPlaceholder(
                    systemImage: Icons.file,
                    title: "Select a file",
                    subtitle: "Pick a changed file to see its diff."
                )
            }
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
        .task(id: project.diff?.diff) {
            guard let diff = project.diff?.diff, !diff.isEmpty else {
                counts = nil
                return
            }
            counts = await Task.detached(priority: .userInitiated) { DiffCounts.count(diff) }.value
        }
    }

    private func header(_ file: GitFileStatus) -> some View {
        HStack(spacing: 8) {
            if width >= 220 {
                Image(systemName: Icons.file)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .accessibilityHidden(true)
            }
            // Takes what the fixed-size chips and buttons leave; the chips
            // drop out by pane width so there is always room for some path.
            Text(verbatim: file.path)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.middle)
                .selectableText()
                .layoutPriority(1)
            headerChips(file)
            if project.diffLoading {
                BrailleSpinner()
            }
            Spacer(minLength: 8)
            CopyButton(text: file.path, label: "Copy path")
            IconButton(Icons.folder, label: "Reveal in Finder", size: 14) {
                appState.reveal(path: file.path, in: project, action: .revealInFinder)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(Theme.surface)
        .hairline(.bottom)
        .contextMenu {
            Button("Open in Editor") { appState.reveal(path: file.path, in: project, action: .openInEditor) }
            Button("Reveal in Finder") { appState.reveal(path: file.path, in: project, action: .revealInFinder) }
            Button("Open with Default Editor") { appState.reveal(path: file.path, in: project, action: .openWithEditor) }
            Button("Copy Path") { appState.reveal(path: file.path, in: project, action: .copyPath) }
        }
    }

    /// By pane width: the diff mode chip goes first, then the status chip,
    /// then the line counts (the narrowest pane is ~170 pt).
    @ViewBuilder
    private func headerChips(_ file: GitFileStatus) -> some View {
        if width >= 300 {
            HStack(spacing: 8) {
                if width >= 380 {
                    Chip(ChangesStatus.label(for: file.code), style: .tinted(ChangesStatus.tint(for: file.code)))
                }
                lineCounts
                if width >= 480 {
                    Chip(project.showStaged ? "staged diff" : "working tree")
                        .help(project.showStaged ? "git diff --cached" : "git diff")
                }
            }
        }
    }

    @ViewBuilder
    private var lineCounts: some View {
        if let counts, counts.added > 0 || counts.removed > 0 {
            HStack(spacing: 6) {
                Text(verbatim: "+\(counts.added)")
                    .foregroundStyle(Theme.success)
                Text(verbatim: "−\(counts.removed)")
                    .foregroundStyle(Theme.danger)
            }
            .font(.system(size: 12, weight: .medium, design: .monospaced))
            .lineLimit(1)
            .fixedSize()
            .accessibilityLabel("\(counts.added) additions, \(counts.removed) deletions")
        }
    }

    @ViewBuilder
    private func body(for file: GitFileStatus) -> some View {
        if let diff = project.diff {
            if diff.diff.isEmpty {
                emptyDiff(file)
            } else {
                // The pane header names the file: no second file header inside the diff.
                DiffView(diff: diff.diff, truncated: diff.truncated, showFileHeaders: false)
            }
        } else if let error = project.diffError {
            EmptyPlaceholder(
                systemImage: Icons.warning,
                title: "Could not load the diff",
                subtitle: error,
                actionTitle: "Retry"
            ) {
                Task { await appState.selectChange(path: file.path, in: project) }
            }
        } else {
            VStack(spacing: 10) {
                BrailleSpinner(size: 16)
                Text("Loading diff…")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder
    private func emptyDiff(_ file: GitFileStatus) -> some View {
        if file.code == "?" {
            EmptyPlaceholder(
                systemImage: Icons.file,
                title: "Untracked file",
                subtitle: "git diff has nothing to show for untracked files. Stage it (git add) and switch on Staged to review its contents."
            )
        } else if project.showStaged, !file.staged {
            EmptyPlaceholder(
                systemImage: Icons.changes,
                title: "Nothing staged",
                subtitle: "This file has no staged changes. Switch off Staged to see the working tree diff."
            )
        } else if !project.showStaged, file.staged, file.status.count == 1 {
            EmptyPlaceholder(
                systemImage: Icons.changes,
                title: "All changes are staged",
                subtitle: "Switch on Staged to review them."
            )
        } else {
            EmptyPlaceholder(
                systemImage: Icons.changes,
                title: "No differences",
                subtitle: "The diff is empty."
            )
        }
    }
}

// MARK: - Preview

#Preview("Changes") {
    let appState = AppState()
    let info = ProjectInfo(
        projectId: "preview-project",
        root: "/Users/me/Projects/context-agent",
        name: "context-agent",
        git: GitInfo(branch: "main", head: "862959a1f00", dirtyFiles: 3),
        index: IndexStats(generation: 12, files: 310, symbols: 4102, chunks: 5110, relationships: 900, embeddings: 5110, lastFullIndexAt: nil, languages: []),
        indexing: false,
        model: "gpt-4.1",
        permissionProfile: "approval_required",
        sandbox: "none",
        validationCommands: [],
        languages: [],
        buildFiles: ["Cargo.toml"],
        fileCount: 310,
        runningTask: nil
    )
    let project = ProjectState(info: info, models: ModelCatalog(preferences: appState.preferences, baseURL: ""))
    project.gitStatus = GitStatus(branch: "main", head: "862959a", files: [
        GitFileStatus(path: "crates/cli/src/app.rs", status: "M", staged: false, insertions: 42, deletions: 7),
        GitFileStatus(path: "crates/core/src/config.rs", status: "M", staged: true),
        GitFileStatus(path: "docs/desktop-protocol.md", status: "?", staged: false, insertions: 310, deletions: 0),
        GitFileStatus(path: "apps/macos/ContextAgent/Views/Changes/DiffView.swift", status: "A", staged: true),
    ])
    project.selectedChangePath = "crates/cli/src/app.rs"
    project.diff = GitDiff(diff: """
    diff --git a/crates/cli/src/app.rs b/crates/cli/src/app.rs
    index 3b18e51..a4c2f10 100644
    --- a/crates/cli/src/app.rs
    +++ b/crates/cli/src/app.rs
    @@ -10,7 +10,8 @@ impl App {
         pub fn run(&self) -> Result<(), Error> {
    -        println!("starting");
    +        println!("starting {}", self.name);
    +        self.index.ensure_ready()?;
             Ok(())
         }
     }
    """, truncated: false)
    appState.projects.append(project)
    appState.selectedProjectId = project.id
    return ChangesView()
        .environment(appState)
        .frame(width: 960, height: 560)
}
