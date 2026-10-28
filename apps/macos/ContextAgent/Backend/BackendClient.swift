import Foundation
import SwiftUI

// Typed async API over one `ctx serve` child. Everything the UI touches is
// main-actor; the process reader, the RPC actor and the exit watcher run off
// it and hop back through a single ordered stream.

extension BackendError {
    /// `kind` of the `.rpc` error `BackendClient.start` throws when the binary
    /// does not speak protocol 1 (too old, or a different protocol version).
    static let incompatibleKind = "incompatible"
    static let incompatibleCode = -1

    static func incompatible(_ detail: String) -> BackendError {
        .rpc(code: incompatibleCode, message: detail, kind: incompatibleKind)
    }

    var isIncompatible: Bool {
        if case .rpc(_, _, let kind) = self { return kind == Self.incompatibleKind }
        return false
    }
}

@MainActor
@Observable
final class BackendClient {
    enum State: Equatable {
        case stopped
        case starting
        case running(pid: Int32)
        case failed(String)
    }

    /// Why the last `start` failed, in the shape `BackendFailure` wants.
    enum StartFailure: Equatable {
        case launchFailed(String)
        case incompatible(found: String, detail: String)
        case exited(code: Int32, tail: String)
        case transport(String)
    }

    static let protocolVersion = 1
    static let initializeTimeout: TimeInterval = 30

    var state: State = .stopped
    /// How the running (or last) child was started; nil before the first start.
    var launch: BackendLaunch?
    /// The remote host of an SSH launch, else nil.
    var host: SSHHost? { launch?.host }
    var isRemote: Bool { launch?.isRemote == true }
    /// Delivered on the main actor, in the order the backend sent them.
    /// `.exited` is sent only for exits that were not requested by `stop()`
    /// and did not happen while `start()` was still initialising (those are
    /// reported through the thrown error instead).
    var onNotification: ((BackendNotification) -> Void)?
    /// Last 500 stderr lines of the current (or last) child.
    var stderrLog: [String] = []
    var isMock: Bool = false
    var binaryURL: URL?
    /// The `initialize` result of the running child.
    var initializeResult: InitializeResult?
    /// Set by `start()` when it throws; cleared on the next successful start.
    var lastStartFailure: StartFailure?
    /// True from `stop()` until the next `start()`.
    private(set) var stopRequested = false

    @ObservationIgnored private var process: BackendProcess?
    @ObservationIgnored private var connection: RPCConnection?
    @ObservationIgnored private var readerTask: Task<Void, Never>?
    @ObservationIgnored private var deliveryTask: Task<Void, Never>?
    @ObservationIgnored private var exitTask: Task<Void, Never>?
    @ObservationIgnored private var initialized = false

    init() {}

    var isRunning: Bool {
        if case .running = state { return true }
        return false
    }

    /// Running and past `initialize`: requests can be sent.
    var isReady: Bool { isRunning && initializeResult != nil }

    // MARK: - Lifecycle

    /// Spawns `ctx serve [--mock] [--data-dir DIR]`, sends `initialize` and
    /// checks the protocol version. Throws `BackendError`; an incompatible
    /// binary throws `.rpc(kind: "incompatible")` (`isIncompatible`).
    func start(binary: URL, environment: [String: String], dataDir: URL?, mock: Bool) async throws -> InitializeResult {
        try await start(launch: .local(binary: binary), environment: environment, dataDir: dataDir, mock: mock)
    }

    /// `start(binary:…)` for any launch: the local binary, or `ssh <host>
    /// ctx serve` (`BackendLaunch`), which speaks the same protocol over the
    /// SSH session's pipes. `dataDir` applies to the local binary only (the
    /// remote server uses its own).
    func start(launch: BackendLaunch, environment: [String: String], dataDir: URL?, mock: Bool) async throws -> InitializeResult {
        if process != nil { await stop() }
        stopRequested = false
        initialized = false
        state = .starting
        isMock = mock
        self.launch = launch
        let binary = launch.executable
        binaryURL = launch.isRemote ? nil : binary
        initializeResult = nil
        stderrLog = []

        var serveArguments = ["serve"]
        if mock { serveArguments.append("--mock") }
        if let dataDir, !launch.isRemote { serveArguments += ["--data-dir", dataDir.path] }
        let arguments = launch.arguments(serve: serveArguments)
        var environment = environment
        if mock, (environment["CTX_MOCK_PACE_MS"] ?? "").isEmpty {
            // Pace the scripted mock (20 ms per streamed delta, ~1.4 s per run)
            // so streaming text, running tool rows and the status bar can be
            // seen; `ctx serve --mock` itself stays instant.
            environment["CTX_MOCK_PACE_MS"] = "20"
        }

        let proc = BackendProcess(binary: binary, arguments: arguments, environment: environment)
        let (stream, continuation) = AsyncStream<BackendNotification>.makeStream(bufferingPolicy: .unbounded)
        let conn = RPCConnection(output: proc.stdinHandle) { raw in
            if let notification = BackendClient.decodeNotification(raw) {
                continuation.yield(notification)
            }
        }
        proc.onStderrLines = { [weak self] lines in
            Task { @MainActor in self?.appendStderr(lines) }
        }

        do {
            try proc.launch()
        } catch {
            continuation.finish()
            let message = "Could not launch \(launch.label): \(error.localizedDescription)"
            lastStartFailure = .launchFailed(message)
            state = .failed(message)
            throw BackendError.transport(message)
        }

        process = proc
        connection = conn
        state = .running(pid: proc.processIdentifier)

        let reader = Task.detached(priority: .userInitiated) {
            for await line in proc.stdoutLines {
                await conn.receive(line: line)
            }
        }
        readerTask = reader

        deliveryTask = Task { @MainActor [weak self] in
            for await notification in stream {
                self?.onNotification?(notification)
            }
        }

        exitTask = Task.detached { [weak self] in
            guard let code = await proc.waitForExit() else { return }
            // stdout first: every line the child wrote reaches the actor before pending calls fail.
            let closer = Task {
                try? await Task.sleep(nanoseconds: 3_000_000_000)
                proc.forceCloseStdout()
            }
            await reader.value
            closer.cancel()
            await conn.close(with: .transport("ctx exited (\(code))"))
            proc.closeStdin()
            let tail = proc.stderrTail()
            let shouldEmit = await MainActor.run { [weak self] in self?.handleExit(of: proc, code: code) ?? false }
            if shouldEmit {
                continuation.yield(.exited(code: code, stderrTail: tail))
            }
            continuation.finish()
        }

        do {
            let result = try await Self.withTimeout(Self.initializeTimeout) { [self] in
                try await self.initialize()
            }
            guard result.protocolVersion == Self.protocolVersion else {
                let detail = "ctx \(result.ctxVersion) speaks desktop protocol \(result.protocolVersion); this app needs protocol \(Self.protocolVersion)."
                await stop()
                lastStartFailure = .incompatible(found: launch.label, detail: detail)
                state = .failed(detail)
                throw BackendError.incompatible(detail)
            }
            initialized = true
            initializeResult = result
            lastStartFailure = nil
            return result
        } catch let error as BackendError {
            if error.isIncompatible { throw error }
            // Give a fast-failing child a moment to be reaped so its stderr is complete.
            let code = await proc.waitForExit(timeout: 1)
            let tail = proc.stderrTail()
            await stop()
            if let code, Self.looksIncompatible(stderr: tail) {
                let detail = "\(launch.label) does not support `ctx serve` (exit \(code)): \(Self.firstLine(of: tail))"
                lastStartFailure = .incompatible(found: launch.label, detail: detail)
                state = .failed(detail)
                throw BackendError.incompatible(detail)
            }
            if let code {
                let what = launch.isRemote ? "\(launch.label) exited (\(code))" : "ctx exited (\(code))"
                let message = what + " before answering initialize" + (tail.isEmpty ? "" : ": \(Self.lastLine(of: tail))")
                lastStartFailure = .exited(code: code, tail: tail)
                state = .failed(message)
                throw BackendError.transport(message)
            }
            let message: String
            switch error {
            case .transport(let m): message = m
            case .rpc(_, let m, _): message = "initialize failed: \(m)"
            case .decoding(let m): message = m
            default: message = error.localizedDescription
            }
            lastStartFailure = .transport(message)
            state = .failed(message)
            throw error
        } catch {
            await stop()
            let message = error.localizedDescription
            lastStartFailure = .transport(message)
            state = .failed(message)
            throw BackendError.transport(message)
        }
    }

    /// `shutdown` → wait ≤ 2 s → SIGTERM → 1 s → SIGKILL. Never throws; the
    /// `.exited` notification is suppressed for this exit.
    func stop() async {
        stopRequested = true
        guard let proc = process else {
            state = .stopped
            return
        }
        if proc.isRunning {
            if let conn = connection {
                let shutdown = Task { _ = try? await conn.call("shutdown") }
                let exited = await proc.waitForExit(timeout: 2) != nil
                shutdown.cancel()
                if !exited {
                    proc.terminate()
                    if await proc.waitForExit(timeout: 1) == nil {
                        proc.forceKill()
                    }
                }
            } else {
                proc.terminate()
                if await proc.waitForExit(timeout: 1) == nil {
                    proc.forceKill()
                }
            }
            _ = await proc.waitForExit(timeout: 2)
        }
        process = nil
        connection = nil
        readerTask = nil
        initialized = false
        state = .stopped
    }

    // MARK: - Generic RPC

    /// Raw request; the typed methods below wrap it.
    func call(_ method: String, params: JSONValue? = nil) async throws -> JSONValue {
        guard let connection else { throw BackendError.notRunning }
        do {
            return try await connection.call(method, params: params)
        } catch let error as BackendError {
            throw error
        } catch {
            throw BackendError.transport(error.localizedDescription)
        }
    }

    /// Typed results are decoded off the main actor: a long session's
    /// history is megabytes of events, and the re-encode / decode pass must
    /// not stall the UI.
    private func request<T: Decodable>(_ method: String, params: JSONValue? = nil) async throws -> T {
        let value = try await call(method, params: params)
        return try await Task.detached(priority: .userInitiated) { try Self.decode(T.self, from: value, context: method) }.value
    }

    // MARK: - Methods

    func initialize() async throws -> InitializeResult {
        try await request("initialize")
    }

    func configGet() async throws -> InitializeResult {
        try await request("config/get")
    }

    func configPatch(values: [String: JSONValue]) async throws -> InitializeResult {
        try await request("config/patch", params: .object(["values": .object(values)]))
    }

    /// `initGit` asks the backend to `git init` a folder outside any repository first.
    func projectOpen(path: String, exact: Bool = false, initGit: Bool = false) async throws -> ProjectInfo {
        try await request("project/open", params: object(["path": .string(path), "exact": .bool(exact), "init_git": .bool(initGit)]))
    }

    func projectClose(id: String) async throws {
        _ = try await call("project/close", params: object(["project_id": .string(id)]))
    }

    func projectStatus(id: String) async throws -> ProjectInfo {
        try await request("project/status", params: object(["project_id": .string(id)]))
    }

    func indexRebuild(id: String) async throws {
        _ = try await call("index/rebuild", params: object(["project_id": .string(id)]))
    }

    func sessionsList(projectId: String, limit: Int = 100) async throws -> [SessionSummary] {
        let envelope: SessionsEnvelope = try await request("sessions/list", params: object(["project_id": .string(projectId), "limit": .number(Double(limit))]))
        return envelope.sessions
    }

    func sessionCreate(projectId: String, title: String? = nil) async throws -> SessionSummary {
        try await request("session/create", params: object(["project_id": .string(projectId), "title": title.map(JSONValue.string)]))
    }

    func sessionRename(projectId: String, sessionId: String, title: String) async throws -> SessionSummary {
        try await request("session/rename", params: object(["project_id": .string(projectId), "session_id": .string(sessionId), "title": .string(title)]))
    }

    func sessionDelete(projectId: String, sessionId: String) async throws {
        _ = try await call("session/delete", params: object(["project_id": .string(projectId), "session_id": .string(sessionId)]))
    }

    /// Removes user message `messageId` and everything after it; `restoreFiles`
    /// puts the working tree back to the snapshot taken before it.
    func sessionRewind(projectId: String, sessionId: String, messageId: Int, restoreFiles: Bool) async throws -> RewindResult {
        try await request("session/rewind", params: object([
            "project_id": .string(projectId),
            "session_id": .string(sessionId),
            "message_id": .number(Double(messageId)),
            "restore_files": .bool(restoreFiles),
        ]))
    }

    func sessionHistory(projectId: String, sessionId: String) async throws -> SessionHistory {
        try await request("session/history", params: object(["project_id": .string(projectId), "session_id": .string(sessionId)]))
    }

    func taskRun(projectId: String, sessionId: String, prompt: String, images: [String] = [], inlineImages: [InlineImage] = [], files: [FileUpload] = [], options: TaskRunOptions = .default) async throws {
        let encodedOptions = try Self.encode(options, context: "task/run options")
        _ = try await call("task/run", params: object([
            "project_id": .string(projectId),
            "session_id": .string(sessionId),
            "prompt": .string(prompt),
            "images": Self.imageParams(paths: images, inline: inlineImages),
            "files": Self.fileParams(files),
            "options": encodedOptions,
        ]))
    }

    /// `task/run` / `task/steer` `images`: paths as strings, inline images as
    /// `{name, data}` objects; nil when there are none.
    nonisolated static func imageParams(paths: [String], inline: [InlineImage]) -> JSONValue? {
        let values = paths.map(JSONValue.string) + inline.map { JSONValue.object(["name": .string($0.name), "data": .string($0.data)]) }
        return values.isEmpty ? nil : .array(values)
    }

    /// `files`: `{path, name}` for files the backend reads itself, `{name,
    /// content[, path]}` for text sent inline.
    nonisolated static func fileParams(_ files: [FileUpload]) -> JSONValue? {
        guard !files.isEmpty else { return nil }
        return .array(files.map { file in
            var object: [String: JSONValue] = ["name": .string(file.name)]
            if let content = file.content {
                object["content"] = .string(content)
                if !file.path.isEmpty { object["path"] = .string(file.path) }
            } else {
                object["path"] = .string(file.path)
            }
            return .object(object)
        })
    }

    /// `fs/list`: a directory of the backend's machine (`path` nil = home).
    func fsList(path: String?, dirsOnly: Bool = false) async throws -> FsListing {
        try await request("fs/list", params: object(["path": path.map(JSONValue.string), "dirs_only": .bool(dirsOnly)]))
    }

    /// Hands a message to `sessionId`'s running turn (read at its next step);
    /// `BackendError.noTaskRunning` when the run is idle or ending.
    func taskSteer(projectId: String, sessionId: String, prompt: String, images: [String] = [], inlineImages: [InlineImage] = [], files: [FileUpload] = []) async throws {
        _ = try await call("task/steer", params: object([
            "project_id": .string(projectId),
            "session_id": .string(sessionId),
            "prompt": .string(prompt),
            "images": Self.imageParams(paths: images, inline: inlineImages),
            "files": Self.fileParams(files),
        ]))
    }

    /// Cancels `sessionId`'s run; without one, every run of the project.
    func taskCancel(projectId: String, sessionId: String? = nil) async throws {
        _ = try await call("task/cancel", params: object(["project_id": .string(projectId), "session_id": sessionId.map(JSONValue.string)]))
    }

    func approvalRespond(projectId: String, sessionId: String, requestId: String, decision: ApprovalDecision) async throws {
        _ = try await call("approval/respond", params: object(["project_id": .string(projectId), "session_id": .string(sessionId), "request_id": .string(requestId), "decision": .string(decision.rawValue)]))
    }

    /// Answers a `browser/request` notification.
    func browserRespond(projectId: String, sessionId: String, requestId: String, reply: BrowserReply) async throws {
        _ = try await call("browser/respond", params: object([
            "project_id": .string(projectId), "session_id": .string(sessionId), "request_id": .string(requestId),
            "ok": .bool(reply.ok), "content": .string(reply.content),
            "url": reply.url.map(JSONValue.string), "title": reply.title.map(JSONValue.string), "error": reply.error.map(JSONValue.string),
        ]))
    }

    /// Answers a `question/request`: one answer per question (nil: none), and/or
    /// a free-form message; neither tells the agent the user skipped.
    func questionRespond(projectId: String, sessionId: String, requestId: String, answers: [QuestionAnswer]?, message: String?) async throws {
        let answerValues = answers.map { list in
            JSONValue.array(list.map { answer in
                object(["selected": .array(answer.selected.map(JSONValue.string)), "other": answer.other.map(JSONValue.string)])
            })
        }
        _ = try await call("question/respond", params: object([
            "project_id": .string(projectId), "session_id": .string(sessionId), "request_id": .string(requestId),
            "answers": answerValues, "message": message.map(JSONValue.string),
        ]))
    }

    func permissionsSet(projectId: String, profile: String) async throws -> ProjectInfo {
        try await request("permissions/set", params: object(["project_id": .string(projectId), "profile": .string(profile)]))
    }

    func modelsList(projectId: String? = nil, refresh: Bool = false) async throws -> ModelsList {
        try await request("models/list", params: object(["project_id": projectId.map(JSONValue.string), "refresh": .bool(refresh)]))
    }

    func modelsSwitch(projectId: String, model: String) async throws -> ModelSwitchResult {
        try await request("models/switch", params: object(["project_id": .string(projectId), "model": .string(model)]))
    }

    /// `suggest/next`: the user's likely next message, or nil.
    func suggestNext(projectId: String, sessionId: String) async throws -> String? {
        let result: NextMessageSuggestion = try await request("suggest/next", params: object(["project_id": .string(projectId), "session_id": .string(sessionId)]))
        return result.suggestion
    }

    /// `suggest/starters`: prompts for the empty chat (empty when the model failed).
    func suggestStarters(projectId: String) async throws -> [StarterSuggestion] {
        let result: StarterSuggestions = try await request("suggest/starters", params: object(["project_id": .string(projectId)]))
        return result.suggestions
    }

    func gitStatus(projectId: String) async throws -> GitStatus {
        try await request("git/status", params: object(["project_id": .string(projectId)]))
    }

    func gitDiff(projectId: String, path: String? = nil, staged: Bool = false) async throws -> GitDiff {
        try await request("git/diff", params: object(["project_id": .string(projectId), "path": path.map(JSONValue.string), "staged": .bool(staged)]))
    }

    // MARK: Git actions (Changes tab)

    private func paths(_ projectId: String, _ paths: [String]) -> JSONValue {
        object(["project_id": .string(projectId), "paths": .array(paths.map(JSONValue.string))])
    }

    /// Empty `paths` stages every change.
    func gitStage(projectId: String, paths: [String]) async throws {
        _ = try await call("git/stage", params: self.paths(projectId, paths))
    }

    func gitUnstage(projectId: String, paths: [String]) async throws {
        _ = try await call("git/unstage", params: self.paths(projectId, paths))
    }

    /// Tracked files go back to the index, untracked ones are deleted.
    func gitDiscard(projectId: String, paths: [String]) async throws -> GitDiscardResult {
        try await request("git/discard", params: self.paths(projectId, paths))
    }

    /// `all` stages everything first; `paths` commits only those files.
    func gitCommit(projectId: String, message: String, amend: Bool = false, all: Bool = false, paths: [String] = []) async throws -> GitCommitResult {
        try await request("git/commit", params: object([
            "project_id": .string(projectId),
            "message": .string(message),
            "amend": .bool(amend),
            "all": .bool(all),
            "paths": .array(paths.map(JSONValue.string)),
        ]))
    }

    /// `git reset --soft HEAD~1`; returns the undone commit.
    func gitUndoCommit(projectId: String) async throws -> GitCommitResult {
        try await request("git/undo_commit", params: object(["project_id": .string(projectId)]))
    }

    func gitPush(projectId: String, remote: String?, branch: String?, setUpstream: Bool, force: Bool) async throws -> GitCommandOutput {
        try await request("git/push", params: object([
            "project_id": .string(projectId),
            "remote": remote.map(JSONValue.string),
            "branch": branch.map(JSONValue.string),
            "set_upstream": .bool(setUpstream),
            "force": .bool(force),
        ]))
    }

    func gitPull(projectId: String, rebase: Bool) async throws -> GitCommandOutput {
        try await request("git/pull", params: object(["project_id": .string(projectId), "rebase": .bool(rebase)]))
    }

    func gitFetch(projectId: String, prune: Bool = true) async throws -> GitCommandOutput {
        try await request("git/fetch", params: object(["project_id": .string(projectId), "prune": .bool(prune)]))
    }

    func gitBranches(projectId: String) async throws -> GitBranches {
        try await request("git/branches", params: object(["project_id": .string(projectId)]))
    }

    /// Returns the branch now checked out.
    func gitCheckout(projectId: String, branch: String, create: Bool) async throws -> String? {
        let value = try await call("git/checkout", params: object(["project_id": .string(projectId), "branch": .string(branch), "create": .bool(create)]))
        if case .object(let fields) = value, case .string(let name)? = fields["branch"] { return name }
        return nil
    }

    func gitLog(projectId: String, limit: Int = 20) async throws -> [GitLogEntry] {
        struct Log: Decodable { var commits: [GitLogEntry] }
        let log: Log = try await request("git/log", params: object(["project_id": .string(projectId), "limit": .number(Double(limit))]))
        return log.commits
    }

    func gitCommitMessage(projectId: String) async throws -> GitCommitMessage {
        try await request("git/commit_message", params: object(["project_id": .string(projectId)]))
    }

    // MARK: Worktrees (`worktree/*`, docs/desktop-protocol.md "Worktrees")

    /// Creates a linked worktree of the project's repository, opens it as a
    /// project and returns that project's info.
    func worktreeCreate(projectId: String, name: String? = nil, base: String? = nil) async throws -> ProjectInfo {
        try await request("worktree/create", params: object(["project_id": .string(projectId), "name": name.map(JSONValue.string), "base": base.map(JSONValue.string)]))
    }

    func worktreeList(projectId: String) async throws -> [WorktreeEntry] {
        struct Envelope: Decodable { var worktrees: [WorktreeEntry] }
        let envelope: Envelope = try await request("worktree/list", params: object(["project_id": .string(projectId)]))
        return envelope.worktrees
    }

    /// Closes the worktree's project and removes the checkout (and its branch on request).
    func worktreeRemove(projectId: String, deleteBranch: Bool, force: Bool) async throws -> WorktreeRemoveResult {
        try await request("worktree/remove", params: object(["project_id": .string(projectId), "delete_branch": .bool(deleteBranch), "force": .bool(force)]))
    }

    /// Merges the worktree's branch into the parent checkout (`--no-ff`) and,
    /// with `deleteAfter`, removes the worktree and its branch.
    func worktreeMerge(projectId: String, deleteAfter: Bool) async throws -> WorktreeMergeResult {
        try await request("worktree/merge", params: object(["project_id": .string(projectId), "delete_after": .bool(deleteAfter)]))
    }

    func fileRead(projectId: String, path: String, maxBytes: Int? = nil) async throws -> FileContent {
        try await request("file/read", params: object(["project_id": .string(projectId), "path": .string(path), "max_bytes": maxBytes.map { .number(Double($0)) }]))
    }

    /// Replaces the file atomically; `expectedHash` (from `file/read`) makes a
    /// file changed meanwhile a `conflict` error instead of an overwrite.
    /// `files/search`: repository paths best matching `query`, for `@` completion.
    func filesSearch(projectId: String, query: String, limit: Int = 8) async throws -> [String] {
        struct Envelope: Decodable { var files: [String] }
        let envelope: Envelope = try await request("files/search", params: .object(["project_id": .string(projectId), "query": .string(query), "limit": .number(Double(limit))]))
        return envelope.files
    }

    func fileWrite(projectId: String, path: String, content: String, expectedHash: String? = nil) async throws -> FileWriteResult {
        try await request("file/write", params: object(["project_id": .string(projectId), "path": .string(path), "content": .string(content), "expected_hash": expectedHash.map { .string($0) }]))
    }

    func symbolsFind(projectId: String, query: String, mode: SymbolsFindMode = .prefix, limit: Int = 50) async throws -> [SymbolInfo] {
        struct Envelope: Decodable { var symbols: [SymbolInfo] }
        let envelope: Envelope = try await request("symbols/find", params: object(["project_id": .string(projectId), "query": .string(query), "mode": .string(mode.rawValue), "limit": .number(Double(limit))]))
        return envelope.symbols
    }

    // MARK: Language servers (the editor)

    func lspStatus(projectId: String, path: String) async throws -> LspStatus {
        try await request("lsp/status", params: object(["project_id": .string(projectId), "path": .string(path)]))
    }

    /// `didOpen` with the buffer; starts the language's server on first use.
    func lspOpen(projectId: String, path: String, text: String) async throws -> LspStatus {
        try await request("lsp/open", params: object(["project_id": .string(projectId), "path": .string(path), "text": .string(text)]))
    }

    func lspChange(projectId: String, path: String, text: String) async throws {
        struct Envelope: Decodable { var version: Int? }
        let _: Envelope = try await request("lsp/change", params: object(["project_id": .string(projectId), "path": .string(path), "text": .string(text)]))
    }

    func lspClose(projectId: String, path: String) async throws {
        let _: JSONValue = try await request("lsp/close", params: object(["project_id": .string(projectId), "path": .string(path)]))
    }

    /// `line` / `character` are 0-based.
    func lspCompletion(projectId: String, path: String, line: Int, character: Int) async throws -> LspCompletionResult {
        try await request("lsp/completion", params: object(["project_id": .string(projectId), "path": .string(path), "line": .number(Double(line)), "character": .number(Double(character))]))
    }

    func lspHover(projectId: String, path: String, line: Int, character: Int) async throws -> LspHover {
        try await request("lsp/hover", params: object(["project_id": .string(projectId), "path": .string(path), "line": .number(Double(line)), "character": .number(Double(character))]))
    }

    func lspDefinition(projectId: String, path: String, line: Int, character: Int) async throws -> [LspLocation] {
        struct Envelope: Decodable { var locations: [LspLocation] }
        let envelope: Envelope = try await request("lsp/definition", params: object(["project_id": .string(projectId), "path": .string(path), "line": .number(Double(line)), "character": .number(Double(character))]))
        return envelope.locations
    }

    func tasksList(projectId: String, limit: Int = 50) async throws -> [TaskSummary] {
        let envelope: TasksEnvelope = try await request("tasks/list", params: object(["project_id": .string(projectId), "limit": .number(Double(limit))]))
        return envelope.tasks
    }

    func memoryList(projectId: String, category: String? = nil, limit: Int = 50) async throws -> [MemoryEntry] {
        let envelope: MemoriesEnvelope = try await request("memory/list", params: object(["project_id": .string(projectId), "category": category.map(JSONValue.string), "limit": .number(Double(limit))]))
        return envelope.memories
    }

    func doctorRun(projectId: String? = nil, probe: Bool = false) async throws -> DoctorReport {
        try await request("doctor/run", params: object(["project_id": projectId.map(JSONValue.string), "probe": .bool(probe)]))
    }

    // MARK: MCP (`mcp/*`, docs/desktop-protocol.md "MCP servers")

    func mcpStatus() async throws -> [McpServerStatus] {
        let envelope: McpStatusNotification = try await request("mcp/status", params: .object([:]))
        return envelope.servers
    }

    /// Reconnects one server (or every active one) and returns the state afterwards.
    func mcpReconnect(server: String? = nil) async throws -> [McpServerStatus] {
        let envelope: McpStatusNotification = try await request("mcp/reconnect", params: object(["server": server.map(JSONValue.string)]))
        return envelope.servers
    }

    func mcpCatalog() async throws -> [McpCatalogApp] {
        let envelope: McpCatalogResponse = try await request("mcp/catalog", params: .object([:]))
        return envelope.apps
    }

    /// Installs a catalog app with the user's input values; returns the configuration afterwards.
    func mcpInstall(id: String, inputs: [String: String], excludeTools: [String] = []) async throws -> InitializeResult {
        try await request("mcp/install", params: .object(["id": .string(id), "inputs": .object(inputs.mapValues(JSONValue.string)), "exclude_tools": .array(excludeTools.map(JSONValue.string))]))
    }

    func mcpUninstall(id: String) async throws -> InitializeResult {
        try await request("mcp/uninstall", params: .object(["id": .string(id)]))
    }

    func mcpOAuthStart(app: String) async throws -> McpDeviceCode {
        try await request("mcp/oauth/start", params: .object(["app": .string(app)]))
    }

    func mcpOAuthPoll(app: String, deviceCode: String) async throws -> McpDevicePoll {
        try await request("mcp/oauth/poll", params: .object(["app": .string(app), "device_code": .string(deviceCode)]))
    }

    func mcpOAuthAccount(app: String) async throws -> String? {
        struct Envelope: Decodable { var account: String? }
        let envelope: Envelope = try await request("mcp/oauth/account", params: .object(["app": .string(app)]))
        return envelope.account
    }

    func mcpOAuthSignOut(app: String) async throws {
        _ = try await call("mcp/oauth/signout", params: .object(["app": .string(app)]))
    }

    func mcpPermissions() async throws -> McpPermissions {
        try await request("mcp/permissions", params: .object([:]))
    }

    // MARK: ModelGateway (`gateway/*`, docs/desktop-protocol.md "ModelGateway")

    func gatewayDeviceStart(baseURL: String, deviceName: String?) async throws -> GatewayDeviceStart {
        try await request("gateway/device/start", params: object(["base_url": .string(baseURL), "device_name": deviceName.map(JSONValue.string)]))
    }

    func gatewayDevicePoll(baseURL: String, deviceCode: String) async throws -> GatewayDevicePoll {
        try await request("gateway/device/poll", params: object(["base_url": .string(baseURL), "device_code": .string(deviceCode)]))
    }

    /// Whether the backend's credential store holds a device key.
    func gatewayStatus() async throws -> GatewayStatus {
        try await request("gateway/status")
    }

    /// The remaining calls authenticate with that stored key.
    func gatewayAccount(baseURL: String) async throws -> GatewayAccount {
        try await request("gateway/account", params: object(["base_url": .string(baseURL)]))
    }

    func gatewayUsage(baseURL: String, startTimestamp: Int64?, endTimestamp: Int64?) async throws -> GatewayUsageReport {
        try await request("gateway/usage", params: object([
            "base_url": .string(baseURL),
            "start_timestamp": startTimestamp.map { .number(Double($0)) },
            "end_timestamp": endTimestamp.map { .number(Double($0)) },
        ]))
    }

    /// Revokes the key on the gateway and removes it from the store.
    func gatewaySignOut(baseURL: String) async throws -> GatewaySignOutResult {
        try await request("gateway/signout", params: object(["base_url": .string(baseURL)]))
    }

    /// Top-up options, plans on sale and the billing preference.
    func gatewayBilling(baseURL: String) async throws -> GatewayBilling {
        try await request("gateway/billing", params: object(["base_url": .string(baseURL)]))
    }

    func gatewayBillingQuote(baseURL: String, amount: Int) async throws -> GatewayTopUpQuote {
        try await request("gateway/billing/quote", params: object(["base_url": .string(baseURL), "amount": .number(Double(amount))]))
    }

    /// A checkout page for adding `amount` to the balance.
    func gatewayBillingTopUp(baseURL: String, amount: Int) async throws -> GatewayCheckout {
        try await request("gateway/billing/topup", params: object(["base_url": .string(baseURL), "amount": .number(Double(amount))]))
    }

    /// `method` "stripe" returns a checkout page; "balance" buys at once.
    func gatewayBillingSubscribe(baseURL: String, planId: Int, method: String) async throws -> GatewayCheckout {
        try await request("gateway/billing/subscribe", params: object(["base_url": .string(baseURL), "plan_id": .number(Double(planId)), "method": .string(method)]))
    }

    func gatewayBillingPreference(baseURL: String, preference: String) async throws -> GatewayPreferenceResult {
        try await request("gateway/billing/preference", params: object(["base_url": .string(baseURL), "billing_preference": .string(preference)]))
    }

    /// Claims the welcome Context credit; the reply is the new summary.
    func gatewayClaimContextCredit(baseURL: String) async throws -> GatewayContextCredit {
        try await request("gateway/context_credit/claim", params: object(["base_url": .string(baseURL)]))
    }

    /// The device key's price limits and spending cap.
    func gatewayKeySettings(baseURL: String) async throws -> GatewayKeySettings {
        try await request("gateway/key", params: object(["base_url": .string(baseURL)]))
    }

    /// `values`: any of `price_limits`, `unlimited_quota`, `remain_quota`.
    func gatewayUpdateKeySettings(baseURL: String, values: [String: JSONValue]) async throws -> GatewayKeySettings {
        var params = values
        params["base_url"] = .string(baseURL)
        return try await request("gateway/key/update", params: .object(params))
    }

    // MARK: Spending (`spend/summary`, `pricing/lookup`)

    func spendSummary() async throws -> SpendSummary {
        try await request("spend/summary")
    }

    func pricingLookup(models: [String]) async throws -> [PriceEntry] {
        let result: PriceLookupResult = try await request("pricing/lookup", params: object(["models": .array(models.map(JSONValue.string))]))
        return result.prices
    }

    /// Asks the child to exit. The exit that follows counts as requested:
    /// `state` becomes `.stopped` and no `.exited` notification is sent.
    /// Prefer `stop()`, which also escalates when the child does not comply.
    func shutdown() async throws {
        guard connection != nil else { throw BackendError.notRunning }
        stopRequested = true
        _ = try await call("shutdown")
    }

    // MARK: - Exit and stderr (main actor)

    /// Returns whether an `.exited` notification should be delivered.
    private func handleExit(of proc: BackendProcess, code: Int32) -> Bool {
        guard proc === process else { return false }   // already torn down by stop(), or a newer child
        process = nil
        connection = nil
        readerTask = nil
        if stopRequested {
            state = .stopped
            return false
        }
        guard initialized else { return false }        // start() reports this through its error
        initialized = false
        state = .failed("ctx exited (\(code))")
        return true
    }

    private func appendStderr(_ lines: [String]) {
        stderrLog.append(contentsOf: lines)
        if stderrLog.count > BackendProcess.stderrRingCapacity {
            stderrLog.removeFirst(stderrLog.count - BackendProcess.stderrRingCapacity)
        }
    }

    // MARK: - Helpers

    private func object(_ pairs: [String: JSONValue?]) -> JSONValue {
        .object(pairs.compactMapValues { $0 })
    }

    nonisolated static func looksIncompatible(stderr: String) -> Bool {
        let lowered = stderr.lowercased()
        return lowered.contains("unrecognized subcommand") || lowered.contains("unexpected argument")
    }

    nonisolated private static func firstLine(of text: String) -> String {
        text.split(whereSeparator: \.isNewline).map(String.init).first(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }) ?? ""
    }

    /// ssh prints its own diagnosis last ("Permission denied (publickey).",
    /// "bash: ctx: command not found"), after any banner.
    nonisolated private static func lastLine(of text: String) -> String {
        text.split(whereSeparator: \.isNewline).map(String.init).last(where: { !$0.trimmingCharacters(in: .whitespaces).isEmpty }) ?? ""
    }

    /// Decodes a typed value from a `result` / `params` subtree by re-encoding it.
    nonisolated static func decode<T: Decodable>(_ type: T.Type, from value: JSONValue, context: String) throws -> T {
        do {
            let data = try JSONEncoder().encode(value)
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw BackendError.decoding("\(context): \(Self.describe(error))")
        }
    }

    nonisolated static func encode<T: Encodable>(_ value: T, context: String) throws -> JSONValue {
        do {
            let data = try JSONEncoder().encode(value)
            return try JSONDecoder().decode(JSONValue.self, from: data)
        } catch {
            throw BackendError.decoding("\(context): \(Self.describe(error))")
        }
    }

    /// Maps a notification frame onto `BackendNotification`; unknown methods
    /// and undecodable params are dropped (logged in debug).
    nonisolated static func decodeNotification(_ raw: RPCNotification) -> BackendNotification? {
        let decoder = JSONDecoder()
        func params<P: Decodable>(_ type: P.Type) -> P? {
            do {
                return try decoder.decode(ParamsEnvelope<P>.self, from: raw.raw).params
            } catch {
                BackendLog.debug("undecodable \(raw.method) params: \(describe(error))")
                return nil
            }
        }
        switch raw.method {
        case "event":
            return params(EventNotification.self).map { .event($0) }
        case "task/finished":
            return params(TaskFinished.self).map { .taskFinished($0) }
        case "index/progress":
            return params(IndexProgress.self).map { .indexProgress($0) }
        case "index/ready":
            return params(IndexReady.self).map { .indexReady($0) }
        case "log":
            return params(LogNotification.self).map { .log($0) }
        case "project/changed":
            return .projectChanged(raw.params ?? .null)
        case "lsp/diagnostics":
            return params(LspDiagnostics.self).map { .lspDiagnostics($0) }
        case "browser/request":
            return params(BrowserRequest.self).map { .browserRequest($0) }
        case "question/request":
            return params(QuestionRequest.self).map { .questionRequest($0) }
        case "mcp/status":
            return params(McpStatusNotification.self).map { .mcpStatus($0.servers) }
        default:
            BackendLog.debug("ignored notification \(raw.method)")
            return nil
        }
    }

    nonisolated private static func describe(_ error: Error) -> String {
        if let decoding = error as? DecodingError {
            switch decoding {
            case .keyNotFound(let key, let context):
                return "missing key \"\(key.stringValue)\" at \(path(context))"
            case .typeMismatch(let type, let context):
                return "expected \(type) at \(path(context))"
            case .valueNotFound(let type, let context):
                return "null \(type) at \(path(context))"
            case .dataCorrupted(let context):
                return context.debugDescription
            @unknown default:
                return String(describing: decoding)
            }
        }
        return error.localizedDescription
    }

    nonisolated private static func path(_ context: DecodingError.Context) -> String {
        let joined = context.codingPath.map(\.stringValue).joined(separator: ".")
        return joined.isEmpty ? "root" : joined
    }

    /// Races `operation` against a timer; the operation must respond to cancellation.
    nonisolated private static func withTimeout<T>(_ seconds: TimeInterval, _ operation: @escaping () async throws -> T) async throws -> T {
        try await withThrowingTaskGroup(of: T.self) { group in
            group.addTask { try await operation() }
            group.addTask {
                try await Task.sleep(nanoseconds: UInt64(max(0, seconds) * 1_000_000_000))
                throw BackendError.transport("ctx did not answer within \(Int(seconds)) s")
            }
            guard let result = try await group.next() else {
                throw BackendError.transport("ctx did not answer")
            }
            group.cancelAll()
            return result
        }
    }
}

// MARK: - Private wire envelopes

private struct ParamsEnvelope<P: Decodable>: Decodable {
    var params: P
}

private struct SessionsEnvelope: Decodable {
    var sessions: [SessionSummary]
}

private struct TasksEnvelope: Decodable {
    var tasks: [TaskSummary]
}

private struct MemoriesEnvelope: Decodable {
    var memories: [MemoryEntry]
}
