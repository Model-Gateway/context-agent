import SwiftUI
import AppKit
import Foundation

// "Sign in with ModelGateway" (ARCHITECTURE.md "Account"): the optional
// account whose API key routes model requests through the gateway and whose
// usage the Account page charts. `GatewayState` is the observable state;
// the `AppState` extension below holds the intents. The app never speaks
// HTTP itself and never sees the key: `BackendClient.gateway*` relay to
// `ctx serve`'s `gateway/*` methods, the backend keeps the approved device
// key in its credential store, and `model.credential = "modelgateway"`
// makes the provider use it (no environment variable, no Keychain).

@MainActor
@Observable
final class GatewayState {
    /// The backend's stored credential (`model.credential`) holding the device key.
    static let credentialName = "modelgateway"
    static let defaultBaseURL = "https://www.modelgateway.co"
    /// The BYOK endpoint when none was saved before routing through the gateway.
    static let byokDefaultBaseURL = "https://api.openai.com/v1"
    /// The key variable of the retired Keychain-based gateway sign-in; the
    /// BYOK key uses `byokKeyVariable` instead.
    static let legacyKeyVariable = "MODELGATEWAY_API_KEY"
    static let byokKeyVariable = "OPENAI_API_KEY"
    /// Sent with every model request while per-project usage is on
    /// (`model.project_header`); the gateway stores it with its usage rows.
    static let projectHeader = "X-ModelGateway-Project"
    /// 53 weeks: what the heatmap shows.
    static let usageDays = 371

    enum Flow: Equatable {
        /// `gateway/device/start` in flight.
        case starting
        /// The code is shown; polling until the browser decides.
        case waiting(userCode: String, verificationURL: String, expiresAt: Date)
        /// Approved: storing the key and switching the endpoint.
        case completing
        case failed(String)
    }

    var account: GatewayAccount?
    var flow: Flow?
    /// The last usage report, broken down for each of the Usage card's ranges.
    var usage: [UsageRange: UsageBreakdown]?
    var usageLoading = false
    var usageError: String?
    var accountError: String?
    var refreshing = false
    var signingOut = false
    var lastRefreshed: Date?

    /// Settings → Billing: top-up options, plans on sale, the preference.
    var billing: GatewayBilling?
    var billingLoading = false
    var billingError: String?
    /// A checkout, plan purchase or preference change in flight.
    var billingBusy = false
    /// The Account page's Context credit claim in flight.
    var claimingContextCredit = false
    /// The Account page's Invite card (`gateway/referral`).
    var referral: GatewayReferral?
    var referralLoading = false
    var referralError: String?
    /// A split change or a code attach in flight.
    var referralBusy = false
    /// Settings → Spending: the device key's price limits and spending cap.
    var keySettings: GatewayKeySettings?
    var keySettingsError: String?
    /// Settings → Spending: the model ids ModelGateway serves, one price
    /// limit row each.
    var models: [String] = []
    /// Assistant messages reported this session (`AssistantTurn.reportKey`),
    /// so their Report button reads Reported.
    var reportedResponses: Set<String> = []

    @ObservationIgnored var pollTask: Task<Void, Never>?

    init(preferences: Preferences) {
        if let data = preferences.gatewayAccount {
            account = try? JSONDecoder().decode(GatewayAccount.self, from: data)
        }
    }

    var isSignedIn: Bool { account != nil }

    /// A sign-in is under way (not a failed one waiting to be dismissed).
    var isSigningIn: Bool {
        switch flow {
        case .starting, .waiting, .completing: return true
        case .failed, nil: return false
        }
    }
}

// MARK: - Intents

extension AppState {
    /// The gateway origin ("https://www.modelgateway.co"); not user-configurable.
    var gatewayOrigin: String { GatewayState.defaultBaseURL }

    /// The OpenAI-compatible endpoint of the gateway.
    var gatewayModelBaseURL: String { gatewayOrigin + "/v1" }

    /// Lower-cased scheme + host, no trailing slash or `/v1`; the default
    /// origin for an empty value.
    static func normalizedGatewayOrigin(_ raw: String) -> String {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { text = GatewayState.defaultBaseURL }
        if !text.lowercased().hasPrefix("http://") && !text.lowercased().hasPrefix("https://") {
            text = "https://" + text
        }
        while text.hasSuffix("/") { text.removeLast() }
        if text.hasSuffix("/v1") { text.removeLast(3) }
        while text.hasSuffix("/") { text.removeLast() }
        return text
    }

    /// Whether the user-level model endpoint is the gateway's.
    func isGatewayEndpoint(_ model: ModelSettings) -> Bool {
        model.credential == GatewayState.credentialName || Self.normalizedGatewayOrigin(model.baseUrl) == gatewayOrigin
    }

    /// Signed in and the backend is sending model requests to the gateway.
    var gatewayRoutingActive: Bool {
        guard let model = config?.model else { return false }
        return gateway.isSignedIn && isGatewayEndpoint(model)
    }

    /// After the backend is up: refresh a remembered account, or forget it
    /// when the backend no longer holds the key (signed out elsewhere, a
    /// fresh data dir).
    func restoreGatewaySession() async {
        guard gateway.isSignedIn, connection.isReady else { return }
        if let status = try? await backend.gatewayStatus(), !status.signedIn {
            gateway.account = nil
            gateway.usage = nil
            persistGatewayAccount()
            return
        }
        await refreshGateway()
    }

    // MARK: Sign in

    /// Starts the device flow: a code pair from the gateway, the browser
    /// opened on the approval page, then polling until the user decides.
    /// Clicking again while a code is pending just reopens the browser.
    func beginGatewaySignIn() {
        if case .waiting(_, let url, _)? = gateway.flow {
            Self.openInBrowser(url)
            return
        }
        guard !gateway.isSigningIn else { return }
        guard connection.isReady, config?.mock != true else {
            gateway.flow = .failed(config?.mock == true ? "Sign-in needs the real backend; turn off the mock backend in Settings → General." : "Sign-in needs the backend running; start it and try again.")
            return
        }
        gateway.flow = .starting
        gateway.pollTask?.cancel()
        let origin = gatewayOrigin
        gateway.pollTask = Task { [weak self] in
            guard let self else { return }
            let start: GatewayDeviceStart
            do {
                start = try await backend.gatewayDeviceStart(baseURL: origin, deviceName: nil)
            } catch {
                if !Task.isCancelled { gateway.flow = .failed(Self.gatewayMessage(error)) }
                return
            }
            guard !Task.isCancelled else { return }
            let expiresAt = Date().addingTimeInterval(TimeInterval(max(start.expiresIn, 60)))
            gateway.flow = .waiting(userCode: start.userCode, verificationURL: start.verificationUriComplete, expiresAt: expiresAt)
            Self.openInBrowser(start.verificationUriComplete)
            await pollGateway(origin: origin, deviceCode: start.deviceCode, interval: max(start.interval, 2), expiresAt: expiresAt)
        }
    }

    func cancelGatewaySignIn() {
        gateway.pollTask?.cancel()
        gateway.pollTask = nil
        gateway.flow = nil
    }

    /// Clears a failed sign-in's message.
    func dismissGatewayError() {
        if case .failed? = gateway.flow { gateway.flow = nil }
    }

    private func pollGateway(origin: String, deviceCode: String, interval: Int, expiresAt: Date) async {
        var consecutiveFailures = 0
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(interval))
            if Task.isCancelled { return }
            if Date() >= expiresAt {
                gateway.flow = .failed("The code expired before it was approved. Start the sign-in again.")
                return
            }
            do {
                let poll = try await backend.gatewayDevicePoll(baseURL: origin, deviceCode: deviceCode)
                consecutiveFailures = 0
                switch poll.status {
                case "pending":
                    continue
                case "approved":
                    // The backend has stored the key by now.
                    gateway.flow = .completing
                    await completeGatewaySignIn(account: poll.account)
                    return
                case "denied":
                    gateway.flow = .failed("The request was denied in the browser.")
                    return
                case "expired":
                    gateway.flow = .failed("The code expired before it was approved. Start the sign-in again.")
                    return
                default:
                    gateway.flow = .failed("Unexpected reply from ModelGateway: \(poll.status)")
                    return
                }
            } catch {
                if Task.isCancelled { return }
                // The gateway refused the code outright: no point polling on.
                if case BackendError.rpc(_, _, let kind) = error, kind == "gateway" {
                    gateway.flow = .failed(Self.gatewayMessage(error))
                    return
                }
                consecutiveFailures += 1
                if consecutiveFailures >= 3 {
                    gateway.flow = .failed(Self.gatewayMessage(error))
                    return
                }
            }
        }
    }

    private func completeGatewaySignIn(account: GatewayAccount?) async {
        gateway.account = account
        gateway.accountError = nil
        persistGatewayAccount()
        gateway.flow = nil
        gateway.pollTask = nil
        toasts.show("Signed in to ModelGateway" + (account.map { " as \($0.name)" } ?? ""), kind: .success)
        if preferences.gatewayRouteRequests {
            do {
                try await applyGatewayEndpoint()
            } catch {
                toasts.show("Signed in, but the model endpoint could not be switched to ModelGateway: \(Self.gatewayMessage(error))", kind: .error)
            }
        }
        await refreshGateway()
        // Connected hosts use this sign-in too.
        await forwardSignInToHosts()
    }

    // MARK: Sign out

    /// Has the backend revoke the device's key on the gateway and forget it,
    /// then puts the previous model endpoint back.
    func signOutOfGateway() async {
        guard !gateway.signingOut else { return }
        gateway.signingOut = true
        defer { gateway.signingOut = false }
        cancelGatewaySignIn()
        let tokenName = gateway.account?.token?.name ?? "Context"
        if connection.isReady {
            do {
                let result = try await backend.gatewaySignOut(baseURL: gatewayOrigin)
                if !result.revoked {
                    toasts.show("The device key could not be revoked on ModelGateway. Hidden device keys are revoked from the gateway when you sign in again.", kind: .warning)
                }
            } catch {
                toasts.show("The device key could not be revoked on ModelGateway (\(Self.gatewayMessage(error))); “\(tokenName)” may still be active there.", kind: .warning)
            }
        } else {
            toasts.show("The backend is not running; the device key stays stored until it is.", kind: .warning)
        }
        gateway.account = nil
        gateway.usage = nil
        gateway.billing = nil
        gateway.billingError = nil
        gateway.keySettings = nil
        gateway.keySettingsError = nil
        gateway.usageError = nil
        gateway.accountError = nil
        gateway.lastRefreshed = nil
        persistGatewayAccount()
        do {
            try await restoreEndpointAfterGateway()
        } catch {
            toasts.show("Signed out, but the previous model endpoint could not be restored: \(Self.gatewayMessage(error))", kind: .error)
        }
        toasts.show("Signed out of ModelGateway", kind: .info)
        // Connected hosts drop the forwarded key too.
        await forwardSignInToHosts()
    }

    // MARK: Refresh

    /// Account summary, then the usage rows.
    func refreshGateway() async {
        guard gateway.isSignedIn, connection.isReady, !gateway.refreshing else { return }
        gateway.refreshing = true
        defer { gateway.refreshing = false }
        let origin = gatewayOrigin
        do {
            gateway.account = try await backend.gatewayAccount(baseURL: origin)
            gateway.accountError = nil
            persistGatewayAccount()
        } catch {
            gateway.accountError = Self.isUnauthorized(error)
                ? "ModelGateway no longer accepts this device's key (it may have been revoked). Sign out and sign in again."
                : Self.isSignedOut(error)
                ? "The backend has no ModelGateway key stored. Sign out and sign in again."
                : Self.gatewayMessage(error)
        }
        await refreshGatewayUsage(origin: origin)
        gateway.lastRefreshed = Date()
    }

    func refreshGatewayUsage() async {
        guard gateway.isSignedIn, connection.isReady else { return }
        await refreshGatewayUsage(origin: gatewayOrigin)
    }

    private func refreshGatewayUsage(origin: String) async {
        guard !gateway.usageLoading else { return }
        gateway.usageLoading = true
        defer { gateway.usageLoading = false }
        let end = Int64(Date().timeIntervalSince1970)
        let start = end - Int64(GatewayState.usageDays) * 86_400
        do {
            let report = try await backend.gatewayUsage(baseURL: origin, startTimestamp: start, endTimestamp: end)
            // Folding a year of hourly rows into days is CPU work; keep it off the main actor.
            gateway.usage = await Task.detached(priority: .userInitiated) { UsageBreakdown.byRange(report: report) }.value
            gateway.usageError = nil
        } catch {
            gateway.usageError = Self.gatewayMessage(error)
        }
    }

    // MARK: Context credit

    /// Claims the welcome Context credit (the Account page's card).
    func claimContextCredit() async {
        guard gateway.isSignedIn, connection.isReady, !gateway.claimingContextCredit else { return }
        gateway.claimingContextCredit = true
        defer { gateway.claimingContextCredit = false }
        do {
            let credit = try await backend.gatewayClaimContextCredit(baseURL: gatewayOrigin)
            gateway.account?.contextCredit = credit
            persistGatewayAccount()
            toasts.show("Claimed \(gateway.account?.formatQuota(credit.granted) ?? "your") Context credit", kind: .success)
        } catch {
            toasts.show("The credit could not be claimed: \(Self.gatewayMessage(error))", kind: .error)
            // The gateway knows best whether it is claimable; show its view.
            await refreshGateway()
        }
    }

    // MARK: Referrals

    /// The Invite card's referees and terms (`gateway/referral`).
    func refreshGatewayReferral() async {
        guard gateway.isSignedIn, connection.isReady, !gateway.referralLoading else { return }
        gateway.referralLoading = true
        defer { gateway.referralLoading = false }
        do {
            gateway.referral = try await backend.gatewayReferral(baseURL: gatewayOrigin)
            gateway.referralError = nil
        } catch {
            gateway.referralError = Self.gatewayMessage(error)
        }
    }

    /// Changes how much of the match pool future referees get.
    func setGatewayReferralSplit(_ cents: Int) async {
        guard gateway.isSignedIn, connection.isReady, !gateway.referralBusy else { return }
        gateway.referralBusy = true
        defer { gateway.referralBusy = false }
        do {
            let result = try await backend.gatewayUpdateReferralSplit(baseURL: gatewayOrigin, splitRefereeCents: cents)
            gateway.referral?.summary.splitRefereeCents = result.splitRefereeCents
            gateway.account?.referral?.splitRefereeCents = result.splitRefereeCents
            persistGatewayAccount()
        } catch {
            toasts.show("The split could not be changed: \(Self.gatewayMessage(error))", kind: .error)
            await refreshGatewayReferral()
        }
    }

    /// Adds a friend's code to this (new) account; the gift shows on the
    /// Context credit card once it goes through.
    func attachGatewayReferralCode(_ code: String) async -> Bool {
        guard gateway.isSignedIn, connection.isReady, !gateway.referralBusy else { return false }
        gateway.referralBusy = true
        defer { gateway.referralBusy = false }
        do {
            let result = try await backend.gatewayAttachReferralCode(baseURL: gatewayOrigin, code: code)
            gateway.account?.referral?.invitedBy = result.invitedBy
            gateway.account?.referral?.attachAllowed = false
            persistGatewayAccount()
            toasts.show("\(result.invitedBy.displayName) is now your referrer", kind: .success)
            await refreshGateway()
            return true
        } catch {
            toasts.show("The code could not be added: \(Self.gatewayMessage(error))", kind: .error)
            return false
        }
    }

    /// Reports a finished assistant message as a bad response; the gateway
    /// counts it against the provider that served it. True once filed.
    func reportResponse(projectId: String, turn: AssistantTurn, reason: ResponseReportReason, comment: String) async -> Bool {
        guard let taskId = turn.taskId, connection.isReady else { return false }
        do {
            _ = try await backend.gatewayReport(baseURL: gatewayOrigin, projectId: projectId, taskId: taskId, step: turn.step, reason: reason.rawValue, comment: comment)
            gateway.reportedResponses.insert(turn.reportKey)
            toasts.show("Thanks, the response was reported", kind: .success)
            return true
        } catch {
            toasts.show("The response could not be reported: \(Self.gatewayMessage(error))", kind: .error)
            return false
        }
    }

    // MARK: Settings

    /// "Route model requests through ModelGateway".
    func setGatewayRouting(_ enabled: Bool) async {
        preferences.gatewayRouteRequests = enabled
        guard gateway.isSignedIn else { return }
        do {
            if enabled {
                try await applyGatewayEndpoint()
            } else {
                try await restoreEndpointAfterGateway()
            }
        } catch {
            toasts.show("The model endpoint could not be changed: \(Self.gatewayMessage(error))", kind: .error)
        }
    }

    /// "Attribute usage to projects" (the `X-ModelGateway-Project` header).
    func setGatewayProjectAttribution(_ enabled: Bool) async {
        preferences.gatewayAttributeProjects = enabled
        guard gatewayRoutingActive else { return }
        do {
            try await patchConfig(["model.project_header": enabled ? .string(GatewayState.projectHeader) : .null])
        } catch {
            toasts.show("The project header setting could not be saved: \(Self.gatewayMessage(error))", kind: .error)
        }
    }

    // MARK: Key settings (Settings → Spending)

    func refreshGatewayKeySettings() async {
        guard gateway.isSignedIn, connection.isReady else { return }
        do {
            gateway.keySettings = try await backend.gatewayKeySettings(baseURL: gatewayOrigin)
            gateway.keySettingsError = nil
        } catch {
            gateway.keySettingsError = Self.gatewayMessage(error)
        }
    }

    /// `models/list` on the user-level endpoint, kept only while that is the
    /// gateway (otherwise it lists the BYOK endpoint's models) and the list
    /// came from it rather than the configured-model fallback.
    func refreshGatewayModels() async {
        guard gatewayRoutingActive, connection.isReady else { return }
        guard let list = try? await backend.modelsList(), list.source != "fallback" else { return }
        gateway.models = list.models
    }

    /// Saves `values` (`price_limits`, `unlimited_quota`, `remain_quota`)
    /// on the device key.
    func updateGatewayKeySettings(_ values: [String: JSONValue]) async throws {
        gateway.keySettings = try await backend.gatewayUpdateKeySettings(baseURL: gatewayOrigin, values: values)
        gateway.keySettingsError = nil
    }

    // MARK: Billing (Settings → Billing)

    func refreshGatewayBilling() async {
        guard gateway.isSignedIn, connection.isReady, !gateway.billingLoading else { return }
        gateway.billingLoading = true
        defer { gateway.billingLoading = false }
        do {
            gateway.billing = try await backend.gatewayBilling(baseURL: gatewayOrigin)
            gateway.billingError = nil
        } catch {
            gateway.billingError = Self.gatewayMessage(error)
        }
    }

    /// Whether requests draw on the plan or the credit balance first.
    func setGatewayBillingPreference(_ preference: GatewayBillingPreference) async throws {
        guard !gateway.billingBusy else { return }
        gateway.billingBusy = true
        defer { gateway.billingBusy = false }
        let result = try await backend.gatewayBillingPreference(baseURL: gatewayOrigin, preference: preference.rawValue)
        gateway.billing?.billingPreference = result.billingPreference
        gateway.account?.billingPreference = result.billingPreference
        persistGatewayAccount()
    }

    func quoteGatewayTopUp(_ amount: Int) async -> GatewayTopUpQuote? {
        try? await backend.gatewayBillingQuote(baseURL: gatewayOrigin, amount: amount)
    }

    /// Opens a checkout page for `amount` in the browser (the wallet page
    /// when the gateway cannot make one); the balance grows once paid.
    func addGatewayFunds(_ amount: Int) async throws {
        guard !gateway.billingBusy else { return }
        guard gateway.billing?.topUp.inApp ?? false else {
            openGatewayWallet()
            return
        }
        gateway.billingBusy = true
        defer { gateway.billingBusy = false }
        let checkout = try await backend.gatewayBillingTopUp(baseURL: gatewayOrigin, amount: amount)
        guard let url = checkout.checkoutURL, !url.isEmpty else { throw BackendError.rpc(code: -32000, message: "ModelGateway did not return a checkout page.", kind: "gateway") }
        Self.openInBrowser(url)
    }

    /// Buys `plan`: by card in the browser (`stripe`), or from the balance at
    /// once. Whether the plan is active now (a balance purchase).
    @discardableResult
    func subscribeToGatewayPlan(_ plan: GatewayPlan, fromBalance: Bool) async throws -> Bool {
        guard !gateway.billingBusy else { return false }
        gateway.billingBusy = true
        defer { gateway.billingBusy = false }
        let result = try await backend.gatewayBillingSubscribe(baseURL: gatewayOrigin, planId: plan.id, method: fromBalance ? "balance" : "stripe")
        if let url = result.checkoutURL, !url.isEmpty {
            Self.openInBrowser(url)
            return false
        }
        if result.subscribed == true {
            gateway.billingBusy = false
            await refreshGateway()
            await refreshGatewayBilling()
            return true
        }
        throw BackendError.rpc(code: -32000, message: "ModelGateway did not return a checkout page.", kind: "gateway")
    }

    /// The gateway's own wallet page (other payment methods, redemption codes).
    func openGatewayWallet() {
        Self.openInBrowser(gateway.billing?.walletURL.flatMap { $0.isEmpty ? nil : $0 } ?? gatewayOrigin + "/wallet")
    }

    func openGatewayDashboard() {
        Self.openInBrowser(gatewayOrigin + "/dashboard")
    }

    /// The wallet page, where plans are bought and listed.
    func openGatewayPlans() {
        Self.openInBrowser(gatewayOrigin + "/wallet")
    }

    // MARK: Endpoint switching

    /// Points `model.base_url` at the gateway (keeping the previous URL to
    /// restore), names the stored credential as the key source and sets the
    /// project header. Idle projects pick the new provider up at once; the
    /// `api_key_env` variable is left alone for when the user signs out.
    private func applyGatewayEndpoint() async throws {
        guard let model = config?.model else { return }
        if !isGatewayEndpoint(model) {
            preferences.gatewayPreviousEndpoint = ["base_url": model.baseUrl]
        }
        var values: [String: JSONValue] = [:]
        if model.baseUrl != gatewayModelBaseURL { values["model.base_url"] = .string(gatewayModelBaseURL) }
        if model.credential != GatewayState.credentialName { values["model.credential"] = .string(GatewayState.credentialName) }
        values["model.project_header"] = preferences.gatewayAttributeProjects ? .string(GatewayState.projectHeader) : .null
        try await patchConfig(values)
    }

    /// The endpoint from before sign-in (or the default), credential and
    /// project header off; the key variable is as the user left it.
    private func restoreEndpointAfterGateway() async throws {
        guard let model = config?.model, isGatewayEndpoint(model) else { return }
        let previous = preferences.gatewayPreviousEndpoint
        let baseURL = previous["base_url"].flatMap { $0.isEmpty ? nil : $0 } ?? GatewayState.byokDefaultBaseURL
        try await patchConfig(["model.base_url": .string(baseURL), "model.credential": .null, "model.project_header": .null])
        preferences.gatewayPreviousEndpoint = [:]
    }

    /// Configurations from the retired Keychain-based gateway sign-in name
    /// `MODELGATEWAY_API_KEY` as the BYOK key variable, so the BYOK key field
    /// would show that old gateway key. Points the variable back at
    /// `OPENAI_API_KEY` and deletes the old Keychain entry.
    func retireLegacyGatewayKey() async {
        guard let model = config?.model, model.apiKeyEnv == GatewayState.legacyKeyVariable else { return }
        do {
            try await patchConfig(["model.api_key_env": .string(GatewayState.byokKeyVariable)])
            let account = GatewayState.legacyKeyVariable
            try await Task.detached(priority: .utility) { try KeychainStore.remove(account: account) }.value
        } catch {
            toasts.show("The old ModelGateway key variable could not be retired: \(Self.gatewayMessage(error))", kind: .warning)
        }
    }

    // MARK: Helpers

    private func persistGatewayAccount() {
        preferences.gatewayAccount = gateway.account.flatMap { try? JSONEncoder().encode($0) }
    }

    static func openInBrowser(_ string: String) {
        guard let url = URL(string: string) else { return }
        NSWorkspace.shared.open(url)
    }

    static func gatewayMessage(_ error: Error) -> String {
        if let backendError = error as? BackendError {
            switch backendError {
            case .rpc(_, let message, let kind):
                return kind == "transport" ? "ModelGateway could not be reached: \(message)" : message
            default:
                return backendError.errorDescription ?? "\(backendError)"
            }
        }
        return (error as? LocalizedError)?.errorDescription ?? "\(error)"
    }

    static func isUnauthorized(_ error: Error) -> Bool {
        if case BackendError.rpc(_, _, let kind) = error { return kind == "unauthorized" }
        return false
    }

    /// The backend has no stored ModelGateway key.
    static func isSignedOut(_ error: Error) -> Bool {
        if case BackendError.rpc(_, _, let kind) = error { return kind == "signed_out" }
        return false
    }
}
