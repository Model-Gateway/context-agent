import SwiftUI
import AppKit

// Projects on remote hosts (docs/desktop-protocol.md "Remote hosts"): one
// `ssh <host> ctx serve` connection per configured host, started when the
// first project on it is opened and kept until the host is disconnected.
// A remote project is an ordinary `ProjectState` whose `backend` is that
// connection and whose `host` names it; everything else — chats, events,
// git, the editor — works unchanged through the same protocol.

/// A host's `ctx serve` over SSH and what the sidebar shows about it.
@MainActor
@Observable
final class RemoteConnection: Identifiable {
    enum State: Equatable {
        case disconnected
        case connecting
        case ready(InitializeResult)
        case failed(BackendFailure)

        var isReady: Bool {
            if case .ready = self { return true }
            return false
        }

        var failure: BackendFailure? {
            if case .failed(let f) = self { return f }
            return nil
        }
    }

    /// The host as configured when the connection was made (Settings edits
    /// apply at the next connect).
    var host: SSHHost
    let client: BackendClient
    var state: State = .disconnected
    /// Why the last attempt failed, in the user's terms (keys, PATH, the
    /// address), under the raw ssh output.
    var hint: String?
    /// A connect in flight, awaited by every opener that arrives meanwhile.
    @ObservationIgnored var connecting: Task<Bool, Never>?

    var id: UUID { host.id }

    init(host: SSHHost, client: BackendClient? = nil) {
        self.host = host
        self.client = client ?? BackendClient()
    }

    var isReady: Bool { state.isReady }

    /// "ctx 0.1.0 · connected", "Connecting…", "Disconnected", or the failure title.
    var label: String {
        switch state {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting…"
        case .ready(let result): return "ctx \(result.ctxVersion) · connected" + (result.mock ? " · mock" : "")
        case .failed(let failure): return failure.title
        }
    }

    /// "alice@devbox · linux" once connected.
    var hostSummary: String? {
        guard case .ready(let result) = state, let info = result.host else { return nil }
        let who = info.user.isEmpty ? info.hostname : "\(info.user)@\(info.hostname)"
        return "\(who) · \(info.os)"
    }
}

/// The remote folder picker sheet (Views/Remote/RemoteFolderPicker.swift):
/// which host it opens on (nil: choose one first).
struct RemotePickerRequest: Identifiable, Equatable {
    var id = UUID()
    var host: SSHHost?
}

extension AppState {
    // MARK: - Connections

    /// The host's connection object, created (disconnected) on first use.
    func remote(for host: SSHHost) -> RemoteConnection {
        if let existing = remotes[host.id] {
            existing.host = host
            return existing
        }
        let connection = RemoteConnection(host: host)
        connection.client.onNotification = { [weak self, weak connection] notification in
            guard let connection else { return }
            self?.handle(notification, remote: connection)
        }
        remotes[host.id] = connection
        return connection
    }

    /// The connection of the project's host, if any.
    func remote(of project: ProjectState) -> RemoteConnection? {
        project.host.flatMap { remotes[$0.id] }
    }

    /// Starts `ssh <host> ctx serve` unless it is already up; nil (with a
    /// toast) when it cannot be reached. Concurrent callers share one attempt.
    @discardableResult
    func connect(_ host: SSHHost, quietly: Bool = false) async -> RemoteConnection? {
        let connection = remote(for: host)
        if connection.isReady, connection.client.isReady { return connection }
        if let inFlight = connection.connecting {
            return await inFlight.value ? connection : nil
        }
        let attempt = Task { @MainActor [weak self] () -> Bool in
            guard let self else { return false }
            return await self.startRemote(connection)
        }
        connection.connecting = attempt
        let ok = await attempt.value
        connection.connecting = nil
        if !ok, !quietly, let failure = connection.state.failure {
            toasts.show("Could not connect to \(host.displayName): \(BackendFailurePanel.summary(for: failure))", kind: .error, details: [failure.detail, connection.hint].compactMap { $0 }.joined(separator: "\n\n"))
        }
        return ok ? connection : nil
    }

    private func startRemote(_ connection: RemoteConnection) async -> Bool {
        connection.state = .connecting
        connection.hint = nil
        // The login shell's environment carries SSH_AUTH_SOCK and the
        // user's PATH (a Homebrew ssh, say); no key is passed along.
        let environment = await BackendEnvironment.build(apiKeyEnv: nil)
        do {
            let result = try await connection.client.start(launch: .ssh(connection.host), environment: environment, dataDir: nil, mock: mockRequested)
            connection.state = .ready(result)
            appendRemoteLog(connection, "connected: ctx \(result.ctxVersion) on \(result.host.map { "\($0.user)@\($0.hostname)" } ?? connection.host.address)")
            return true
        } catch {
            let failure = Self.failure(from: connection.client.lastStartFailure, error: error)
            connection.state = .failed(failure)
            connection.hint = Self.remoteHint(for: failure, stderr: connection.client.stderrLog, host: connection.host)
            appendRemoteLog(connection, "connection failed: \(BackendFailurePanel.summary(for: failure))", level: "error")
            return false
        }
    }

    /// Closes the host's projects and stops its `ssh`. Running tasks on
    /// the host are cancelled by the server as its stdin closes.
    func disconnect(_ host: SSHHost) async {
        guard let connection = remotes[host.id] else { return }
        for project in projects(on: host.id) {
            await closeProject(project.id, forget: false)
        }
        await connection.client.stop()
        connection.state = .disconnected
        connection.hint = nil
    }

    /// Reconnects a host after a failure or a dropped link and reopens the
    /// projects the app still lists for it.
    func reconnect(_ host: SSHHost) async {
        let remembered = projects(on: host.id).map(\.root)
        if let connection = remotes[host.id], connection.client.isRunning {
            await connection.client.stop()
        }
        guard await connect(host) != nil else { return }
        for root in remembered {
            await openProject(path: root, host: host, select: false, restoreSession: true)
        }
        if remembered.isEmpty {
            for key in preferences.openProjectPaths {
                guard let ref = RemoteProjectRef(encoded: key), ref.hostId == host.id else { continue }
                await openProject(path: ref.path, host: host, select: false, restoreSession: true)
            }
        }
    }

    /// The link to a host went away (ssh exited): its projects' runs are
    /// over, the rows stay with the host marked failed, Reconnect in the
    /// sidebar brings them back. No automatic retry: a dropped VPN would
    /// otherwise spawn ssh every second.
    func handleRemoteExit(_ connection: RemoteConnection, code: Int32, tail: String) {
        for project in projects(on: connection.host.id) {
            for chat in project.allChats where chat.status.isRunning {
                chat.finish(TaskFinished(projectId: project.id, sessionId: chat.sessionId, taskId: nil, status: .failed, error: "Connection to \(connection.host.displayName) lost (ssh exited with code \(code))", errorKind: "backend_exited"), terminal: .stopped)
            }
            project.runningSessionIds = []
            project.switchingModel = nil
            project.indexProgress = nil
        }
        let failure = BackendFailure.exited(code: code, tail: tail)
        connection.state = .failed(failure)
        connection.hint = Self.remoteHint(for: failure, stderr: connection.client.stderrLog, host: connection.host)
        appendRemoteLog(connection, "ssh exited (code \(code))", level: "error")
        toasts.show("Lost the connection to \(connection.host.displayName) (ssh exited with code \(code)).", kind: .error, details: tail.isEmpty ? nil : tail)
        refreshAttention()
    }

    private func appendRemoteLog(_ connection: RemoteConnection, _ message: String, level: String = "info") {
        handle(.log(LogNotification(level: level, message: message)), remote: connection)
    }

    /// What ssh's last words usually mean, for the sidebar and the toast.
    static func remoteHint(for failure: BackendFailure, stderr: [String], host: SSHHost) -> String? {
        let text = (stderr.suffix(12).joined(separator: "\n") + "\n" + failure.detail).lowercased()
        if text.contains("permission denied") || text.contains("no supported authentication") || text.contains("too many authentication failures") {
            return "ssh could not sign in as \(host.user.isEmpty ? "the configured user" : host.user). Passwords cannot be typed here: add your key to ssh-agent (ssh-add), set an identity file for the host, or put the host in ~/.ssh/config."
        }
        if text.contains("could not resolve hostname") || text.contains("name or service not known") || text.contains("nodename nor servname") {
            return "The address \(host.address) could not be resolved. Check the host's address, or use a Host alias from ~/.ssh/config."
        }
        if text.contains("connection refused") || text.contains("connection timed out") || text.contains("operation timed out") || text.contains("no route to host") || text.contains("network is unreachable") {
            return "Nothing answered at \(host.target). Check that the machine is reachable (VPN, firewall) and that sshd listens on \(host.port > 0 ? "port \(host.port)" : "its port")."
        }
        if text.contains("host key verification failed") || text.contains("remote host identification has changed") {
            return "ssh refused the host key. Connect once from Terminal (ssh \(host.target)) to accept it, or fix ~/.ssh/known_hosts."
        }
        if text.contains("command not found") || text.contains("not found") && text.contains("ctx") || text.contains("no such file or directory") && text.contains("ctx") {
            return "ctx is not installed on \(host.displayName), or not on its PATH for non-interactive shells. Install it there (cargo install --path crates/cli, or copy the binary to ~/.cargo/bin) or set the ctx path for the host in Settings → Remote hosts."
        }
        if text.contains("unrecognized subcommand") || text.contains("unexpected argument") {
            return "The ctx on \(host.displayName) is too old for the desktop protocol. Update it to a build with `ctx serve`."
        }
        if case .exited(let code, _) = failure, code == 255 {
            return "ssh exited with code 255: it could not establish the connection. The output above has the reason; a password prompt, a host-key question or a missing key are the usual ones."
        }
        return nil
    }

    // MARK: - Opening projects on hosts

    /// Welcome, the sidebar, ⌘⇧O: the remote folder picker (Settings →
    /// Remote hosts first when no host is configured).
    func chooseRemoteProject(host: SSHHost? = nil) {
        guard !hosts.hosts.isEmpty || host != nil else {
            openSettings(.remoteHosts)
            toasts.show("Add an SSH host first; then projects on it can be opened.", kind: .info)
            return
        }
        remotePicker = RemotePickerRequest(host: host ?? (hosts.hosts.count == 1 ? hosts.hosts[0] : nil))
    }

    /// `fs/list` on the host (connecting first), for the picker.
    func listRemoteDirectory(_ path: String?, on host: SSHHost) async throws -> FsListing {
        guard let connection = await connect(host, quietly: true) else {
            throw BackendError.transport(remotes[host.id]?.state.failure.map { BackendFailurePanel.summary(for: $0) } ?? "\(host.displayName) is not connected")
        }
        return try await connection.client.fsList(path: path, dirsOnly: true)
    }

    /// At launch (after the local backend is up): every remembered open
    /// project on a host, each host in parallel, without blocking anything.
    /// A host that cannot be reached keeps its rows out of the sidebar and
    /// its state in the Remote section; the folders stay remembered.
    func reopenRememberedRemoteProjects() async {
        var byHost: [UUID: [String]] = [:]
        var order: [UUID] = []
        for key in preferences.openProjectPaths {
            guard let ref = RemoteProjectRef(encoded: key) else { continue }
            guard hosts.host(ref.hostId) != nil else {
                preferences.forgetOpenProject(path: key)
                continue
            }
            if byHost[ref.hostId] == nil { order.append(ref.hostId) }
            byHost[ref.hostId, default: []].append(ref.path)
        }
        guard !byHost.isEmpty else { return }
        let last = preferences.lastSelectedProjectId
        await withTaskGroup(of: Void.self) { group in
            for hostId in order {
                guard let host = hosts.host(hostId), let paths = byHost[hostId] else { continue }
                group.addTask { @MainActor in
                    guard await self.connect(host, quietly: true) != nil else { return }
                    for path in paths {
                        let project = await self.openProject(path: path, host: host, select: false, restoreSession: false, initGit: false)
                        if let project, project.id == last, self.selectedProjectId == nil {
                            self.select(project: project.id)
                        }
                    }
                }
            }
        }
    }

    // MARK: - Host settings

    /// Adds or updates a host (Settings → Remote hosts). A connected host
    /// keeps its connection; the new address, key or user apply when it
    /// connects next.
    func saveHost(_ host: SSHHost) {
        hosts.upsert(host)
        if let normalized = hosts.host(host.id) {
            remotes[host.id]?.host = normalized
            recents.renameHost(host.id, to: normalized.displayName)
        }
    }

    /// Removes a host: disconnects it, closes its projects, forgets its
    /// recents and remembered-open folders.
    func removeHost(_ host: SSHHost) async {
        await disconnect(host)
        remotes.removeValue(forKey: host.id)
        hosts.remove(host.id)
        for key in preferences.openProjectPaths where RemoteProjectRef(encoded: key)?.hostId == host.id {
            preferences.forgetOpenProject(path: key)
        }
        for recent in recents.items where recent.hostId == host.id {
            recents.remove(recent)
        }
    }

    /// `ssh <host> ctx --version`-style probe for the settings form: connects
    /// (or reuses the connection) and reports what answered.
    func testHost(_ host: SSHHost) async -> Result<String, Error> {
        let probe = SSHHost(id: host.id, name: host.name, address: host.address, user: host.user, port: host.port, identityFile: host.identityFile, ctxPath: host.ctxPath, extraArguments: host.extraArguments).normalized()
        guard probe.isValid else { return .failure(BackendError.transport("Enter the host's address first.")) }
        // A saved host is tested on its own connection; an unsaved draft on
        // a throwaway client that is stopped afterwards.
        if hosts.host(host.id) == probe, let connection = await connect(probe, quietly: true) {
            return .success(connection.hostSummary.map { "\(connection.label) · \($0)" } ?? connection.label)
        }
        if hosts.host(host.id) == probe, let failure = remotes[host.id]?.state.failure {
            return .failure(BackendError.transport([BackendFailurePanel.summary(for: failure), remotes[host.id]?.hint].compactMap { $0 }.joined(separator: "\n")))
        }
        let client = BackendClient()
        defer { Task { await client.stop() } }
        do {
            let environment = await BackendEnvironment.build(apiKeyEnv: nil)
            let result = try await client.start(launch: .ssh(probe), environment: environment, dataDir: nil, mock: mockRequested)
            let who = result.host.map { "\($0.user)@\($0.hostname) · \($0.os)" } ?? probe.target
            await client.stop()
            return .success("ctx \(result.ctxVersion) · \(who)")
        } catch {
            let failure = Self.failure(from: client.lastStartFailure, error: error)
            let hint = Self.remoteHint(for: failure, stderr: client.stderrLog, host: probe)
            await client.stop()
            return .failure(BackendError.transport([BackendFailurePanel.summary(for: failure), hint].compactMap { $0 }.joined(separator: "\n")))
        }
    }
}
