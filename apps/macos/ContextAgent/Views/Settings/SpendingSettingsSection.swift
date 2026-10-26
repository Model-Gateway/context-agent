import SwiftUI

// MARK: - Spending

/// While signed in to ModelGateway: the device key's price limits (the most
/// it pays, as a share of each model's standard price) and spending cap,
/// the settings the website's key drawer has for ordinary keys
/// (`gateway/key`). Below them, this Mac's spend (`spend/summary`); while
/// requests do not go through ModelGateway, also the local `[spend]` caps
/// per day, week or month, on all models together and per model. A run that
/// reaches a local cap stops as blocked and says which cap it hit.
struct SpendingSettingsSection: View {
    @Environment(AppState.self) private var appState

    @State private var period: SpendPeriod = .month
    @State private var total = ""
    @State private var limits: [LimitDraft] = []
    @State private var synced: SpendSettings?
    @State private var saving = false
    @State private var message: SettingsMessage?
    @State private var summary: SpendSummary?
    @State private var summaryError: String?

    var body: some View {
        SettingsPage {
            if appState.config != nil {
                if appState.gateway.isSignedIn {
                    GatewayPriceLimitGroup()
                    GatewayKeyCapGroup()
                    DiscountDropGroup()
                } else {
                    SettingsGroup("Price limit") {
                        InlineMessage(.info("Sign in to ModelGateway on the Account page to set the most you pay for a model, as a share of its standard price."))
                    }
                }
                usageGroup
                if !appState.gatewayRoutingActive {
                    limitsGroup
                }
            } else {
                BackendUnavailable(skeleton: [2, 2, 3])
            }
        }
        .onChange(of: appState.config?.spend, initial: true) { _, spend in
            sync(spend, force: false)
        }
        .task(id: appState.config?.spend) {
            await loadSummary()
        }
        .task(id: appState.gateway.isSignedIn) {
            await appState.refreshGatewayKeySettings()
        }
    }

    // MARK: This period

    private var usageGroup: some View {
        SettingsGroup("Spent \(summary?.period.phrase ?? period.phrase)", footer: "Counts every request Context sent from this Mac (the app, the terminal UI and ctx run), priced at models.dev's list prices; requests through ModelGateway count what the gateway billed. Spend from other devices is on the Account page.") {
            if let summary {
                SpendBar(title: "All models", spent: summary.totalUsd, limit: summary.maxUsd)
                ForEach(summary.models) { model in
                    SpendBar(title: model.model, spent: model.costUsd, limit: model.limitUsd, note: model.unpricedRequests > 0 ? "\(model.unpricedRequests) request\(model.unpricedRequests == 1 ? "" : "s") without a known price" : nil, mono: true)
                }
                if summary.models.isEmpty {
                    Text("Nothing spent \(summary.period.phrase) yet.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                }
            } else if let summaryError {
                InlineMessage(.error(summaryError))
            } else {
                SettingsRowsSkeleton(count: 2, label: "Loading spending")
            }
            HStack {
                Spacer(minLength: 0)
                Button("Refresh") { Task { await loadSummary() } }
                    .controlSize(.small)
            }
        }
    }

    // MARK: Limits

    private var limitsGroup: some View {
        SettingsGroup("Limits", footer: "Before each model request the agent checks what was spent \(period.phrase) against the limit on its model and the limit on all models; at a limit the task stops and says which one. Leave a field empty for no limit.") {
            SettingsRow("Period") {
                Picker("Period", selection: $period) {
                    ForEach(SpendPeriod.allCases) { period in
                        Text(period.title).tag(period)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 280)
            }
            SettingsRow("All models") {
                dollarField("No limit", text: $total)
            }
            ForEach($limits) { $limit in
                HStack(spacing: 12) {
                    TextField("model id", text: $limit.model)
                        .settingsField()
                        .frame(width: 128)
                    dollarField("No limit", text: $limit.amount)
                    IconButton(Icons.close, label: "Remove this limit", size: 11) {
                        limits.removeAll { $0.id == limit.id }
                    }
                }
            }
            HStack(spacing: 8) {
                Button {
                    limits.append(LimitDraft(model: suggestedModel))
                } label: {
                    Label("Add model limit", systemImage: Icons.addCustom)
                }
                Button("Save") { save() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!isDirty || saving || !appState.connection.isReady)
                Button("Revert") { sync(appState.config?.spend, force: true) }
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

    private func dollarField(_ placeholder: String, text: Binding<String>) -> some View {
        HStack(spacing: 6) {
            Text("$")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textTertiary)
            TextField(placeholder, text: text)
                .settingsField()
                .frame(width: 110)
                .onSubmit(save)
            Text(period.phrase.replacingOccurrences(of: "this ", with: "per ").replacingOccurrences(of: "today", with: "per day"))
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
        }
    }

    /// The configured model, unless it already has a limit.
    private var suggestedModel: String {
        let name = appState.config?.model.name ?? ""
        return limits.contains { $0.model.trimmingCharacters(in: .whitespaces) == name } ? "" : name
    }

    private var isDirty: Bool {
        guard let synced else { return false }
        let base = Self.drafts(synced)
        return period != synced.period || total.trimmingCharacters(in: .whitespaces) != base.total || limits.map(\.normalized) != base.limits.map(\.normalized)
    }

    private static func drafts(_ spend: SpendSettings) -> (total: String, limits: [LimitDraft]) {
        let limits = spend.models.sorted { $0.key.localizedStandardCompare($1.key) == .orderedAscending }.map { LimitDraft(model: $0.key, amount: SettingsNumber.format($0.value)) }
        return (spend.maxUsd.map(SettingsNumber.format) ?? "", limits)
    }

    private func sync(_ spend: SpendSettings?, force: Bool) {
        guard let spend else { return }
        let keepEdits = !force && isDirty
        synced = spend
        if !keepEdits {
            period = spend.period
            let drafts = Self.drafts(spend)
            total = drafts.total
            limits = drafts.limits
        }
        if force { message = nil }
    }

    private func save() {
        guard !saving, appState.connection.isReady else { return }
        var values: [String: JSONValue] = ["spend.period": .string(period.rawValue)]
        let totalText = total.trimmingCharacters(in: .whitespaces)
        if totalText.isEmpty {
            values["spend.max_usd"] = .null
        } else if let amount = SettingsNumber.usd(totalText) {
            values["spend.max_usd"] = .number(amount)
        } else {
            message = .error("The limit on all models must be a dollar amount of zero or more.")
            return
        }
        var models: [String: JSONValue] = [:]
        for limit in limits {
            let model = limit.model.trimmingCharacters(in: .whitespaces)
            let amount = limit.amount.trimmingCharacters(in: .whitespaces)
            if model.isEmpty, amount.isEmpty { continue }
            guard !model.isEmpty else {
                message = .error("Every model limit needs a model id.")
                return
            }
            guard let value = SettingsNumber.usd(amount) else {
                message = .error("The limit on \(model) must be a dollar amount of zero or more.")
                return
            }
            models[model] = .number(value)
        }
        values["spend.models"] = models.isEmpty ? .null : .object(models)
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.patchConfig(values)
                message = .success("Limits saved; they apply from the next request.")
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }

    private func loadSummary() async {
        guard appState.connection.isReady else { return }
        do {
            summary = try await appState.backend.spendSummary()
            summaryError = nil
        } catch {
            summaryError = error.localizedDescription
        }
    }
}

// MARK: - ModelGateway key

/// `price_limits` of the device key: the most a request pays, as a share of
/// the model's standard output-token price and optionally as a dollar rate,
/// for all models and per model (a model's rule replaces the default). Every
/// model ModelGateway serves has a row already; an empty one uses the All
/// models row. Collapsed, only the models with a limit show.
private struct GatewayPriceLimitGroup: View {
    @Environment(AppState.self) private var appState

    @State private var all = RuleDraft(model: "")
    @State private var rows: [RuleDraft] = []
    @State private var synced: GatewayPriceLimits?
    @State private var showAll = false
    @State private var filter = ""
    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsGroup("Price limit", footer: "Requests through ModelGateway only go to providers charging at most this share of the model's standard output price, and at most the max rate when one is set. A model's own limit replaces the All models row; a model left empty uses it. When no provider is under the limit, the request fails instead of costing more. Saved on this Mac's ModelGateway key; leave All models empty for no limit.") {
            if appState.gateway.keySettings == nil {
                KeySettingsStatus()
            } else {
                if !appState.gatewayRoutingActive {
                    InlineMessage(.info("Routing through ModelGateway is off on the BYOK tab; these limits apply once it is on."))
                }
                VStack(alignment: .leading, spacing: 6) {
                    header
                    RuleDraftRow(row: $all, fixedLabel: "All models", remove: nil)
                    if showAll, rows.count > ModelCatalog.groupingThreshold {
                        TextField("Filter models", text: $filter)
                            .settingsField()
                            .padding(.top, 4)
                    }
                    LazyVStack(alignment: .leading, spacing: 6) {
                        ForEach($rows) { $row in
                            if isShown(row) {
                                RuleDraftRow(row: $row, fixedLabel: row.model, inherited: all, remove: row.isBlank ? nil : {
                                    row.paid = ""
                                    row.rate = ""
                                })
                            }
                        }
                    }
                }
                HStack(spacing: 8) {
                    if !appState.gateway.models.isEmpty {
                        Button {
                            showAll.toggle()
                            filter = ""
                        } label: {
                            Label(showAll ? "Show models with a limit" : "Show all \(rows.count) models", systemImage: showAll ? Icons.chevronUp : Icons.chevronDown)
                        }
                    }
                    Button("Save") { save() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!isDirty || saving || !appState.connection.isReady)
                    Button("Revert") { sync(appState.gateway.keySettings?.priceLimits, force: true) }
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
        .onChange(of: appState.gateway.keySettings?.priceLimits, initial: true) { _, limits in
            sync(limits, force: false)
        }
        .onChange(of: appState.gateway.models) { _, models in
            let listed = Set(rows.map(\.model))
            let added = models.filter { !listed.contains($0) }.map { RuleDraft(model: $0) }
            guard !added.isEmpty else { return }
            rows = Self.sorted(rows + added)
        }
        .task(id: appState.gatewayRoutingActive) {
            await appState.refreshGatewayModels()
        }
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("Model").frame(maxWidth: .infinity, alignment: .leading)
            Text("Pay at most").frame(width: RuleDraftRow.percentWidth, alignment: .leading)
            Text("Max per 1M output").frame(width: RuleDraftRow.rateWidth, alignment: .leading)
            Color.clear.frame(width: 20, height: 1)
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.textTertiary)
    }

    /// Expanded: every model matching the filter. Collapsed: the models
    /// with a saved or edited limit.
    private func isShown(_ row: RuleDraft) -> Bool {
        if showAll {
            let query = filter.trimmingCharacters(in: .whitespaces)
            return query.isEmpty || row.model.localizedCaseInsensitiveContains(query)
        }
        return !row.isBlank || synced?.models[row.model] != nil
    }

    private var isDirty: Bool {
        guard let synced else { return false }
        let base = Self.drafts(synced, models: [])
        return all.normalized != base.all.normalized || Self.limited(rows) != Self.limited(base.rows)
    }

    /// The rows that carry a limit, for dirty checks.
    private static func limited(_ rows: [RuleDraft]) -> [[String]] {
        rows.filter { !$0.isBlank }.map(\.normalized)
    }

    private static func sorted(_ rows: [RuleDraft]) -> [RuleDraft] {
        rows.sorted { $0.model.localizedStandardCompare($1.model) == .orderedAscending }
    }

    /// One row per model with a saved rule or served by the gateway.
    private static func drafts(_ limits: GatewayPriceLimits, models: [String]) -> (all: RuleDraft, rows: [RuleDraft]) {
        let names = Set(limits.models.keys).union(models)
        let rows = sorted(names.map { RuleDraft(model: $0, rule: limits.models[$0]) })
        return (RuleDraft(model: "", rule: limits.default), rows)
    }

    private func sync(_ limits: GatewayPriceLimits?, force: Bool) {
        guard let limits else { return }
        let keepEdits = !force && isDirty
        synced = limits
        if !keepEdits {
            let drafts = Self.drafts(limits, models: appState.gateway.models)
            all = drafts.all
            rows = drafts.rows
        }
        if force { message = nil }
    }

    private func save() {
        guard !saving, appState.connection.isReady else { return }
        var limits = GatewayPriceLimits()
        switch all.rule(for: "All models") {
        case .failure(let problem):
            message = .error(problem.message)
            return
        case .success(let rule):
            limits.default = rule.isEmpty ? nil : rule
        }
        for row in rows where !row.isBlank {
            switch row.rule(for: row.model) {
            case .failure(let problem):
                message = .error(problem.message)
                return
            case .success(let rule):
                limits.models[row.model] = rule
            }
        }
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.updateGatewayKeySettings(["price_limits": limits.json])
                message = .success("Saved on your ModelGateway key; it applies from the next request.")
            } catch {
                message = .error(AppState.gatewayMessage(error))
            }
        }
    }
}

/// `spend.confirm_discount_drop`: whether a run asks before ModelGateway
/// falls back to a provider discounted at least 10 points below the model's
/// best offer. The popup's *Always continue* turns it off; saved on change.
private struct DiscountDropGroup: View {
    @Environment(AppState.self) private var appState

    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsGroup("Lower discounts", footer: "When the providers offering a model's best discount are unavailable, ModelGateway can fall back to providers discounted at least 10 points less (98% off becoming 88% off or less). With this on, the run pauses and asks whether to cancel, continue, or always continue.") {
            SettingsRow("Ask before a much lower discount") {
                HStack(spacing: 8) {
                    if saving {
                        BrailleSpinner()
                    }
                    Toggle("Ask before a much lower discount", isOn: confirm)
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .disabled(saving || !appState.connection.isReady)
                }
            }
            if let message {
                InlineMessage(message)
            }
        }
    }

    private var confirm: Binding<Bool> {
        Binding(
            get: { appState.config?.spend.confirmDiscountDrop ?? true },
            set: { save($0) }
        )
    }

    private func save(_ value: Bool) {
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.patchConfig(["spend.confirm_discount_drop": .bool(value)])
            } catch {
                message = .error(error.localizedDescription)
            }
        }
    }
}

/// `unlimited_quota` / `remain_quota` of the device key: what it may still
/// spend before requests through ModelGateway stop.
private struct GatewayKeyCapGroup: View {
    @Environment(AppState.self) private var appState

    @State private var capped = false
    @State private var remaining = ""
    @State private var synced: GatewayKeySettings?
    @State private var saving = false
    @State private var message: SettingsMessage?

    var body: some View {
        SettingsGroup("Spending cap", footer: "What this Mac's ModelGateway key may still spend. Each request through ModelGateway lowers it, and at zero they stop until you raise it. Your credit balance and plan apply as well.") {
            if let key = appState.gateway.keySettings {
                if key.exhausted {
                    InlineMessage(.error("This key reached its cap: requests through ModelGateway fail until you raise it."))
                }
                SettingsRow("Cap this key") {
                    Toggle("Cap this key", isOn: $capped)
                        .labelsHidden()
                        .toggleStyle(.switch)
                        .controlSize(.small)
                }
                if capped {
                    SettingsRow("Remaining") {
                        HStack(spacing: 6) {
                            if key.displayInCurrency {
                                Text("$")
                                    .font(Theme.Font.secondary)
                                    .foregroundStyle(Theme.textTertiary)
                            }
                            TextField(key.displayInCurrency ? "10.00" : "5000000", text: $remaining)
                                .settingsField()
                                .frame(width: 110)
                                .onSubmit(save)
                            if !key.displayInCurrency {
                                Text("quota units")
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.textTertiary)
                            }
                        }
                    }
                }
                SettingsRow("Spent with this key") {
                    Text(verbatim: key.formatQuota(key.usedQuota))
                        .font(Theme.Font.secondary.monospacedDigit())
                        .foregroundStyle(Theme.textSecondary)
                }
                HStack(spacing: 8) {
                    Button("Save") { save() }
                        .disabled(!isDirty || saving || !appState.connection.isReady)
                    Button("Revert") { sync(appState.gateway.keySettings, force: true) }
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
            } else {
                KeySettingsStatus()
            }
        }
        .onChange(of: appState.gateway.keySettings, initial: true) { _, key in
            sync(key, force: false)
        }
    }

    private static func remainingText(_ key: GatewayKeySettings) -> String {
        key.displayInCurrency ? SettingsNumber.format((Double(key.remainQuota) / max(key.quotaPerUnit, 1) * 100).rounded() / 100) : String(key.remainQuota)
    }

    private var isDirty: Bool {
        guard let synced else { return false }
        if capped == synced.unlimitedQuota { return true }
        return capped && remaining.trimmingCharacters(in: .whitespaces) != Self.remainingText(synced)
    }

    private func sync(_ key: GatewayKeySettings?, force: Bool) {
        guard let key else { return }
        let keepEdits = !force && isDirty
        synced = key
        if !keepEdits {
            capped = !key.unlimitedQuota
            remaining = Self.remainingText(key)
        }
        if force { message = nil }
    }

    private func save() {
        guard let key = synced, !saving, appState.connection.isReady else { return }
        var values: [String: JSONValue] = ["unlimited_quota": .bool(!capped)]
        if capped {
            guard let amount = SettingsNumber.usd(remaining) else {
                message = .error(key.displayInCurrency ? "The remaining amount must be a dollar amount of zero or more." : "The remaining amount must be a number of zero or more.")
                return
            }
            let quota = key.displayInCurrency ? (amount * key.quotaPerUnit).rounded() : amount.rounded()
            values["remain_quota"] = .number(quota)
        }
        saving = true
        message = nil
        Task {
            defer { saving = false }
            do {
                try await appState.updateGatewayKeySettings(values)
                message = .success(capped ? "Cap saved on your ModelGateway key." : "This key is no longer capped.")
            } catch {
                message = .error(AppState.gatewayMessage(error))
            }
        }
    }
}

/// Loading, or why the key settings could not be loaded.
private struct KeySettingsStatus: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let error = appState.gateway.keySettingsError {
            HStack(spacing: 8) {
                InlineMessage(.error(error))
                Spacer(minLength: 0)
                Button("Retry") { Task { await appState.refreshGatewayKeySettings() } }
                    .controlSize(.small)
            }
        } else {
            SettingsRowsSkeleton(count: 2, label: "Loading your ModelGateway key")
        }
    }
}

/// One editable price rule: the share of the standard price paid at most
/// (shown as paid, stored as `discount_percent` = 100 − paid) and the max
/// dollar rate per million output tokens.
private struct RuleDraft: Identifiable {
    struct Problem: Error { let message: String }

    let id = UUID()
    var model: String
    var paid = ""
    var rate = ""

    init(model: String) {
        self.model = model
    }

    init(model: String, rule: GatewayPriceRule?) {
        self.model = model
        paid = rule?.paidPercent.map { SettingsNumber.format(($0 * 1_000_000).rounded() / 1_000_000) } ?? ""
        rate = rule?.maxPricePerM.map(SettingsNumber.format) ?? ""
    }

    var isBlank: Bool { [paid, rate].allSatisfy { $0.trimmingCharacters(in: .whitespaces).isEmpty } }

    /// For dirty checks: trimmed fields, without the row id.
    var normalized: [String] { [model, paid, rate].map { $0.trimmingCharacters(in: .whitespaces) } }

    func rule(for name: String) -> Result<GatewayPriceRule, Problem> {
        var rule = GatewayPriceRule()
        var share = paid.trimmingCharacters(in: .whitespaces)
        if share.hasSuffix("%") { share.removeLast() }
        if !share.trimmingCharacters(in: .whitespaces).isEmpty {
            guard let value = Double(share.trimmingCharacters(in: .whitespaces)), value.isFinite, value > 0, value <= 100 else {
                return .failure(Problem(message: "The share \(name) pays must be a percentage above 0 and at most 100."))
            }
            rule.discountPercent = ((100 - value) * 1_000_000).rounded() / 1_000_000
        }
        if !rate.trimmingCharacters(in: .whitespaces).isEmpty {
            guard let value = SettingsNumber.usd(rate), value > 0 else {
                return .failure(Problem(message: "The max rate of \(name) must be a dollar amount above zero."))
            }
            rule.maxPricePerM = value
        }
        return .success(rule)
    }
}

private struct RuleDraftRow: View {
    static let percentWidth: CGFloat = 96
    static let rateWidth: CGFloat = 120

    @Binding var row: RuleDraft
    var fixedLabel: String? = nil
    /// The All models row a model row falls back to, shown as placeholders.
    var inherited: RuleDraft? = nil
    let remove: (() -> Void)?

    var body: some View {
        HStack(spacing: 8) {
            if let fixedLabel {
                Text(verbatim: fixedLabel)
                    .font(inherited == nil ? Theme.Font.secondary : Theme.Font.mono)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                TextField("model id", text: $row.model)
                    .settingsField()
                    .frame(maxWidth: .infinity)
            }
            HStack(spacing: 4) {
                TextField(placeholder(\.paid, or: "100"), text: $row.paid)
                    .settingsField()
                    .frame(width: 64)
                Text("%")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textTertiary)
            }
            .frame(width: Self.percentWidth, alignment: .leading)
            HStack(spacing: 4) {
                Text("$")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textTertiary)
                TextField(placeholder(\.rate, or: "No max"), text: $row.rate)
                    .settingsField()
                    .frame(width: 80)
            }
            .frame(width: Self.rateWidth, alignment: .leading)
            if let remove {
                IconButton(Icons.close, label: "Clear this model's limit", size: 11, action: remove)
                    .frame(width: 20)
            } else {
                Color.clear.frame(width: 20, height: 1)
            }
        }
    }

    private func placeholder(_ field: KeyPath<RuleDraft, String>, or fallback: String) -> String {
        let value = inherited?[keyPath: field].trimmingCharacters(in: .whitespaces) ?? ""
        return value.isEmpty ? fallback : value
    }
}

private struct LimitDraft: Identifiable {
    let id = UUID()
    var model: String
    var amount = ""

    var normalized: [String] { [model, amount].map { $0.trimmingCharacters(in: .whitespaces) } }
}

/// "gpt-4.1   $4.20 of $10.00" with a bar filled to the share used.
private struct SpendBar: View {
    let title: String
    let spent: Double
    let limit: Double?
    var note: String? = nil
    var mono = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(verbatim: title)
                    .font(mono ? Theme.Font.mono : Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                Text(verbatim: amount)
                    .font(Theme.Font.secondary.monospacedDigit())
                    .foregroundStyle(reached ? Theme.danger : Theme.textSecondary)
            }
            if let limit, limit > 0 {
                GeometryReader { proxy in
                    ZStack(alignment: .leading) {
                        Capsule().fill(Theme.border)
                        Capsule().fill(reached ? Theme.danger : fraction > 0.8 ? Theme.warning : Theme.accent)
                            .frame(width: proxy.size.width * fraction)
                    }
                }
                .frame(height: 4)
            }
            if let note {
                Text(note)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var fraction: Double {
        guard let limit, limit > 0 else { return 0 }
        return min(1, spent / limit)
    }

    private var reached: Bool { limit.map { spent >= $0 } ?? false }

    private var amount: String {
        let spentText = UsageFormat.usd(spent)
        guard let limit else { return spentText }
        return "\(spentText) of \(UsageFormat.usd(limit))"
    }
}
