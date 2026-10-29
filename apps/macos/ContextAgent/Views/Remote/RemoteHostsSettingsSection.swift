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
            SettingsGroup("Hosts", footer: "Context runs ctx serve on the host over ssh and opens projects that live there: files, index, chats and git are all the host's. Adding a host installs ctx on it when it is missing or older than the app (the Linux build of this version, x86_64 or arm64, copied to ~/.local/bin over ssh: no root, Rust or internet access needed there). The host uses this Mac's sign-in and model, kept in its memory only, unless it has a model key of its own. Authentication is ssh's: keys and ssh-agent work, passwords cannot be typed; ~/.ssh/config aliases apply, so a ProxyJump or IdentityFile set there is used.") {
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
                    Button("Add SSH connection…") {
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

/// One line of the editor's progress list.
private struct EditorStep: Identifiable, Equatable {
    enum ID: Int, Comparable {
        case connect, install, signIn
        static func < (a: ID, b: ID) -> Bool { a.rawValue < b.rawValue }
    }
    enum State { case running, done, failed, warning }
    var id: ID
    var state: State
    var text: String
    /// Bytes of the ctx archive while it is downloaded or uploaded.
    var done: Int64? = nil
    var total: Int64? = nil
}

/// Add SSH connection / edit a host, laid out like Claude Code's: name,
/// SSH host (user@host or a ~/.ssh/config alias), port and key, with the
/// ctx path and extra ssh options under Advanced. Adding connects the way
/// the app will: when ctx is missing or older than the app it is installed
/// on the spot (with the download and upload shown), the host is saved and
/// connected, and this Mac's sign-in is forwarded to it.
private struct HostEditorSheet: View {
    @State var host: SSHHost
    let isNew: Bool

    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var login = ""
    @State private var extraArguments = ""
    @State private var portText = ""
    @State private var advanced = false
    @State private var busy = false
    @State private var steps: [EditorStep] = []
    @State private var error: String?
    /// The last attempt failed after the fields were fine: Add anyway saves without a connection.
    @State private var failedOnce = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text(isNew ? "Add SSH connection" : "Edit \(host.displayName)")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Text("Connect to a remote machine to run Context there. It uses this Mac's sign-in.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
            field("Name", help: "A friendly name for this SSH connection.") {
                TextField("devbox", text: $host.name)
                    .textFieldStyle(.roundedBorder)
            }
            field("SSH host", help: "user@myserver.com, or a host from ~/.ssh/config.") {
                HStack(spacing: 6) {
                    TextField("root@192.168.1.10", text: $login)
                        .textFieldStyle(.roundedBorder)
                    let aliases = SSHConfigHosts.aliases()
                    if !aliases.isEmpty {
                        Menu {
                            ForEach(aliases, id: \.self) { alias in
                                Button(alias) {
                                    login = alias
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
            field("SSH port", help: "Leave empty to use the default (22) or your SSH configuration.") {
                TextField("22", text: $portText)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 110)
            }
            field("SSH key (optional)", help: "Path to a private key. Leave empty to use your SSH configuration or agent. Passwords cannot be typed here.") {
                HStack(spacing: 6) {
                    TextField("~/.ssh/id_ed25519", text: $host.identityFile)
                        .textFieldStyle(.roundedBorder)
                    Button("Browse…") { chooseIdentity() }
                        .controlSize(.small)
                }
            }
            Button {
                advanced.toggle()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: advanced ? Icons.chevronDown : Icons.chevronRight)
                        .font(.system(size: 9, weight: .semibold))
                    Text("Advanced")
                }
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            }
            .buttonStyle(.plain)
            if advanced {
                field("ctx path", help: nil) {
                    TextField("\(RemoteInstaller.installDirectory)/ctx, installed for you when missing", text: $host.ctxPath)
                        .textFieldStyle(.roundedBorder)
                }
                field("ssh options", help: nil) {
                    TextField("-o ProxyJump=bastion", text: $extraArguments)
                        .textFieldStyle(.roundedBorder)
                        .font(Theme.Font.mono)
                }
            }
            if !steps.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(steps) { step in
                        stepRow(step)
                    }
                }
                .padding(.top, 2)
            }
            if let error {
                InlineMessage(.error(error))
            }
            HStack(spacing: 8) {
                if !isNew {
                    Button {
                        Task { _ = await run(keep: false) }
                    } label: {
                        HStack(spacing: 6) {
                            if busy { BrailleSpinner(size: 11, tint: Theme.textSecondary) }
                            Text("Test connection")
                        }
                    }
                    .disabled(!draft.isValid || busy)
                }
                Spacer(minLength: 8)
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                    .disabled(busy)
                if isNew, failedOnce, !busy {
                    Button("Add anyway") {
                        appState.saveHost(draft)
                        dismiss()
                    }
                    .help("Save the host without a working connection; connect from the sidebar later")
                    .disabled(!draft.isValid)
                }
                Button {
                    if isNew {
                        Task { await add() }
                    } else {
                        appState.saveHost(draft)
                        dismiss()
                    }
                } label: {
                    HStack(spacing: 6) {
                        if isNew, busy { BrailleSpinner(size: 11, tint: .white) }
                        Text(isNew ? "Add SSH connection" : "Save")
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(Theme.accent)
                .keyboardShortcut(.defaultAction)
                .disabled(!draft.isValid || busy)
            }
        }
        .padding(20)
        .frame(width: 520)
        .background(Theme.canvas)
        .interactiveDismissDisabled(busy)
        .onAppear {
            login = host.login
            portText = host.port > 0 ? String(host.port) : ""
            extraArguments = host.extraArguments.joined(separator: " ")
            advanced = !host.ctxPath.isEmpty || !host.extraArguments.isEmpty
        }
    }

    private func field<Content: View>(_ label: String, help: String?, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(Theme.Font.secondary.weight(.semibold))
                .foregroundStyle(Theme.textPrimary)
            content()
                .disabled(busy)
            if let help {
                Text(help)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func stepRow(_ step: EditorStep) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Group {
                if step.state == .running {
                    BrailleSpinner(size: 10, tint: Theme.accent)
                } else {
                    Circle()
                        .fill(step.state == .done ? Theme.success : step.state == .failed ? Theme.danger : Theme.warning)
                        .frame(width: 7, height: 7)
                }
            }
            .frame(width: 12)
            VStack(alignment: .leading, spacing: 6) {
                Text(step.text)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(step.state == .failed ? Theme.danger : Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
                if let done = step.done {
                    if let total = step.total, total > 0 {
                        ProgressView(value: Double(min(done, total)), total: Double(total))
                            .progressViewStyle(.linear)
                            .tint(Theme.accent)
                    } else {
                        ProgressView()
                            .progressViewStyle(.linear)
                            .tint(Theme.accent)
                    }
                }
            }
        }
    }

    /// The form's host: the SSH host field split into user and address, the options on whitespace.
    private var draft: SSHHost {
        var draft = host
        let split = SSHHost.splitLogin(login)
        draft.user = split.user
        draft.address = split.address
        draft.port = Int(portText.trimmingCharacters(in: .whitespaces)) ?? 0
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

    private func setStep(_ id: EditorStep.ID, _ state: EditorStep.State, _ text: String, done: Int64? = nil, total: Int64? = nil) {
        steps.removeAll { $0.id == id }
        steps.append(EditorStep(id: id, state: state, text: text, done: done, total: total))
        steps.sort { $0.id < $1.id }
    }

    private func fail(_ id: EditorStep.ID, _ text: String, message: String) {
        setStep(id, .failed, text)
        error = message
        failedOnce = true
    }

    private static func megabytes(_ bytes: Int64) -> String {
        String(format: bytes < 10_000_000 ? "%.1f" : "%.0f", Double(bytes) / 1_000_000)
    }

    /// "Uploading ctx to the host (3.1/9.8 MB)…" for an install phase.
    private static func installText(_ progress: RemoteInstaller.Progress) -> String {
        let of = progress.total.map { "\(megabytes(progress.done))/\(megabytes($0)) MB" } ?? "\(megabytes(progress.done)) MB"
        switch progress.phase {
        case .probe: return "Checking what the host runs…"
        case .download: return "Downloading ctx \(Updater.version) for Linux (\(of))…"
        case .upload: return "Uploading ctx to the host (\(of))…"
        case .verify: return "Unpacking ctx in \(RemoteInstaller.installDirectory)…"
        }
    }

    /// Connects like the app will (Test connection for an existing host; the
    /// whole of Add for a new one): `ssh … ctx serve`, an install when ctx
    /// is missing or older than the app, then — when `keep` — save, connect
    /// for real and forward the sign-in. True when it got through.
    private func run(keep: Bool) async -> Bool {
        var probe = draft
        let target = probe.target
        busy = true
        error = nil
        steps = []
        defer { busy = false }
        setStep(.connect, .running, "Connecting to \(target)…")
        var tested = await appState.testHost(probe)
        var outdated = false
        if case .success(let success) = tested, !success.mock, RemoteInstaller.isOlder(success.ctxVersion, than: Updater.version) { outdated = true }
        var missing = false
        if case .failure(let failure) = tested, (failure as? HostTestFailure)?.needsCtx == true { missing = true }
        if missing || outdated {
            if case .success(let success) = tested {
                setStep(.connect, .done, "Connected to \(target): ctx \(success.ctxVersion) is older than this app")
            } else {
                setStep(.connect, .done, "Connected to \(target): ctx is not installed")
            }
            setStep(.install, .running, "Installing ctx \(Updater.version)…")
            let installed = await appState.installCtx(on: probe) { progress in
                Task { @MainActor in
                    guard steps.contains(where: { $0.id == .install && $0.state == .running }) else { return }
                    let bytes = progress.phase == .download || progress.phase == .upload
                    setStep(.install, .running, Self.installText(progress), done: bytes ? progress.done : nil, total: bytes ? progress.total : nil)
                }
            }
            switch installed {
            case .success(let version):
                probe.ctxPath = ""
                host.ctxPath = ""
                setStep(.install, .done, "Installed \(version) in \(RemoteInstaller.installDirectory)")
            case .failure(let failure):
                fail(.install, "Could not install ctx", message: failure.localizedDescription)
                return false
            }
            setStep(.connect, .running, "Starting ctx on \(target)…")
            tested = await appState.testHost(probe)
        }
        switch tested {
        case .success(let success):
            setStep(.connect, .done, "Connected: \(success.summary)")
        case .failure(let failure):
            fail(.connect, "Could not connect to \(target)", message: failure.localizedDescription)
            return false
        }
        guard keep else { return true }
        appState.saveHost(probe)
        let saved = appState.hosts.host(probe.id) ?? probe
        setStep(.signIn, .running, "Signing the host in with this Mac's account…")
        guard let connection = await appState.connect(saved, quietly: true), let model = connection.state.result?.model else {
            let failed = appState.remotes[saved.id]
            let reason = [failed?.state.failure.map { BackendFailurePanel.summary(for: $0) }, failed?.hint].compactMap { $0 }.joined(separator: "\n")
            fail(.signIn, "Saved, but the connection did not come up", message: reason)
            return false
        }
        if model.keyForwarded == true {
            setStep(.signIn, .done, "Using this Mac's sign-in (kept in memory on the host)")
        } else if model.apiKeySet {
            setStep(.signIn, .done, "The host uses its own model key")
        } else {
            setStep(.signIn, .warning, "No model key: sign in on this Mac (Settings → Account) or configure a key on the host")
        }
        return true
    }

    private func add() async {
        guard draft.isValid, !busy else { return }
        if await run(keep: true) {
            appState.toasts.show("Connected to \(draft.displayName).", kind: .success)
            dismiss()
        }
    }
}
