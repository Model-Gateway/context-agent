import SwiftUI
import AppKit
import Foundation

// Root application state: backend lifecycle, open projects, selection, every
// user intent the views call, and the routing of backend notifications into
// `ProjectState` / `ChatState`. Project-level loads live in ProjectState.swift.

enum SidebarItem: String, CaseIterable, Identifiable {
    case welcome, projects, apps, chat, changes, project, editor, account
    var id: String { rawValue }

    /// Chat · Changes · Project · Editor: the tabs remembered per project.
    /// Welcome, Projects, Apps and Account are app-wide screens.
    var isProjectTab: Bool {
        switch self {
        case .chat, .changes, .project, .editor: return true
        case .welcome, .projects, .apps, .account: return false
        }
    }
}

enum InspectorTab: String, CaseIterable, Identifiable {
    case run, files, terminal, agents, browser
    var id: String { rawValue }

    var title: String {
        switch self {
        case .run: return "Run"
        case .files: return "Files"
        case .terminal: return "Terminal"
        case .agents: return "Agents"
        case .browser: return "Browser"
        }
    }

    var systemImage: String {
        switch self {
        case .run: return Icons.chart
        case .files: return Icons.folder
        case .terminal: return Icons.terminal
        case .agents: return Icons.agents
        case .browser: return Icons.browser
        }
    }
}

/// Context-menu actions on a repository path.
enum PathAction {
    case showInChanges, revealInFinder, openWithEditor, openInEditor, copyPath
}

enum BackendFailure: Equatable {
    case notFound(searched: [String])
    case incompatible(found: String, detail: String)
    case exited(code: Int32, tail: String)
    case transport(String)

    var title: String {
        switch self {
        case .notFound: return "ctx not found"
        case .incompatible: return "ctx is too old"
        case .exited: return "Backend stopped"
        case .transport: return "Backend unavailable"
        }
    }

    var detail: String {
        switch self {
        case .notFound(let searched):
            return searched.isEmpty ? "No ctx binary was found." : "Searched:\n" + searched.joined(separator: "\n")
        case .incompatible(let found, let detail):
            return "\(found)\n\(detail)"
        case .exited(let code, let tail):
            return tail.isEmpty ? "ctx exited with code \(code)." : "ctx exited with code \(code).\n\(tail)"
        case .transport(let message):
            return message
        }
    }

    var searchedPaths: [String] {
        if case .notFound(let searched) = self { return searched }
        return []
    }

    var foundPath: String? {
        if case .incompatible(let found, _) = self { return found }
        return nil
    }

    var stderrTail: String? {
        if case .exited(_, let tail) = self { return tail.isEmpty ? nil : tail }
        return nil
    }
}

enum ConnectionState: Equatable {
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

@MainActor
@Observable
final class AppState {
    var connection: ConnectionState = .connecting
    var config: InitializeResult?
    var projects: [ProjectState] = []
    var selectedProjectId: String?
    /// nil → EmptyChatView.
    var selectedSessionId: String?
    var sidebarItem: SidebarItem = .welcome {
        didSet {
            guard sidebarItem.isProjectTab, let project = selectedProject else { return }
            project.lastTab = sidebarItem
            preferences.lastTab[project.id] = sidebarItem.rawValue
        }
    }
    var sidebarVisible: Bool { didSet { preferences.sidebarVisible = sidebarVisible } }
    var inspectorVisible: Bool { didSet { preferences.inspectorVisible = inspectorVisible } }
    var inspectorWidth: CGFloat { didSet { preferences.inspectorWidth = Double(inspectorWidth) } }
    var inspectorTab: InspectorTab = .run
    var paletteVisible = false
    /// The model picker, anchored to the composer's model chip.
    var modelPickerVisible = false
    /// Bumped by newChat, send, select(session:) and popover dismissal.
    var composerFocusToken = 0
    /// projectId → draft typed in the empty state.
    var pendingDraft: [String: String] = [:]
    /// projectId → attachments of the empty-state draft.
    var pendingAttachments: [String: [ComposerAttachment]] = [:]
    /// `log` notifications (errors also raise a toast).
    var backendLog: [LogLine] = []
    /// Doctor results without a project (Settings → Probe endpoint).
    var doctorReport: DoctorReport?
    var doctorRunning = false
    var doctorError: String?
    /// `mcp/status`: every configured MCP server's live state (kept current
    /// by the `mcp/status` notification).
    var mcpServers: [McpServerStatus] = []
    /// `mcp/catalog`: the curated apps (Apps tab); fetched once per backend.
    var mcpCatalog: [McpCatalogApp] = []
    /// True while `startBackend` / `restartBackend` run.
    var backendStarting = false
    /// Folders whose `project/open` has not returned yet: the remembered
    /// projects from the moment the app launches (before the backend is
    /// up), then any folder being opened. The sidebar shows them as loading
    /// rows and their recent cards say "Opening…".
    var openingProjectPaths: [String] = []
    /// The remembered chat `select(project:)` is still waiting for the chat
    /// list to find; the Chat tab shows the transcript skeleton meanwhile
    /// instead of flashing the empty state.
    var restoringSessionId: String?

    let preferences: Preferences
    let toasts: ToastCenter
    /// The local `ctx serve`. A project on a remote host uses its own
    /// connection (`ProjectState.backend`, App/RemoteHosts.swift).
    let backend: BackendClient
    let recents: RecentProjectStore
    /// Settings → Remote hosts.
    let hosts: SSHHostStore
    /// host id → its `ssh … ctx serve` connection, created on first use.
    var remotes: [UUID: RemoteConnection] = [:]
    /// The remote folder picker sheet (Welcome, the sidebar, ⌘⇧O).
    var remotePicker: RemotePickerRequest?
    let notifier: TaskNotifier
    /// "Sign in with ModelGateway" (App/GatewayState.swift).
    let gateway: GatewayState
    /// Sparkle self-updates (App/Updater.swift).
    let updater: Updater
    /// models.dev metadata: display names and the picker's hover cards.
    let modelsDev = ModelsDevCatalog()

    @ObservationIgnored var gitRefreshTasks: [String: Task<Void, Never>] = [:]
    /// projectId → the Inspector Terminal tab's shells, kept across tab switches.
    @ObservationIgnored var terminals: [String: TerminalWorkspace] = [:]
    /// projectId → the Editor tab's open documents (App/EditorState.swift).
    @ObservationIgnored var editors: [String: EditorWorkspace] = [:]
    /// projectId → the Inspector Browser tab's page (App/BrowserWorkspace.swift);
    /// observed through `browserDriven` so the tab can show a dot.
    var browsers: [String: BrowserWorkspace] = [:]
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private var lastSuccessfulStart: Date?
    @ObservationIgnored private var autoRestartPending = false
    /// Consecutive automatic restarts; reset after a long healthy run.
    @ObservationIgnored private var autoRestartCount = 0
    @ObservationIgnored private var backendStartClaimed = false
    /// The remembered remote projects are reopened once per launch.
    @ObservationIgnored private var remoteRestoreStarted = false
    @ObservationIgnored private var backendLogNextId = 0
    /// Notifications for a project whose `project/open` has not returned yet
    /// (a small repository's `index/ready` beats the response frame),
    /// replayed once the `ProjectState` exists.
    @ObservationIgnored private var pendingProjectNotifications: [String: [BackendNotification]] = [:]
    /// The remembered projects in sidebar order while launch reopens them in
    /// parallel, so rows land in place however the opens finish.
    @ObservationIgnored private var restoreOrder: [String] = []

    static let maxAutoRestarts = 2
    static let autoRestartResetAfter: TimeInterval = 300
    private static let maxBufferedNotifications = 64

    convenience init() {
        // `--autotest` gets its own defaults suite and support folder (LaunchOptions, "Storage").
        let launch = LaunchOptions.current
        self.init(preferences: Preferences(defaults: launch.makeDefaults()), backend: BackendClient(), recents: RecentProjectStore(), toasts: ToastCenter(), notifier: TaskNotifier())
    }

    init(preferences: Preferences, backend: BackendClient, recents: RecentProjectStore, toasts: ToastCenter, notifier: TaskNotifier, hosts: SSHHostStore? = nil) {
        self.preferences = preferences
        self.backend = backend
        self.recents = recents
        self.hosts = hosts ?? SSHHostStore()
        self.toasts = toasts
        self.notifier = notifier
        gateway = GatewayState(preferences: preferences)
        updater = Updater()
        sidebarVisible = preferences.sidebarVisible
        inspectorVisible = preferences.inspectorVisible
        inspectorWidth = CGFloat(preferences.inspectorWidth)

        toasts.onDetails = { toast in
            let alert = NSAlert()
            alert.messageText = toast.message
            alert.informativeText = toast.details ?? ""
            alert.alertStyle = toast.kind == .error ? .warning : .informational
            alert.addButton(withTitle: "OK")
            alert.addButton(withTitle: "Copy")
            if alert.runModal() == .alertSecondButtonReturn {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(toast.details ?? toast.message, forType: .string)
            }
        }
        notifier.onOpen = { [weak self] projectId, sessionId in
            guard let self else { return }
            NSApp?.activate(ignoringOtherApps: true)
            self.select(project: projectId)
            Task { await self.select(session: sessionId) }
        }
        updater.isBusy = { [weak self] in self?.anyProjectRunning ?? false }
        updater.onPostpone = { [weak toasts] in
            toasts?.show("Context will restart to finish updating once the running tasks are done.")
        }
        backend.onNotification = { [weak self] notification in
            self?.handle(notification, remote: nil)
        }
        observers.append(NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in self?.appDidBecomeActive() }
        })
    }

    // MARK: - Derived

    /// Runs `command` in a new terminal tab of the selected project and shows
    /// the Inspector Terminal tab (the play button on shell code blocks).
    func runInTerminal(_ command: String) {
        guard let project = selectedProject else { return }
        terminals(for: project).newSession(running: command)
        inspectorVisible = true
        withAnimation(.spring(response: 0.34, dampingFraction: 0.82)) {
            inspectorTab = .terminal
        }
    }

    /// Shows a sub-agent of `chat` in the Inspector Agents tab (clicking an
    /// agent row in the transcript).
    func showAgent(_ agentId: String, in chat: ChatState) {
        chat.inspectedAgentId = agentId
        inspectorVisible = true
        withAnimation(.spring(response: 0.34, dampingFraction: 0.82)) {
            inspectorTab = .agents
        }
    }

    /// The project's editor documents, created empty on first use.
    func editor(for project: ProjectState) -> EditorWorkspace {
        if let existing = editors[project.id] { return existing }
        let workspace = EditorWorkspace(projectId: project.id)
        editors[project.id] = workspace
        return workspace
    }

    /// The project's browser page, created (blank) on first use.
    func browser(for project: ProjectState) -> BrowserWorkspace {
        if let existing = browsers[project.id] { return existing }
        let workspace = BrowserWorkspace(projectId: project.id)
        browsers[project.id] = workspace
        return workspace
    }

    /// Whether the agent is acting in the selected project's browser right
    /// now (the Browser tab button shows a dot while it is not selected).
    var browserDriven: Bool {
        guard let selectedProjectId, let workspace = browsers[selectedProjectId] else { return false }
        return workspace.isAgentDriving
    }

    /// The project's terminal tabs, created (with no shell yet) on first use.
    func terminals(for project: ProjectState) -> TerminalWorkspace {
        if let existing = terminals[project.id] { return existing }
        let workspace = TerminalWorkspace(directory: project.root, host: project.host)
        terminals[project.id] = workspace
        return workspace
    }

    /// The ModelGateway key comes from sign-in only, so a missing one is
    /// never asked for here (the Account page says to sign in again).
    var needsAPIKey: Bool {
        Self.needsAPIKey(config)
    }

    /// Whether the project's backend (the remote host's for a remote
    /// project) has no key for its model.
    func needsAPIKey(for project: ProjectState) -> Bool {
        guard project.isRemote else { return needsAPIKey }
        return Self.needsAPIKey(project.backend.initializeResult)
    }

    private static func needsAPIKey(_ config: InitializeResult?) -> Bool {
        (config?.model.apiKeySet == false) && config?.mock != true && config?.model.credential != GatewayState.credentialName
    }

    /// The backend a project id's requests go to: the project's own, or the
    /// local one for an id that is not open (its answer is an error either way).
    func backend(for projectId: String) -> BackendClient {
        project(for: projectId)?.backend ?? backend
    }

    /// Whether requests about the project can be sent right now.
    func isReady(_ project: ProjectState) -> Bool {
        project.backend.isReady
    }

    var selectedProject: ProjectState? {
        guard let selectedProjectId else { return nil }
        return projects.first { $0.id == selectedProjectId }
    }

    /// Projects shown at the top level of the sidebar's Projects section:
    /// the local ones, minus the worktrees whose parent checkout is open
    /// (those nest under it). Remote projects list under their host.
    var topLevelProjects: [ProjectState] {
        topLevelProjects(on: nil)
    }

    /// The top-level projects of one host (nil: local).
    func topLevelProjects(on hostId: UUID?) -> [ProjectState] {
        let mine = projects.filter { $0.host?.id == hostId }
        let roots = Set(mine.map(\.root))
        return mine.filter { project in
            guard let parent = project.parentRoot else { return true }
            return !roots.contains(parent)
        }
    }

    /// The open worktrees of `project`'s checkout (on the same host).
    func worktrees(of project: ProjectState) -> [ProjectState] {
        projects.filter { $0.parentRoot == project.root && $0.host?.id == project.host?.id }
    }

    /// Whether the project's worktree rows are folded away in the sidebar.
    func isCollapsed(_ project: ProjectState) -> Bool {
        preferences.collapsedProjects.contains(project.root)
    }

    func setCollapsed(_ collapsed: Bool, _ project: ProjectState) {
        if collapsed {
            preferences.collapsedProjects.insert(project.root)
        } else {
            preferences.collapsedProjects.remove(project.root)
        }
    }

    /// The worktree rows the sidebar shows under the project: all of them,
    /// or only the selected one while the project is collapsed.
    func visibleWorktrees(of project: ProjectState) -> [ProjectState] {
        let all = worktrees(of: project)
        guard isCollapsed(project) else { return all }
        return all.filter { $0.id == selectedProjectId }
    }

    /// Worktrees a collapsed project hides; their spinner and badge roll up into its row.
    func hiddenWorktrees(of project: ProjectState) -> [ProjectState] {
        guard isCollapsed(project) else { return [] }
        return worktrees(of: project).filter { $0.id != selectedProjectId }
    }

    /// The open project of a worktree's parent checkout (on the same host).
    func parentProject(of project: ProjectState) -> ProjectState? {
        guard let parent = project.parentRoot else { return nil }
        return projects.first { $0.root == parent && $0.host?.id == project.host?.id }
    }

    var selectedChat: ChatState? {
        guard let selectedSessionId, let project = selectedProject else { return nil }
        return project.existingChat(for: selectedSessionId)
    }

    /// Pending approvals across every open project.
    var pendingApprovals: [(chat: ChatState, request: ApprovalRequest)] {
        projects.flatMap { project in
            project.pendingApprovalChats.compactMap { chat in
                chat.pendingApproval.map { (chat: chat, request: $0) }
            }
        }
    }

    /// A run asking whether to go on at a much lower ModelGateway discount
    /// (the selected chat's question first), shown as an alert over the
    /// window; nil while its answer is being sent.
    var pendingDiscountDrop: (chat: ChatState, request: ApprovalRequest)? {
        if let chat = selectedChat, let request = chat.pendingApproval, request.isDiscountDrop, !chat.approvalInFlight {
            return (chat, request)
        }
        return pendingApprovals.first { $0.request.isDiscountDrop && !$0.chat.approvalInFlight }
    }

    /// Chats that are unseen or waiting for an approval → dock badge (a chat
    /// with both counts once).
    var attentionCount: Int {
        projects.reduce(0) { $0 + $1.attentionCount }
    }

    var anyProjectRunning: Bool { projects.contains { $0.isRunning } }

    /// The projects of the local backend.
    var localProjects: [ProjectState] { projects.filter { !$0.isRemote } }

    /// The open projects on a host.
    func projects(on hostId: UUID) -> [ProjectState] {
        projects.filter { $0.host?.id == hostId }
    }

    /// Folders still opening that have no project row yet (`ProjectState.key`s).
    var pendingProjectPaths: [String] {
        let keys = Set(projects.map(\.key))
        return openingProjectPaths.filter { !keys.contains($0) }
    }

    /// A remembered project's position at launch; later opens sort last.
    func restoreRank(of root: String) -> Int {
        restoreOrder.firstIndex(of: root) ?? Int.max
    }

    var recentProjects: [RecentProject] { recents.sorted }

    /// The recents grids' cards (Welcome, Projects): the remembered projects
    /// after the folders being opened for the first time, so a new project
    /// has its card as soon as it is chosen instead of once `project/open`
    /// returns. Folders reopened at launch are left out: a worktree is not
    /// among the recents and gets no card.
    var recentProjectCards: [RecentProject] {
        let remembered = recentProjects
        let keys = Set(remembered.map(\.id))
        let opening = pendingProjectPaths
            .filter { !keys.contains($0) && restoreRank(of: $0) == Int.max }
            .reversed()
            .map { key -> RecentProject in
                if let ref = RemoteProjectRef(encoded: key) {
                    return RecentProject(path: ref.path, name: (ref.path as NSString).lastPathComponent, lastOpened: Date(), hostId: ref.hostId, hostName: hosts.host(ref.hostId)?.displayName)
                }
                return RecentProject(path: key, name: URL(fileURLWithPath: key).lastPathComponent, lastOpened: Date())
            }
        return opening + remembered
    }

    /// "ctx 0.1.0 · connected" for the sidebar card.
    var connectionLabel: String {
        switch connection {
        case .connecting: return "Starting backend…"
        case .ready(let r): return "ctx \(r.ctxVersion) · connected" + (r.mock ? " · mock" : "")
        case .failed(let f): return f.title
        }
    }

    var mockRequested: Bool {
        let info = ProcessInfo.processInfo
        return info.arguments.contains("--mock") || info.environment["CTX_DESKTOP_MOCK"] == "1" || preferences.useMockBackend
    }

    func project(for id: String) -> ProjectState? {
        projects.first { $0.id == id }
    }

    /// "Ask", "Auto", "Bypass".
    static func approvalModeLabel(_ profile: String) -> String {
        switch profile {
        case "approval_required": return "Ask"
        case "workspace_autonomous": return "Auto"
        case "bypass_permissions": return "Bypass"
        default: return profile.replacingOccurrences(of: "_", with: " ")
        }
    }

    // MARK: - Backend lifecycle

    /// True the first time it is called: the launch-time `startBackend()`
    /// runs once per process, however many main windows SwiftUI opens.
    func claimBackendStart() -> Bool {
        if backendStartClaimed { return false }
        backendStartClaimed = true
        return true
    }

    /// Locate → environment → spawn → initialize → protocol check → reopen
    /// projects (remembered ones on first start; in place on a restart).
    func startBackend() async {
        guard !backendStarting else { return }
        backendStarting = true
        defer { backendStarting = false }
        connection = .connecting
        if projects.isEmpty { listRememberedProjects() }

        let binary: URL
        if let override = Self.launchArgument("--binary") {
            binary = URL(fileURLWithPath: (override as NSString).expandingTildeInPath)
        } else {
            let located = BackendLocator.locate(preferences: preferences)
            guard let found = located.found else {
                connection = .failed(.notFound(searched: located.searched.map(\.path)))
                abandonRestore()
                return
            }
            binary = found
        }

        let environmentInfo = ProcessInfo.processInfo.environment
        let dataDir = environmentInfo["CTX_DATA_DIR"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath) }
        let mock = mockRequested
        var apiKeyEnv = config?.model.apiKeyEnv ?? environmentInfo["CTX_MODEL_API_KEY_ENV"] ?? "OPENAI_API_KEY"
        var environment = await BackendEnvironment.build(apiKeyEnv: mock ? nil : apiKeyEnv)

        do {
            var result = try await backend.start(binary: binary, environment: environment, dataDir: dataDir, mock: mock)
            // The configured key variable is only known after initialize: retry once with the right one.
            if !result.mock, !result.model.apiKeySet, result.model.apiKeyEnv != apiKeyEnv {
                apiKeyEnv = result.model.apiKeyEnv
                environment = await BackendEnvironment.build(apiKeyEnv: apiKeyEnv)
                if let key = environment[apiKeyEnv], !key.isEmpty {
                    result = try await backend.start(binary: binary, environment: environment, dataDir: dataDir, mock: mock)
                }
            }
            config = result
            connection = .ready(result)
            lastSuccessfulStart = Date()
        } catch {
            connection = .failed(Self.failure(from: backend.lastStartFailure, error: error))
            abandonRestore()
            return
        }

        // A new backend may see a different key or endpoint: refetch models.
        for project in localProjects {
            project.models.baseURL = config?.model.baseUrl ?? ""
            project.models.invalidate()
        }
        if projects.isEmpty {
            await reopenRememberedProjects()
        } else {
            await reopenProjectsInPlace()
        }
        // A remembered ModelGateway account: refresh its summary and usage
        // through the new backend, without holding the start up.
        Task {
            await retireLegacyGatewayKey()
            await restoreGatewaySession()
        }
        // Projects on remote hosts come back in parallel, each through its
        // own `ssh … ctx serve`, without holding the local start up.
        if !remoteRestoreStarted {
            remoteRestoreStarted = true
            Task { await reopenRememberedRemoteProjects() }
        }
    }

    /// Stop, start, `project/open` for every open project, `sessions/list` for the selected one.
    func restartBackend() async {
        guard !backendStarting else { return }
        // The old child still emits task/finished while it shuts down: with
        // the connection already marked as restarting, `dequeue` keeps queued
        // prompts instead of sending them into a process that stopped reading.
        connection = .connecting
        for project in localProjects {
            for chat in project.allChats where chat.status.isRunning {
                chat.finish(TaskFinished(projectId: project.id, sessionId: chat.sessionId, taskId: nil, status: .cancelled, error: "Backend restarted", errorKind: "backend_restarted"), terminal: .stopped)
            }
            project.runningSessionIds = []
            project.switchingModel = nil
        }
        refreshAttention()
        await backend.stop()
        await startBackend()
    }

    /// Lists the projects launch will reopen (`--project`, else the
    /// remembered open folders that still exist) as opening, before the
    /// backend is up, and shows the Chat tab's skeleton instead of Welcome
    /// while they load.
    private func listRememberedProjects() {
        var paths: [String] = []
        if let path = Self.launchArgument("--project") {
            paths = [Self.projectFolder(URL(fileURLWithPath: (path as NSString).expandingTildeInPath)).path]
        } else {
            let fm = FileManager.default
            // Remote folders are reopened by `reopenRememberedRemoteProjects`.
            for path in preferences.openProjectPaths where !RemoteProjectRef.isRemote(path) {
                var isDirectory: ObjCBool = false
                guard fm.fileExists(atPath: path, isDirectory: &isDirectory), isDirectory.boolValue else {
                    preferences.forgetOpenProject(path: path)
                    continue
                }
                paths.append(Self.projectFolder(URL(fileURLWithPath: path)).path)
            }
        }
        restoreOrder = paths
        openingProjectPaths = paths
        if !paths.isEmpty, sidebarItem == .welcome {
            sidebarItem = .chat
        }
    }

    /// The backend did not start: nothing listed by `listRememberedProjects` is coming.
    private func abandonRestore() {
        guard !restoreOrder.isEmpty else { return }
        openingProjectPaths.removeAll { restoreOrder.contains($0) }
        restoreOrder = []
        if projects.isEmpty, sidebarItem == .chat {
            sidebarItem = .welcome
        }
    }

    /// Opens the remembered projects in parallel (`ctx serve` handles each
    /// request on its own task) and selects the last selected one as soon as
    /// it is open rather than after the slowest of them.
    private func reopenRememberedProjects() async {
        let paths = restoreOrder
        defer { restoreOrder = [] }
        if Self.launchArgument("--project") != nil, let path = paths.first {
            await openProject(url: URL(fileURLWithPath: path), select: true, restoreSession: false)
            return
        }
        // Remembered folders are reopened without `init_git` (see `openProject`).
        let last = preferences.lastSelectedProjectId
        await withTaskGroup(of: Void.self) { group in
            for path in paths {
                group.addTask { @MainActor in
                    let project = await self.openProject(url: URL(fileURLWithPath: path), select: false, restoreSession: false, initGit: false)
                    if let project, project.id == last, self.selectedProjectId == nil {
                        self.selectRestoredProject(project.id)
                    }
                }
            }
        }
        if selectedProjectId == nil {
            if let first = projects.first {
                selectRestoredProject(first.id)
            } else if sidebarItem == .chat {
                sidebarItem = .welcome
            }
        }
    }

    /// Selects a project reopened at launch without pulling the user off
    /// Welcome, Projects or Account if they went there while it loaded.
    private func selectRestoredProject(_ id: String) {
        let item = sidebarItem
        select(project: id)
        if !item.isProjectTab { sidebarItem = item }
    }

    private func reopenProjectsInPlace() async {
        for project in localProjects {
            do {
                let info = try await backend.projectOpen(path: project.root, exact: false, initGit: false)
                project.info = info
                project.runningSessionIds = info.runningSessionIds
                project.indexProgress = nil
                project.lastIndexReady = nil
                await restorePermissionProfile(project)
                Task { await prefetchModels(project) }
            } catch {
                toasts.show("Could not reopen \(project.name): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
        if let project = selectedProject, !project.isRemote {
            await project.loadSessions()
            Task {
                await project.refreshGit()
                await project.loadExtras()
            }
        }
    }

    // MARK: - Projects

    /// NSOpenPanel (directories only) → `openProject(url:)`. `newProject`
    /// lets the panel create the folder (Projects → New project).
    func chooseProject(newProject: Bool = false) async {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = newProject
        panel.prompt = newProject ? "Create" : "Open"
        panel.message = newProject ? "Choose or create a folder for the new project" : "Choose a repository folder"
        let response = await panel.begin()
        guard response == .OK, let url = panel.url else { return }
        await openProject(url: url)
    }

    func openProject(url: URL) async {
        await openProject(url: url, select: true, restoreSession: false)
    }

    /// The folder `openProject` opens for `url`: the URL itself, or the
    /// folder holding it when it is a file.
    static func projectFolder(_ url: URL) -> URL {
        let target = url.standardizedFileURL
        var isDirectory: ObjCBool = false
        if FileManager.default.fileExists(atPath: target.path, isDirectory: &isDirectory), !isDirectory.boolValue {
            return target.deletingLastPathComponent()
        }
        return target
    }

    @discardableResult
    func openProject(url: URL, select: Bool, restoreSession: Bool, initGit: Bool = true) async -> ProjectState? {
        await openProject(path: Self.projectFolder(url).path, host: nil, select: select, restoreSession: restoreSession, initGit: initGit)
    }

    /// `project/open` on the local backend, or on the host's connection
    /// (connected first when it is not), then the usual registration:
    /// recents, remembered-open list, permission profile, loads.
    /// `initGit` false (projects reopened at launch): a remembered folder
    /// was a repository, or got one, when it was first opened, so one that
    /// has none now lost it to something outside the app (a pruned linked
    /// worktree, say) and must not be quietly made a new repository.
    @discardableResult
    func openProject(path: String, host: SSHHost?, select: Bool, restoreSession: Bool, initGit: Bool = true) async -> ProjectState? {
        let key = RecentProject.key(path: path, hostId: host?.id)
        if !openingProjectPaths.contains(key), !projects.contains(where: { $0.key == key }) {
            openingProjectPaths.append(key)
        }
        defer { openingProjectPaths.removeAll { $0 == key } }
        let client: BackendClient
        if let host {
            guard let connected = await connect(host) else { return nil }
            client = connected.client
        } else {
            guard connection.isReady else {
                toasts.show("The backend is not running; open a project once it is connected.", kind: .warning)
                return nil
            }
            client = backend
        }
        let folderName = (path as NSString).lastPathComponent
        do {
            let info = try await client.projectOpen(path: path, exact: false, initGit: initGit && preferences.autoInitGit)
            // One repository id per app: the same path on two hosts hashes
            // to the same id and would mix their events.
            if let clash = self.project(for: info.projectId), clash.host?.id != host?.id {
                toasts.show("\(info.name) is already open \(clash.host.map { "on \($0.displayName)" } ?? "locally") at the same path; close it first.", kind: .warning)
                return nil
            }
            let project = register(info, host: host)
            // The real row replaces the loading one now, even when the
            // backend resolved the folder to a different repository root.
            openingProjectPaths.removeAll { $0 == key }
            // A worktree is listed under its parent, not among the recents.
            if !project.isWorktree {
                recents.touch(path: info.root, name: info.name, host: host)
            }
            if info.gitInitialized == true {
                toasts.show("Initialized a Git repository in \(info.name) so turns can be undone.", kind: .info)
            }
            await restorePermissionProfile(project)
            if select {
                self.select(project: project.id, restoreSession: restoreSession)
                if !restoreSession { newChat() }
            }
            startLoads(for: project)
            return project
        } catch {
            let location = host.map { " on \($0.displayName)" } ?? ""
            toasts.show("Could not open \(folderName)\(location): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            return nil
        }
    }

    /// Opens a remembered project, local or remote.
    @discardableResult
    func openRecent(_ recent: RecentProject, select: Bool = true, restoreSession: Bool = false) async -> ProjectState? {
        if let hostId = recent.hostId {
            guard let host = hosts.host(hostId) else {
                toasts.show("The host of \(recent.name) is no longer configured; add it again in Settings → Remote hosts.", kind: .warning)
                return nil
            }
            return await openProject(path: recent.path, host: host, select: select, restoreSession: restoreSession)
        }
        return await openProject(url: recent.url, select: select, restoreSession: restoreSession)
    }

    /// Adopts a `project/open` (or `worktree/create`) result: reuses the
    /// open `ProjectState` for that id, else creates it, replays the
    /// notifications that beat the response, and remembers it for the next launch.
    func register(_ info: ProjectInfo, host: SSHHost? = nil) -> ProjectState {
        let project: ProjectState
        if let existing = self.project(for: info.projectId) {
            existing.info = info
            project = existing
        } else {
            project = makeProject(info, host: host)
            // A project reopened at launch goes to its remembered position.
            let rank = restoreRank(of: project.key)
            let index = projects.firstIndex { restoreRank(of: $0.key) > rank } ?? projects.endIndex
            projects.insert(project, at: index)
            replayBufferedNotifications(for: project.id)
        }
        preferences.rememberOpenProject(path: project.key)
        return project
    }

    /// The background loads every freshly opened project starts.
    /// The chat list does not wait for Git: `ctx serve` answers both at once.
    func startLoads(for project: ProjectState) {
        Task { await prefetchModels(project) }
        Task { await project.loadSessions() }
        Task {
            await project.refreshGit()
            await project.loadExtras()
        }
    }

    /// Closes a project (`project/close`) and drops its row; `forget` false
    /// keeps it in the remembered-open list (a host being disconnected, whose
    /// projects come back on reconnect).
    func closeProject(_ id: String, forget: Bool = true) async {
        guard let index = projects.firstIndex(where: { $0.id == id }) else { return }
        let project = projects[index]
        // Its open worktrees go with it (their checkouts and chats are kept).
        for child in worktrees(of: project) {
            await closeProject(child.id, forget: forget)
        }
        gitRefreshTasks[id]?.cancel()
        gitRefreshTasks[id] = nil
        terminals.removeValue(forKey: id)?.terminateAll()
        browsers.removeValue(forKey: id)?.tearDown()
        editors.removeValue(forKey: id)
        if project.backend.isReady {
            do {
                try await project.backend.projectClose(id: id)
            } catch {
                toasts.show("Could not close \(project.name): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
        if let current = projects.firstIndex(where: { $0.id == id }) {
            projects.remove(at: current)
        }
        if forget { preferences.forgetOpenProject(path: project.key) }
        if selectedProjectId == id {
            selectedProjectId = nil
            selectedSessionId = nil
            if let next = projects.first {
                select(project: next.id)
            } else {
                preferences.lastSelectedProjectId = nil
                sidebarItem = .welcome
            }
        }
        refreshAttention()
    }

    /// Shows the profile page (`ProfileView`, the sidebar's account button) on
    /// `tab`; with no tab, on the last settings tab, or General when the last
    /// one was Account. ⌘, and every "Open Settings" link land here.
    func openSettings(_ tab: SettingsTab? = nil) {
        if let tab {
            preferences.settingsTab = tab.rawValue
        } else if preferences.settingsTab == SettingsTab.account.rawValue {
            preferences.settingsTab = SettingsTab.general.rawValue
        }
        sidebarItem = .account
    }

    func select(project id: String) {
        select(project: id, restoreSession: true)
    }

    func select(project id: String, restoreSession: Bool) {
        guard let project = project(for: id) else { return }
        let last = restoreSession ? preferences.lastSelectedSession[id] : nil
        // With the chat list already loaded, the remembered chat is selected
        // in this same update: clearing it first and restoring it a run-loop
        // later flashed "New chat" and the empty state in between.
        let ready = last.flatMap { project.sessionsLoad == nil ? project.session($0) : nil }?.id
        if selectedProjectId != id {
            selectedProjectId = id
            selectedSessionId = ready
            restoringSessionId = nil
            preferences.lastSelectedProjectId = id
        }
        sidebarItem = project.lastTab.isProjectTab ? project.lastTab : .chat
        composerFocusToken += 1
        refreshAttention()
        guard let last else { return }
        if let ready, selectedSessionId == ready {
            // Already showing; this loads its history and clears its unseen mark.
            Task { await select(session: ready) }
            return
        }
        restoringSessionId = last
        Task {
            // Cleared by newChat or picking another chat or project meanwhile.
            defer { if restoringSessionId == last { restoringSessionId = nil } }
            // Wait for a `sessions/list` already in flight (openProject starts
            // one) instead of skipping it: the remembered session is only
            // known once the list has arrived.
            if let inFlight = project.sessionsLoad {
                await inFlight.value
            } else if project.sessions.isEmpty {
                await project.loadSessions()
            }
            guard selectedProjectId == id, selectedSessionId == nil, restoringSessionId == last, project.session(last) != nil else { return }
            await select(session: last)
        }
    }

    // MARK: - Sessions

    /// Loads history only when the transcript is empty and the chat idle; clears unseen.
    func select(session id: String?) async {
        guard let project = selectedProject else {
            selectedSessionId = nil
            return
        }
        selectedSessionId = id
        restoringSessionId = nil
        sidebarItem = .chat
        composerFocusToken += 1
        if let id {
            preferences.lastSelectedSession[project.id] = id
        } else {
            preferences.lastSelectedSession.removeValue(forKey: project.id)
        }
        guard let id else {
            refreshAttention()
            return
        }
        let chat = project.chat(for: id)
        chat.unseen = nil
        notifier.clearDelivered(sessionId: id)
        refreshAttention()

        if chat.transcript.isEmpty, chat.status == .idle {
            chat.status = .loadingHistory
            do {
                let history = try await project.backend.sessionHistory(projectId: project.id, sessionId: id)
                if chat.status == .loadingHistory {
                    chat.loadHistory(history)
                } else {
                    // A run started meanwhile (Return was pressed, or a queued
                    // prompt was dequeued): keep the new turn and put the
                    // earlier conversation in front of it.
                    chat.prependHistory(history)
                }
            } catch {
                chat.historyError = error.localizedDescription
                if chat.status == .loadingHistory { chat.status = .idle }
            }
            if project.isRunning(session: id), !chat.status.isRunning {
                let running = project.info.runningTasks?.first { $0.sessionId == id } ?? project.info.runningTask.flatMap { $0.sessionId == id ? $0 : nil }
                let startedAt = running.flatMap { RFC3339.parse($0.startedAt) } ?? Date()
                chat.status = .running(RunProgress(state: .investigating, step: 0, startedAt: startedAt))
            }
        }
    }

    /// Empty state for the selected project; Welcome when none is open.
    func newChat() {
        guard let project = selectedProject else {
            sidebarItem = .welcome
            return
        }
        selectedSessionId = nil
        restoringSessionId = nil
        preferences.lastSelectedSession.removeValue(forKey: project.id)
        sidebarItem = .chat
        composerFocusToken += 1
    }

    // MARK: - Sending

    /// Creates the session when needed; queues when the project is busy.
    /// `suggest/next` for a chat whose run just completed; the answer is
    /// kept only if the composer is still empty and the chat idle.
    func requestSuggestion(for chat: ChatState, projectId: String) {
        chat.suggestion = nil
        guard preferences.suggestNextMessage, let project = project(for: projectId), isReady(project) else { return }
        let sessionId = chat.sessionId
        Task {
            guard let text = try? await project.backend.suggestNext(projectId: projectId, sessionId: sessionId) else { return }
            guard chat.draft.isEmpty, !chat.isRunning, chat.queued.isEmpty else { return }
            chat.suggestion = text
        }
    }

    /// `suggest/starters` for the empty chat, at most every ten minutes per
    /// project. Fire-and-forget: nothing waits on it, so opening and sending
    /// chats are never held up; a failed or empty answer falls back to
    /// `StarterSuggestion.fallback`.
    func loadStarters(for project: ProjectState) {
        guard !project.startersLoading else { return }
        guard isReady(project), !needsAPIKey(for: project) else {
            if project.starters == nil { project.starters = StarterSuggestion.fallback(for: project.info) }
            return
        }
        if let loaded = project.startersLoadedAt, Date().timeIntervalSince(loaded) < 600 { return }
        project.startersLoading = true
        let projectId = project.id
        Task {
            let generated = (try? await project.backend.suggestStarters(projectId: projectId)) ?? []
            project.starters = generated.isEmpty ? StarterSuggestion.fallback(for: project.info) : generated
            project.startersLoadedAt = Date()
            project.startersLoading = false
        }
    }

    func send(_ text: String) async {
        await send(OutgoingPrompt(text: text))
    }

    /// Sends a prompt with its attachments (`AttachmentUpload.prompt`); one
    /// with attachments but no text gets a short stand-in text. While the
    /// chat runs, Return steers the running turn and ⌘Return (`queue`)
    /// waits for it to end.
    func send(_ outgoing: OutgoingPrompt, queue: Bool = false) async {
        var prompt = outgoing
        prompt.text = prompt.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.text.isEmpty || !prompt.images.isEmpty || !prompt.files.isEmpty else { return }
        if prompt.text.isEmpty {
            prompt.text = prompt.files.isEmpty && prompt.images.count == 1 ? "See the attached image." : "See the attached files."
        }
        selectedChat?.suggestion = nil
        guard let project = selectedProject else {
            sidebarItem = .welcome
            return
        }
        guard isReady(project) else {
            toasts.show(project.isRemote ? "\(project.host?.displayName ?? "The host") is not connected." : "The backend is not running.", kind: .error)
            return
        }
        let chat: ChatState
        if let id = selectedSessionId {
            chat = project.chat(for: id)
        } else {
            guard let created = await createSession(in: project) else { return }
            chat = created
        }
        chat.draft = ""
        chat.attachments = []
        pendingDraft[project.id] = nil
        pendingAttachments[project.id] = nil
        // Other sessions of the project may be running; only this chat's
        // own run makes the prompt wait.
        if project.isRunning(session: chat.sessionId), !queue, let question = chat.pendingQuestion, prompt.images.isEmpty, prompt.files.isEmpty {
            // The run waits on the agent's questions, so it would never read
            // a steered message: the text answers them instead.
            await respond(question: question.requestId, in: chat, answers: nil, message: prompt.text)
        } else if project.isRunning(session: chat.sessionId) {
            if queue {
                chat.enqueue(prompt)
            } else {
                await steer(prompt, in: chat, project: project)
            }
        } else {
            await run(prompt: prompt, in: chat, project: project)
        }
        composerFocusToken += 1
    }

    /// `task/steer`: the prompt waits above the composer until the `steered`
    /// event shows the model read it. A run that is already ending refuses
    /// it; it then runs as the next turn.
    private func steer(_ prompt: OutgoingPrompt, in chat: ChatState, project: ProjectState) async {
        chat.steering.append(prompt)
        do {
            let attachments = await outgoingAttachments(prompt, for: project)
            try await project.backend.taskSteer(projectId: project.id, sessionId: chat.sessionId, prompt: prompt.text, images: attachments.images, inlineImages: attachments.inlineImages, files: attachments.files)
        } catch {
            // `task/finished` may have moved it to the queue meanwhile.
            guard let index = chat.steering.firstIndex(of: prompt) else { return }
            chat.steering.remove(at: index)
            if case .rpc(let code, _, _)? = error as? BackendError, code == BackendError.noTaskRunning {
                if project.isRunning(session: chat.sessionId) {
                    chat.queued.insert(prompt, at: 0)
                    if chat.queuedSince == nil { chat.queuedSince = Date() }
                } else {
                    await run(prompt: prompt, in: chat, project: project)
                }
            } else {
                moveToComposer(prompt, in: chat)
                toasts.show("Could not send the message to the running task: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    /// Re-sends the chat's last user message.
    func retry(chat: ChatState) async {
        guard let prompt = chat.transcript.lastUserPrompt, let project = project(for: chat.projectId) else { return }
        if selectedProjectId != project.id { select(project: project.id, restoreSession: false) }
        if selectedSessionId != chat.sessionId { await select(session: chat.sessionId) }
        if project.isRunning(session: chat.sessionId) {
            chat.enqueue(prompt)
        } else {
            await run(prompt: prompt, in: chat, project: project)
        }
    }

    /// Stops the selected chat's run; other sessions of the project keep going.
    func cancelRunningTask() async {
        guard let project = selectedProject, let sessionId = selectedSessionId, project.isRunning(session: sessionId) else { return }
        let chat = project.chat(for: sessionId)
        chat.cancelRequested = true
        do {
            try await project.backend.taskCancel(projectId: project.id, sessionId: sessionId)
        } catch {
            chat.cancelRequested = false
            if case .rpc(let code, _, _)? = error as? BackendError, code == BackendError.noTaskRunning {
                // The backend has no run for this session (a stale
                // running_tasks entry): no task/finished will ever come, so
                // settle the chat here.
                if chat.status.isRunning {
                    chat.finish(TaskFinished(projectId: project.id, sessionId: chat.sessionId, taskId: nil, status: .cancelled, error: nil, errorKind: nil), terminal: .stopped)
                }
                project.runningSessionIds.remove(sessionId)
                refreshAttention()
            } else {
                toasts.show("Could not cancel the task: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    func removeQueued(at index: Int, in chat: ChatState) {
        chat.removeQueued(at: index)
    }

    /// Queue row click: moves the item back into the composer.
    func editQueued(at index: Int, in chat: ChatState) {
        guard chat.queued.indices.contains(index) else { return }
        let prompt = chat.queued[index]
        chat.removeQueued(at: index)
        moveToComposer(prompt, in: chat)
        composerFocusToken += 1
    }

    /// The text goes into the draft and pasted images and documents back onto
    /// it as chips (attached files are still referenced in the text).
    private func moveToComposer(_ prompt: OutgoingPrompt, in chat: ChatState) {
        chat.draft = chat.draft.isEmpty ? prompt.text : chat.draft + "\n" + prompt.text
        let pasted = (prompt.images + prompt.files.map(\.path)).map { URL(fileURLWithPath: $0) }.filter { PastedFiles.contains($0) && FileManager.default.fileExists(atPath: $0.path) }
        chat.attachments = ComposerAttachment.appending(pasted, to: chat.attachments)
    }

    private func createSession(in project: ProjectState) async -> ChatState? {
        do {
            let summary = try await project.backend.sessionCreate(projectId: project.id, title: nil)
            project.upsert(session: summary)
            let chat = project.chat(for: summary.id)
            if selectedProjectId == project.id {
                selectedSessionId = summary.id
                preferences.lastSelectedSession[project.id] = summary.id
                sidebarItem = .chat
            }
            return chat
        } catch {
            toasts.show("Could not create a chat: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            return nil
        }
    }

    /// Appends the optimistic user item and calls `task/run`.
    private func run(prompt: OutgoingPrompt, in chat: ChatState, project: ProjectState) async {
        let item = UserTurn(text: prompt.text, images: prompt.images, files: prompt.files)
        chat.transcript.append(.user(item))
        chat.pendingUserItemId = item.id
        chat.lastError = nil
        chat.lastErrorKind = nil
        chat.historyError = nil
        chat.unseen = nil
        chat.status = .running(RunProgress(state: .preparing, step: 0, startedAt: Date()))
        project.runningSessionIds.insert(chat.sessionId)
        project.touchSession(chat.sessionId, firstPrompt: prompt.text)

        let options = TaskRunOptions(proactiveRetrieval: preferences.contextEngineEnabled ? nil : false, debugPrompt: preferences.developerMode ? true : nil)
        do {
            let attachments = await outgoingAttachments(prompt, for: project)
            try await project.backend.taskRun(projectId: project.id, sessionId: chat.sessionId, prompt: prompt.text, images: attachments.images, inlineImages: attachments.inlineImages, files: attachments.files, options: options)
        } catch let error as BackendError where error.isTaskAlreadyRunning {
            // The session is still running (a stale view): queue instead of failing.
            removeUserItem(item.id, from: chat)
            chat.status = .idle
            project.runningSessionIds.remove(chat.sessionId)
            chat.queued.insert(prompt, at: 0)
            if chat.queuedSince == nil { chat.queuedSince = Date() }
            await project.refreshStatus()
        } catch let error as BackendError where error.isConnectionLoss {
            // The backend went away between the dequeue and the request (a
            // restart in progress): keep the prompt for the next run instead
            // of losing it behind an error notice.
            removeUserItem(item.id, from: chat)
            chat.status = .idle
            project.runningSessionIds.remove(chat.sessionId)
            chat.queued.insert(prompt, at: 0)
            if chat.queuedSince == nil { chat.queuedSince = Date() }
            chat.transcript.append(.notice(NoticeItem(lines: ["Backend connection lost; the message stays queued."])))
            chat.addLog("queued after a connection loss: \(ChatState.clip(prompt.text, 60))", level: "warn")
        } catch {
            let message = error.localizedDescription
            chat.transcript.append(.notice(NoticeItem(lines: [message], isError: true)))
            chat.lastError = message
            if case .rpc(_, _, let kind)? = error as? BackendError { chat.lastErrorKind = kind }
            chat.pendingUserItemId = nil
            chat.status = .idle
            project.runningSessionIds.remove(chat.sessionId)
            chat.addLog("error: \(message)", level: "error")
        }
    }

    /// What `task/run` / `task/steer` get for a prompt's attachments: the
    /// local paths for a local project; for a project on a remote host the
    /// bytes and text, read now (the host cannot open the app's files).
    private func outgoingAttachments(_ prompt: OutgoingPrompt, for project: ProjectState) async -> (images: [String], inlineImages: [InlineImage], files: [FileUpload]) {
        guard project.isRemote else { return (prompt.images, [], prompt.files) }
        let prepared = await RemoteAttachments.prepare(images: prompt.images, files: prompt.files)
        if !prepared.skipped.isEmpty {
            toasts.show("\(prepared.skipped.count) attachment\(prepared.skipped.count == 1 ? " is" : "s are") no longer available and \(prepared.skipped.count == 1 ? "was" : "were") not sent.", kind: .warning, details: prepared.skipped.joined(separator: "\n"))
        }
        return ([], prepared.inlineImages, prepared.files)
    }

    private func removeUserItem(_ id: String, from chat: ChatState) {
        if let last = chat.transcript.last, last.id == id {
            chat.transcript.removeLast()
        }
        chat.pendingUserItemId = nil
    }

    // MARK: - Rewind (edit a sent message · undo a turn)

    /// The edit / undo waiting on the keep-or-revert sheet (`RewindSheet`).
    var rewindRequest: RewindRequest?
    var rewindInFlight = false

    /// Whether the chat can be rewound right now: its session idle and its
    /// history loaded (other sessions may run; a rewind restores only this
    /// session's files).
    func canRewind(_ chat: ChatState) -> Bool {
        if chat.status.isRunning || chat.status == .loadingHistory { return false }
        guard let project = project(for: chat.projectId) else { return false }
        if project.isRunning(session: chat.sessionId) { return false }
        return isReady(project)
    }

    /// Hover pencil / context menu: the bubble becomes an editor.
    func beginEdit(turn: UserTurn, in chat: ChatState) {
        guard turn.isRewindable else {
            toasts.show("This message cannot be edited yet.", kind: .info)
            return
        }
        guard canRewind(chat) else {
            toasts.show("Wait for the running task to finish before editing a message.", kind: .info)
            return
        }
        chat.editingTurnId = turn.id
        chat.editingText = turn.text
    }

    func cancelEdit(in chat: ChatState) {
        chat.editingTurnId = nil
        chat.editingText = ""
        composerFocusToken += 1
    }

    /// Send of an inline edit: asks keep-or-revert, then rewinds and re-sends.
    func submitEdit(in chat: ChatState) {
        guard let id = chat.editingTurnId, let index = chat.transcript.locate(userId: id), case .user(let turn) = chat.transcript[index] else { return }
        let text = chat.editingText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        guard canRewind(chat) else {
            toasts.show("Wait for the running task to finish before sending the edit.", kind: .info)
            return
        }
        rewindRequest = RewindRequest(chat: chat, turn: turn, at: index, mode: .edit(newText: text))
    }

    /// Chat → Undo Last Turn (⌘⌥Z): the newest message the user typed goes,
    /// with everything after it, and comes back into the composer.
    func undoLastTurn() {
        guard let chat = selectedChat else {
            toasts.show("Select a chat to undo its last turn.", kind: .info)
            return
        }
        guard let turn = chat.transcript.lastRewindableUser else {
            toasts.show("Nothing to undo in this chat.", kind: .info)
            return
        }
        undo(from: turn, in: chat)
    }

    /// Context menu "Undo from this message…".
    func undo(from turn: UserTurn, in chat: ChatState) {
        guard turn.isRewindable else {
            toasts.show("This message cannot be undone yet.", kind: .info)
            return
        }
        guard canRewind(chat) else {
            toasts.show("Wait for the running task to finish before undoing.", kind: .info)
            return
        }
        guard let index = chat.transcript.locate(userId: turn.id) else { return }
        if chat.editingTurnId != nil { cancelEdit(in: chat) }
        rewindRequest = RewindRequest(chat: chat, turn: turn, at: index, mode: .undo)
    }

    /// The sheet's Keep files / Revert files: `session/rewind`, reload the
    /// chat from storage, then re-send (edit) or refill the composer (undo).
    func performRewind(_ request: RewindRequest, restoreFiles: Bool) async {
        let chat = request.chat
        guard let messageId = request.turn.messageId, let project = project(for: chat.projectId) else { return }
        guard canRewind(chat) else {
            toasts.show("Wait for the running task to finish before rewinding.", kind: .info)
            return
        }
        rewindInFlight = true
        defer { rewindInFlight = false }
        let result: RewindResult
        do {
            result = try await project.backend.sessionRewind(projectId: project.id, sessionId: chat.sessionId, messageId: messageId, restoreFiles: restoreFiles)
        } catch {
            toasts.show("Could not rewind the chat: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            return
        }
        rewindRequest = nil
        chat.editingTurnId = nil
        chat.editingText = ""
        project.upsert(session: result.session)
        // Reload rather than trim locally: a turn cut in the middle is
        // rebuilt exactly as storage now has it.
        chat.status = .loadingHistory
        do {
            let history = try await project.backend.sessionHistory(projectId: project.id, sessionId: chat.sessionId)
            chat.loadHistory(history)
        } catch {
            chat.status = .idle
            chat.historyError = error.localizedDescription
        }
        chat.addLog("rewound to message \(messageId); removed \(result.removedTasks) task(s)" + (result.files.map { "; restored \($0.restored), deleted \($0.deleted) file(s)" + (($0.skipped ?? []).isEmpty ? "" : "; kept \(($0.skipped ?? []).joined(separator: ", ")), changed since") } ?? ""))
        Task {
            await project.refreshGit()
            await project.loadExtras()
        }
        if let files = result.files {
            let skipped = files.skipped ?? []
            let reverted = files.total == 0 ? "Files were already as before that message." : "Reverted \(files.total) file\(files.total == 1 ? "" : "s")."
            if skipped.isEmpty {
                toasts.show(reverted, kind: .success)
            } else {
                // Changed again since by another session or by hand: kept.
                let kept = "Kept \(skipped.count) file\(skipped.count == 1 ? "" : "s") changed since: \(skipped.prefix(3).joined(separator: ", "))\(skipped.count > 3 ? "…" : "")."
                toasts.show(files.total == 0 ? kept : "\(reverted) \(kept)", kind: .info, details: skipped.joined(separator: "\n"))
            }
        }
        switch request.mode {
        case .edit(let newText):
            // The edited text goes with the original message's attachments.
            let prompt = OutgoingPrompt(text: newText, images: request.turn.images, files: request.turn.files)
            if project.isRunning(session: chat.sessionId) {
                chat.enqueue(prompt)
            } else {
                await run(prompt: prompt, in: chat, project: project)
            }
        case .undo:
            moveToComposer(OutgoingPrompt(text: request.turn.text, images: request.turn.images, files: request.turn.files), in: chat)
            if result.files == nil {
                toasts.show("Undid the last turn; the message is back in the composer.", kind: .success)
            }
        }
        composerFocusToken += 1
    }

    // MARK: - Approvals

    /// ⌘⏎: approve the selected chat's pending approval, else submit the
    /// inline edit, else send the draft.
    func primaryAction() {
        if let chat = selectedChat, let request = chat.pendingApproval, !chat.approvalInFlight {
            Task { await respond(approval: request.id, in: chat, decision: .approve) }
            return
        }
        if let chat = selectedChat, chat.editingTurnId != nil {
            submitEdit(in: chat)
            return
        }
        // Changes tab with a commit message typed: ⌘⏎ commits.
        if sidebarItem == .changes, let project = selectedProject, project.isGitRepository,
           !project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Task { await commitFromBox(in: project) }
            return
        }
        // ⌘⏎ sends; while the chat runs it queues the prompt for after the run.
        let draft = selectedChat?.draft ?? pendingDraft[selectedProjectId ?? ""] ?? ""
        let attachments = selectedChat?.attachments ?? pendingAttachments[selectedProjectId ?? ""] ?? []
        guard !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !attachments.isEmpty, let root = selectedProject?.root else { return }
        Task { await send(await AttachmentUpload.prompt(text: draft, attachments: attachments, root: root), queue: true) }
    }

    /// ⌘⌫
    func denyPendingApproval() {
        guard let chat = selectedChat, let request = chat.pendingApproval, !chat.approvalInFlight else { return }
        Task { await respond(approval: request.id, in: chat, decision: .deny) }
    }

    func respond(approval requestId: String, in chat: ChatState, decision: ApprovalDecision) async {
        chat.approvalInFlight = true
        chat.approvalError = nil
        defer { chat.approvalInFlight = false }
        // "Always continue" on a lower discount: stop asking from now on
        // (Settings → Spending), then continue this run.
        if decision == .approveForSession, let request = chat.pendingApproval, request.id == requestId, request.isDiscountDrop {
            do {
                try await patchConfig(["spend.confirm_discount_drop": .bool(false)])
            } catch {
                toasts.show("Could not turn off the lower-discount question: \(error.localizedDescription)", kind: .error)
            }
        }
        do {
            try await backend(for: chat.projectId).approvalRespond(projectId: chat.projectId, sessionId: chat.sessionId, requestId: requestId, decision: decision)
        } catch {
            chat.approvalError = error.localizedDescription
        }
    }

    // MARK: - Questions

    /// Answers the chat's pending `ask_user` questions (`answers` nil and
    /// `message` nil: the user skipped them).
    func respond(question requestId: String, in chat: ChatState, answers: [QuestionAnswer]?, message: String? = nil) async {
        guard !chat.questionInFlight else { return }
        chat.questionInFlight = true
        chat.questionError = nil
        defer { chat.questionInFlight = false }
        do {
            try await backend(for: chat.projectId).questionRespond(projectId: chat.projectId, sessionId: chat.sessionId, requestId: requestId, answers: answers, message: message)
            if chat.pendingQuestion?.requestId == requestId { chat.pendingQuestion = nil }
        } catch {
            chat.questionError = error.localizedDescription
        }
        refreshAttention()
    }

    // MARK: - Config

    /// `config/patch`; the result replaces `config`. Errors propagate to the caller (Settings shows them inline).
    func patchConfig(_ values: [String: JSONValue]) async throws {
        let previous = config?.model
        let result = try await backend.configPatch(values: values)
        config = result
        connection = .ready(result)
        let endpointChanged = previous?.baseUrl != result.model.baseUrl || previous?.apiKeyEnv != result.model.apiKeyEnv
        for project in localProjects {
            project.models.baseURL = result.model.baseUrl
            if endpointChanged {
                project.models.invalidate()
                Task { await prefetchModels(project) }
            }
            if !project.isRunning {
                Task { await project.refreshStatus() }
            }
        }
    }

    // MARK: - MCP servers (Settings)

    /// Fetches every MCP server's state; the notification keeps it current afterwards.
    func refreshMcpStatus() async {
        guard connection.isReady else { return }
        if let servers = try? await backend.mcpStatus() {
            mcpServers = servers
        }
    }

    /// Reconnects one server (or all) and waits for the outcome.
    func reconnectMcp(server: String? = nil) async throws {
        mcpServers = try await backend.mcpReconnect(server: server)
    }

    /// Fetches the app catalog (once; it is static for a backend).
    func loadMcpCatalog() async {
        guard connection.isReady, mcpCatalog.isEmpty else { return }
        if let apps = try? await backend.mcpCatalog() {
            mcpCatalog = apps
        }
    }

    /// Installs a catalog app; the backend connects it in the background.
    func installMcpApp(id: String, inputs: [String: String], excludeTools: [String] = []) async throws {
        let result = try await backend.mcpInstall(id: id, inputs: inputs, excludeTools: excludeTools)
        config = result
        connection = .ready(result)
        await refreshMcpStatus()
    }

    func uninstallMcpApp(id: String) async throws {
        let result = try await backend.mcpUninstall(id: id)
        config = result
        connection = .ready(result)
        await refreshMcpStatus()
    }

    // MARK: - API key and binary (Connect-a-model card, Settings)

    var apiKeyEnvName: String { config?.model.apiKeyEnv ?? "OPENAI_API_KEY" }

    /// Keychain calls run off the main actor: the ad-hoc-signed build makes
    /// the Keychain prompt after every rebuild, and that prompt must not
    /// block the UI.
    func isAPIKeyStored() async -> Bool {
        let account = apiKeyEnvName
        return await Task.detached(priority: .userInitiated) { KeychainStore.isSet(account: account) }.value
    }

    /// Stores the key in the Keychain and restarts the backend when idle.
    func saveAPIKey(_ key: String) async throws {
        let value = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return }
        let account = apiKeyEnvName
        try await Task.detached(priority: .userInitiated) { try KeychainStore.set(value, account: account) }.value
        if !anyProjectRunning { await restartBackend() }
    }

    func removeAPIKey() async throws {
        let account = apiKeyEnvName
        try await Task.detached(priority: .userInitiated) { try KeychainStore.remove(account: account) }.value
        if !anyProjectRunning { await restartBackend() }
    }

    /// "Choose binary…": file panel → `preferences.backendPathOverride` → restart.
    func chooseBackendBinary() async {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        panel.prompt = "Choose"
        panel.message = "Choose the ctx binary"
        let response = await panel.begin()
        guard response == .OK, let url = panel.url else { return }
        preferences.backendPathOverride = url.path
        await restartBackend()
    }

    func copyBuildCommand() {
        copyToPasteboard(BackendLocator.buildCommand)
    }

    func copyToPasteboard(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }

    // MARK: - Notifications from the backend

    /// Routes a notification of the local backend (`remote` nil) or of a
    /// host's connection. Project ids are unique across backends (`openProject`
    /// refuses a clash), so project-scoped notifications route by id alone.
    func handle(_ notification: BackendNotification, remote: RemoteConnection?) {
        switch notification {
        case .event(let e):
            guard let project = project(for: e.projectId) else { return }
            let chat = project.chat(for: e.sessionId)
            if !project.isRunning(session: e.sessionId) { project.runningSessionIds.insert(e.sessionId) }
            chat.apply(e.event)
            switch e.event {
            case .approvalRequested(_, let request):
                if preferences.notifyOnApproval, !isAppActive || chat !== selectedChat {
                    // A remembered ("approve for session") request is resolved
                    // by the very next event: give that pair a moment before
                    // announcing a request nobody needs to answer.
                    let projectId = project.id
                    Task { [weak self, weak chat] in
                        try? await Task.sleep(nanoseconds: 150_000_000)
                        guard let self, let chat, chat.pendingApproval?.id == request.id else { return }
                        self.notifier.notifyApprovalNeeded(summary: request.summary, chatTitle: chat.title, projectId: projectId, sessionId: chat.sessionId)
                    }
                }
            case .fileChanged:
                scheduleGitRefresh(project)
            default:
                break
            }
            refreshAttention()

        case .taskFinished(let f):
            guard let project = project(for: f.projectId) else { return }
            let chat = project.chat(for: f.sessionId)
            chat.finish(f)
            project.runningSessionIds.remove(f.sessionId)
            if preferences.notifyOnFinish, !isAppActive || chat !== selectedChat {
                // A cancel before the runtime started is a cancel, not a failure.
                let status: TaskStatus? = f.isEarlyCancel ? .cancelled : f.status
                notifier.notifyTaskFinished(status: status, error: f.isEarlyCancel ? nil : f.error, chatTitle: chat.title, projectId: project.id, sessionId: chat.sessionId)
            }
            refreshAttention()
            gitRefreshTasks[project.id]?.cancel()
            gitRefreshTasks[project.id] = nil
            Task {
                await project.refreshGit()
                await project.loadSessions()
                await project.refreshStatus()
                await project.loadExtras()
            }
            if f.errorKind != "provider_auth" {
                dequeue(project: project, preferring: chat)
            }
            if f.status == .completed, chat.queued.isEmpty {
                requestSuggestion(for: chat, projectId: project.id)
            }

        case .indexProgress(let progress):
            guard let project = project(for: progress.projectId) else {
                buffer(notification, for: progress.projectId)
                return
            }
            project.indexProgress = progress
            project.info.indexing = true

        case .indexReady(let ready):
            guard let project = project(for: ready.projectId) else {
                buffer(notification, for: ready.projectId)
                return
            }
            project.indexProgress = nil
            project.lastIndexReady = ready
            project.info.indexing = false
            Task { await project.refreshStatus() }

        case .log(let line):
            let message = remote.map { "[\($0.host.displayName)] \(line.message)" } ?? line.message
            appendBackendLog(level: line.level, message: message)
            if line.level.lowercased() == "error" {
                toasts.show(message, kind: .error, details: message)
            }

        case .lspDiagnostics(let d):
            guard let document = editors[d.projectId]?.document(for: d.path) else { return }
            document.diagnostics = d.diagnostics

        case .browserRequest(let request):
            handleBrowserRequest(request)

        case .questionRequest(let request):
            handleQuestionRequest(request)

        case .mcpStatus(let servers):
            // The Apps tab shows the local backend's servers.
            if remote == nil { mcpServers = servers }

        case .projectChanged(let value):
            guard let id = value["project_id"]?.stringValue, let project = project(for: id) else { return }
            if let git = value["git"], let info = try? BackendClient.decode(GitInfo.self, from: git, context: "project/changed") {
                project.info.git = info
            }
            scheduleGitRefresh(project)

        case .exited(let code, let tail):
            if let remote {
                handleRemoteExit(remote, code: code, tail: tail)
            } else {
                handleExit(code: code, tail: tail)
            }
        }
    }

    /// The `browser` tool wants something done in the project's Browser tab:
    /// the page acts and the reply goes back with `browser/respond`. The
    /// Inspector is left on whatever tab the user has open.
    private func handleBrowserRequest(_ request: BrowserRequest) {
        let backend = backend(for: request.projectId)
        guard let project = project(for: request.projectId) else {
            Task {
                try? await backend.browserRespond(projectId: request.projectId, sessionId: request.sessionId, requestId: request.requestId, reply: BrowserReply(ok: false, error: "the project is not open in the app"))
            }
            return
        }
        let workspace = browser(for: project)
        Task {
            let reply = await workspace.perform(request.action)
            do {
                try await backend.browserRespond(projectId: request.projectId, sessionId: request.sessionId, requestId: request.requestId, reply: reply)
            } catch {
                BackendLog.debug("browser/respond failed: \(error.localizedDescription)")
            }
        }
    }

    /// The `ask_user` tool put questions to the user: the chat shows them
    /// above its composer (and gets the orange dot) until they are answered.
    private func handleQuestionRequest(_ request: QuestionRequest) {
        guard let project = project(for: request.projectId) else {
            let backend = backend(for: request.projectId)
            Task {
                try? await backend.questionRespond(projectId: request.projectId, sessionId: request.sessionId, requestId: request.requestId, answers: nil, message: nil)
            }
            return
        }
        let chat = project.chat(for: request.sessionId)
        chat.pendingQuestion = request
        chat.questionError = nil
        chat.questionInFlight = false
        if preferences.notifyOnApproval, !isAppActive || chat !== selectedChat {
            let first = request.questions.first?.question ?? "The agent has a question"
            notifier.notifyQuestionAsked(question: first, chatTitle: chat.title, projectId: project.id, sessionId: chat.sessionId)
        }
        refreshAttention()
    }

    private func handleExit(code: Int32, tail: String) {
        for project in localProjects {
            for chat in project.allChats where chat.status.isRunning {
                chat.finish(TaskFinished(projectId: project.id, sessionId: chat.sessionId, taskId: nil, status: .failed, error: "Backend exited (code \(code))", errorKind: "backend_exited"), terminal: .stopped)
            }
            project.runningSessionIds = []
            project.switchingModel = nil
            project.indexProgress = nil
        }
        connection = .failed(.exited(code: code, tail: tail))
        appendBackendLog(level: "error", message: "Backend exited (code \(code))")
        toasts.show("Backend stopped (code \(code))", kind: .error, details: tail.isEmpty ? "ctx exited with code \(code)" : tail)
        refreshAttention()

        // One automatic restart per healthy run (not after an explicit stop,
        // not in a crash loop). A child that keeps dying after the 5 s mark
        // (during the first index of a large repository, say) gets at most
        // `maxAutoRestarts` consecutive retries; then the panel's Retry.
        let healthyFor = lastSuccessfulStart.map { Date().timeIntervalSince($0) } ?? 0
        if healthyFor > Self.autoRestartResetAfter { autoRestartCount = 0 }
        guard !backend.stopRequested, !autoRestartPending, healthyFor > 5, autoRestartCount < Self.maxAutoRestarts else { return }
        autoRestartCount += 1
        autoRestartPending = true
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_000_000_000)
            guard let self else { return }
            self.autoRestartPending = false
            if case .failed(.exited) = self.connection {
                await self.restartBackend()
            }
        }
    }

    /// After `task/finished`: every chat of the project with queued prompts
    /// whose session is idle sends its next one, the finished chat first.
    private func dequeue(project: ProjectState, preferring chat: ChatState) {
        guard isReady(project) else { return }
        var started = Set<String>()
        for target in [chat] + project.queuedChats where !target.queued.isEmpty && !project.isRunning(session: target.sessionId) && started.insert(target.sessionId).inserted {
            guard let prompt = target.dequeueFirst() else { continue }
            Task { await run(prompt: prompt, in: target, project: project) }
        }
    }

    private func buffer(_ notification: BackendNotification, for projectId: String) {
        var list = pendingProjectNotifications[projectId] ?? []
        list.append(notification)
        if list.count > Self.maxBufferedNotifications {
            list.removeFirst(list.count - Self.maxBufferedNotifications)
        }
        pendingProjectNotifications[projectId] = list
    }

    private func replayBufferedNotifications(for projectId: String) {
        guard let list = pendingProjectNotifications.removeValue(forKey: projectId) else { return }
        // Only project-scoped frames are buffered, so the connection matters
        // only for the log prefix of a remote project.
        let connection = project(for: projectId).flatMap { remote(of: $0) }
        for notification in list {
            handle(notification, remote: connection)
        }
    }

    private func appendBackendLog(level: String, message: String) {
        backendLogNextId += 1
        backendLog.append(LogLine(id: backendLogNextId, date: Date(), text: message, level: level))
        if backendLog.count > ChatState.maxLogLines {
            backendLog.removeFirst(backendLog.count - ChatState.maxLogLines)
        }
    }

    func clearBackendLog() {
        backendLog.removeAll()
    }

    // MARK: - Attention

    private var isAppActive: Bool { NSApp?.isActive ?? true }

    private func appDidBecomeActive() {
        if let chat = selectedChat {
            chat.unseen = nil
            notifier.clearDelivered(sessionId: chat.sessionId)
        }
        refreshAttention()
    }

    func refreshAttention() {
        notifier.updateBadge(attentionCount)
    }

    // MARK: - Helpers

    private func makeProject(_ info: ProjectInfo, host: SSHHost?) -> ProjectState {
        let remote = host.flatMap { remotes[$0.id] }
        let backendConfig = remote?.client.initializeResult ?? config
        let catalog = ModelCatalog(preferences: preferences, baseURL: backendConfig?.model.baseUrl ?? "")
        catalog.current = info.model
        let project = ProjectState(info: info, models: catalog, backend: remote?.client ?? backend, host: host)
        project.lastTab = preferences.lastTab[info.projectId].flatMap(SidebarItem.init(rawValue:)) ?? .chat
        project.selectedChangePath = preferences.lastChangesPath[info.projectId]
        project.onChatCreated = { [weak self] chat in
            chat.isVisible = { [weak self, weak chat] in
                guard let self, let chat else { return false }
                return self.selectedChat === chat && self.isAppActive
            }
        }
        return project
    }

    private static func launchArgument(_ flag: String) -> String? {
        let arguments = ProcessInfo.processInfo.arguments
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else { return nil }
        let value = arguments[index + 1]
        return value.hasPrefix("--") ? nil : value
    }

    static func failure(from start: BackendClient.StartFailure?, error: Error) -> BackendFailure {
        switch start {
        case .launchFailed(let message)?: return .transport(message)
        case .incompatible(let found, let detail)?: return .incompatible(found: found, detail: detail)
        case .exited(let code, let tail)?: return .exited(code: code, tail: tail)
        case .transport(let message)?: return .transport(message)
        case nil: return .transport(error.localizedDescription)
        }
    }

    nonisolated static func relativePath(_ absolute: String, root: String) -> String {
        let normalizedRoot = root.hasSuffix("/") ? root : root + "/"
        return absolute.hasPrefix(normalizedRoot) ? String(absolute.dropFirst(normalizedRoot.count)) : absolute
    }
}
