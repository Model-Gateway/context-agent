import SwiftUI
import AppKit

/// The left pane (ARCHITECTURE.md "Sidebar"): fixed navigation rows, the
/// "Projects" section (open projects, then pinned recents that are not open),
/// the "Chats" section (the selected project's chats) and the account button
/// at the bottom. It sits under the window-wide top bar.
struct SidebarView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(spacing: 0) {
            navigation
                .padding(.top, 10)
                .padding(.horizontal, 10)
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 2) {
                    projectsSection
                    RemoteHostsSection()
                    recentsSection
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 12)
            }
            .scrollIndicators(.automatic)
            AccountButton()
                .padding(10)
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Theme.sidebar)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Sidebar")
    }

    // MARK: - Navigation

    private var navigation: some View {
        let hasProject = appState.selectedProject != nil
        // Changes and Project live in the top bar's tabs only.
        return VStack(alignment: .leading, spacing: 2) {
            NavRow(systemImage: Icons.welcome, title: "Welcome", isSelected: appState.sidebarItem == .welcome) {
                appState.sidebarItem = .welcome
            }
            NavRow(systemImage: Icons.newChat, title: "New chat", isSelected: appState.sidebarItem == .chat && appState.selectedSessionId == nil && hasProject, isEnabled: hasProject) {
                appState.newChat()
            }
            NavRow(systemImage: Icons.projects, title: "Projects", isSelected: appState.sidebarItem == .projects) {
                appState.sidebarItem = .projects
            }
            NavRow(systemImage: Icons.apps, title: "Apps", isSelected: appState.sidebarItem == .apps) {
                appState.sidebarItem = .apps
            }
        }
    }

    // MARK: - Projects

    private var openRoots: Set<String> {
        Set(appState.projects.map(\.key))
    }

    /// Pinned local recents that are not open (remote ones list under their host).
    private var pinnedNotOpen: [RecentProject] {
        let open = openRoots.union(appState.openingProjectPaths)
        return appState.recents.pinned.filter { !$0.isRemote && !open.contains($0.id) }
    }

    /// An open project, or a folder still opening shown as a loading row.
    private enum ProjectEntry: Identifiable {
        case open(ProjectState)
        case opening(String)

        var id: String {
            switch self {
            case .open(let project): return project.id
            case .opening(let path): return "opening:" + path
            }
        }

        @MainActor var root: String {
            switch self {
            case .open(let project): return project.root
            case .opening(let path): return path
            }
        }
    }

    /// Open local projects with the folders still opening slotted in at their
    /// remembered launch position (new opens go last), so rows fill in place.
    private var projectEntries: [ProjectEntry] {
        var entries = appState.topLevelProjects.map(ProjectEntry.open)
        for path in appState.pendingProjectPaths where !RemoteProjectRef.isRemote(path) {
            let rank = appState.restoreRank(of: path)
            let index = entries.firstIndex { appState.restoreRank(of: $0.root) > rank } ?? entries.endIndex
            entries.insert(.opening(path), at: index)
        }
        return entries
    }

    @State private var projectsHeaderHovering = false

    @ViewBuilder
    private var projectsSection: some View {
        let pinned = pinnedNotOpen
        let entries = projectEntries
        if !entries.isEmpty || !pinned.isEmpty {
            SidebarSectionHeader("Projects") {
                // Fades in while any part of the row is hovered, like the Chats header's.
                IconButton(Icons.newChat, label: "Open a project…", size: 11) {
                    Task { await appState.chooseProject() }
                }
                .disabled(!appState.connection.isReady)
                .opacity(projectsHeaderHovering ? 1 : 0)
            }
            .contentShape(Rectangle())
            .onHover { inside in
                withAnimation(.easeOut(duration: 0.12)) { projectsHeaderHovering = inside }
            }
            ForEach(entries) { entry in
                switch entry {
                case .open(let project):
                    ProjectRow(project: project)
                    ForEach(appState.visibleWorktrees(of: project)) { worktree in
                        WorktreeRow(project: worktree)
                    }
                case .opening(let path):
                    OpeningProjectRow(path: path)
                }
            }
            ForEach(pinned) { recent in
                PinnedRecentRow(recent: recent)
            }
        }
    }

    // MARK: - Chats

    @State private var chatsHeaderHovering = false

    @ViewBuilder
    private var recentsSection: some View {
        if appState.selectedProject == nil, !appState.pendingProjectPaths.isEmpty {
            // The project about to be selected is still opening.
            SidebarSectionHeader("Chats")
            SessionRowsSkeleton()
        } else if let project = appState.selectedProject {
            SidebarSectionHeader("Chats") {
                // The button always holds its space so the header never
                // changes height; it fades in while any part of the row is
                // hovered (the spinner takes its place otherwise).
                ZStack {
                    if !chatsHeaderHovering, project.sessionsLoading, !project.sessions.isEmpty {
                        BrailleSpinner(size: 10, tint: Theme.textTertiary)
                    }
                    IconButton(Icons.newChat, label: "New chat", size: 11) {
                        appState.newChat()
                    }
                    .opacity(chatsHeaderHovering ? 1 : 0)
                }
            }
            // The whole row, Spacer included, counts as hovered.
            .contentShape(Rectangle())
            .onHover { inside in
                withAnimation(.easeOut(duration: 0.12)) { chatsHeaderHovering = inside }
            }
            if project.sessions.isEmpty {
                if project.sessionsLoading {
                    SessionRowsSkeleton()
                } else if let error = project.sessionsError {
                    sessionsError(error, project: project)
                } else {
                    Text("No chats yet")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(.horizontal, 8)
                        .frame(height: 30)
                }
            } else {
                ForEach(project.sessions) { session in
                    SessionRow(session: session, project: project)
                }
                if let error = project.sessionsError {
                    sessionsError(error, project: project)
                }
            }
        }
    }

    private func sessionsError(_ error: String, project: ProjectState) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: error)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.danger)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
            Button("Retry") {
                Task { await project.loadSessions() }
            }
            .buttonStyle(.bordered)
            .controlSize(.mini)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}

// MARK: - Account button

/// Bottom-left "Sign in with ModelGateway" (ARCHITECTURE.md "Account"):
/// opens the profile page (`ProfileView`: Account and the settings tabs) on
/// its last tab. Signed out and landing on Account, it also starts the
/// device sign-in; signed in it shows the account's name and balance.
private struct AccountButton: View {
    @Environment(AppState.self) private var appState
    @State private var hovering = false

    var body: some View {
        let gateway = appState.gateway
        let selected = appState.sidebarItem == .account
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Button {
            appState.sidebarItem = .account
            if appState.preferences.settingsTab == SettingsTab.account.rawValue, !gateway.isSignedIn, !gateway.isSigningIn {
                appState.beginGatewaySignIn()
            }
        } label: {
            HStack(spacing: 10) {
                avatar(gateway.account)
                VStack(alignment: .leading, spacing: 1) {
                    Text(verbatim: title(gateway))
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Text(verbatim: subtitle(gateway))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Spacer(minLength: 4)
                if gateway.isSigningIn {
                    BrailleSpinner(size: 10, tint: Theme.textSecondary)
                } else {
                    Image(systemName: Icons.chevronRight)
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(Theme.textTertiary)
                        .accessibilityHidden(true)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(selected || hovering ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(gateway.isSignedIn ? "ModelGateway account, usage and settings" : "Account and settings")
        .accessibilityLabel(title(gateway) + ". " + subtitle(gateway))
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    @ViewBuilder
    private func avatar(_ account: GatewayAccount?) -> some View {
        if let account {
            Text(verbatim: account.initial)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Theme.accent)
                .frame(width: 22, height: 22)
                .background(Theme.accentSoft, in: Circle())
                .accessibilityHidden(true)
        } else {
            Image(systemName: Icons.account)
                .font(.system(size: 18, weight: .regular))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 22, height: 22)
                .accessibilityHidden(true)
        }
    }

    private func title(_ gateway: GatewayState) -> String {
        if let account = gateway.account { return account.name }
        return gateway.isSigningIn ? "Signing in to ModelGateway…" : "Sign in with ModelGateway"
    }

    private func subtitle(_ gateway: GatewayState) -> String {
        if let account = gateway.account {
            return account.displayInCurrency ? "ModelGateway · \(UsageFormat.usd(account.balanceUSD)) left" : "ModelGateway · \(UsageFormat.compact(account.quota)) left"
        }
        if case .waiting? = gateway.flow { return "Approve the code in your browser" }
        if gateway.isSigningIn { return "Contacting ModelGateway…" }
        return "Optional · usage and billing"
    }
}

// MARK: - Preview

#Preview("Sidebar") {
    SidebarView()
        .frame(width: Theme.Metrics.sidebarWidth, height: 720)
        .environment(MainWindowPreview.appState())
}

#Preview("Sidebar · backend failed") {
    SidebarView()
        .frame(width: Theme.Metrics.sidebarWidth, height: 720)
        .environment(MainWindowPreview.appState(failure: .exited(code: 101, tail: "thread 'main' panicked at src/serve.rs:42")))
}
