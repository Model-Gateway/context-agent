import SwiftUI
import AppKit
import UniformTypeIdentifiers

// The individual Settings forms; ProfileView (SettingsView.swift) arranges them in tabs.
// Every form reads `appState.config` (the user-level configuration the
// backend reported with `initialize` / `config/get`) and writes through
// `appState.patchConfig` (config/patch), `appState.preferences` or the
// Keychain helpers on AppState. RPC failures stay inline in the form.

// MARK: - General

/// Backend binary (detected path, Choose…, Restart backend), mock toggle,
/// data directory, notification toggles.
struct GeneralSettingsSection: View {
    @Environment(AppState.self) private var appState
    @State private var mockHint: String?

    var body: some View {
        @Bindable var preferences = appState.preferences
        SettingsPage {
            SettingsGroup("Backend", footer: "The ctx binary is auto-detected: the app bundle, $CTX_BINARY, ~/.cargo/bin, Homebrew, /usr/local/bin, then a repository checkout. Choose one to override the search.") {
                SettingsRow("Binary", alignment: .top) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            Text(verbatim: binaryPath)
                                .font(Theme.Font.mono)
                                .foregroundStyle(binaryKnown ? Theme.textPrimary : Theme.textTertiary)
                                .lineLimit(2)
                                .truncationMode(.middle)
                                .selectableText()
                            if preferences.backendPathOverride != nil {
                                Chip("override")
                                    .help("Chosen in Settings; auto-detection is skipped")
                            }
                        }
                        HStack(spacing: 8) {
                            Button("Choose…") {
                                Task { await appState.chooseBackendBinary() }
                            }
                            .disabled(appState.backendStarting)
                            if preferences.backendPathOverride != nil {
                                Button("Use auto-detect") {
                                    preferences.backendPathOverride = nil
                                    Task { await appState.restartBackend() }
                                }
                                .disabled(restartDisabled)
                                .help(restartHelp)
                            }
                            Button("Restart backend") {
                                Task { await appState.restartBackend() }
                            }
                            .disabled(restartDisabled)
                            .help(restartHelp)
                            if appState.backendStarting {
                                BrailleSpinner()
                            }
                        }
                        .controlSize(.small)
                    }
                }
                SettingsRow("Status", alignment: .top) {
                    connectionStatus
                }
                SettingsRow("Mock mode", alignment: .top) {
                    VStack(alignment: .leading, spacing: 4) {
                        Toggle("Use mock backend (no API key)", isOn: mockBinding)
                            .toggleStyle(.switch)
                            .controlSize(.small)
                            .disabled(forcedMock || appState.backendStarting)
                        Text(forcedMock
                             ? "Forced for this launch by --mock / CTX_DESKTOP_MOCK."
                             : "Runs ctx serve --mock: a scripted model that reads files and never edits. Handy to try the app without a key.")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                        if let mockHint {
                            Text(mockHint)
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.warning)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }

            SettingsGroup("Storage", footer: "Sessions, tasks, the index and memories live in the data directory and are shared with the ctx command line.") {
                SettingsRow("Data directory") {
                    PathValue(path: appState.config?.dataDir)
                }
                SettingsRow("Configuration") {
                    PathValue(path: appState.config?.configFile)
                }
            }

            SettingsGroup("Editor", footer: "File names in chat open in the Editor tab; ⌘-click opens them in the editor chosen here instead. System default uses the app macOS associates with each file type. Auto save (also File → Auto Save) writes an edited file a second after you stop typing, like VS Code, and closing an edited file then saves it without asking. A file changed on disk meanwhile is never overwritten: its banner offers Reload or Overwrite.") {
                SettingsRow("Default editor") {
                    DefaultEditorPicker()
                }
                SettingsRow("Auto save") {
                    Toggle("Auto save", isOn: Binding(get: { preferences.editorAutoSave }, set: { appState.setEditorAutoSave($0) }))
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.small)
                }
            }

            SettingsGroup("Git", footer: "A snapshot of the project files is taken before every run so a turn can be undone (Chat → Undo Last Turn) or a sent message edited and re-sent with the files reverted. Snapshots need a Git repository: with this on, a folder opened without one gets `git init` (never your home folder or a top-level directory).") {
                Toggle("Initialize a Git repository in projects that have none", isOn: $preferences.autoInitGit)
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }

            SettingsGroup("Context engine", footer: "Retrieves relevant code, symbols and memories from the project index before the first model call of every run. Turn it off to start runs with only the prompt and conversation.") {
                Toggle("Use the context engine", isOn: $preferences.contextEngineEnabled)
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }

            SettingsGroup("Suggestions", footer: "After a run completes, the project's model guesses your next message; it appears greyed out in the empty composer and → accepts it. Each suggestion is one short model call.") {
                Toggle("Suggest a next message", isOn: $preferences.suggestNextMessage)
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }

            SettingsGroup("Developer", footer: "Shows the full prompt sent to the model (policy, instructions, retrieved context, conversation and the tools offered) in a collapsed panel above the message that started each run. It is kept only while the app is open; reopened chats do not have it.") {
                Toggle("Developer mode", isOn: $preferences.developerMode)
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }

            SettingsGroup("Notifications", footer: "Posted when the app is in the background or another chat is selected. macOS asks for permission the first time one is sent.") {
                Toggle("Notify when a task finishes", isOn: $preferences.notifyOnFinish)
                    .toggleStyle(.switch)
                    .controlSize(.small)
                Toggle("Notify when approval is needed", isOn: $preferences.notifyOnApproval)
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }
        }
    }

    // MARK: Binary and status

    private var binaryPath: String {
        if let url = appState.backend.binaryURL { return url.path }
        if let override = appState.preferences.backendPathOverride, !override.isEmpty { return override }
        if case .failed(.notFound) = appState.connection { return "Not found" }
        return "Auto-detect"
    }

    private var binaryKnown: Bool {
        appState.backend.binaryURL != nil || !(appState.preferences.backendPathOverride ?? "").isEmpty
    }

    private var forcedMock: Bool {
        let info = ProcessInfo.processInfo
        return info.arguments.contains("--mock") || info.environment["CTX_DESKTOP_MOCK"] == "1"
    }

    private var restartDisabled: Bool { appState.backendStarting || appState.anyProjectRunning }

    private var restartHelp: String {
        if appState.backendStarting { return "The backend is starting" }
        if appState.anyProjectRunning { return "Finishes after the running task" }
        return "Stop ctx and start it again with the current settings"
    }

    @ViewBuilder
    private var connectionStatus: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if case .connecting = appState.connection {
                    BrailleSpinner(size: 11)
                } else {
                    Circle()
                        .fill(statusTint)
                        .frame(width: 7, height: 7)
                        .accessibilityHidden(true)
                }
                Text(appState.connectionLabel)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
            }
            if let failure = appState.connection.failure {
                Text(verbatim: failure.detail)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
                    .lineLimit(8)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
                if case .notFound = failure {
                    Button("Copy build command") {
                        appState.copyBuildCommand()
                    }
                    .controlSize(.small)
                    .help(BackendLocator.buildCommand)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var statusTint: Color {
        switch appState.connection {
        case .ready: return Theme.success
        case .connecting: return Theme.warning
        case .failed: return Theme.danger
        }
    }

    private var mockBinding: Binding<Bool> {
        Binding(
            get: { appState.preferences.useMockBackend || forcedMock },
            set: { value in
                appState.preferences.useMockBackend = value
                if appState.anyProjectRunning {
                    mockHint = "Applies when the backend restarts; finish or cancel the running task, then use Restart backend."
                } else {
                    mockHint = nil
                    Task { await appState.restartBackend() }
                }
            }
        )
    }
}

/// System default, the installed well-known editors, a previously chosen
/// app, and Choose… (an .app open panel).
private struct DefaultEditorPicker: View {
    @Environment(AppState.self) private var appState
    @State private var installed = EditorApp.installed()

    private static let chooseTag = "\u{0}choose"

    var body: some View {
        Picker("Default editor", selection: selection) {
            Text("System default").tag("")
            Divider()
            ForEach(options) { editor in
                Label {
                    Text(editor.name)
                } icon: {
                    Image(nsImage: editor.icon)
                }
                .tag(editor.id)
            }
            Divider()
            Text("Choose…").tag(Self.chooseTag)
        }
        .labelsHidden()
        .fixedSize()
        .controlSize(.small)
    }

    /// Installed editors plus the chosen one when it is not among them.
    private var options: [EditorApp] {
        guard let chosen = appState.preferences.defaultEditor, !installed.contains(chosen) else { return installed }
        return installed + [chosen]
    }

    private var selection: Binding<String> {
        Binding(
            get: { appState.preferences.defaultEditor?.id ?? "" },
            set: { tag in
                if tag == Self.chooseTag {
                    chooseApp()
                } else {
                    appState.preferences.defaultEditorPath = tag.isEmpty ? nil : tag
                }
            }
        )
    }

    private func chooseApp() {
        let panel = NSOpenPanel()
        panel.title = "Choose an Editor"
        panel.prompt = "Choose"
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        appState.preferences.defaultEditorPath = url.path
    }
}

// MARK: - Model

/// The model (name, limits, reasoning effort). Where requests go and the
/// key they use are on the BYOK tab (or ModelGateway's, when signed in).
struct ModelSettingsSection: View {
    @Environment(AppState.self) private var appState

    @State private var draft = ModelDraft()
    @State private var synced: ModelSettings?
    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsPage {
            if let config = appState.config {
                modelGroup(config)
            } else {
                BackendUnavailable(skeleton: [4, 2])
            }
        }
        .onChange(of: appState.config?.model, initial: true) { _, model in
            sync(model)
        }
    }

    // MARK: Model

    private func modelGroup(_ config: InitializeResult) -> some View {
        SettingsGroup("Model", footer: appState.gatewayRoutingActive
            ? "Requests go through ModelGateway, which serves the models of its catalogue under their own names. Saving rewrites ~/.context-agent/config.toml; idle projects use the new model at once, running ones after their task finishes."
            : "Saving rewrites ~/.context-agent/config.toml; idle projects use the new model at once, running ones after their task finishes. The endpoint and its key are on the BYOK tab.") {
            SettingsRow("Model name") {
                TextField("gpt-4.1", text: $draft.name)
                    .settingsField()
                    .onSubmit(save)
            }
            SettingsRow("Context window") {
                HStack(spacing: 6) {
                    TextField("128000", text: $draft.contextWindow)
                        .settingsField()
                        .frame(width: 120)
                        .onSubmit(save)
                    Text("tokens")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            SettingsRow("Max output") {
                HStack(spacing: 6) {
                    TextField("16000", text: $draft.maxOutputTokens)
                        .settingsField()
                        .frame(width: 120)
                        .onSubmit(save)
                    Text("tokens")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            SettingsRow("Reasoning effort") {
                HStack(spacing: 8) {
                    Picker("Reasoning effort", selection: effortBinding(config)) {
                        let offered = EffortLevel.levels(for: appState.modelInfo(config.model.name)?.effortLevels)
                        ForEach(offered) { level in
                            Text(level.value.isEmpty ? "Default (model decides)" : level.isUltra ? "Ultra (deploys agents)" : level.title).tag(level.value)
                        }
                        if let current = config.model.reasoningEffort, !offered.contains(where: { $0.value == current }) {
                            Text(verbatim: EffortLevel.level(for: current).title).tag(current)
                        }
                    }
                    .labelsHidden()
                    .frame(maxWidth: 220)
                    .disabled(saving || !appState.connection.isReady)
                    Text(config.model.reasoningEffort == "ultra" ? "Ultra asks the model for its strongest reasoning and lets it deploy sub-agents that explore and implement in parallel." : "The model's own effort values. Only sent to models that support it.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            HStack(spacing: 8) {
                Button("Save") { save() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!isDirty || saving || !appState.connection.isReady)
                Button("Revert") { revert() }
                    .disabled(!isDirty || saving)
                if saving {
                    BrailleSpinner()
                }
                Spacer(minLength: 0)
                if isDirty, !saving {
                    Text("Unsaved changes")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.warning)
                }
            }
            .controlSize(.small)
            if let message {
                InlineMessage(message)
            }
        }
    }

    private var isDirty: Bool {
        guard let synced else { return false }
        return draft.isDirty(against: synced)
    }

    private func sync(_ model: ModelSettings?) {
        guard let model else { return }
        let keepEdits = synced.map { draft.isDirty(against: $0) } ?? false
        synced = model
        if !keepEdits { draft = ModelDraft(model) }
    }

    private func revert() {
        guard let synced else { return }
        draft = ModelDraft(synced)
        message = nil
    }

    private func save() {
        guard let base = synced, !saving, appState.connection.isReady else { return }
        let prepared = draft.patch(against: base)
        if let problem = prepared.problem {
            message = .error(problem)
            return
        }
        guard !prepared.values.isEmpty else { return }
        apply(prepared.values, success: "Saved. Idle projects use the new model now; the context window and max output tokens apply when a project is reopened.")
    }

    private func effortBinding(_ config: InitializeResult) -> Binding<String> {
        Binding(
            get: { config.model.reasoningEffort ?? "" },
            set: { value in
                guard value != (config.model.reasoningEffort ?? "") else { return }
                apply(["model.reasoning_effort": value.isEmpty ? .null : .string(value)], success: "Reasoning effort saved.")
            }
        )
    }

    private func apply(_ values: [String: JSONValue], success: String) {
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.patchConfig(values)
                message = .success(success)
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }
}

/// Editable copy of `ModelSettings` as strings, so partial input never
/// clobbers the configuration before Save.
private struct ModelDraft: Equatable {
    var name = ""
    var contextWindow = ""
    var maxOutputTokens = ""

    init() {}

    init(_ model: ModelSettings) {
        name = model.name
        contextWindow = String(model.contextWindow)
        maxOutputTokens = String(model.maxOutputTokens)
    }

    func isDirty(against model: ModelSettings) -> Bool {
        self != ModelDraft(model)
    }

    /// The `config/patch` values for every field that differs from `model`,
    /// or a problem to show instead.
    func patch(against model: ModelSettings) -> (values: [String: JSONValue], problem: String?) {
        var values: [String: JSONValue] = [:]
        let modelName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !modelName.isEmpty else { return ([:], "Model name cannot be empty.") }
        guard let window = Self.integer(contextWindow), window > 0 else { return ([:], "Context window must be a positive whole number of tokens.") }
        guard let output = Self.integer(maxOutputTokens), output > 0 else { return ([:], "Max output must be a positive whole number of tokens.") }
        guard output < window else { return ([:], "Max output must be smaller than the context window.") }

        if modelName != model.name { values["model.name"] = .string(modelName) }
        if window != model.contextWindow { values["model.context_window"] = .number(Double(window)) }
        if output != model.maxOutputTokens { values["model.max_output_tokens"] = .number(Double(output)) }
        return (values, nil)
    }

    /// "128000", "128,000" and "128_000" all parse.
    static func integer(_ text: String) -> Int? {
        let cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
            .replacingOccurrences(of: ",", with: "")
            .replacingOccurrences(of: "_", with: "")
        return Int(cleaned)
    }
}

// MARK: - Permissions

/// Default permission profile (with the profile descriptions), network
/// commands, allowed / denied command prefixes — all through config/patch.
struct PermissionsSettingsSection: View {
    @Environment(AppState.self) private var appState

    @State private var allowed = ""
    @State private var denied = ""
    @State private var synced: PermissionSettings?
    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsPage {
            if let config = appState.config {
                SettingsGroup("Default profile", footer: "The profile every new run starts with. The permissions menu in the composer switches one project's mode for its next runs without changing this default.") {
                    ForEach(PermissionProfileOption.options(current: config.permissions.profile)) { option in
                        ProfileOptionRow(option: option, isSelected: option.id == config.permissions.profile, disabled: saving || !appState.connection.isReady) {
                            apply(["permissions.profile": .string(option.id)], success: "Default profile set to \(option.title).")
                        }
                    }
                }

                SettingsGroup("Commands", footer: "One command prefix per line. Allowed prefixes run without asking under every profile (destructive commands still ask); denied prefixes never run, whatever the profile.") {
                    Toggle("Allow network commands without asking (Auto profile)", isOn: networkBinding(config))
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .disabled(saving || !appState.connection.isReady)
                    SettingsRow("Allowed", alignment: .top) {
                        CommandListEditor(text: $allowed, placeholder: "cargo test\nnpm run lint")
                    }
                    SettingsRow("Denied", alignment: .top) {
                        CommandListEditor(text: $denied, placeholder: "git push --force\nrm -rf")
                    }
                    HStack(spacing: 8) {
                        Button("Save") { saveLists() }
                            .disabled(!listsDirty || saving || !appState.connection.isReady)
                        Button("Revert") { sync(config.permissions, force: true) }
                            .disabled(!listsDirty || saving)
                        if saving {
                            BrailleSpinner()
                        }
                        Spacer(minLength: 0)
                        if listsDirty, !saving {
                            Text("Unsaved changes")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.warning)
                        }
                    }
                    .controlSize(.small)
                }

                if let message {
                    InlineMessage(message)
                }
            } else {
                BackendUnavailable()
            }
        }
        .onChange(of: appState.config?.permissions, initial: true) { _, permissions in
            sync(permissions, force: false)
        }
    }

    private var listsDirty: Bool {
        guard let synced else { return false }
        return Self.lines(allowed) != synced.allowedCommands || Self.lines(denied) != synced.deniedCommands
    }

    private func sync(_ permissions: PermissionSettings?, force: Bool) {
        guard let permissions else { return }
        let keepEdits = !force && listsDirty
        synced = permissions
        if !keepEdits {
            allowed = permissions.allowedCommands.joined(separator: "\n")
            denied = permissions.deniedCommands.joined(separator: "\n")
        }
        if force { message = nil }
    }

    private func saveLists() {
        guard let synced, !saving else { return }
        var values: [String: JSONValue] = [:]
        let allowedLines = Self.lines(allowed)
        let deniedLines = Self.lines(denied)
        if allowedLines != synced.allowedCommands { values["permissions.allowed_commands"] = .array(allowedLines.map(JSONValue.string)) }
        if deniedLines != synced.deniedCommands { values["permissions.denied_commands"] = .array(deniedLines.map(JSONValue.string)) }
        guard !values.isEmpty else { return }
        apply(values, success: "Command lists saved. They apply to projects opened from now on.")
    }

    private func networkBinding(_ config: InitializeResult) -> Binding<Bool> {
        Binding(
            get: { config.permissions.allowNetworkCommands },
            set: { value in
                guard value != config.permissions.allowNetworkCommands else { return }
                apply(["permissions.allow_network_commands": .bool(value)], success: value ? "Network commands run without asking." : "Network commands ask for approval.")
            }
        )
    }

    private func apply(_ values: [String: JSONValue], success: String) {
        guard appState.connection.isReady else { return }
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.patchConfig(values)
                message = .success(success)
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }

    /// Non-empty trimmed lines, in order.
    static func lines(_ text: String) -> [String] {
        text.split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }
}

private struct ProfileOptionRow: View {
    let option: PermissionProfileOption
    let isSelected: Bool
    let disabled: Bool
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: isSelected ? Icons.radioOn : Icons.radioOff)
                    .font(.system(size: 14))
                    .foregroundStyle(isSelected ? Theme.accent : Theme.textTertiary)
                    .padding(.top, 1)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(option.title)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(Theme.textPrimary)
                        Text(verbatim: option.id)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                    Text(option.description)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .fill(isSelected ? Theme.surfaceRaised : Color.clear)
            )
            .contentShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(disabled)
        .hoverHighlight()
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
        .accessibilityLabel("\(option.title): \(option.description)")
    }
}

/// Multi-line, monospace list of command prefixes (one per line).
private struct CommandListEditor: View {
    @Binding var text: String
    let placeholder: String

    var body: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: $text)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textPrimary)
                .scrollContentBackground(.hidden)
                .padding(.horizontal, 4)
                .padding(.vertical, 4)
            if text.isEmpty {
                Text(verbatim: placeholder)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textTertiary)
                    .padding(.horizontal, 9)
                    .padding(.vertical, 4)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
        }
        .frame(height: 84)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
    }
}

// MARK: - Appearance

/// System / Light / Dark (`Preferences.appearance`, applied live).
struct AppearanceSettingsSection: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        @Bindable var preferences = appState.preferences
        SettingsPage {
            SettingsGroup("Appearance", footer: "System follows the macOS appearance; Light and Dark pin the app. Transcript text, code blocks and diffs use matching light and dark palettes.") {
                Picker("Appearance", selection: $preferences.appearance) {
                    ForEach(Appearance.allCases) { appearance in
                        Text(appearance.label).tag(appearance)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 320)
                .accessibilityLabel("Appearance")
                HStack(spacing: 14) {
                    ForEach(Appearance.allCases) { appearance in
                        AppearanceSwatch(appearance: appearance, isSelected: preferences.appearance == appearance) {
                            preferences.appearance = appearance
                        }
                    }
                }
            }
        }
    }
}

/// A tiny window mock-up showing the palette of one appearance.
private struct AppearanceSwatch: View {
    let appearance: Appearance
    let isSelected: Bool
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            VStack(spacing: 6) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(canvas)
                    HStack(spacing: 4) {
                        RoundedRectangle(cornerRadius: 3, style: .continuous)
                            .fill(sidebar)
                            .frame(width: 22)
                        VStack(alignment: .leading, spacing: 4) {
                            RoundedRectangle(cornerRadius: 2).fill(text).frame(width: 36, height: 4)
                            RoundedRectangle(cornerRadius: 2).fill(text.opacity(0.5)).frame(width: 48, height: 4)
                            Spacer(minLength: 0)
                            RoundedRectangle(cornerRadius: 3, style: .continuous).fill(surface).frame(height: 12)
                        }
                        .padding(.vertical, 2)
                    }
                    .padding(6)
                }
                .frame(width: 112, height: 68)
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(isSelected ? Theme.accent : Theme.border, lineWidth: isSelected ? 2 : 1)
                )
                Text(appearance.label)
                    .font(Theme.Font.caption)
                    .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(appearance.label) appearance")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private var isDark: Bool {
        switch appearance {
        case .dark: return true
        case .light: return false
        case .system: return NSApp?.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        }
    }

    // Fixed light or dark values (the swatch previews one appearance), read
    // from the same hex pairs the tokens are built from.
    private var canvas: Color { fixed(light: Theme.Hex.canvasLight, dark: Theme.Hex.canvasDark) }
    private var sidebar: Color { fixed(light: Theme.Hex.sidebarLight, dark: Theme.Hex.sidebarDark) }
    private var surface: Color { fixed(light: Theme.Hex.surfaceLight, dark: Theme.Hex.surfaceDark) }
    private var text: Color { fixed(light: Theme.Hex.textPrimaryLight, dark: Theme.Hex.textPrimaryDark) }

    private func fixed(light: String, dark: String) -> Color {
        Color(nsColor: NSColor(hexString: isDark ? dark : light) ?? (isDark ? .black : .white))
    }
}

// MARK: - AppState helpers (public API only)

extension AppState {
    /// `doctor/run` without a project, so the result lands in
    /// `doctorReport` for Settings even while a project is selected
    /// (`runDoctor(probe:)` routes to the selected project's card instead).
    func probeEndpointFromSettings() async {
        guard !doctorRunning else { return }
        doctorRunning = true
        defer { doctorRunning = false }
        do {
            doctorReport = try await backend.doctorRun(projectId: nil, probe: true)
            doctorError = nil
        } catch {
            doctorError = error.localizedDescription
        }
    }
}

// MARK: - Shared chrome

/// Scrollable page with the canvas background and 20 pt padding, no wider
/// than the main window's readable width.
struct SettingsPage<Content: View>: View {
    @ViewBuilder let content: () -> Content

    init(@ViewBuilder content: @escaping () -> Content) {
        self.content = content
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                content()
            }
            .frame(maxWidth: Theme.Metrics.contentMaxWidth, alignment: .leading)
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Theme.canvas)
    }
}

/// A titled card with an optional footer line.
struct SettingsGroup<Content: View>: View {
    let title: String
    let footer: String?
    @ViewBuilder let content: () -> Content

    init(_ title: String, footer: String? = nil, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.footer = footer
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            VStack(alignment: .leading, spacing: 10) {
                content()
            }
            if let footer {
                Text(footer)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }
}

/// Label column (128 pt, right-aligned) + control.
struct SettingsRow<Content: View>: View {
    let label: String
    let alignment: VerticalAlignment
    @ViewBuilder let content: () -> Content

    init(_ label: String, alignment: VerticalAlignment = .firstTextBaseline, @ViewBuilder content: @escaping () -> Content) {
        self.label = label
        self.alignment = alignment
        self.content = content
    }

    var body: some View {
        HStack(alignment: alignment, spacing: 12) {
            Text(label)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 128, alignment: .trailing)
                .lineLimit(2)
                .multilineTextAlignment(.trailing)
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct SettingsMessage: Equatable {
    enum Kind: Equatable { case success, error, info }
    var kind: Kind
    var text: String

    static func success(_ text: String) -> SettingsMessage { SettingsMessage(kind: .success, text: text) }
    static func error(_ text: String) -> SettingsMessage { SettingsMessage(kind: .error, text: text) }
    static func info(_ text: String) -> SettingsMessage { SettingsMessage(kind: .info, text: text) }
}

/// One-line feedback under a form: success, error or a neutral note.
struct InlineMessage: View {
    let message: SettingsMessage

    init(_ message: SettingsMessage) {
        self.message = message
    }

    var body: some View {
        HStack(alignment: .top, spacing: 6) {
            Image(systemName: icon)
                .font(.system(size: 12, weight: .medium))
                .padding(.top, 1)
            Text(verbatim: message.text)
                .font(Theme.Font.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .selectableText()
        }
        .foregroundStyle(tint)
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tint.opacity(0.08), in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .accessibilityElement(children: .combine)
    }

    private var icon: String {
        switch message.kind {
        case .success: return Icons.check
        case .error: return Icons.warning
        case .info: return Icons.info
        }
    }

    private var tint: Color {
        switch message.kind {
        case .success: return Theme.success
        case .error: return Theme.danger
        case .info: return Theme.textSecondary
        }
    }
}

/// Monospace path with Reveal in Finder and Copy.
private struct PathValue: View {
    let path: String?

    var body: some View {
        if let path, !path.isEmpty {
            HStack(spacing: 4) {
                Text(verbatim: path)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .selectableText()
                CopyButton(text: path, label: "Copy path")
                IconButton(Icons.folder, label: "Reveal in Finder", size: 13) {
                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                }
            }
        } else {
            Text("—")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textTertiary)
                .accessibilityLabel("Unknown until the backend connects")
        }
    }
}

/// Shown by the forms that need `appState.config` while the backend is down.
struct BackendUnavailable: View {
    @Environment(AppState.self) private var appState
    /// Rows per card of the page's skeleton, shown while ctx is starting or
    /// its configuration has not arrived yet.
    var skeleton: [Int] = [3, 2]

    var body: some View {
        if loading {
            SettingsPageSkeleton(rows: skeleton, label: "Loading settings")
        } else {
            placeholder
        }
    }

    private var loading: Bool {
        if appState.backendStarting { return true }
        switch appState.connection {
        case .connecting: return true
        case .ready: return appState.config == nil
        case .failed: return false
        }
    }

    private var placeholder: some View {
        EmptyPlaceholder(
            systemImage: Icons.warning,
            title: title,
            subtitle: subtitle,
            actionTitle: appState.backendStarting ? nil : "Retry"
        ) {
            Task { await appState.restartBackend() }
        }
        .frame(height: 320)
    }

    private var title: String {
        if case .connecting = appState.connection { return "Starting the backend…" }
        return appState.connection.failure?.title ?? "Backend not connected"
    }

    private var subtitle: String {
        if case .connecting = appState.connection { return "Settings load once ctx answers." }
        return appState.connection.failure?.detail ?? "The configuration is read from ctx once it is running."
    }
}

/// `DoctorReport` as sections of label/value rows with ✓/✗ marks.
struct DoctorResultView: View {
    let report: DoctorReport

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Text(verbatim: report.problems == 0 ? "✓" : "✗")
                    .font(.system(size: 13, weight: .semibold, design: .monospaced))
                Text(report.problems == 0 ? "No problems found" : "\(report.problems) problem\(report.problems == 1 ? "" : "s") found")
                    .font(Theme.Font.secondary)
            }
            .foregroundStyle(report.problems == 0 ? Theme.success : Theme.danger)
            ForEach(report.sections) { section in
                VStack(alignment: .leading, spacing: 4) {
                    Text(verbatim: section.title.prefix(1).uppercased() + section.title.dropFirst())
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(.top, 2)
                    ForEach(Array(section.rows.enumerated()), id: \.offset) { _, row in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(verbatim: row.ok ? "✓" : "✗")
                                .font(.system(size: 12, weight: .semibold, design: .monospaced))
                                .foregroundStyle(row.ok ? Theme.success : Theme.danger)
                                .frame(width: 12)
                            Text(verbatim: row.label)
                                .font(Theme.Font.secondary)
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 128, alignment: .leading)
                                .lineLimit(2)
                            Text(verbatim: row.value)
                                .font(Theme.Font.secondary)
                                .foregroundStyle(row.ok ? Theme.textPrimary : Theme.danger)
                                .lineLimit(3)
                                .truncationMode(.middle)
                                .fixedSize(horizontal: false, vertical: true)
                                .selectableText()
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("\(row.ok ? "OK" : "Problem"): \(row.label) \(row.value)")
                    }
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
    }
}

extension View {
    /// Rounded text field in the secondary font.
    func settingsField() -> some View {
        self
            .textFieldStyle(.roundedBorder)
            .font(Theme.Font.secondary)
            .controlSize(.regular)
    }
}

// MARK: - Previews

@MainActor
private func previewAppState() -> AppState {
    let appState = AppState()
    let config = InitializeResult(
        protocolVersion: 1,
        ctxVersion: "0.1.0",
        dataDir: "/Users/me/.context-agent",
        configFile: "/Users/me/.context-agent/config.toml",
        configSources: ["/Users/me/.context-agent/config.toml"],
        mock: false,
        model: ModelSettings(
            name: "gpt-4.1",
            baseUrl: "https://api.openai.com/v1",
            apiKeyEnv: "OPENAI_API_KEY",
            apiKeySet: true,
            contextWindow: 128_000,
            maxOutputTokens: 16_000,
            reasoningEffort: "medium"
        ),
        permissions: PermissionSettings(
            profile: "workspace_autonomous",
            allowedCommands: ["cargo test", "cargo clippy"],
            deniedCommands: ["git push --force"],
            allowNetworkCommands: false
        ),
        pricing: nil
    )
    appState.config = config
    appState.connection = .ready(config)
    appState.doctorReport = DoctorReport(problems: 1, sections: [
        DoctorSection(title: "configuration", rows: [DoctorRow(label: "data dir", value: "/Users/me/.context-agent", ok: true)]),
        DoctorSection(title: "model provider", rows: [
            DoctorRow(label: "endpoint", value: "https://api.openai.com/v1", ok: true),
            DoctorRow(label: "tool calling", value: "not supported by gpt-4.1-nano", ok: false),
        ]),
    ])
    return appState
}

#Preview("Model") {
    ModelSettingsSection()
        .environment(previewAppState())
        .frame(width: 560, height: 720)
}

#Preview("Permissions") {
    PermissionsSettingsSection()
        .environment(previewAppState())
        .frame(width: 560, height: 640)
}

#Preview("General, backend down") {
    let appState = previewAppState()
    appState.connection = .failed(.notFound(searched: ["/Applications/Context.app/Contents/Helpers/ctx", "~/.cargo/bin/ctx", "/opt/homebrew/bin/ctx"]))
    return GeneralSettingsSection()
        .environment(appState)
        .frame(width: 560, height: 640)
}
