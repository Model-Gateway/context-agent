import SwiftUI
import AppKit
import Foundation

/// Per-project state: sessions and their chats, index/git status, the model
/// catalog, doctor, memories and tasks. Chats are created once and never
/// replaced (`chat(for:)` always returns the same instance).
@MainActor
@Observable
final class ProjectState: Identifiable {
    let id: String
    var info: ProjectInfo

    var sessions: [SessionSummary] = []
    var sessionsLoading = false
    /// Inline error of the last `sessions/list`.
    var sessionsError: String?

    private var chats: [String: ChatState] = [:]

    var indexProgress: IndexProgress?
    var lastIndexReady: IndexReady?

    var gitStatus: GitStatus?
    var gitError: String?
    var selectedChangePath: String?
    var diff: GitDiff?
    var diffLoading = false
    var diffError: String?
    /// Changes tab "staged" toggle.
    var showStaged = false

    // Git actions (Changes tab; see App/GitActions.swift)
    /// The commit box's draft.
    var commitMessage = ""
    /// Label of the git action in flight ("Pushing…"), nil when idle.
    var gitBusy: String?
    var generatingMessage = false
    var branches: GitBranches?
    var commitLog: [GitLogEntry] = []
    /// Commit was asked with nothing staged: the view shows "stage all and commit?".
    var askCommitAll = false

    /// Sessions with a task running: every session of a project can run at
    /// the same time, in the same checkout.
    var runningSessionIds: Set<String> = []
    /// Whether any session of the project is running a task (model switch,
    /// index rebuild, git rewrites and merges wait for all of them).
    var isRunning: Bool { !runningSessionIds.isEmpty }

    func isRunning(session id: String) -> Bool {
        runningSessionIds.contains(id)
    }

    var models: ModelCatalog
    /// The model name a `models/switch` is probing, while it runs.
    var switchingModel: String?

    var doctor: DoctorReport?
    var doctorRunning = false
    var doctorError: String?

    var memories: [MemoryEntry] = []
    var tasks: [TaskSummary] = []
    var extrasLoading = false
    var extrasError: String?

    var lastTab: SidebarItem = .chat

    /// `suggest/starters` under the empty chat's composer; nil until the
    /// first answer (the list shows its skeleton while `startersLoading`).
    var starters: [StarterSuggestion]?
    var startersLoading = false
    @ObservationIgnored var startersLoadedAt: Date?

    /// Called for every newly created chat (AppState wires visibility).
    @ObservationIgnored var onChatCreated: ((ChatState) -> Void)?

    init(info: ProjectInfo, models: ModelCatalog) {
        self.id = info.projectId
        self.info = info
        self.models = models
        self.runningSessionIds = info.runningSessionIds
    }

    // MARK: - Derived

    var name: String { info.name }
    var root: String { info.root }
    var rootURL: URL { URL(fileURLWithPath: info.root) }
    var branch: String? { gitStatus?.branch ?? info.git?.branch }
    var isGitRepository: Bool { info.git != nil }

    // MARK: Worktrees

    /// The main checkout's root when this project is a linked worktree.
    var parentRoot: String? { info.git?.worktree?.parentRoot }
    var isWorktree: Bool { parentRoot != nil }
    /// The branch without the `ctx/` prefix `worktree/create` gives it.
    var worktreeLabel: String {
        guard let branch, !branch.isEmpty else { return name }
        return branch.hasPrefix("ctx/") ? String(branch.dropFirst(4)) : branch
    }
    /// Every worktree of the repository (`worktree/list`), refreshed with git status.
    var worktrees: [WorktreeEntry] = []
    /// True while `worktree/create` runs for this project.
    var creatingWorktree = false

    /// Dirty file count for the sidebar chip.
    var dirtyCount: Int {
        if let gitStatus { return gitStatus.files.count }
        return info.git?.dirtyFiles ?? 0
    }

    /// Lines added / removed across the working tree, for the Inspector Run tab badge.
    var lineChanges: (added: Int, removed: Int) {
        let files = gitStatus?.files ?? []
        return (files.reduce(0) { $0 + ($1.insertions ?? 0) }, files.reduce(0) { $0 + ($1.deletions ?? 0) })
    }

    /// The Inspector Files tab's tree; kept here so expanded folders survive tab switches.
    @ObservationIgnored lazy var fileTree = FileTreeModel(root: info.root)

    var unseenCount: Int {
        chats.values.reduce(0) { $0 + ($1.unseen != nil ? 1 : 0) }
    }

    /// Chats that want attention (unseen, or waiting for an approval or an
    /// answer), each counted once however many reasons it has.
    var attentionCount: Int {
        chats.values.reduce(0) { $0 + ($1.unseen != nil || $1.needsHelp ? 1 : 0) }
    }

    /// Every chat this project has instantiated (running or not).
    var allChats: [ChatState] { Array(chats.values) }

    /// A session's title: its open chat's, else its summary's.
    func chatTitle(_ id: String) -> String? {
        chats[id]?.title ?? session(id)?.title
    }

    /// Chats with queued prompts, earliest queue first.
    var queuedChats: [ChatState] {
        chats.values.filter { !$0.queued.isEmpty }.sorted { ($0.queuedSince ?? .distantFuture) < ($1.queuedSince ?? .distantFuture) }
    }

    var pendingApprovalChats: [ChatState] {
        chats.values.filter { $0.pendingApproval != nil }
    }

    var hasIndexError: Bool { lastIndexReady?.error != nil }

    /// "Index ready · 310 files · generation 12" / "Indexing 120/310" / failure.
    var indexSummary: String {
        if let progress = indexProgress, progress.total > 0 {
            return "Indexing \(progress.done)/\(progress.total)"
        }
        if info.indexing, lastIndexReady == nil {
            return "Indexing…"
        }
        if let error = lastIndexReady?.error {
            return "Indexing failed: \(error) · retrieval falls back to text tools"
        }
        return "Index ready · \(info.index.files) files · generation \(info.index.generation)"
    }

    // MARK: - Chats

    func existingChat(for sessionId: String) -> ChatState? {
        chats[sessionId]
    }

    /// The one `ChatState` per session; created on first use from the session
    /// summary (or a placeholder for runs started elsewhere).
    func chat(for sessionId: String) -> ChatState {
        if let existing = chats[sessionId] { return existing }
        let summary = sessions.first { $0.id == sessionId } ?? SessionSummary(
            id: sessionId,
            model: info.model,
            mode: "desktop",
            title: "New chat",
            createdAt: Self.now(),
            updatedAt: Self.now(),
            activeTaskId: nil,
            taskCount: 0,
            lastStatus: nil
        )
        let chat = ChatState(summary: summary, projectId: id)
        chats[sessionId] = chat
        if !sessions.contains(where: { $0.id == sessionId }) {
            sessions.insert(summary, at: 0)
        }
        onChatCreated?(chat)
        return chat
    }

    func session(_ id: String) -> SessionSummary? {
        sessions.first { $0.id == id }
    }

    /// Inserts or replaces a summary (newest first) and syncs the chat's copy.
    func upsert(session summary: SessionSummary) {
        sessions.removeAll { $0.id == summary.id }
        sessions.insert(summary, at: 0)
        sessions.sort { $0.updatedDate > $1.updatedDate }
        chats[summary.id]?.summary = summary
    }

    func replaceSessions(_ list: [SessionSummary]) {
        sessions = list
        for summary in list {
            if let chat = chats[summary.id] {
                chat.summary = summary
            }
        }
        // Keep placeholders for chats that have content but are missing from the list (very new sessions).
        for (sessionId, chat) in chats where !list.contains(where: { $0.id == sessionId }) && chat.hasContent {
            sessions.append(chat.summary)
        }
    }

    func removeSession(_ sessionId: String) {
        sessions.removeAll { $0.id == sessionId }
        chats.removeValue(forKey: sessionId)
    }

    /// Bumps the local summary after a send so the sidebar reorders immediately.
    func touchSession(_ sessionId: String, firstPrompt: String?) {
        guard let index = sessions.firstIndex(where: { $0.id == sessionId }) else { return }
        var summary = sessions[index]
        summary.updatedAt = Self.now()
        if let prompt = firstPrompt, summary.taskCount == 0, summary.title == "New chat" {
            summary.title = ChatState.clip(prompt, 80)
        }
        sessions.remove(at: index)
        sessions.insert(summary, at: 0)
        chats[sessionId]?.summary = summary
    }

    // MARK: - Backend loads (called by AppState)

    /// The `sessions/list` in flight, so a caller that needs the list can
    /// await it instead of skipping the load (session restore at launch).
    @ObservationIgnored private(set) var sessionsLoad: Task<Void, Never>?

    func loadSessions(backend: BackendClient) async {
        let load = Task { [weak self] in
            guard let self else { return }
            self.sessionsLoading = true
            defer { self.sessionsLoading = false }
            do {
                let list = try await backend.sessionsList(projectId: self.id, limit: 100)
                self.replaceSessions(list)
                self.sessionsError = nil
            } catch {
                self.sessionsError = error.localizedDescription
            }
        }
        sessionsLoad = load
        await load.value
        if sessionsLoad == load { sessionsLoad = nil }
    }

    /// `git/status` (+ `git/diff` for the selected path). Errors stay inline in the Changes tab.
    func refreshGit(backend: BackendClient) async {
        guard isGitRepository else {
            gitStatus = nil
            gitError = nil
            diff = nil
            return
        }
        do {
            let status = try await backend.gitStatus(projectId: id)
            if status != gitStatus { MarkdownInline.invalidateFileReferences() }
            gitStatus = status
            gitError = nil
            if let list = try? await backend.worktreeList(projectId: id) {
                worktrees = list
            }
            if let path = selectedChangePath, !status.files.contains(where: { $0.path == path }) {
                selectedChangePath = nil
                diff = nil
            }
            if selectedChangePath != nil {
                await loadDiff(backend: backend)
            }
        } catch {
            gitError = error.localizedDescription
        }
    }

    func loadDiff(backend: BackendClient) async {
        guard let path = selectedChangePath else {
            diff = nil
            return
        }
        diffLoading = true
        defer { diffLoading = false }
        do {
            let result = try await backend.gitDiff(projectId: id, path: path, staged: showStaged)
            // The selection may have moved while the diff was loading.
            if selectedChangePath == path {
                diff = result
                diffError = nil
            }
        } catch {
            if selectedChangePath == path {
                diff = nil
                diffError = error.localizedDescription
            }
        }
    }

    /// Fresh `project/status` (index stats, git, model).
    func refreshStatus(backend: BackendClient) async {
        if let fresh = try? await backend.projectStatus(id: id) {
            info = fresh
            let backendRunning = fresh.runningSessionIds
            // Runs the app missed are added; a session the backend no longer
            // runs and whose chat is not running either was a stale lock.
            runningSessionIds = runningSessionIds.filter { backendRunning.contains($0) || chats[$0]?.status.isRunning == true }.union(backendRunning)
        }
    }

    /// Recent tasks and memories for the Project tab and the palette.
    func loadExtras(backend: BackendClient) async {
        extrasLoading = true
        defer { extrasLoading = false }
        var failure: String?
        do {
            tasks = try await backend.tasksList(projectId: id, limit: 50)
        } catch {
            failure = error.localizedDescription
        }
        do {
            memories = try await backend.memoryList(projectId: id, category: nil, limit: 50)
        } catch {
            failure = failure ?? error.localizedDescription
        }
        extrasError = failure
    }

    func refreshModels(backend: BackendClient, force: Bool) async {
        models.loading = true
        defer { models.loading = false }
        do {
            let list = try await backend.modelsList(projectId: id, refresh: force)
            models.apply(list)
        } catch {
            models.error = error.localizedDescription
            if models.current == nil { models.current = info.model }
        }
    }

    func runDoctor(backend: BackendClient, probe: Bool) async {
        doctorRunning = true
        defer { doctorRunning = false }
        do {
            doctor = try await backend.doctorRun(projectId: id, probe: probe)
            doctorError = nil
        } catch {
            doctorError = error.localizedDescription
        }
    }

    // MARK: - Helpers

    private static func now() -> String {
        ISO8601DateFormatter().string(from: Date())
    }
}

// MARK: - Project-level intents (AppState API, delegated here to keep AppState.swift short)

extension AppState {
    func renameSession(_ id: String, title: String) async {
        guard let project = selectedProject else { return }
        do {
            let summary = try await backend.sessionRename(projectId: project.id, sessionId: id, title: title.trimmingCharacters(in: .whitespacesAndNewlines))
            project.upsert(session: summary)
        } catch {
            toasts.show("Could not rename the chat: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    func deleteSession(_ id: String) async {
        guard let project = selectedProject else { return }
        do {
            try await backend.sessionDelete(projectId: project.id, sessionId: id)
            project.removeSession(id)
            if selectedSessionId == id { newChat() }
            refreshAttention()
        } catch {
            toasts.show("Could not delete the chat: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    /// `models/switch` for the selected project; failures surface inline in the picker (`models.switchError`) and are rethrown.
    func switchModel(_ name: String) async throws -> ModelSwitchResult {
        let model = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let project = selectedProject, !model.isEmpty else { throw BackendError.notRunning }
        project.switchingModel = model
        project.models.switchError = nil
        defer { project.switchingModel = nil }
        do {
            let result = try await backend.modelsSwitch(projectId: project.id, model: model)
            project.info.model = result.model
            project.models.current = result.model
            project.models.lastSwitch = result
            if !project.models.models.contains(result.model) {
                project.models.models.insert(result.model, at: 0)
            }
            project.models.promote(result.model)
            return result
        } catch {
            project.models.switchError = error.localizedDescription
            throw error
        }
    }

    /// ⌘⇧M and the palette: the picker hangs off the composer, so opening it
    /// switches to the Chat tab first.
    func toggleModelPicker() {
        if modelPickerVisible {
            modelPickerVisible = false
            return
        }
        guard selectedProject != nil else {
            toasts.show("Open a project to choose its model.", kind: .info)
            return
        }
        if sidebarItem != .chat { sidebarItem = .chat }
        modelPickerVisible = true
    }

    /// "GPT-4.1" for "gpt-4.1": models.dev's name for the configured endpoint, else `ModelNames.pretty`.
    func modelDisplayName(_ id: String) -> String {
        modelsDev.displayName(for: id, baseURL: config?.model.baseUrl ?? "")
    }

    func modelInfo(_ id: String) -> ModelsDevInfo? {
        modelsDev.info(for: id, baseURL: config?.model.baseUrl ?? "")
    }

    /// ModelGateway's savings and 24 h error rate for a model of `project`'s
    /// endpoint (the selected project's by default); nil while signed out.
    func modelStat(_ id: String, in project: ProjectState? = nil) -> ModelStat? {
        guard gateway.isSignedIn else { return nil }
        return (project ?? selectedProject)?.models.stats[id]
    }

    /// The picker's hover card: models.dev's entry and/or the gateway's stats; nil with neither.
    func modelCard(_ id: String) -> ModelHoverCard? {
        let info = modelInfo(id)
        let stat = modelStat(id)
        guard info != nil || stat != nil else { return nil }
        return ModelHoverCard(modelId: id, name: modelDisplayName(id), info: info, stat: stat, catalog: modelsDev)
    }

    /// The picker's fetch: a no-op unless the list is missing or stale
    /// (`ModelCatalog.needsLoad`) or `force` (Refresh).
    func refreshModels(force: Bool) async {
        guard let project = selectedProject else { return }
        if force {
            guard !project.models.loading || project.models.models.isEmpty else { return }
            Task { await modelsDev.load(force: true) }
            await project.refreshModels(backend: backend, force: true)
        } else {
            await prefetchModels(project)
        }
    }

    /// Fetches the model list in the background when a project opens or its
    /// endpoint changes, so the picker opens on a ready list.
    func prefetchModels(_ project: ProjectState) async {
        // models.dev rides along (disk cache, refetched once a day).
        Task { await modelsDev.load() }
        guard connection.isReady, project.models.needsLoad, !project.models.loading else { return }
        await project.refreshModels(backend: backend, force: false)
    }

    /// `permissions/set`; appends a notice to the selected chat.
    /// Re-applies the mode last chosen for this repository (the backend
    /// starts every opened project on the configured default). A profile the
    /// backend rejects is forgotten.
    func restorePermissionProfile(_ project: ProjectState) async {
        guard let saved = preferences.permissionProfiles[project.root], saved != project.info.permissionProfile else { return }
        do {
            project.info = try await backend.permissionsSet(projectId: project.id, profile: saved)
        } catch {
            preferences.permissionProfiles.removeValue(forKey: project.root)
        }
    }

    func setApprovalMode(_ profile: String) async {
        guard let project = selectedProject else { return }
        do {
            let info = try await backend.permissionsSet(projectId: project.id, profile: profile)
            project.info = info
            preferences.permissionProfiles[project.root] = info.permissionProfile
            selectedChat?.transcript.append(.notice(NoticeItem(lines: ["Approval mode set to \(Self.approvalModeLabel(info.permissionProfile))"])))
        } catch {
            toasts.show("Could not change the approval mode: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    func rebuildIndex() async {
        guard let project = selectedProject else { return }
        do {
            try await backend.indexRebuild(id: project.id)
            project.indexProgress = IndexProgress(projectId: project.id, done: 0, total: 0)
            project.lastIndexReady = nil
            project.info.indexing = true
        } catch {
            toasts.show("Could not rebuild the index: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    /// `doctor/run` for the selected project (results inline in its Doctor card), else without a project (Settings).
    func runDoctor(probe: Bool) async {
        if let project = selectedProject {
            await project.runDoctor(backend: backend, probe: probe)
            return
        }
        doctorRunning = true
        defer { doctorRunning = false }
        do {
            doctorReport = try await backend.doctorRun(projectId: nil, probe: probe)
            doctorError = nil
        } catch {
            doctorError = error.localizedDescription
        }
    }

    // MARK: Git / paths

    func refreshGit(project: ProjectState) async {
        await project.refreshGit(backend: backend)
    }

    func loadProjectExtras(project: ProjectState) async {
        await project.loadExtras(backend: backend)
    }

    /// Changes tab selection → loads the diff and remembers the path.
    func selectChange(path: String?, in project: ProjectState) async {
        project.selectedChangePath = path
        if let path {
            preferences.lastChangesPath[project.id] = path
        } else {
            preferences.lastChangesPath.removeValue(forKey: project.id)
        }
        await project.loadDiff(backend: backend)
    }

    func setShowStaged(_ staged: Bool, in project: ProjectState) async {
        project.showStaged = staged
        await project.loadDiff(backend: backend)
    }

    func reveal(path: String, in project: ProjectState, action: PathAction) {
        let absolute = path.hasPrefix("/") ? path : (project.root as NSString).appendingPathComponent(path)
        let url = URL(fileURLWithPath: absolute)
        switch action {
        case .showInChanges:
            let relative = Self.relativePath(absolute, root: project.root)
            select(project: project.id, restoreSession: false)
            sidebarItem = .changes
            Task {
                if project.gitStatus == nil { await project.refreshGit(backend: backend) }
                await selectChange(path: relative, in: project)
            }
        case .revealInFinder:
            NSWorkspace.shared.activateFileViewerSelecting([url])
        case .openWithEditor:
            openInEditor(url)
        case .openInEditor:
            openInEditor(path: Self.relativePath(absolute, root: project.root), in: project)
        case .copyPath:
            copyToPasteboard(absolute)
        }
    }

    /// Opens `url` in Settings → General "Default editor", or with the app
    /// macOS associates with the file when none is chosen.
    func openInEditor(_ url: URL) {
        if let editor = preferences.defaultEditor {
            NSWorkspace.shared.open([url], withApplicationAt: editor.url, configuration: NSWorkspace.OpenConfiguration())
        } else {
            NSWorkspace.shared.open(url)
        }
    }

    /// Debounced `git/status` after `file_changed` / `project/changed`.
    func scheduleGitRefresh(_ project: ProjectState) {
        gitRefreshTasks[project.id]?.cancel()
        gitRefreshTasks[project.id] = Task { [weak self, weak project] in
            try? await Task.sleep(nanoseconds: 800_000_000)
            guard !Task.isCancelled, let self, let project else { return }
            self.gitRefreshTasks[project.id] = nil
            await project.refreshGit(backend: self.backend)
        }
    }
}
