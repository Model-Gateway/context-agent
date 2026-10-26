import SwiftUI
import AppKit
import Foundation

/// Appearance override (Settings → Appearance).
enum Appearance: String, CaseIterable, Identifiable {
    case system, light, dark

    var id: String { rawValue }

    var label: String {
        switch self {
        case .system: return "System"
        case .light: return "Light"
        case .dark: return "Dark"
        }
    }

    /// `nil` follows the system.
    var nsAppearance: NSAppearance? {
        switch self {
        case .system: return nil
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        }
    }
}

/// UserDefaults-backed settings. Plain `var`s with `didSet` writers (property
/// wrappers are not allowed inside `@Observable` classes). Everything here is
/// small per-user state; API keys live in the Keychain, recents in
/// `RecentProjectStore`.
@MainActor
@Observable
final class Preferences: @MainActor BackendLocatorPreferences {
    enum Key {
        static let backendPathOverride = "backendPathOverride"
        static let appearance = "appearance"
        static let contextEngineEnabled = "contextEngineEnabled"
        static let lastSelectedProjectId = "lastSelectedProjectId"
        static let lastSelectedSession = "lastSelectedSession"
        static let sidebarVisible = "sidebarVisible"
        static let inspectorVisible = "inspectorVisible"
        static let inspectorWidth = "inspectorWidth"
        static let lastTab = "lastTab"
        static let lastChangesPath = "lastChangesPath"
        static let permissionProfiles = "permissionProfiles"
        static let suggestNextMessage = "suggestNextMessage"
        static let developerMode = "developerMode"
        static let recentModels = "recentModels"
        static let notifyOnFinish = "notifyOnFinish"
        static let notifyOnApproval = "notifyOnApproval"
        static let useMockBackend = "useMockBackend"
        static let openProjectPaths = "openProjectPaths"
        static let collapsedProjects = "collapsedProjects"
        static let autoInitGit = "autoInitGit"
        static let defaultEditorPath = "defaultEditorPath"
        static let editorAutoSave = "editorAutoSave"
        static let settingsTab = "settingsTab"
        static let gatewayAccount = "gatewayAccount"
        static let gatewayRouteRequests = "gatewayRouteRequests"
        static let gatewayAttributeProjects = "gatewayAttributeProjects"
        static let gatewayPreviousEndpoint = "gatewayPreviousEndpoint"
    }

    static let maxRecentModels = 8

    @ObservationIgnored private let defaults: UserDefaults

    /// Settings → General "Choose…" (also the backend failure panel). Empty means "auto-detect".
    var backendPathOverride: String? {
        didSet { write(backendPathOverride, Key.backendPathOverride) }
    }

    var appearance: Appearance {
        didSet {
            defaults.set(appearance.rawValue, forKey: Key.appearance)
            applyAppearance()
        }
    }

    /// Settings → General: retrieve repository context before the first
    /// model call (proactive retrieval). On unless turned off there.
    var contextEngineEnabled: Bool {
        didSet { defaults.set(contextEngineEnabled, forKey: Key.contextEngineEnabled) }
    }

    var lastSelectedProjectId: String? {
        didSet { write(lastSelectedProjectId, Key.lastSelectedProjectId) }
    }

    /// project id → last selected session id.
    var lastSelectedSession: [String: String] {
        didSet { defaults.set(lastSelectedSession, forKey: Key.lastSelectedSession) }
    }

    var sidebarVisible: Bool {
        didSet { defaults.set(sidebarVisible, forKey: Key.sidebarVisible) }
    }

    var inspectorVisible: Bool {
        didSet { defaults.set(inspectorVisible, forKey: Key.inspectorVisible) }
    }

    /// Last width the inspector was dragged to (pt).
    var inspectorWidth: Double {
        didSet { defaults.set(inspectorWidth, forKey: Key.inspectorWidth) }
    }

    /// project id → `SidebarItem` raw value.
    var lastTab: [String: String] {
        didSet { defaults.set(lastTab, forKey: Key.lastTab) }
    }

    /// project id → last selected file in the Changes tab.
    var lastChangesPath: [String: String] {
        didSet { defaults.set(lastChangesPath, forKey: Key.lastChangesPath) }
    }

    /// Settings → General: after a completed run, ask the model for the
    /// user's likely next message (composer ghost text). On by default.
    var suggestNextMessage: Bool {
        didSet { defaults.set(suggestNextMessage, forKey: Key.suggestNextMessage) }
    }

    /// Settings → General: ask every run for the full prompt of its first
    /// model request (`task/run.options.debug_prompt`) and show it above
    /// the message that started the run. Off unless turned on there.
    var developerMode: Bool {
        didSet { defaults.set(developerMode, forKey: Key.developerMode) }
    }

    /// Repository root → the permission profile last chosen for it in the
    /// composer; re-applied whenever the project is opened.
    var permissionProfiles: [String: String] {
        didSet { defaults.set(permissionProfiles, forKey: Key.permissionProfiles) }
    }

    /// base URL → recently used model names (most recent first, max 8).
    var recentModels: [String: [String]] {
        didSet { defaults.set(recentModels, forKey: Key.recentModels) }
    }

    var notifyOnFinish: Bool {
        didSet { defaults.set(notifyOnFinish, forKey: Key.notifyOnFinish) }
    }

    var notifyOnApproval: Bool {
        didSet { defaults.set(notifyOnApproval, forKey: Key.notifyOnApproval) }
    }

    /// Settings → General "Use mock backend (no API key)".
    var useMockBackend: Bool {
        didSet { defaults.set(useMockBackend, forKey: Key.useMockBackend) }
    }

    /// Repository roots open at quit; reopened at launch.
    var openProjectPaths: [String] {
        didSet { defaults.set(openProjectPaths, forKey: Key.openProjectPaths) }
    }

    /// Repository roots whose worktree rows are folded away in the sidebar.
    var collapsedProjects: Set<String> {
        didSet { defaults.set(Array(collapsedProjects).sorted(), forKey: Key.collapsedProjects) }
    }

    /// Settings → General "Initialize a Git repository in projects that have
    /// none": `project/open` runs `git init` for a folder outside any
    /// repository, so the undo snapshots taken before every run exist. On
    /// unless turned off.
    var autoInitGit: Bool {
        didSet { defaults.set(autoInitGit, forKey: Key.autoInitGit) }
    }

    /// Settings → General "Default editor": the .app that Open with
    /// Default Editor uses. Empty means the app macOS associates
    /// with each file type.
    var defaultEditorPath: String? {
        didSet { write(defaultEditorPath, Key.defaultEditorPath) }
    }

    /// Settings → General "Auto save" and File → Auto Save, as VS Code's
    /// `files.autoSave: afterDelay`: an edited file is saved a second after
    /// the last edit, and closing one saves instead of asking. Off unless
    /// turned on (through `AppState.setEditorAutoSave`).
    var editorAutoSave: Bool {
        didSet { defaults.set(editorAutoSave, forKey: Key.editorAutoSave) }
    }

    /// The profile page's last tab (`SettingsTab` raw value).
    var settingsTab: String {
        didSet { defaults.set(settingsTab, forKey: Key.settingsTab) }
    }

    // MARK: ModelGateway (App/GatewayState.swift)

    /// The signed-in account (`GatewayAccount` as JSON); nil when signed out.
    /// The key itself is in the Keychain.
    var gatewayAccount: Data? {
        didSet {
            if let gatewayAccount {
                defaults.set(gatewayAccount, forKey: Key.gatewayAccount)
            } else {
                defaults.removeObject(forKey: Key.gatewayAccount)
            }
        }
    }

    /// Account → Settings "Route model requests through ModelGateway". On by default.
    var gatewayRouteRequests: Bool {
        didSet { defaults.set(gatewayRouteRequests, forKey: Key.gatewayRouteRequests) }
    }

    /// Account → Settings "Attribute usage to projects" (sends the project
    /// name as a request header). On by default.
    var gatewayAttributeProjects: Bool {
        didSet { defaults.set(gatewayAttributeProjects, forKey: Key.gatewayAttributeProjects) }
    }

    /// `base_url` / `api_key_env` from before sign-in switched the endpoint
    /// to the gateway; restored on sign-out.
    var gatewayPreviousEndpoint: [String: String] {
        didSet { defaults.set(gatewayPreviousEndpoint, forKey: Key.gatewayPreviousEndpoint) }
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        backendPathOverride = defaults.string(forKey: Key.backendPathOverride)
        appearance = Appearance(rawValue: defaults.string(forKey: Key.appearance) ?? "") ?? .system
        contextEngineEnabled = defaults.object(forKey: Key.contextEngineEnabled) as? Bool ?? true
        lastSelectedProjectId = defaults.string(forKey: Key.lastSelectedProjectId)
        lastSelectedSession = defaults.dictionary(forKey: Key.lastSelectedSession) as? [String: String] ?? [:]
        sidebarVisible = defaults.object(forKey: Key.sidebarVisible) as? Bool ?? true
        inspectorVisible = defaults.object(forKey: Key.inspectorVisible) as? Bool ?? false
        inspectorWidth = defaults.object(forKey: Key.inspectorWidth) as? Double ?? Double(Theme.Metrics.inspectorWidth)
        lastTab = defaults.dictionary(forKey: Key.lastTab) as? [String: String] ?? [:]
        lastChangesPath = defaults.dictionary(forKey: Key.lastChangesPath) as? [String: String] ?? [:]
        permissionProfiles = defaults.dictionary(forKey: Key.permissionProfiles) as? [String: String] ?? [:]
        suggestNextMessage = defaults.object(forKey: Key.suggestNextMessage) as? Bool ?? true
        developerMode = defaults.bool(forKey: Key.developerMode)
        recentModels = defaults.dictionary(forKey: Key.recentModels) as? [String: [String]] ?? [:]
        notifyOnFinish = defaults.object(forKey: Key.notifyOnFinish) as? Bool ?? true
        notifyOnApproval = defaults.object(forKey: Key.notifyOnApproval) as? Bool ?? true
        useMockBackend = defaults.bool(forKey: Key.useMockBackend)
        openProjectPaths = defaults.stringArray(forKey: Key.openProjectPaths) ?? []
        collapsedProjects = Set(defaults.stringArray(forKey: Key.collapsedProjects) ?? [])
        autoInitGit = defaults.object(forKey: Key.autoInitGit) as? Bool ?? true
        defaultEditorPath = defaults.string(forKey: Key.defaultEditorPath)
        editorAutoSave = defaults.object(forKey: Key.editorAutoSave) as? Bool ?? false
        defaults.removeObject(forKey: "gatewayBaseURL")  // the origin is no longer configurable
        settingsTab = defaults.string(forKey: Key.settingsTab) ?? "account"
        gatewayAccount = defaults.data(forKey: Key.gatewayAccount)
        gatewayRouteRequests = defaults.object(forKey: Key.gatewayRouteRequests) as? Bool ?? true
        gatewayAttributeProjects = defaults.object(forKey: Key.gatewayAttributeProjects) as? Bool ?? true
        gatewayPreviousEndpoint = defaults.dictionary(forKey: Key.gatewayPreviousEndpoint) as? [String: String] ?? [:]
    }

    // MARK: - Helpers

    func recentModels(for baseURL: String) -> [String] {
        recentModels[normalized(baseURL)] ?? []
    }

    /// Moves `name` to the front of the base URL's recents (max 8).
    func promoteRecentModel(_ name: String, baseURL: String) {
        let key = normalized(baseURL)
        var list = recentModels[key] ?? []
        list.removeAll { $0 == name }
        list.insert(name, at: 0)
        if list.count > Self.maxRecentModels {
            list.removeLast(list.count - Self.maxRecentModels)
        }
        recentModels[key] = list
    }

    func removeRecentModel(_ name: String, baseURL: String) {
        let key = normalized(baseURL)
        var list = recentModels[key] ?? []
        list.removeAll { $0 == name }
        if list.isEmpty {
            recentModels.removeValue(forKey: key)
        } else {
            recentModels[key] = list
        }
    }

    /// The chosen editor, or `nil` for the system default (also when the
    /// chosen app has since been deleted).
    var defaultEditor: EditorApp? {
        guard let path = defaultEditorPath, !path.isEmpty else { return nil }
        return EditorApp(url: URL(fileURLWithPath: path))
    }

    func lastTab(for projectId: String) -> String? {
        lastTab[projectId]
    }

    /// Applies `appearance` to the running application.
    func applyAppearance() {
        NSApp?.appearance = appearance.nsAppearance
    }

    /// Appends a newly opened folder; one already listed keeps its place, so
    /// launch's parallel reopen (finishing in any order) keeps the sidebar order.
    func rememberOpenProject(path: String) {
        guard !openProjectPaths.contains(path) else { return }
        openProjectPaths.append(path)
    }

    func forgetOpenProject(path: String) {
        openProjectPaths.removeAll { $0 == path }
    }

    private func write(_ value: String?, _ key: String) {
        if let value, !value.isEmpty {
            defaults.set(value, forKey: key)
        } else {
            defaults.removeObject(forKey: key)
        }
    }

    private func normalized(_ baseURL: String) -> String {
        let trimmed = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasSuffix("/") ? String(trimmed.dropLast()) : trimmed
    }
}
