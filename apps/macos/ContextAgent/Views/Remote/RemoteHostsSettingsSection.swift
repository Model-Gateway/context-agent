import SwiftUI
import AppKit

/// Settings → Remote hosts: the SSH hosts Context can run `ctx serve` on,
/// like Claude Code's and Codex's remote sessions. Each row shows the
/// host's connection state and opens the editor; "Add host…" offers the
/// `Host` aliases of ~/.ssh/config. The editor tests the connection the
/// same way the app connects (`ssh … ctx serve`, then `initialize`).
struct RemoteHostsSettingsSection: View {
    @Environment(AppState.self) private var appState
    @State private var editing: SSHHost?

    var body: some View {
        SettingsPage {
            SettingsGroup("Hosts", footer: "Context runs ctx serve on the host over ssh and opens projects that live there: files, index, chats, API key and configuration are all the host's, so ctx must be installed there and have a model configured. When it is missing or older than the app, Install ctx on host copies the Linux build of this version (x86_64 or arm64) to ~/.local/bin over ssh: no root, Rust or internet access needed on the host. Authentication is ssh's: keys and ssh-agent work, passwords cannot be typed; ~/.ssh/config aliases apply, so a ProxyJump or IdentityFile set there is used.") {
                if appState.hosts.hosts.isEmpty {
                    Text("No remote hosts yet.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, 6)
                } else {
                    VStack(spacing: 0) {
                        ForEach(appState.hosts.hosts) { host in
                            HostSettingsRow(host: host) { editing = host }
                            if host.id != appState.hosts.hosts.last?.id {
                                Divider()
                            }
                        }
                    }
                }
                HStack(spacing: 8) {
                    Button("Add host…") {
                        editing = SSHHost(name: "", address: "")
                    }
                    let aliases = SSHConfigHosts.aliases().filter { alias in !appState.hosts.hosts.contains { $0.address == alias } }
                    if !aliases.isEmpty {
                        Menu("From ~/.ssh/config") {
                            ForEach(aliases, id: \.self) { alias in
                                Button(alias) {
                                    editing = SSHHost(name: alias, address: alias)
                                }
                            }
                        }
                        .fixedSize()
                    }
                }
                .controlSize(.small)
            }

            SettingsGroup("How it works", footer: "Projects on a host appear in the sidebar's Remote section under the host and are remembered across launches. The Terminal tab opens ssh shells in the project's folder; images and PDFs of a remote project are not shown in the Editor; attachments you send are uploaded with the message.") {
                SettingsRow("Open") {
                    Text("Welcome → Open on a remote host…, the host's row in the sidebar, or ⌘⇧O.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                SettingsRow("ssh") {
                    Text(verbatim: "ssh -o BatchMode=yes -T [-p port] [-i key] [-l user] <host> -- env PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:…:$PATH\" ctx serve")
                        .font(Theme.Font.monoSmall)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
            }
        }
        .sheet(item: $editing) { host in
            HostEditorSheet(host: host, isNew: appState.hosts.host(host.id) == nil)
        }
    }
}

private struct HostSettingsRow: View {
    let host: SSHHost
    let edit: () -> Void

    @Environment(AppState.self) private var appState
    @State private var confirmRemove = false

    private var connection: RemoteConnection? { appState.remotes[host.id] }

    private func install() async {
        if case .failure(let error) = await appState.installCtx(on: host) {
            appState.toasts.show("Could not install ctx on \(host.displayName).", kind: .error, details: error.localizedDescription)
        }
    }

    private var statusTint: Color {
        switch connection?.state {
        case .ready: return Theme.success
        case .failed: return Theme.danger
        case .connecting: return Theme.warning
        case .disconnected, nil: return Theme.textTertiary
        }
    }

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: Icons.remoteHost)
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 20)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(verbatim: host.displayName)
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    if host.displayName != host.target {
                        Text(verbatim: host.target)
                            .font(Theme.Font.monoSmall)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                HStack(spacing: 6) {
                    if case .connecting = connection?.state {
                        BrailleSpinner(size: 10, tint: Theme.textTertiary)
                    } else {
                        Circle().fill(statusTint).frame(width: 6, height: 6).accessibilityHidden(true)
                    }
                    Text(verbatim: connection.map { c in c.hostSummary.map { "\(c.label) · \($0)" } ?? c.label } ?? "Not connected")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    let open = appState.projects(on: host.id).count
                    if open > 0 {
                        Chip("\(open) open")
                    }
                }
                if let hint = connection?.hint, case .failed = connection?.state {
                    Text(verbatim: hint)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.danger)
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
            }
            Spacer(minLength: 8)
            HStack(spacing: 6) {
                if let connection, connection.needsCtx || connection.isOutdated {
                    Button {
                        Task { await install() }
                    } label: {
                        HStack(spacing: 5) {
                            if connection.installing { BrailleSpinner(size: 10, tint: Theme.textSecondary) }
                            Text(connection.isOutdated ? "Update ctx" : "Install ctx")
                        }
                    }
                    .help(connection.isOutdated ? "Replace ctx on the host with the build of this version (\(Updater.version))" : "Copy ctx \(Updater.version) for Linux to \(RemoteInstaller.installDirectory) on the host")
                    .disabled(connection.installing || appState.projects(on: host.id).contains { $0.isRunning })
                }
                if connection?.isReady == true {
                    Button("Disconnect") { Task { await appState.disconnect(host) } }
                        .disabled(appState.projects(on: host.id).contains { $0.isRunning })
                } else {
                    Button(connection?.state.failure == nil ? "Connect" : "Reconnect") { Task { await appState.reconnect(host) } }
                        .disabled({ if case .connecting = connection?.state { return true } else { return false } }())
                }
                Button("Edit…", action: edit)
                Button("Remove…", role: .destructive) { confirmRemove = true }
            }
            .controlSize(.small)
        }
        .padding(.vertical, 8)
        .confirmationDialog("Remove “\(host.displayName)”?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                Task { await appState.removeHost(host) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Its open projects close and its recents are forgotten. Nothing on the host is touched.")
        }
    }
}

// MARK: - Editor

/// Add / edit a host: name, address (with ~/.ssh/config suggestions), user,
/// port, identity file, the remote ctx path and extra ssh options, with
/// Test connection and Save.
private struct HostEditorSheet: View {
    @State var host: SSHHost
    let isNew: Bool

    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var extraArguments = ""
    @State private var portText = ""
    @State private var testing = false
    @State private var installing = false
    @State private var message: SettingsMessage?
    /// The last Test connection found ctx missing or too old.
    @State private var needsCtx = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(isNew ? "Add a remote host" : "Edit \(host.displayName)")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
                .accessibilityAddTraits(.isHeader)
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 10) {
                GridRow {
                    label("Address")
                    HStack(spacing: 6) {
                        TextField("devbox, 10.0.0.5 or a ~/.ssh/config alias", text: $host.address)
                            .textFieldStyle(.roundedBorder)
                        let aliases = SSHConfigHosts.aliases()
                        if !aliases.isEmpty {
                            Menu {
                                ForEach(aliases, id: \.self) { alias in
                                    Button(alias) {
                                        host.address = alias
                                        if host.name.isEmpty { host.name = alias }
                                    }
                                }
                            } label: {
                                Image(systemName: Icons.chevronDown)
                            }
                            .menuStyle(.borderlessButton)
                            .fixedSize()
                            .help("Hosts from ~/.ssh/config")
                        }
                    }
                }
                GridRow {
                    label("Name")
                    TextField("Shown in the sidebar (defaults to the address)", text: $host.name)
                        .textFieldStyle(.roundedBorder)
                }
                GridRow {
                    label("User")
                    TextField("ssh's default, or the config file's", text: $host.user)
                        .textFieldStyle(.roundedBorder)
                }
                GridRow {
                    label("Port")
                    TextField("22", text: $portText)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 90)
                        .onChange(of: portText) { _, text in host.port = Int(text.trimmingCharacters(in: .whitespaces)) ?? 0 }
                }
                GridRow {
                    label("Identity file")
                    HStack(spacing: 6) {
                        TextField("~/.ssh/id_ed25519 (optional)", text: $host.identityFile)
                            .textFieldStyle(.roundedBorder)
                        Button("Choose…") { chooseIdentity() }
                            .controlSize(.small)
                    }
                }
                GridRow {
                    label("ctx path")
                    TextField("ctx on the host's PATH, or /path/to/ctx", text: $host.ctxPath)
                        .textFieldStyle(.roundedBorder)
                }
                GridRow {
                    label("ssh options")
                    TextField("-o ProxyJump=bastion -o IdentitiesOnly=yes (optional)", text: $extraArguments)
                        .textFieldStyle(.roundedBorder)
                        .font(Theme.Font.mono)
                }
            }
            if let message {
                InlineMessage(message)
            }
            HStack(spacing: 8) {
                Button {
                    Task { await test() }
                } label: {
                    HStack(spacing: 6) {
                        if testing { BrailleSpinner(size: 11, tint: Theme.textSecondary) }
                        Text("Test connection")
                    }
                }
                .disabled(!draft.isValid || testing || installing)
                if needsCtx {
                    Button {
                        Task { await install() }
                    } label: {
                        HStack(spacing: 6) {
                            if installing { BrailleSpinner(size: 11, tint: Theme.textSecondary) }
                            Text("Install ctx on host")
                        }
                    }
                    .help("Copy ctx \(Updater.version) for Linux to \(RemoteInstaller.installDirectory) on the host over ssh, then test again")
                    .disabled(!draft.isValid || testing || installing)
                }
                Spacer(minLength: 8)
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(isNew ? "Add" : "Save") {
                    appState.saveHost(draft)
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .tint(Theme.accent)
                .keyboardShortcut(.defaultAction)
                .disabled(!draft.isValid)
            }
        }
        .padding(20)
        .frame(width: 560)
        .background(Theme.canvas)
        .onAppear {
            portText = host.port > 0 ? String(host.port) : ""
            extraArguments = host.extraArguments.joined(separator: " ")
        }
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .font(Theme.Font.secondary)
            .foregroundStyle(Theme.textSecondary)
            .gridColumnAlignment(.trailing)
    }

    /// The form's host with the option field split on whitespace.
    private var draft: SSHHost {
        var draft = host
        draft.extraArguments = extraArguments.split(whereSeparator: \.isWhitespace).map(String.init)
        return draft.normalized()
    }

    private func chooseIdentity() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        panel.directoryURL = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh")
        panel.prompt = "Choose"
        panel.message = "Choose the private key for this host"
        if panel.runModal() == .OK, let url = panel.url {
            host.identityFile = (url.path as NSString).abbreviatingWithTildeInPath
        }
    }

    private func test() async {
        testing = true
        message = .info("Connecting to \(draft.target) and starting ctx serve…")
        defer { testing = false }
        switch await appState.testHost(draft) {
        case .success(let summary):
            needsCtx = false
            message = .success("Connected: \(summary)")
        case .failure(let error):
            needsCtx = (error as? HostTestFailure)?.needsCtx == true
            message = .error(error.localizedDescription)
        }
    }

    /// Install ctx on host, then the same test as the button's, so the
    /// message ends on what the app will find when it connects.
    private func install() async {
        installing = true
        message = .info("Installing ctx \(Updater.version) on \(draft.target)…")
        let result = await appState.installCtx(on: draft)
        installing = false
        switch result {
        case .success(let installed):
            host.ctxPath = ""
            needsCtx = false
            await test()
            if message?.kind == .success {
                message = .success("Installed \(installed) in \(RemoteInstaller.installDirectory). \(message?.text ?? "")")
            }
        case .failure(let error):
            message = .error(error.localizedDescription)
        }
    }
}
