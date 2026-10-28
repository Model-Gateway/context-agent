import SwiftUI
import AppKit

/// The sidebar's "Remote" section: every configured SSH host as a row with
/// its connection state, followed by the projects open on it (nested like
/// the local ones, worktrees under their parent) and its pinned recents
/// that are not open. Hidden while no host is configured.
struct RemoteHostsSection: View {
    @Environment(AppState.self) private var appState
    @State private var headerHovering = false

    var body: some View {
        let hosts = appState.hosts.hosts
        if !hosts.isEmpty {
            SidebarSectionHeader("Remote") {
                IconButton(Icons.newChat, label: "Open a project on a remote host…", size: 11) {
                    appState.chooseRemoteProject()
                }
                .opacity(headerHovering ? 1 : 0)
            }
            .contentShape(Rectangle())
            .onHover { inside in
                withAnimation(.easeOut(duration: 0.12)) { headerHovering = inside }
            }
            ForEach(hosts) { host in
                HostRow(host: host)
                ForEach(appState.topLevelProjects(on: host.id)) { project in
                    ProjectRow(project: project)
                        .padding(.leading, 14)
                    ForEach(appState.visibleWorktrees(of: project)) { worktree in
                        WorktreeRow(project: worktree)
                            .padding(.leading, 14)
                    }
                }
                ForEach(openingRows(on: host), id: \.self) { key in
                    OpeningProjectRow(path: key)
                        .padding(.leading, 14)
                }
                ForEach(pinnedNotOpen(on: host)) { recent in
                    PinnedRecentRow(recent: recent)
                        .padding(.leading, 14)
                }
            }
        }
    }

    private func openingRows(on host: SSHHost) -> [String] {
        appState.pendingProjectPaths.filter { RemoteProjectRef(encoded: $0)?.hostId == host.id }
    }

    private func pinnedNotOpen(on host: SSHHost) -> [RecentProject] {
        let open = Set(appState.projects.map(\.key)).union(appState.openingProjectPaths)
        return appState.recents.pinned.filter { $0.hostId == host.id && !open.contains($0.id) }
    }
}

/// One remote host: its name with the connection state where the badge
/// sits (spinner while connecting, a dot otherwise); a click opens the
/// folder picker on it; the context menu connects, reconnects, disconnects
/// or removes it.
struct HostRow: View {
    var host: SSHHost

    @Environment(AppState.self) private var appState
    @State private var hovering = false
    @State private var confirmRemove = false

    private var connection: RemoteConnection? { appState.remotes[host.id] }

    private var isConnecting: Bool {
        if case .connecting = connection?.state { return true }
        return false
    }

    private var statusTint: Color {
        switch connection?.state {
        case .ready: return Theme.success
        case .failed: return Theme.danger
        case .connecting: return Theme.warning
        case .disconnected, nil: return Theme.textTertiary
        }
    }

    private var statusText: String {
        guard let connection else { return "Not connected" }
        if let hint = connection.hint, case .failed = connection.state {
            return connection.label + "\n" + hint
        }
        return connection.hostSummary.map { "\(connection.label) · \($0)" } ?? connection.label
    }

    private var openCount: Int { appState.projects(on: host.id).count }

    var body: some View {
        Button {
            appState.chooseRemoteProject(host: host)
        } label: {
            HStack(spacing: 10) {
                RowIcon(systemImage: Icons.remoteHost, tint: Theme.textSecondary)
                Text(verbatim: host.displayName)
                    .font(Theme.Font.sidebar)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: 4)
                if isConnecting {
                    BrailleSpinner(size: 11, tint: Theme.textTertiary)
                } else if hovering {
                    Image(systemName: Icons.newChat)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                        .accessibilityHidden(true)
                } else {
                    Circle()
                        .fill(statusTint)
                        .frame(width: 7, height: 7)
                        .padding(.trailing, 4)
                        .accessibilityHidden(true)
                }
            }
            .sidebarRow(selected: false, hovering: hovering)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help("\(host.target)\n\(statusText)")
        .contextMenu {
            Button("Open Project on \(host.displayName)…") {
                appState.chooseRemoteProject(host: host)
            }
            if connection?.isReady == true {
                Button("Disconnect") {
                    Task { await appState.disconnect(host) }
                }
                .disabled(appState.projects(on: host.id).contains { $0.isRunning })
            } else {
                Button(connection?.state.failure == nil ? "Connect" : "Reconnect") {
                    Task { await appState.reconnect(host) }
                }
                .disabled(isConnecting)
            }
            Divider()
            Button("Edit in Settings…") {
                appState.openSettings(.remoteHosts)
            }
            Button("Copy ssh Command") {
                appState.copyToPasteboard("ssh \(host.target)")
            }
            Divider()
            Button("Remove Host…", role: .destructive) {
                confirmRemove = true
            }
        }
        .confirmationDialog("Remove “\(host.displayName)”?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                Task { await appState.removeHost(host) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(openCount > 0
                 ? "Its \(openCount) open project\(openCount == 1 ? "" : "s") close\(openCount == 1 ? "s" : "") and its recents are forgotten. Nothing on the host is touched."
                 : "Its recents are forgotten. Nothing on the host is touched.")
        }
        .accessibilityLabel("Remote host \(host.displayName), \(connection?.label ?? "not connected")")
    }
}
