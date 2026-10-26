import SwiftUI
import AppKit
import ApplicationServices

/// The Apps page (sidebar "Apps"): a marketplace of the MCP servers Context
/// can install (`mcp/catalog`) — a title, a search field, a banner of
/// example prompts, then the catalog two columns wide with a "+" to install
/// and a check for what is installed — and, for one app, its own page
/// (`AppPage`): status, Connect / Disconnect, the tools it adds with a
/// permission toggle each, and for Computer Use the macOS permissions.
/// Installing writes the server through `mcp/install`; the backend connects
/// it and pushes `mcp/status` as it comes up. Computer Use is built in and
/// installed by default.
struct AppsView: View {
    @Environment(AppState.self) private var appState
    @State private var selectedAppId: String?
    /// The app whose connect page is showing (Connect, "+", or Update…).
    @State private var connectingAppId: String?
    /// The connect page for a server of the user's own, by URL.
    @State private var addingCustom = false
    @State private var busy: Set<String> = []
    @State private var message: SettingsMessage?

    var body: some View {
        Group {
            if addingCustom {
                CustomConnectView(taken: Set(appState.config?.mcp.servers.keys.map { $0 } ?? []).union(appState.mcpCatalog.map(\.id)), cancel: { addingCustom = false }, connect: { name, server in
                    addingCustom = false
                    addCustom(name, server)
                })
                .transition(.opacity)
            } else if let id = connectingAppId, let app = appState.mcpCatalog.first(where: { $0.id == id }) {
                ConnectView(app: app, existing: appState.config?.mcp.servers[app.id], cancel: { connectingAppId = nil }, approve: { inputs, excluded in
                    connectingAppId = nil
                    install(app, inputs: inputs, excludeTools: excluded)
                })
                .transition(.opacity)
            } else if let id = selectedAppId, let app = appState.mcpCatalog.first(where: { $0.id == id }) {
                AppPage(app: app, busy: busy.contains(app.id), message: message, back: { selectedAppId = nil }, install: { beginInstall(app) }, uninstall: { uninstall(app) }, reconnect: { reconnect(app.id) }, setEnabled: { setEnabled(app.id, $0) }, setToolEnabled: { setToolEnabled(app.id, tool: $0, enabled: $1) }, setBypass: { setBypass(app.id, $0) }, setToolPermission: { setToolPermission(app.id, tool: $0, $1) }, configure: { connectingAppId = app.id })
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            } else {
                Marketplace(busy: busy, message: message, open: { selectedAppId = $0.id }, install: { beginInstall($0) }, connectCustom: {
                    message = nil
                    addingCustom = true
                })
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.18), value: selectedAppId)
        .animation(.easeInOut(duration: 0.18), value: connectingAppId)
        .animation(.easeInOut(duration: 0.18), value: addingCustom)
        .background(Theme.canvas)
        .task(id: appState.connection.isReady) {
            guard appState.connection.isReady else { return }
            await appState.loadMcpCatalog()
            await appState.refreshMcpStatus()
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Apps")
    }

    // MARK: Actions

    /// Every connection goes through the connect page: what the app can
    /// access, its credentials, then Approve.
    private func beginInstall(_ app: McpCatalogApp) {
        message = nil
        connectingAppId = app.id
    }

    private func install(_ app: McpCatalogApp, inputs: [String: String], excludeTools: [String]) {
        selectedAppId = app.id
        run(app.id, success: "\(app.name) connected; its tools are available in every chat.") {
            try await appState.installMcpApp(id: app.id, inputs: inputs, excludeTools: excludeTools)
        }
    }

    /// A server of the user's own: written as `[mcp.servers.<name>]`, which
    /// the backend connects like any other entry.
    private func addCustom(_ name: String, _ server: McpServerConfig) {
        patch(["mcp.servers.\(name)": server.patchValue], busyId: name, success: "\(name) added; it shows under Your servers once it connects.")
    }

    private func uninstall(_ app: McpCatalogApp) {
        let alert = NSAlert()
        alert.messageText = "Disconnect \(app.name)?"
        alert.informativeText = app.builtin ? "The agent loses its tools. You can connect it again from Apps." : "Its server entry (and any key you entered for it) is deleted from ~/.context-agent/config.toml. You can connect it again from Apps."
        alert.alertStyle = .warning
        alert.addButton(withTitle: "Disconnect")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        run(app.id, success: "\(app.name) disconnected.") {
            try await appState.uninstallMcpApp(id: app.id)
        }
    }

    private func setEnabled(_ id: String, _ enabled: Bool) {
        guard var server = appState.config?.mcp.servers[id] else { return }
        server.enabled = enabled
        patch(["mcp.servers.\(id)": server.patchValue], busyId: id, success: enabled ? "\(id) is connecting…" : "\(id) is paused.")
    }

    /// Per-tool permission. The toggles work on exact names: the tools on
    /// now (as the backend reports them, groups switched off on the connect
    /// page included) with this one flipped become the entry's `tools`
    /// allowlist (empty = every tool), and the pattern exclusions are
    /// folded in and cleared.
    private func setToolEnabled(_ id: String, tool: String, enabled: Bool) {
        guard var server = appState.config?.mcp.servers[id], let status = appState.mcpServers.first(where: { $0.name == id }) else { return }
        let all = status.tools.map(\.name)
        var allowed = Set(status.tools.filter(\.enabled).map(\.name))
        if enabled { allowed.insert(tool) } else { allowed.remove(tool) }
        server.tools = all.allSatisfy(allowed.contains) ? [] : all.filter(allowed.contains)
        server.excludeTools = []
        patch(["mcp.servers.\(id)": server.patchValue], busyId: id, success: nil)
    }

    /// The app's "Bypass permissions" switch: every tool runs without
    /// asking, except one set to Always ask.
    private func setBypass(_ id: String, _ on: Bool) {
        guard var server = appState.config?.mcp.servers[id] else { return }
        server.bypassPermissions = on
        patch(["mcp.servers.\(id)": server.patchValue], busyId: id, success: nil)
    }

    /// One tool's permission from its dropdown; `nil` is Default (the
    /// server's annotations and the chat's profile decide).
    private func setToolPermission(_ id: String, tool: String, _ permission: McpToolPermission?) {
        guard var server = appState.config?.mcp.servers[id] else { return }
        server.permissions[tool] = permission
        patch(["mcp.servers.\(id)": server.patchValue], busyId: id, success: nil)
    }

    private func reconnect(_ id: String) {
        run(id, success: nil) {
            try await appState.reconnectMcp(server: id)
            if let status = appState.mcpServers.first(where: { $0.name == id }), status.state == .failed {
                throw AppsError(text: "\(id) failed: \(status.error ?? "unknown error")")
            }
        }
    }

    private func patch(_ values: [String: JSONValue], busyId: String?, success: String?) {
        run(busyId, success: success) {
            try await appState.patchConfig(values)
            await appState.refreshMcpStatus()
        }
    }

    private func run(_ busyId: String?, success: String?, _ work: @escaping () async throws -> Void) {
        guard appState.connection.isReady else { return }
        if let busyId { busy.insert(busyId) }
        message = nil
        Task {
            defer { if let busyId { busy.remove(busyId) } }
            do {
                try await work()
                if let success { message = .success(success) }
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }
}

private struct AppsError: LocalizedError {
    let text: String
    var errorDescription: String? { text }
}

// MARK: - Shared state helpers

/// How an app stands right now, from the configuration and `mcp/status`.
enum AppStanding: Equatable {
    case notInstalled, off, connecting, connected(tools: Int), failed(String), installed

    var title: String {
        switch self {
        case .notInstalled: return "Not connected"
        case .off: return "Paused"
        case .connecting: return "Connecting"
        case .connected: return "Connected"
        case .failed: return "Failed"
        case .installed: return "Connected"
        }
    }

    var isInstalled: Bool {
        if case .notInstalled = self { return false }
        return true
    }
}

extension AppState {
    func standing(of id: String) -> AppStanding {
        guard let config, let server = config.mcp.servers[id] else { return .notInstalled }
        if !server.enabled || !config.mcp.enabled { return .off }
        switch mcpServers.first(where: { $0.name == id }) {
        case let status? where status.state == .connected: return .connected(tools: status.tools.filter(\.enabled).count)
        case let status? where status.state == .connecting: return .connecting
        case let status? where status.state == .failed: return .failed(status.error ?? "unknown error")
        case let status? where status.state == .disabled: return .off
        default: return .installed
        }
    }
}

// MARK: - Marketplace

private struct Marketplace: View {
    @Environment(AppState.self) private var appState
    let busy: Set<String>
    let message: SettingsMessage?
    let open: (McpCatalogApp) -> Void
    let install: (McpCatalogApp) -> Void
    let connectCustom: () -> Void
    @State private var query = ""

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 24) {
                Text("Connect the tools your agent already uses")
                    .font(.system(size: 20, weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                    .frame(maxWidth: .infinity)
                    .accessibilityAddTraits(.isHeader)
                HStack(spacing: 10) {
                    searchField
                    Button(action: connectCustom) {
                        Label("Connect", systemImage: "plus")
                            .font(Theme.Font.secondary.weight(.medium))
                            .foregroundStyle(Color(nsColor: .windowBackgroundColor))
                            .padding(.horizontal, 14)
                            .frame(height: 32)
                            .background(Theme.textPrimary, in: Capsule())
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                    .disabled(!appState.connection.isReady)
                    .help("Connect your own MCP server by its URL")
                }
                if query.isEmpty {
                    PromptBanner(apps: Array(featured.prefix(3)), style: .marketplace)
                }
                if let message {
                    InlineMessage(message)
                }
                if !appState.connection.isReady {
                    EmptyPlaceholder(systemImage: Icons.apps, title: "Apps need the backend", subtitle: appState.connectionLabel)
                } else if appState.mcpCatalog.isEmpty {
                    HStack(spacing: 8) {
                        BrailleSpinner()
                        Text("Loading the catalog…")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                    }
                } else {
                    section(query.isEmpty ? "Featured" : "Results", apps: filtered)
                    customServers
                }
            }
            .frame(maxWidth: 720)
            .padding(.horizontal, 32)
            .padding(.top, 44)
            .padding(.bottom, 40)
            .frame(maxWidth: .infinity)
        }
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: Icons.search)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            TextField("Search apps…", text: $query)
                .textFieldStyle(.plain)
                .font(Theme.Font.secondary)
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: Icons.clear)
                        .foregroundStyle(Theme.textTertiary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 32)
        .background(Theme.surface, in: Capsule())
        .overlay(Capsule().strokeBorder(Theme.border, lineWidth: 1))
    }

    private var featured: [McpCatalogApp] {
        // The banner shows apps with a logo first, Computer Use last.
        appState.mcpCatalog.sorted { a, b in
            if a.builtin != b.builtin { return !a.builtin }
            return (a.logo != nil ? 0 : 1, a.name) < (b.logo != nil ? 0 : 1, b.name)
        }
    }

    private var filtered: [McpCatalogApp] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return appState.mcpCatalog }
        return appState.mcpCatalog.filter { [$0.name, $0.tagline, $0.category, $0.vendor, $0.description].contains { $0.lowercased().contains(q) } }
    }

    private func section(_ title: String, apps: [McpCatalogApp]) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title)
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            if apps.isEmpty {
                Text("No app matches “\(query)”.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textTertiary)
            } else {
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 24, alignment: .top), GridItem(.flexible(), spacing: 24, alignment: .top)], alignment: .leading, spacing: 18) {
                    ForEach(apps) { app in
                        MarketplaceRow(app: app, standing: appState.standing(of: app.id), busy: busy.contains(app.id), open: { open(app) }, install: { install(app) })
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var customServers: some View {
        let catalogIds = Set(appState.mcpCatalog.map(\.id))
        let custom = (appState.config?.mcp.servers ?? [:]).filter { name, server in server.app == nil && !catalogIds.contains(name) }.keys.sorted()
        if !custom.isEmpty, query.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                Text("Your servers")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Text("Added with Connect, ctx mcp add or in ~/.context-agent/config.toml.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                VStack(spacing: 0) {
                    ForEach(Array(custom.enumerated()), id: \.element) { index, name in
                        if let server = appState.config?.mcp.servers[name] {
                            HStack(spacing: 10) {
                                AppIcon(symbol: Icons.mcp, size: 32)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(name)
                                        .font(Theme.Font.secondary.weight(.medium))
                                        .foregroundStyle(Theme.textPrimary)
                                    Text(server.target)
                                        .font(Theme.Font.monoSmall)
                                        .foregroundStyle(Theme.textTertiary)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                                Spacer(minLength: 8)
                                StandingChip(standing: appState.standing(of: name))
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 10)
                            if index < custom.count - 1 { Divider() }
                        }
                    }
                }
                .card(padding: 0)
            }
        }
    }
}

/// Icon tile, name, one-line tagline, then "+" (install) or a check (installed).
private struct MarketplaceRow: View {
    let app: McpCatalogApp
    let standing: AppStanding
    let busy: Bool
    let open: () -> Void
    let install: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 12) {
            AppIcon(app: app, size: 44)
            VStack(alignment: .leading, spacing: 3) {
                Text(app.name)
                    .font(Theme.Font.secondary.weight(.medium))
                    .foregroundStyle(Theme.textPrimary)
                Text(app.tagline)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            Spacer(minLength: 8)
            if busy {
                BrailleSpinner(size: 12)
                    .frame(width: 28, height: 28)
            } else if standing.isInstalled {
                Image(systemName: "checkmark")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(standingTint)
                    .frame(width: 28, height: 28)
                    .accessibilityLabel(standing.title)
            } else {
                Button(action: install) {
                    Image(systemName: "plus")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(Theme.textSecondary)
                        .frame(width: 28, height: 28)
                        .background(Theme.textPrimary.opacity(0.06), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Connect \(app.name)")
            }
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 6)
        .contentShape(Rectangle())
        .background(hovering ? Theme.textPrimary.opacity(0.04) : .clear, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .onTapGesture(perform: open)
        .onHover { hovering = $0 }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(app.name), \(standing.title)")
        .accessibilityAddTraits(.isButton)
    }

    private var standingTint: Color {
        switch standing {
        case .connected, .installed: return Theme.textSecondary
        case .failed: return Theme.danger
        case .off, .connecting, .notInstalled: return Theme.textTertiary
        }
    }
}

// MARK: - Per-app page

private struct AppPage: View {
    @Environment(AppState.self) private var appState
    let app: McpCatalogApp
    let busy: Bool
    let message: SettingsMessage?
    let back: () -> Void
    let install: () -> Void
    let uninstall: () -> Void
    let reconnect: () -> Void
    let setEnabled: (Bool) -> Void
    let setToolEnabled: (String, Bool) -> Void
    let setBypass: (Bool) -> Void
    let setToolPermission: (String, McpToolPermission?) -> Void
    let configure: () -> Void

    var body: some View {
        let standing = appState.standing(of: app.id)
        let status = appState.mcpServers.first { $0.name == app.id }
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 24) {
                breadcrumb
                header(standing)
                PromptBanner(apps: [app], style: .app)
                if let message {
                    InlineMessage(message)
                }
                if case .failed(let error) = standing {
                    InlineMessage(.error(error))
                }
                if standing.isInstalled {
                    overview(standing, status)
                    if let status, status.state == .connected {
                        permissions(status)
                    }
                } else {
                    about
                }
                if app.builtin, app.id == "computer-use" {
                    ComputerUsePermissions()
                        .card()
                }
                if standing.isInstalled {
                    about
                }
            }
            .frame(maxWidth: 800)
            .padding(.horizontal, 32)
            .padding(.top, 28)
            .padding(.bottom, 40)
            .frame(maxWidth: .infinity)
        }
        // Esc goes back to the marketplace (a keyboard shortcut on the
        // breadcrumb button itself would stop it taking clicks).
        .onExitCommand(perform: back)
    }

    private var breadcrumb: some View {
        HStack(spacing: 8) {
            BreadcrumbButton(title: "Apps", systemImage: Icons.apps, action: back)
            Text("/")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textTertiary)
            Text(app.name)
                .font(Theme.Font.secondary.weight(.medium))
                .foregroundStyle(Theme.textPrimary)
        }
    }

    private func header(_ standing: AppStanding) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            AppIcon(app: app, size: 44)
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 8) {
                        Text(app.name)
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(Theme.textPrimary)
                        StandingChip(standing: standing)
                    }
                    Text(app.tagline + ".")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
                Spacer(minLength: 12)
                if let homepage = app.homepage, let url = URL(string: homepage) {
                    Button {
                        NSWorkspace.shared.open(url)
                    } label: {
                        HStack(spacing: 5) {
                            Text("Homepage")
                            Image(systemName: Icons.externalLink)
                                .font(.system(size: 10, weight: .semibold))
                        }
                    }
                }
                if busy {
                    BrailleSpinner()
                } else if standing.isInstalled {
                    Button("Disconnect", role: .destructive, action: uninstall)
                        .tint(Theme.danger)
                        .buttonStyle(.borderedProminent)
                } else {
                    Button("Connect", action: install)
                        .buttonStyle(.borderedProminent)
                        .disabled(!appState.connection.isReady)
                }
            }
        }
    }

    /// Like the sync card: counts, then the status line with Reconnect.
    private func overview(_ standing: AppStanding, _ status: McpServerStatus?) -> some View {
        let tools = status?.tools ?? []
        let enabled = tools.filter(\.enabled)
        let readOnly = enabled.filter(\.readOnly).count
        return VStack(alignment: .leading, spacing: 10) {
            Text("Tools")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            VStack(spacing: 0) {
                countRow(icon: Icons.mcp, label: "Tools available", value: tools.isEmpty ? "—" : "\(enabled.count)")
                Divider()
                countRow(icon: Icons.check, label: "Run without asking", value: tools.isEmpty ? "—" : "\(readOnly)")
                Divider()
                HStack(spacing: 8) {
                    Text("Status")
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    Text(statusLine(standing, status))
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                    Spacer(minLength: 8)
                    Toggle("Enabled", isOn: Binding(get: { appState.config?.mcp.servers[app.id]?.enabled ?? false }, set: setEnabled))
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .disabled(busy)
                    Button("Reconnect", action: reconnect)
                        .controlSize(.small)
                        .disabled(busy || standing == .off)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(Theme.canvas.opacity(0.6))
                if !app.inputs.isEmpty {
                    Divider()
                    HStack(spacing: 8) {
                        Text("Keys")
                            .font(Theme.Font.secondary.weight(.medium))
                            .foregroundStyle(Theme.textPrimary)
                        Text(app.inputs.map(\.label).joined(separator: ", "))
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                        Spacer(minLength: 8)
                        Button("Update…", action: configure)
                            .controlSize(.small)
                            .disabled(busy)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(Theme.canvas.opacity(0.6))
                }
            }
            .card(padding: 0)
        }
    }

    private func countRow(icon: String, label: String, value: String) -> some View {
        HStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .frame(width: 16)
                Text(label)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
            .frame(width: 220, alignment: .leading)
            .padding(.horizontal, 16)
            Divider()
            Text(value)
                .font(Theme.Font.secondary.weight(.medium))
                .foregroundStyle(Theme.textPrimary)
                .padding(.horizontal, 16)
            Spacer(minLength: 0)
        }
        .frame(height: 44)
    }

    private func statusLine(_ standing: AppStanding, _ status: McpServerStatus?) -> String {
        switch standing {
        case .connected:
            let server = [status?.serverName, status?.serverVersion].compactMap { $0 }.joined(separator: " ")
            return server.isEmpty ? "Connected" : "Connected • \(server)"
        case .connecting: return "Connecting…"
        case .failed: return "Failed"
        case .off: return "Paused • turn it on to connect"
        case .installed: return "Not reported by the backend yet"
        case .notInstalled: return "Not connected"
        }
    }

    /// One row per tool with its permission toggle (the entry's allowlist).
    private func permissions(_ status: McpServerStatus) -> some View {
        let server = appState.config?.mcp.servers[app.id]
        let bypass = server?.bypassPermissions ?? false
        return VStack(alignment: .leading, spacing: 10) {
            Text("Tool permissions")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            VStack(spacing: 0) {
                HStack(spacing: 12) {
                    Image(systemName: "bolt.shield")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(bypass ? Theme.warning : Theme.textSecondary)
                        .frame(width: 26, height: 26)
                        .background(bypass ? Theme.warning.opacity(0.12) : Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Bypass permissions")
                            .font(Theme.Font.secondary.weight(.medium))
                            .foregroundStyle(Theme.textPrimary)
                        Text(bypass ? "Every \(app.name) tool runs without asking, except tools set to Always ask." : "Run every \(app.name) tool without asking, destructive ones included.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 8)
                    Toggle("Bypass permissions", isOn: Binding(get: { bypass }, set: { setBypass($0) }))
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .disabled(busy || server == nil)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                if !status.tools.isEmpty { Divider() }
                if status.tools.isEmpty {
                    Text("The server offers no tools.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(16)
                } else {
                    ForEach(Array(status.tools.enumerated()), id: \.element.id) { index, tool in
                        HStack(alignment: .top, spacing: 12) {
                            VStack(alignment: .leading, spacing: 3) {
                                HStack(spacing: 6) {
                                    Text(tool.name.replacingOccurrences(of: "_", with: " ").capitalizedFirst)
                                        .font(Theme.Font.secondary.weight(.medium))
                                        .foregroundStyle(Theme.textPrimary)
                                    ToolPermissionButton(
                                        tool: tool,
                                        override: server?.permissions[tool.name],
                                        bypass: bypass,
                                        enabled: !busy && server != nil
                                    ) { setToolPermission(tool.name, $0) }
                                }
                                Text(tool.description.isEmpty ? tool.qualifiedName : tool.description)
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.textSecondary)
                                    .lineLimit(2)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: 8)
                            Toggle(tool.name, isOn: Binding(get: { tool.enabled }, set: { setToolEnabled(tool.name, $0) }))
                                .labelsHidden()
                                .toggleStyle(.switch)
                                .controlSize(.small)
                                .disabled(busy)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                        if index < status.tools.count - 1 { Divider().padding(.leading, 16) }
                    }
                }
            }
            .card(padding: 0)
            Text("Off takes the tool out of the model's tool list for this app. Click a tool's permission icon to change when it asks. By default read-only tools run without asking under every profile, other tools ask under Ask, and destructive ones ask under Ask and Auto. The chat's Bypass profile never asks.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var about: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("About")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            VStack(alignment: .leading, spacing: 10) {
                Text(app.description)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 16) {
                    labeled("By", app.vendor)
                    labeled("Category", app.category)
                    if !app.requirements.isEmpty {
                        labeled("Requires", app.requirements.joined(separator: ", "))
                    }
                    if !app.inputs.isEmpty {
                        labeled("Needs", app.inputs.map { $0.label + ($0.required ? "" : " (optional)") }.joined(separator: ", "))
                    }
                }
            }
            .card()
        }
    }

    private func labeled(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
            Text(value)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textPrimary)
        }
    }
}

/// "⊞ Apps" in the breadcrumb: a plain button with a full hit area and a
/// hover tint, so the whole label takes the click.
private struct BreadcrumbButton: View {
    let title: String
    let systemImage: String
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: systemImage)
                    .font(.system(size: 12, weight: .medium))
                Text(title)
            }
            .font(Theme.Font.secondary)
            .foregroundStyle(hovering ? Theme.textPrimary : Theme.textSecondary)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(hovering ? Theme.textPrimary.opacity(0.06) : .clear, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .accessibilityLabel("Back to \(title)")
    }
}

private extension String {
    var capitalizedFirst: String {
        guard let first = first else { return self }
        return first.uppercased() + dropFirst()
    }
}

// MARK: - Pieces

/// "Connected", "Connecting", "Failed", "Paused", "Not connected".
private struct StandingChip: View {
    let standing: AppStanding

    var body: some View {
        switch standing {
        case .connected, .installed:
            Chip("Connected", style: .tinted(Theme.success))
        case .connecting:
            HStack(spacing: 4) {
                BrailleSpinner(size: 11)
                Text("Connecting")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
            }
        case .failed:
            Chip("Failed", systemImage: Icons.warning, style: .tinted(Theme.danger))
        case .off:
            Chip("Paused")
        case .notInstalled:
            Chip("Not connected")
        }
    }
}

/// The banner with example prompts, each a pill: logo + name chip, then
/// the prompt, over a photographic background from the asset catalog
/// (`banner-marketplace` on the marketplace; on an app's page, one of the
/// `banner-app-*` skies picked to suit the app).
/// Marketplace: three stacked, offset like a conversation; app page: one.
private struct PromptBanner: View {
    enum Style { case marketplace, app }
    let apps: [McpCatalogApp]
    let style: Style

    var body: some View {
        VStack(spacing: 10) {
            ForEach(Array(apps.enumerated()), id: \.element.id) { index, app in
                pill(app)
                    .offset(x: style == .marketplace ? CGFloat([-12, 10, -4][index % 3]) : 0)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, style == .marketplace ? 32 : 28)
        .background {
            let shape = RoundedRectangle(cornerRadius: 14, style: .continuous)
            if NSImage(named: imageName) != nil {
                // The container takes the banner's size; the image fills and
                // overflows it, and the container is what gets rounded.
                // The filled image overflows the banner; a clip hides the
                // overflow but does not stop it taking clicks meant for the
                // views above and below, so the background takes none.
                Color.clear
                    .overlay {
                        Image(imageName)
                            .resizable()
                            .interpolation(.high)
                            .scaledToFill()
                    }
                    .clipShape(shape)
                    .overlay(shape.strokeBorder(.black.opacity(0.06), lineWidth: 1))
                    .allowsHitTesting(false)
            } else {
                shape.fill(gradient)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Example prompts")
    }

    private var imageName: String {
        switch style {
        case .marketplace: return "banner-marketplace"
        case .app: return Self.appBanner(for: apps.first?.id ?? "")
        }
    }

    /// The sky behind an app's banner: chosen per catalog app, and for any
    /// other id a stable pick so the same app keeps the same one.
    static func appBanner(for id: String) -> String {
        let skies = ["dusk", "pastel", "ember", "forest"]
        let chosen: [String: String] = [
            "computer-use": "ember",
            "context7": "forest",
            "fetch": "pastel",
            "github": "dusk",
            "memory": "dusk",
            "playwright": "forest",
            "sequential-thinking": "pastel",
        ]
        let sky = chosen[id] ?? skies[id.unicodeScalars.reduce(0) { $0 + Int($1.value) } % skies.count]
        return "banner-app-\(sky)"
    }

    private func pill(_ app: McpCatalogApp) -> some View {
        HStack(spacing: 8) {
            HStack(spacing: 5) {
                AppIcon(app: app, size: 16, bare: true)
                Text(app.name)
                    .font(Theme.Font.caption.weight(.medium))
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Theme.accentSoft, in: Capsule())
            .foregroundStyle(Theme.accent)
            Text(app.examplePrompt.isEmpty ? app.tagline : app.examplePrompt)
                .font(Theme.Font.secondary)
                .foregroundStyle(Color(nsColor: .labelColor))
                .lineLimit(1)
        }
        .padding(.leading, 6)
        .padding(.trailing, 14)
        .padding(.vertical, 6)
        .background(Color(nsColor: .windowBackgroundColor), in: Capsule())
        .shadow(color: .black.opacity(0.08), radius: 6, y: 2)
    }

    private var gradient: some ShapeStyle {
        switch style {
        case .marketplace:
            return AnyShapeStyle(LinearGradient(colors: [Color(red: 0.38, green: 0.47, blue: 0.95), Color(red: 0.72, green: 0.52, blue: 0.96), Color(red: 0.96, green: 0.70, blue: 0.86), Color(red: 0.55, green: 0.80, blue: 0.98)], startPoint: .topLeading, endPoint: .bottomTrailing))
        case .app:
            return AnyShapeStyle(LinearGradient(colors: [Color(red: 0.86, green: 0.92, blue: 0.80), Color(red: 0.80, green: 0.90, blue: 0.86), Color(red: 0.75, green: 0.86, blue: 0.84)], startPoint: .topLeading, endPoint: .bottomTrailing))
        }
    }
}

/// An app's tile: its brand logo from the asset catalog (`logo-<name>`,
/// tinted when it is a single-colour mark) when the catalog names one and
/// the asset exists, else its SF Symbol on the accent tile. `bare` draws
/// the mark alone (inside a chip). Shared with the composer's `@` menu.
struct AppIcon: View {
    let symbol: String
    var logo: String? = nil
    var monochrome = false
    let size: CGFloat
    var bare = false

    init(symbol: String, logo: String? = nil, monochrome: Bool = false, size: CGFloat, bare: Bool = false) {
        self.symbol = symbol
        self.logo = logo
        self.monochrome = monochrome
        self.size = size
        self.bare = bare
    }

    init(app: McpCatalogApp, size: CGFloat, bare: Bool = false) {
        self.init(symbol: app.icon, logo: app.logo, monochrome: app.logoMonochrome, size: size, bare: bare)
    }

    var body: some View {
        Group {
            if let name = logo.map({ "logo-\($0)" }), NSImage(named: name) != nil {
                let mark = Image(name)
                    .renderingMode(monochrome ? .template : .original)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                if bare {
                    mark
                        .foregroundStyle(Theme.accent)
                        .frame(width: size, height: size)
                } else {
                    mark
                        .foregroundStyle(Theme.textPrimary)
                        .padding(size * 0.22)
                        .frame(width: size, height: size)
                        .background(Theme.surface, in: RoundedRectangle(cornerRadius: size * 0.27, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: size * 0.27, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
                }
            } else if bare {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.75, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: size, height: size)
            } else {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.45, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: size, height: size)
                    .background(Theme.surface, in: RoundedRectangle(cornerRadius: size * 0.27, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: size * 0.27, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
            }
        }
        .accessibilityHidden(true)
    }
}

// MARK: - Computer Use permissions

/// Screen Recording and Accessibility for Computer Use. Two answers matter:
/// this app's (macOS attributes the server's permissions to Context.app,
/// which launched the backend) and the backend's (`mcp/permissions`: what
/// the process that will act actually has). Both are re-read every two
/// seconds while the card is visible and whenever the app comes to the
/// front, so a grant shows up without a relaunch. When the app has a
/// permission the backend lacks, the backend started before the grant
/// (Screen Recording applies to new processes only) and a restart fixes
/// it; when System Settings lists Context but neither has it, a rebuilt
/// app has gone stale in TCC and "Reset" clears the entry for a fresh grant.
private struct ComputerUsePermissions: View {
    @Environment(AppState.self) private var appState
    @State private var screen = CGPreflightScreenCaptureAccess()
    @State private var input = AXIsProcessTrusted()
    @State private var backend: McpPermissions?
    @State private var working = false
    @State private var note: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("macOS permissions for Context")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 0)
                Button {
                    Task { await refresh() }
                } label: {
                    Image(systemName: Icons.refresh)
                }
                .buttonStyle(.plain)
                .foregroundStyle(Theme.textTertiary)
                .help("Check again")
                .accessibilityLabel("Check permissions again")
            }
            permissionRow(.screenRecording)
            permissionRow(.accessibility)
            if let note {
                InlineMessage(.info(note))
            }
            Text("Checked live, in Context and in the running backend. Screen Recording only applies to processes started after you grant it, so the backend may need a restart; a rebuilt Context can be listed in System Settings yet no longer honoured, which Reset clears so macOS asks again.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .task {
            // Live: TCC grants arrive while the user is in System Settings.
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await refresh() }
        }
    }

    private enum Permission {
        case screenRecording, accessibility

        var title: String {
            switch self {
            case .screenRecording: return "Screen Recording"
            case .accessibility: return "Accessibility"
            }
        }

        var detail: String {
            switch self {
            case .screenRecording: return "Needed for screenshots."
            case .accessibility: return "Needed to move the mouse and type."
            }
        }

        /// The System Settings pane and the `tccutil` service.
        var pane: String {
            switch self {
            case .screenRecording: return "Privacy_ScreenCapture"
            case .accessibility: return "Privacy_Accessibility"
            }
        }

        var service: String {
            switch self {
            case .screenRecording: return "ScreenCapture"
            case .accessibility: return "Accessibility"
            }
        }
    }

    private func appHas(_ permission: Permission) -> Bool {
        switch permission {
        case .screenRecording: return screen
        case .accessibility: return input
        }
    }

    private func backendHas(_ permission: Permission) -> Bool? {
        guard let backend else { return nil }
        switch permission {
        case .screenRecording: return backend.screenRecording
        case .accessibility: return backend.accessibility
        }
    }

    @ViewBuilder
    private func permissionRow(_ permission: Permission) -> some View {
        let app = appHas(permission)
        let backend = backendHas(permission)
        let granted = app && (backend ?? true)
        HStack(spacing: 8) {
            Image(systemName: granted ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(granted ? Theme.success : Theme.warning)
                .frame(width: 16)
            VStack(alignment: .leading, spacing: 1) {
                Text(permission.title)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                Text(app && backend == false ? "Granted to Context; the backend started before that." : permission.detail)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            Spacer(minLength: 0)
            if working {
                BrailleSpinner(size: 11)
            } else if granted {
                Chip("Granted", systemImage: "checkmark", style: .tinted(Theme.success))
            } else if app && backend == false {
                Button("Restart backend") { restartBackend() }
                    .controlSize(.small)
                    .disabled(appState.anyProjectRunning || appState.backendStarting)
                    .help(appState.anyProjectRunning ? "Finishes after the running task" : "Restart ctx so it picks up the permission")
            } else {
                Button("Grant…") { request(permission) }
                    .controlSize(.small)
                Menu {
                    Button("Open System Settings") { openPane(permission) }
                    Button("Reset and ask again") { reset(permission) }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .frame(width: 22)
                .accessibilityLabel("More options for \(permission.title)")
            }
        }
    }

    private func refresh() async {
        screen = CGPreflightScreenCaptureAccess()
        input = AXIsProcessTrusted()
        if appState.connection.isReady, let answer = try? await appState.backend.mcpPermissions() {
            backend = answer
        } else {
            backend = nil
        }
    }

    private func request(_ permission: Permission) {
        note = nil
        switch permission {
        case .screenRecording:
            CGRequestScreenCaptureAccess()
        case .accessibility:
            let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
        }
        openPane(permission)
    }

    private func openPane(_ permission: Permission) {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(permission.pane)") {
            NSWorkspace.shared.open(url)
        }
    }

    /// `tccutil reset <service> <bundle id>`: drops the stale entry a rebuilt
    /// (re-signed) app leaves behind, then asks again.
    private func reset(_ permission: Permission) {
        guard let bundleId = Bundle.main.bundleIdentifier else {
            note = "This build has no bundle identifier; run `tccutil reset \(permission.service)` in Terminal instead."
            return
        }
        working = true
        note = nil
        Task.detached(priority: .userInitiated) {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
            process.arguments = ["reset", permission.service, bundleId]
            let pipe = Pipe()
            process.standardError = pipe
            process.standardOutput = pipe
            var failure: String?
            do {
                try process.run()
                process.waitUntilExit()
                if process.terminationStatus != 0 {
                    let text = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                    failure = text.isEmpty ? "tccutil exited with \(process.terminationStatus)" : text
                }
            } catch {
                failure = error.localizedDescription
            }
            await MainActor.run {
                working = false
                if let failure {
                    note = "Could not reset \(permission.title): \(failure). Remove Context from the list in System Settings and add it again."
                } else {
                    note = "\(permission.title) was reset for Context; macOS will ask again."
                    request(permission)
                }
            }
        }
    }

    private func restartBackend() {
        working = true
        note = nil
        Task {
            await appState.restartBackend()
            await refresh()
            working = false
        }
    }
}

// MARK: - Connect page

/// The connect page (Connect, "+" or Update…), in place of a modal: the
/// Context mark and the app's, "Connect Context to GitHub", where the app
/// runs or the credentials it needs, "What Context can access" with a
/// checkbox per group of tools, then Cancel and Approve. Groups left
/// unchecked go to the entry's `exclude_tools`; required groups stay on.
private struct ConnectView: View {
    @Environment(AppState.self) private var appState
    let app: McpCatalogApp
    /// The entry when the app is installed already (Update…).
    let existing: McpServerConfig?
    let cancel: () -> Void
    let approve: (_ inputs: [String: String], _ excludeTools: [String]) -> Void

    @State private var values: [String: String] = [:]
    @State private var granted: Set<String> = []
    @State private var problem: String?
    @State private var auth: SignIn = .checking
    /// "Use a personal access token instead".
    @State private var useToken = false

    /// Where the device-flow sign-in stands.
    enum SignIn: Equatable {
        case checking, signedOut, starting, waiting(McpDeviceCode), signedIn(String), failed(String)
    }

    /// The app signs in instead of taking its token input.
    private var usesSignIn: Bool { app.oauth?.available == true && !useToken }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView(.vertical) {
                VStack(spacing: 24) {
                    header
                    if app.inputs.isEmpty {
                        whereItRuns
                    } else if usesSignIn {
                        signInCard
                    } else {
                        credentials
                    }
                    accessList
                    if let problem {
                        InlineMessage(.error(problem))
                    }
                }
                .frame(maxWidth: 460)
                .padding(.horizontal, 24)
                .padding(.top, 48)
                .padding(.bottom, 24)
                .frame(maxWidth: .infinity)
            }
            VStack(spacing: 10) {
                Button(action: cancel) {
                    Text("Cancel")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(ConnectButtonStyle(prominent: false))
                .keyboardShortcut(.cancelAction)
                Button(action: submit) {
                    Text(existing == nil ? "Approve" : "Save")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(ConnectButtonStyle(prominent: true))
                .keyboardShortcut(.defaultAction)
                .disabled(!appState.connection.isReady || (usesSignIn && !isSignedIn))
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 24)
            .padding(.top, 12)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
        }
        .onAppear(perform: seed)
        .task { await loadAccount() }
        .task(id: waitingCode) { await pollWhileWaiting() }
        .onExitCommand(perform: cancel)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Connect \(app.name)")
    }

    // MARK: Header

    private var header: some View {
        VStack(spacing: 12) {
            HStack(spacing: -8) {
                Icons.LogoView(size: 20)
                    .frame(width: 40, height: 40)
                    .background(Theme.surface, in: Circle())
                    .overlay(Circle().strokeBorder(Theme.border, lineWidth: 1))
                AppIcon(app: app, size: 24, bare: true)
                    .frame(width: 40, height: 40)
                    .background(Theme.surface, in: Circle())
                    .overlay(Circle().strokeBorder(Theme.border, lineWidth: 1))
            }
            .accessibilityHidden(true)
            VStack(spacing: 6) {
                Text(existing == nil ? "Connect Context to \(app.name)" : "Update \(app.name)")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Text(subtitle)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var subtitle: String {
        if app.builtin { return "Let the agent use \(app.name) on this Mac." }
        if app.inputs.contains(where: \.required) { return "Grant Context access to your \(app.name) account." }
        return "Let the agent use \(app.name) in your chats."
    }

    // MARK: Where it runs / credentials

    /// The selector row of the design: what the agent connects to.
    private var whereItRuns: some View {
        HStack(spacing: 10) {
            Image(systemName: app.builtin ? "laptopcomputer" : "shippingbox")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 1) {
                Text(app.builtin ? "This Mac" : "Runs on this Mac")
                    .font(Theme.Font.secondary.weight(.medium))
                    .foregroundStyle(Theme.textPrimary)
                Text(app.builtin ? "Built into Context; nothing to install" : "Started with " + app.requirements.joined(separator: ", "))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
    }

    private var credentials: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(app.inputs) { input in
                VStack(alignment: .leading, spacing: 5) {
                    Text(input.label + (input.required ? "" : " (optional)"))
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    Group {
                        if input.secret {
                            SecureField(existing == nil ? input.key : "Leave empty to keep the current value", text: binding(input.key))
                        } else {
                            TextField(input.key, text: binding(input.key))
                        }
                    }
                    .textFieldStyle(.plain)
                    .font(Theme.Font.secondary)
                    .padding(.horizontal, 10)
                    .frame(height: 32)
                    .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
                    Text(input.help)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Text("Saved to ~/.context-agent/config.toml (readable only by you). Enter ${VAR} to read an environment variable instead.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            if app.oauth?.available == true {
                Button("Sign in with \(app.name) instead") { useToken = false }
                    .buttonStyle(.link)
                    .font(Theme.Font.caption)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
    }

    // MARK: Sign-in

    /// The account row of the design: who Context acts as. Signed out, a
    /// "Sign in with GitHub" button; waiting, the code to enter on GitHub.
    @ViewBuilder
    private var signInCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch auth {
            case .checking:
                HStack(spacing: 8) {
                    BrailleSpinner(size: 11)
                    Text("Checking your \(app.name) sign-in…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
            case .signedIn(let account):
                HStack(spacing: 10) {
                    AppIcon(app: app, size: 18, bare: true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(account)
                            .font(Theme.Font.secondary.weight(.medium))
                            .foregroundStyle(Theme.textPrimary)
                        Text("Signed in to \(app.name)")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                    Spacer(minLength: 0)
                    Button("Switch account") { switchAccount() }
                        .controlSize(.small)
                }
            case .waiting(let code):
                VStack(alignment: .leading, spacing: 10) {
                    Text("Enter this code on GitHub")
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    HStack(spacing: 10) {
                        Text(code.userCode)
                            .font(.system(size: 22, weight: .semibold, design: .monospaced))
                            .foregroundStyle(Theme.textPrimary)
                            .textSelection(.enabled)
                        Spacer(minLength: 0)
                        Button("Copy code and open GitHub") { openVerification(code) }
                            .controlSize(.small)
                    }
                    HStack(spacing: 6) {
                        BrailleSpinner(size: 11)
                        Text("Waiting for you to approve on github.com…")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textSecondary)
                        Spacer(minLength: 0)
                        Button("Cancel") { auth = .signedOut }
                            .buttonStyle(.link)
                            .font(Theme.Font.caption)
                    }
                }
            case .signedOut, .starting, .failed:
                HStack(spacing: 10) {
                    AppIcon(app: app, size: 18, bare: true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text("Sign in with \(app.name)")
                            .font(Theme.Font.secondary.weight(.medium))
                            .foregroundStyle(Theme.textPrimary)
                        Text("Approve Context in your browser; no token to copy.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                    Spacer(minLength: 0)
                    if auth == .starting {
                        BrailleSpinner(size: 11)
                    } else {
                        Button("Sign in…") { startSignIn() }
                            .controlSize(.small)
                            .disabled(!appState.connection.isReady)
                    }
                }
                if case .failed(let message) = auth {
                    InlineMessage(.error(message))
                }
            }
            if !isSignedIn, auth != .checking {
                Button("Use a personal access token instead") { useToken = true }
                    .buttonStyle(.link)
                    .font(Theme.Font.caption)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
    }

    private var isSignedIn: Bool {
        if case .signedIn = auth { return true }
        return false
    }

    private var waitingCode: String? {
        if case .waiting(let code) = auth { return code.deviceCode }
        return nil
    }

    private func loadAccount() async {
        guard let oauth = app.oauth, oauth.available else { return }
        // An entry with a pasted token keeps the token field.
        if let existing, let value = existing.env[oauth.input] ?? existing.headers[oauth.input], !value.isEmpty, value != oauth.reference {
            useToken = true
        }
        let account = try? await appState.backend.mcpOAuthAccount(app: app.id)
        if auth == .checking {
            auth = account.map { .signedIn($0) } ?? .signedOut
        }
    }

    private func startSignIn() {
        problem = nil
        auth = .starting
        Task {
            do {
                let code = try await appState.backend.mcpOAuthStart(app: app.id)
                auth = .waiting(code)
                openVerification(code)
            } catch {
                auth = .failed(error.localizedDescription)
            }
        }
    }

    private func openVerification(_ code: McpDeviceCode) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(code.userCode, forType: .string)
        if let url = URL(string: code.verificationUri) {
            NSWorkspace.shared.open(url)
        }
    }

    /// Polls at GitHub's interval until the code is approved, declined or
    /// expires; cancelled when the view leaves or the code changes.
    private func pollWhileWaiting() async {
        guard case .waiting(let code) = auth else { return }
        var interval = max(code.interval, 5)
        let deadline = Date().addingTimeInterval(TimeInterval(max(code.expiresIn, 60)))
        while !Task.isCancelled {
            try? await Task.sleep(nanoseconds: UInt64(interval) * 1_000_000_000)
            guard !Task.isCancelled, case .waiting(let current) = auth, current == code else { return }
            if Date() > deadline {
                auth = .failed("The code expired. Sign in again.")
                return
            }
            do {
                let poll = try await appState.backend.mcpOAuthPoll(app: app.id, deviceCode: code.deviceCode)
                switch poll.status {
                case "approved":
                    auth = .signedIn(poll.account ?? app.name)
                    NSApp.activate()
                    return
                case "denied":
                    auth = .failed("The sign-in was declined on GitHub.")
                    return
                case "expired":
                    auth = .failed("The code expired. Sign in again.")
                    return
                default:
                    interval = max(poll.interval ?? interval, 5)
                }
            } catch {
                auth = .failed(error.localizedDescription)
                return
            }
        }
    }

    private func switchAccount() {
        Task {
            try? await appState.backend.mcpOAuthSignOut(app: app.id)
            startSignIn()
        }
    }

    // MARK: Access

    private var accessList: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("What Context can access")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .padding(.horizontal, 4)
            VStack(alignment: .leading, spacing: 16) {
                ForEach(app.access) { group in
                    AccessRow(group: group, isOn: Binding(
                        get: { group.required || granted.contains(group.id) },
                        set: { on in
                            if on { granted.insert(group.id) } else { granted.remove(group.id) }
                        }
                    ))
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.textPrimary.opacity(0.03), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .padding(6)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
            Text("You can change this later from the app's page.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .padding(.horizontal, 4)
        }
    }

    // MARK: State

    /// Every group on for a new connection; for Update…, the groups whose
    /// tools the entry does not exclude.
    private func seed() {
        let excluded = Set(existing?.excludeTools ?? [])
        granted = Set(app.access.filter { group in group.required || !group.tools.allSatisfy(excluded.contains) }.map(\.id))
    }

    private func binding(_ key: String) -> Binding<String> {
        Binding(get: { values[key] ?? "" }, set: { values[key] = $0 })
    }

    private func submit() {
        var inputs = values.filter { !$0.value.trimmingCharacters(in: .whitespaces).isEmpty }
        if usesSignIn, let oauth = app.oauth {
            guard isSignedIn else {
                problem = "Sign in with \(app.name) first."
                return
            }
            // The server reads the stored token; config.toml only names it.
            inputs[oauth.input] = oauth.reference
        }
        // Update…: an empty field keeps what is stored.
        if let existing {
            for input in app.inputs where inputs[input.key] == nil {
                if let stored = input.kind == "header" ? existing.headers[input.key] : existing.env[input.key] {
                    inputs[input.key] = stored
                }
            }
        }
        if let missing = app.inputs.first(where: { $0.required && (inputs[$0.key] ?? "").isEmpty }) {
            problem = "\(missing.label) is required."
            return
        }
        let optional = app.access.filter { !$0.required }
        if !optional.isEmpty, optional.allSatisfy({ !granted.contains($0.id) }), !app.access.contains(where: \.required) {
            problem = "Choose at least one thing \(app.name) can do."
            return
        }
        let excluded = app.access.filter { !$0.required && !granted.contains($0.id) }.flatMap(\.tools)
        approve(inputs, excluded)
    }
}

/// A checkbox, a title and a one-line description.
private struct AccessRow: View {
    let group: McpCatalogAccess
    @Binding var isOn: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Toggle(group.title, isOn: $isOn)
                .toggleStyle(.checkbox)
                .labelsHidden()
                .disabled(group.required)
                .alignmentGuide(.firstTextBaseline) { d in d[.bottom] - 3 }
            VStack(alignment: .leading, spacing: 3) {
                Text(group.title)
                    .font(Theme.Font.secondary.weight(.medium))
                    .foregroundStyle(Theme.textPrimary)
                Text(group.description + (group.required ? " (always on)" : ""))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .onTapGesture {
            if !group.required { isOn.toggle() }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityValue(isOn ? "On" : "Off")
    }
}

/// Full-width buttons of the connect page: outlined Cancel, solid Approve.
// MARK: - Tool permission

/// A tool's permission icon on the app page: what happens when the agent
/// calls it (its own setting, else the app's bypass, else the server's
/// annotations). Clicking opens `ToolPermissionPopover`.
private struct ToolPermissionButton: View {
    let tool: McpToolInfo
    let override: McpToolPermission?
    let bypass: Bool
    let enabled: Bool
    let select: (McpToolPermission?) -> Void
    @State private var presented = false

    var body: some View {
        let look = ToolPermissionLook.current(tool: tool, override: override, bypass: bypass)
        Button {
            presented = true
        } label: {
            HStack(spacing: 3) {
                Image(systemName: look.systemImage)
                    .font(.system(size: 12))
                    .foregroundStyle(look.tint)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 7, weight: .bold))
                    .foregroundStyle(Theme.textTertiary)
            }
            .padding(.horizontal, 5)
            .frame(height: 20)
            .contentShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
        }
        .buttonStyle(.plain)
        .hoverHighlight(radius: 5)
        .disabled(!enabled)
        .help("\(look.title): \(look.detail) · click to change")
        .accessibilityLabel("Permission for \(tool.name): \(look.title)")
        .popover(isPresented: $presented, arrowEdge: .bottom) {
            ToolPermissionPopover(tool: tool, override: override, bypass: bypass) { choice in
                presented = false
                if choice != override { select(choice) }
            }
        }
    }
}

/// Icon, tint and wording for one permission state.
private struct ToolPermissionLook {
    let title: String
    let detail: String
    let systemImage: String
    let tint: Color

    static let allow = ToolPermissionLook(title: "Always allow", detail: "Runs without asking", systemImage: "checkmark.circle.fill", tint: Theme.success)
    static let ask = ToolPermissionLook(title: "Always ask", detail: "Asks every time, under every profile but Bypass", systemImage: Icons.approval, tint: Theme.accent)

    /// What the server's annotations and the chat's profile decide.
    static func annotated(_ tool: McpToolInfo) -> ToolPermissionLook {
        if tool.readOnly {
            return ToolPermissionLook(title: "Default", detail: "Read-only: runs without asking", systemImage: "checkmark.circle", tint: Theme.success)
        } else if tool.destructive {
            return ToolPermissionLook(title: "Default", detail: "Destructive: asks under Ask and Auto", systemImage: "exclamationmark.triangle.fill", tint: Theme.danger)
        }
        return ToolPermissionLook(title: "Default", detail: "Asks under the Ask profile", systemImage: Icons.approval, tint: Theme.textTertiary)
    }

    static func current(tool: McpToolInfo, override: McpToolPermission?, bypass: Bool) -> ToolPermissionLook {
        switch override {
        case .allow: return .allow
        case .ask: return .ask
        case nil:
            if bypass {
                return ToolPermissionLook(title: "Bypassed", detail: "Runs without asking (the app bypasses permissions)", systemImage: "bolt.circle.fill", tint: Theme.warning)
            }
            return annotated(tool)
        }
    }
}

/// The dropdown: Default (described for this tool), Always allow, Always
/// ask, the current one checked. Arrow keys and Return pick, like the
/// composer's permissions popover.
private struct ToolPermissionPopover: View {
    let tool: McpToolInfo
    let override: McpToolPermission?
    let bypass: Bool
    let choose: (McpToolPermission?) -> Void
    @State private var highlighted: Int?
    @FocusState private var focused: Bool

    private var options: [(value: McpToolPermission?, look: ToolPermissionLook)] {
        let annotated = ToolPermissionLook.annotated(tool)
        let fallback = bypass
            ? ToolPermissionLook(title: "Default", detail: "Runs without asking while the app bypasses permissions", systemImage: "bolt.circle.fill", tint: Theme.warning)
            : annotated
        return [(nil, fallback), (.allow, .allow), (.ask, .ask)]
    }

    var body: some View {
        let options = options
        VStack(alignment: .leading, spacing: 2) {
            Text(tool.name)
                .font(Theme.Font.caption.weight(.medium))
                .foregroundStyle(Theme.textTertiary)
                .lineLimit(1)
                .padding(.horizontal, 8)
                .padding(.top, 4)
                .padding(.bottom, 2)
            ForEach(Array(options.enumerated()), id: \.offset) { index, option in
                row(option.look, isCurrent: option.value == override, isHighlighted: highlighted == index) {
                    choose(option.value)
                }
                .onHover { inside in
                    if inside { highlighted = index }
                }
            }
        }
        .padding(6)
        .frame(width: 260)
        .background(Theme.surface)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onKeyPress(.upArrow) {
            highlighted = max((highlighted ?? options.count) - 1, 0)
            return .handled
        }
        .onKeyPress(.downArrow) {
            highlighted = min((highlighted ?? -1) + 1, options.count - 1)
            return .handled
        }
        .onKeyPress(.return) {
            if let highlighted, options.indices.contains(highlighted) { choose(options[highlighted].value) }
            return .handled
        }
        .task {
            highlighted = options.firstIndex { $0.value == override }
            // Popover content is attached a beat after creation; focusing then sticks.
            try? await Task.sleep(for: .milliseconds(60))
            focused = true
        }
    }

    private func row(_ look: ToolPermissionLook, isCurrent: Bool, isHighlighted: Bool, action: @escaping () -> Void) -> some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return Button(action: action) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: look.systemImage)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(look.tint)
                    .frame(width: 26, height: 26)
                    .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(look.title)
                        .font(.system(size: 13, weight: isCurrent ? .semibold : .medium))
                        .foregroundStyle(Theme.textPrimary)
                    Text(look.detail)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                Image(systemName: "checkmark")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(Theme.accent)
                    .opacity(isCurrent ? 1 : 0)
                    .frame(width: 14)
                    .padding(.top, 6)
                    .accessibilityHidden(true)
            }
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(isHighlighted ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(look.title): \(look.detail)")
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
    }
}

// MARK: - Custom connect page

/// The connect page for a server of the user's own (Connect beside the
/// search field): its MCP URL, a name (from the URL's host until edited)
/// and an optional header for a key, then Cancel and Connect. The entry is
/// an http `[mcp.servers.<name>]` table, the same as `ctx mcp add --url`.
private struct CustomConnectView: View {
    @Environment(AppState.self) private var appState
    /// Names already used by servers or catalog apps.
    let taken: Set<String>
    let cancel: () -> Void
    let connect: (_ name: String, _ server: McpServerConfig) -> Void

    @State private var url = ""
    @State private var name = ""
    /// The name was typed, so the URL no longer fills it.
    @State private var nameEdited = false
    @State private var headerName = "Authorization"
    @State private var headerValue = ""
    @State private var problem: String?
    @FocusState private var urlFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            ScrollView(.vertical) {
                VStack(spacing: 24) {
                    header
                    fields
                    if let problem {
                        InlineMessage(.error(problem))
                    }
                }
                .frame(maxWidth: 460)
                .padding(.horizontal, 24)
                .padding(.top, 48)
                .padding(.bottom, 24)
                .frame(maxWidth: .infinity)
            }
            VStack(spacing: 10) {
                Button(action: cancel) {
                    Text("Cancel")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(ConnectButtonStyle(prominent: false))
                .keyboardShortcut(.cancelAction)
                Button(action: submit) {
                    Text("Connect")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(ConnectButtonStyle(prominent: true))
                .keyboardShortcut(.defaultAction)
                .disabled(!appState.connection.isReady || url.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 24)
            .padding(.top, 12)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
        }
        .onAppear { urlFocused = true }
        .onExitCommand(perform: cancel)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Connect a custom app")
    }

    private var header: some View {
        VStack(spacing: 12) {
            HStack(spacing: -8) {
                Icons.LogoView(size: 20)
                    .frame(width: 40, height: 40)
                    .background(Theme.surface, in: Circle())
                    .overlay(Circle().strokeBorder(Theme.border, lineWidth: 1))
                AppIcon(symbol: Icons.mcp, size: 24, bare: true)
                    .frame(width: 40, height: 40)
                    .background(Theme.surface, in: Circle())
                    .overlay(Circle().strokeBorder(Theme.border, lineWidth: 1))
            }
            .accessibilityHidden(true)
            VStack(spacing: 6) {
                Text("Connect a custom app")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Text("Add a remote MCP server by its URL. Its tools become available in every chat.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var fields: some View {
        VStack(alignment: .leading, spacing: 12) {
            field("Server URL", help: "The server's streamable HTTP endpoint, such as https://mcp.example.com/mcp.") {
                TextField("https://", text: $url)
                    .focused($urlFocused)
                    .onChange(of: url) { _, value in
                        if !nameEdited { name = Self.suggestedName(for: value, taken: taken) }
                    }
            }
            field("Name", help: "How the agent's tools are prefixed. Letters, digits, - and _ only.") {
                TextField("my-server", text: Binding(get: { name }, set: { name = $0; nameEdited = true }))
            }
            VStack(alignment: .leading, spacing: 5) {
                Text("Header (optional)")
                    .font(Theme.Font.secondary.weight(.medium))
                    .foregroundStyle(Theme.textPrimary)
                HStack(spacing: 8) {
                    input { TextField("Header", text: $headerName) }
                        .frame(width: 140)
                    input { SecureField("Bearer …", text: $headerValue) }
                }
                Text("Sent with every request, for servers that take an API key. Saved to ~/.context-agent/config.toml (readable only by you); enter ${VAR} to read an environment variable instead.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
    }

    private func field(_ label: String, help: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(label)
                .font(Theme.Font.secondary.weight(.medium))
                .foregroundStyle(Theme.textPrimary)
            input(content)
            Text(help)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func input(@ViewBuilder _ content: () -> some View) -> some View {
        content()
            .textFieldStyle(.plain)
            .font(Theme.Font.secondary)
            .padding(.horizontal, 10)
            .frame(height: 32)
            .background(Theme.canvas, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
    }

    private func submit() {
        let address = url.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let parsed = URL(string: address), ["http", "https"].contains(parsed.scheme?.lowercased() ?? ""), parsed.host?.isEmpty == false else {
            problem = "Enter a URL starting with https:// (or http://)."
            return
        }
        let key = name.trimmingCharacters(in: .whitespaces)
        guard Self.isValidName(key) else {
            problem = "Names use letters, digits, - and _ only (at most 32 characters)."
            return
        }
        guard !taken.contains(key) else {
            problem = "\(key) is already used; choose another name."
            return
        }
        var headers: [String: String] = [:]
        let value = headerValue.trimmingCharacters(in: .whitespaces)
        if !value.isEmpty {
            let header = headerName.trimmingCharacters(in: .whitespaces)
            guard !header.isEmpty, !header.contains(where: { $0 == ":" || $0.isWhitespace }) else {
                problem = "Enter the header's name, such as Authorization."
                return
            }
            headers[header] = value
        }
        problem = nil
        connect(key, McpServerConfig(transport: .http, url: address, headers: headers))
    }

    /// Same rule as `McpServerConfig::valid_name` in ctx-core.
    static func isValidName(_ name: String) -> Bool {
        !name.isEmpty && name.count <= 32 && name.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "_" || $0 == "-") }
    }

    /// `https://mcp.linear.app/sse` → `linear`, made unique among `taken`.
    static func suggestedName(for url: String, taken: Set<String>) -> String {
        guard let host = URL(string: url.trimmingCharacters(in: .whitespaces))?.host?.lowercased() else { return "" }
        let labels = host.split(separator: ".").map(String.init).filter { !["mcp", "www", "api"].contains($0) }
        let base = String((labels.first ?? host).map { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") ? $0 : "-" }.prefix(28))
        guard !base.isEmpty else { return "" }
        var candidate = base
        var n = 2
        while taken.contains(candidate) {
            candidate = "\(base)-\(n)"
            n += 1
        }
        return candidate
    }
}

private struct ConnectButtonStyle: ButtonStyle {
    let prominent: Bool
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Theme.Font.body.weight(.medium))
            .foregroundStyle(prominent ? Color(nsColor: .windowBackgroundColor) : Theme.textPrimary)
            .frame(height: 40)
            .frame(maxWidth: .infinity)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(prominent ? Theme.textPrimary : Theme.surface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(prominent ? .clear : Theme.border, lineWidth: 1)
            )
            .opacity(configuration.isPressed ? 0.8 : (isEnabled ? 1 : 0.5))
            .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}

// MARK: - Preview

#Preview("Apps") {
    let appState = AppState()
    let config = InitializeResult(
        protocolVersion: 1,
        ctxVersion: "0.1.0",
        dataDir: "/Users/me/.context-agent",
        configFile: "/Users/me/.context-agent/config.toml",
        configSources: [],
        mock: false,
        model: ModelSettings(name: "gpt-4.1", baseUrl: "https://api.openai.com/v1", apiKeyEnv: "OPENAI_API_KEY", apiKeySet: true, contextWindow: 128_000, maxOutputTokens: 16_000, reasoningEffort: nil),
        permissions: PermissionSettings(profile: "approval_required", allowedCommands: [], deniedCommands: [], allowNetworkCommands: false),
        pricing: nil,
        mcp: McpSettings(servers: ["computer-use": McpServerConfig(transport: .builtin, builtin: "computer-use", app: "computer-use")])
    )
    appState.config = config
    appState.connection = .ready(config)
    appState.mcpCatalog = [
        McpCatalogApp(id: "computer-use", name: "Computer Use", tagline: "See the screen and drive the mouse and keyboard", examplePrompt: "Open Safari and check the staging site", description: "Lets the agent look at the screen.", icon: "macwindow.and.cursorarrow", category: "Automation", vendor: "Context", homepage: nil, builtin: true, defaultInstalled: true, requirements: [], inputs: [], template: McpServerConfig(transport: .builtin, builtin: "computer-use")),
        McpCatalogApp(id: "github", name: "GitHub", tagline: "Issues, pull requests and repositories", examplePrompt: "Review open issues and pull request activity", description: "…", icon: "chevron.left.forwardslash.chevron.right", logo: "github", logoMonochrome: true, category: "Development", vendor: "GitHub", homepage: "https://github.com", builtin: false, defaultInstalled: false, requirements: ["Node.js (npx)"], inputs: [McpCatalogInput(key: "GITHUB_PERSONAL_ACCESS_TOKEN", label: "Personal access token", help: "From github.com/settings/tokens.", secret: true, required: true, kind: "env")], template: McpServerConfig(command: "npx")),
    ]
    appState.mcpServers = [McpServerStatus(name: "computer-use", enabled: true, app: "computer-use", state: .connected, transport: "builtin", target: "built into ctx (computer-use)", serverName: "context-computer-use", tools: [McpToolInfo(name: "screenshot", qualifiedName: "mcp__computer-use__screenshot", description: "Capture the screen", readOnly: true, destructive: false)])]
    return AppsView()
        .environment(appState)
        .frame(width: 900, height: 700)
}
