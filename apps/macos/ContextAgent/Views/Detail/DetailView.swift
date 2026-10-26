import SwiftUI
import AppKit

/// The centre pane under the window-wide `TopBar`: the content for
/// `appState.sidebarItem` — Welcome, Projects, the Chat tab (`ChatView` /
/// `EmptyChatView`, V2), Changes (V5) or Project (V5).
struct DetailView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.canvas)
    }

    @ViewBuilder
    private var content: some View {
        switch appState.sidebarItem {
        case .welcome:
            WelcomeView()
        case .projects:
            ProjectsView()
        case .apps:
            AppsView()
        case .chat:
            if let chat = appState.selectedChat {
                ChatView(chat: chat)
                    .id(chat.sessionId)
            } else if let project = appState.selectedProject {
                if appState.restoringSessionId != nil {
                    ChatLoadingView()
                } else {
                    EmptyChatView(project: project)
                        .id(project.id)
                }
            } else if !appState.pendingProjectPaths.isEmpty {
                // The project about to be selected is still opening: the
                // shape of the chat it will land on.
                if rememberedChatPending {
                    ChatLoadingView()
                } else {
                    EmptyChatSkeleton()
                }
            } else {
                noProject
            }
        case .changes:
            ChangesView()
        case .project:
            ProjectView()
        case .editor:
            EditorView()
        case .account:
            ProfileView()
        }
    }

    /// The last selected project had a chat open when the app quit.
    private var rememberedChatPending: Bool {
        guard let id = appState.preferences.lastSelectedProjectId else { return false }
        return appState.preferences.lastSelectedSession[id] != nil
    }

    private var noProject: some View {
        EmptyPlaceholder(systemImage: Icons.folder, title: "No project open", subtitle: "Open a repository folder to start a chat.", actionTitle: "Open a project…") {
            Task { await appState.chooseProject() }
        }
        .background(Theme.canvas)
    }
}

// MARK: - Top bar

/// The 52 pt bar across the whole window, above the sidebar, centre pane and
/// inspector (ARCHITECTURE.md "Top bar"): after the traffic lights, the
/// Chat · Changes · Project tabs; the palette search field centred in the
/// window; at the right the approvals pill, the project / branch chip, the
/// approval-mode chip (opens `PermissionsPopover`), the inspector toggle and
/// the Settings gear. The model is chosen from the composer's chip only.
struct TopBar: View {
    @Environment(AppState.self) private var appState
    @State private var permissionsPresented = false

    /// One arrangement of the bar, from roomiest to tightest.
    private struct Layout {
        enum ProjectChip { case nameAndBranch, branch, hidden }
        var project: ProjectChip = .nameAndBranch
        /// Ideal width of the search field; nil shows the magnifier icon only.
        var paletteIdealWidth: CGFloat? = 320
        /// Titles on the selected tab only.
        var compactTabs = false
        /// Approval mode as an icon without its label.
        var compactApproval = false
    }

    /// Widest arrangement that fits: the search field narrows first, then
    /// the project chip loses its name and goes, then tab titles and the
    /// approval label; the field itself turns into an icon last.
    private static let layouts: [Layout] = [
        Layout(paletteIdealWidth: 320),
        Layout(paletteIdealWidth: 220),
        Layout(paletteIdealWidth: 140),
        Layout(project: .branch, paletteIdealWidth: 140),
        Layout(project: .hidden, paletteIdealWidth: 140),
        // Near the 960 pt minimum window width.
        Layout(project: .hidden, paletteIdealWidth: 120, compactTabs: true),
        Layout(project: .hidden, paletteIdealWidth: 120, compactTabs: true, compactApproval: true),
        Layout(project: .hidden, paletteIdealWidth: nil, compactTabs: true, compactApproval: true),
    ]

    var body: some View {
        ViewThatFits(in: .horizontal) {
            ForEach(Self.layouts.indices, id: \.self) { index in
                bar(Self.layouts[index])
            }
        }
        // The traffic lights sit over the bar's left edge.
        .padding(.leading, Self.trafficLightsInset)
        .padding(.trailing, 12)
        .frame(height: 52)
        .frame(maxWidth: .infinity)
        // Empty space acts as the title bar: drag moves, double-click zooms.
        .background(TitleBarArea())
        .background(Theme.canvas)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Top bar")
    }

    /// Room for the close / minimise / zoom buttons of the hidden title bar.
    static let trafficLightsInset: CGFloat = 90

    private func bar(_ layout: Layout) -> some View {
        CenteredBarLayout(spacing: 16, leadingInset: Self.trafficLightsInset, trailingInset: 12) {
            TabSwitcher(compact: layout.compactTabs)
                .fixedSize()
            if let idealWidth = layout.paletteIdealWidth {
                paletteField
                    .frame(width: idealWidth)
            } else {
                IconButton(Icons.search, label: "Search chats, opens the command palette") {
                    appState.paletteVisible = true
                }
                .help("Search chats and actions (⌘K)")
            }
            trailing(layout)
                .fixedSize()
        }
    }

    /// Looks like the search field; clicking it opens the command palette.
    private var paletteField: some View {
        Button {
            appState.paletteVisible = true
        } label: {
            // The field itself never takes clicks (it would focus its text
            // field); the shape gives the button something to hit, or the
            // click falls through to the title strip and drags the window.
            SearchField("Search chats…", text: .constant(""), kbdHint: "⌘K")
                .allowsHitTesting(false)
                .contentShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        }
        .buttonStyle(.plain)
        .help("Search chats and actions (⌘K)")
        .accessibilityLabel("Search chats, opens the command palette")
    }

    private func trailing(_ layout: Layout) -> some View {
        HStack(spacing: 8) {
            if let first = appState.pendingApprovals.first {
                approvalsPill(count: appState.pendingApprovals.count, first: first)
            }
            if let project = appState.selectedProject {
                if layout.project != .hidden {
                    projectChip(project, showName: layout.project == .nameAndBranch)
                }
                approvalModeChip(project, compact: layout.compactApproval)
                    .fixedSize()
            }
            IconButton(Icons.inspector, label: appState.inspectorVisible ? "Hide inspector" : "Show inspector", isActive: appState.inspectorVisible) {
                appState.inspectorVisible.toggle()
            }
            Button {
                appState.openSettings()
            } label: {
                Image(systemName: Icons.settings)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .frame(width: 28, height: 28)
                    .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            }
            .buttonStyle(.plain)
            .hoverHighlight(radius: 6)
            .help("Settings (⌘,)")
            .accessibilityLabel("Settings")
        }
    }

    private func approvalsPill(count: Int, first: (chat: ChatState, request: ApprovalRequest)) -> some View {
        let title = count == 1 ? "1 approval waiting" : "\(count) approvals waiting"
        return Button {
            appState.select(project: first.chat.projectId, restoreSession: false)
            Task { await appState.select(session: first.chat.sessionId) }
        } label: {
            Chip("\(title) · “\(ChatState.clip(first.chat.title, 28))”", systemImage: Icons.approval, style: .tinted(Theme.warning))
        }
        .buttonStyle(.plain)
        .help("\(first.request.summary) — click to open the chat")
        .accessibilityLabel("\(title) in \(first.chat.title); open the chat")
    }

    /// Folder icon + name (dropped first when the bar is tight) + branch chip.
    private func projectChip(_ project: ProjectState, showName: Bool) -> some View {
        HStack(spacing: 6) {
            Image(systemName: Icons.folder)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            if showName {
                Text(verbatim: project.name)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 180)
                    .fixedSize()
            }
            if let branch = project.branch, !branch.isEmpty {
                Chip(branch, systemImage: Icons.branch)
                    .fixedSize()
            }
        }
        .help(project.root)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Project \(project.name)\(project.branch.map { ", branch \($0)" } ?? "")")
    }

    private func approvalModeChip(_ project: ProjectState, compact: Bool) -> some View {
        let label = AppState.approvalModeLabel(project.info.permissionProfile)
        return Button {
            permissionsPresented = true
        } label: {
            if compact {
                Image(systemName: PermissionProfileOption.systemImage(for: project.info.permissionProfile))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .frame(width: 28, height: 28)
                    .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                    .hoverHighlight(radius: 6)
            } else {
                Chip(label, systemImage: PermissionProfileOption.systemImage(for: project.info.permissionProfile))
            }
        }
        .buttonStyle(.plain)
        .help("Approval mode: \(label) · click to change")
        .accessibilityLabel("Approval mode \(label)")
        .popover(isPresented: $permissionsPresented, arrowEdge: .bottom) {
            PermissionsPopover()
        }
        .onChange(of: permissionsPresented) { _, shown in
            if !shown { appState.composerFocusToken += 1 }
        }
    }
}

/// Behind the top bar's controls: empty space behaves like a title bar, and
/// is the only place a drag moves the window (`isMovableByWindowBackground`
/// is off). A double-click does what System Settings → Desktop & Dock →
/// "Double-click a window's title bar to" says (zoom / fill by default, or
/// minimise, or nothing).
struct TitleBarArea: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView { DragView() }

    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class DragView: NSView {
        override func mouseDown(with event: NSEvent) {
            guard let window else { return }
            if event.clickCount == 2 {
                TitleBarArea.performDoubleClickAction(on: window)
            } else {
                window.performDrag(with: event)
            }
        }
    }

    static func performDoubleClickAction(on window: NSWindow) {
        let defaults = UserDefaults.standard
        let action = defaults.string(forKey: "AppleActionOnDoubleClick")
            ?? (defaults.bool(forKey: "AppleMiniaturizeOnDoubleClick") ? "Minimize" : "Maximize")
        switch action {
        case "Minimize":
            window.miniaturize(nil)
        case "None", "Do Nothing":
            break
        default:
            window.zoom(nil)
        }
    }
}

/// Three subviews: leading, centre, trailing. The centre one is centred in
/// the window (the bar is inset by `leadingInset` / `trailingInset` from the
/// window edges) and slides sideways only as far as needed to keep
/// `spacing` from its neighbours. Its ideal width is the three ideal widths
/// plus spacing, which is what `ViewThatFits` compares.
struct CenteredBarLayout: Layout {
    var spacing: CGFloat
    var leadingInset: CGFloat
    var trailingInset: CGFloat

    init(spacing: CGFloat, leadingInset: CGFloat, trailingInset: CGFloat) {
        self.spacing = spacing
        self.leadingInset = leadingInset
        self.trailingInset = trailingInset
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let ideal = sizes.reduce(0) { $0 + $1.width } + spacing * CGFloat(max(subviews.count - 1, 0))
        let height = sizes.map(\.height).max() ?? 0
        return CGSize(width: proposal.width ?? ideal, height: proposal.height ?? height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 3 else { return }
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        subviews[0].place(at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading, proposal: ProposedViewSize(sizes[0]))
        subviews[2].place(at: CGPoint(x: bounds.maxX, y: bounds.midY), anchor: .trailing, proposal: ProposedViewSize(sizes[2]))
        let windowCenter = bounds.midX + (trailingInset - leadingInset) / 2
        let lowest = bounds.minX + sizes[0].width + spacing
        let highest = bounds.maxX - sizes[2].width - spacing - sizes[1].width
        let x = max(lowest, min(windowCenter - sizes[1].width / 2, highest))
        subviews[1].place(at: CGPoint(x: x, y: bounds.midY), anchor: .leading, proposal: ProposedViewSize(sizes[1]))
    }
}

// MARK: - Tabs

/// Chat · Changes · Project pills; the selected one is `surface` with a
/// hairline and a soft shadow. Disabled without a project. `compact` shows
/// the title on the selected pill only.
private struct TabSwitcher: View {
    @Environment(AppState.self) private var appState
    var compact = false

    private struct Tab: Identifiable {
        let item: SidebarItem
        let title: String
        let systemImage: String
        let shortcut: String
        var id: String { item.rawValue }
    }

    private static let tabs: [Tab] = [
        Tab(item: .chat, title: "Chat", systemImage: Icons.chat, shortcut: "⌘1"),
        Tab(item: .changes, title: "Changes", systemImage: Icons.changes, shortcut: "⌘2"),
        Tab(item: .project, title: "Project", systemImage: Icons.project, shortcut: "⌘3"),
        Tab(item: .editor, title: "Editor", systemImage: Icons.editor, shortcut: "⌘4"),
    ]

    var body: some View {
        let enabled = appState.selectedProject != nil
        HStack(spacing: 2) {
            ForEach(Self.tabs) { tab in
                let selected = appState.sidebarItem == tab.item
                TabPill(title: tab.title, systemImage: tab.systemImage, isSelected: selected, showTitle: !compact || selected, badge: badge(for: tab.item)) {
                    appState.sidebarItem = tab.item
                    if tab.item == .chat { appState.composerFocusToken += 1 }
                }
                .help("\(tab.title) (\(tab.shortcut))")
            }
        }
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.5)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Tabs")
    }

    /// Changes: dirty files · Editor: documents with unsaved edits.
    private func badge(for item: SidebarItem) -> Int {
        guard let project = appState.selectedProject else { return 0 }
        switch item {
        case .changes: return project.dirtyCount
        case .editor: return appState.editors[project.id]?.unsavedCount ?? 0
        default: return 0
        }
    }
}

private struct TabPill: View {
    var title: String
    var systemImage: String
    var isSelected: Bool
    var showTitle = true
    var badge: Int
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: systemImage)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                    .accessibilityHidden(true)
                if showTitle {
                    Text(title)
                        .font(.system(size: 13, weight: isSelected ? .semibold : .medium))
                        .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                }
                if badge > 0 {
                    Text("\(badge)")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Theme.textSecondary)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Theme.surfaceRaised, in: Capsule())
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 28)
            .background(isSelected ? Theme.surface : (hovering ? Theme.surfaceRaised : Color.clear), in: shape)
            .overlay(shape.strokeBorder(isSelected ? Theme.border : Color.clear, lineWidth: 1))
            .shadow(color: Color.black.opacity(isSelected ? 0.05 : 0), radius: 3, x: 0, y: 1)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .accessibilityLabel(badge > 0 ? "\(title), \(badge)" : title)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - Previews

#Preview("Detail · welcome") {
    DetailView()
        .frame(width: 900, height: 700)
        .environment(MainWindowPreview.appState())
}

#Preview("Detail · changes") {
    let state = MainWindowPreview.appState()
    state.sidebarItem = .changes
    return DetailView()
        .frame(width: 900, height: 700)
        .environment(state)
}
