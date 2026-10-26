import SwiftUI
import AppKit

// MARK: - BYOK

/// Bring your own key: the OpenAI-compatible endpoint, its API key in the
/// Keychain, and the endpoint probe. While signed in to ModelGateway with
/// routing on, requests use the gateway's key from sign-in; the endpoint
/// fields then edit the endpoint that turning routing off switches to.
struct BYOKSettingsSection: View {
    @Environment(AppState.self) private var appState

    @State private var baseURL = ""
    @State private var syncedURL: String?
    @State private var apiKey = ""
    @State private var apiKeyStored: Bool?
    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsPage {
            if appState.config != nil {
                if appState.gateway.isSignedIn {
                    routingGroup
                }
                endpointGroup
                probeGroup
            } else {
                BackendUnavailable(skeleton: [3, 2])
            }
        }
        .onChange(of: savedBaseURL, initial: true) { _, url in
            sync(url)
        }
        .task(id: appState.apiKeyEnvName) {
            await refreshKeyStatus()
        }
    }

    // MARK: Routing

    private var routingGroup: some View {
        let routing = Binding(
            get: { appState.preferences.gatewayRouteRequests },
            set: { enabled in Task { await appState.setGatewayRouting(enabled) } }
        )
        return SettingsGroup("ModelGateway", footer: "You are signed in to ModelGateway. Turn routing off to send requests to your own endpoint below with your own key; your ModelGateway account stays signed in.") {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Route model requests through ModelGateway")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textPrimary)
                    Text(appState.gatewayRoutingActive ? "On: requests use your ModelGateway balance or plan." : "Off: requests use your own endpoint and key below.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                Spacer(minLength: 8)
                Toggle("Route model requests through ModelGateway", isOn: routing)
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }
        }
    }

    // MARK: Endpoint

    /// The endpoint saved for BYOK: the configured one, or while routing
    /// through ModelGateway the one turning routing off restores.
    private var savedBaseURL: String {
        if appState.gatewayRoutingActive {
            return appState.preferences.gatewayPreviousEndpoint["base_url"].flatMap { $0.isEmpty ? nil : $0 } ?? GatewayState.byokDefaultBaseURL
        }
        return appState.config?.model.baseUrl ?? ""
    }

    private var endpointGroup: some View {
        let routing = appState.gatewayRoutingActive
        return SettingsGroup("Your endpoint", footer: "Any OpenAI-compatible endpoint (OpenAI, OpenRouter, a local server). The key is stored in the macOS Keychain only. Saving rewrites ~/.context-agent/config.toml; idle projects use it at once, running ones after their task finishes.") {
            if routing {
                InlineMessage(.info("Requests go through ModelGateway right now. Turning routing off switches to this endpoint and key."))
            }
            SettingsRow("Base URL") {
                TextField(GatewayState.byokDefaultBaseURL, text: $baseURL)
                    .settingsField()
                    .onSubmit(save)
            }
            SettingsRow("API key") {
                HStack(spacing: 8) {
                    SecureField(apiKeyStored == true ? "Stored in Keychain; paste a new key to replace it" : "Paste your API key", text: $apiKey)
                        .settingsField()
                        .onSubmit(save)
                    switch apiKeyStored {
                    case true?:
                        Chip("Stored", systemImage: "checkmark", style: .tinted(Theme.success))
                        Button("Remove") { removeKey() }
                            .controlSize(.small)
                            .disabled(saving)
                    case false?:
                        Chip("Not set")
                    case nil:
                        BrailleSpinner(size: 11)
                    }
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

    private var trimmedKey: String { apiKey.trimmingCharacters(in: .whitespacesAndNewlines) }

    private var isDirty: Bool {
        guard let syncedURL else { return false }
        return baseURL.trimmingCharacters(in: .whitespacesAndNewlines) != syncedURL || !trimmedKey.isEmpty
    }

    private func sync(_ url: String) {
        let keepEdits = syncedURL != nil && baseURL.trimmingCharacters(in: .whitespacesAndNewlines) != syncedURL
        syncedURL = url
        if !keepEdits { baseURL = url }
    }

    private func revert() {
        guard let syncedURL else { return }
        baseURL = syncedURL
        apiKey = ""
        message = nil
    }

    private func save() {
        guard !saving, appState.connection.isReady, isDirty else { return }
        let url = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty else {
            message = .error("Base URL cannot be empty.")
            return
        }
        guard url.hasPrefix("http://") || url.hasPrefix("https://") else {
            message = .error("Base URL must start with http:// or https://.")
            return
        }
        let key = trimmedKey
        let routing = appState.gatewayRoutingActive
        let restarts = !appState.anyProjectRunning
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                if url != syncedURL {
                    if routing {
                        appState.preferences.gatewayPreviousEndpoint = ["base_url": url]
                        sync(url)
                    } else {
                        try await appState.patchConfig(["model.base_url": .string(url)])
                    }
                }
                if !key.isEmpty {
                    try await appState.saveAPIKey(key)
                    apiKey = ""
                    apiKeyStored = true
                }
                if routing {
                    message = .success("Saved. Turning routing off uses this endpoint.")
                } else if !key.isEmpty, !restarts {
                    message = .info("Saved. The key is used once the running task finishes and the backend restarts (General).")
                } else {
                    message = .success("Saved. Idle projects use the new endpoint now.")
                }
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }

    private func refreshKeyStatus() async {
        let account = appState.apiKeyEnvName
        let stored = await Task.detached(priority: .userInitiated) { KeychainStore.isSet(account: account) }.value
        apiKeyStored = stored
    }

    private func removeKey() {
        guard !saving else { return }
        saving = true
        message = nil
        let restarts = !appState.anyProjectRunning
        Task {
            defer { saving = false }
            do {
                try await appState.removeAPIKey()
                apiKeyStored = false
                message = restarts
                    ? .success("Key removed from the Keychain.")
                    : .info("Key removed from the Keychain. The running backend keeps it until it restarts.")
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }

    // MARK: Probe

    private var probeGroup: some View {
        SettingsGroup("Endpoint check", footer: "Runs ctx doctor with a probe against the user-level configuration: real requests go to the endpoint, so it waits while a task is running. The capability profile it finds is saved for later runs.") {
            HStack(spacing: 8) {
                Button("Probe endpoint") {
                    Task { await appState.probeEndpointFromSettings() }
                }
                .controlSize(.small)
                .disabled(probeDisabled)
                .help(probeHelp)
                if appState.doctorRunning {
                    BrailleSpinner()
                    Text("Probing… this can take up to 2 minutes")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                }
            }
            if let error = appState.doctorError {
                InlineMessage(.error(error))
            }
            if let report = appState.doctorReport {
                DoctorResultView(report: report)
            }
        }
    }

    private var probeDisabled: Bool {
        appState.doctorRunning || appState.anyProjectRunning || !appState.connection.isReady
    }

    private var probeHelp: String {
        if !appState.connection.isReady { return "The backend is not connected" }
        if appState.anyProjectRunning { return "Finishes after the running task" }
        if appState.doctorRunning { return "A probe is already running" }
        return "Send a test request to the model endpoint and check the configuration"
    }
}

/// Dollar amounts typed into and shown by the Settings forms.
enum SettingsNumber {
    /// "2", "2.5", "$0.50", "1,000" → a non-negative amount.
    static func usd(_ text: String) -> Double? {
        var cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if cleaned.hasPrefix("$") { cleaned.removeFirst() }
        cleaned = cleaned.replacingOccurrences(of: ",", with: "")
        guard let value = Double(cleaned), value.isFinite, value >= 0 else { return nil }
        return value
    }

    /// 2.0 → "2", 0.5 → "0.5" (no grouping, up to six decimals).
    static func format(_ value: Double) -> String {
        value.formatted(.number.grouping(.never).precision(.fractionLength(0...6)))
    }
}
