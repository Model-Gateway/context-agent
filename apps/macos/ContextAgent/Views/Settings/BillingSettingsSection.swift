import SwiftUI
import AppKit

// MARK: - Billing

/// The ModelGateway account's money (shown only while signed in): the
/// credit balance and plan, adding funds, buying a plan, and whether
/// requests use the plan or the credits first. Payments happen on the
/// gateway's checkout page in the browser; the balance here refreshes when
/// the app comes back to the front.
struct BillingSettingsSection: View {
    @Environment(AppState.self) private var appState

    @State private var message: SettingsMessage?
    @State private var confirmBalancePlan: GatewayPlan?

    var body: some View {
        let gateway = appState.gateway
        SettingsPage {
            if !gateway.isSignedIn {
                EmptyPlaceholder(systemImage: Icons.gateway, title: "Not signed in to ModelGateway", subtitle: "Sign in on the Account page to manage credits and plans.")
                    .frame(height: 320)
            } else if !appState.connection.isReady {
                BackendUnavailable(skeleton: [2, 3, 2])
            } else {
                balanceGroup
                if let message {
                    InlineMessage(message)
                }
                if gateway.billing == nil, gateway.billingError == nil {
                    // Add funds, plans and the billing preference all come with `gateway/billing`.
                    SettingsPageSkeleton(rows: [2, 3], label: "Loading billing")
                } else {
                    AddFundsGroup(message: $message)
                    plansGroup
                    preferenceGroup
                }
            }
        }
        .task(id: gateway.isSignedIn) { await refresh() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await refresh() }
        }
        .confirmationDialog(
            "Buy \(confirmBalancePlan?.name ?? "this plan") from your balance?",
            isPresented: Binding(get: { confirmBalancePlan != nil }, set: { if !$0 { confirmBalancePlan = nil } }),
            presenting: confirmBalancePlan
        ) { plan in
            Button("Pay \(plan.priceTitle) from balance") { subscribe(plan, fromBalance: true) }
            Button("Cancel", role: .cancel) {}
        } message: { plan in
            Text("\(plan.priceTitle) is taken from your credit balance now and the plan starts at once.")
        }
    }

    private func refresh() async {
        guard appState.gateway.isSignedIn else { return }
        await appState.refreshGateway()
        await appState.refreshGatewayBilling()
    }

    // MARK: Balance

    private var balanceGroup: some View {
        let gateway = appState.gateway
        return SettingsGroup("ModelGateway") {
            if gateway.account == nil, gateway.accountError == nil {
                SettingsRowsSkeleton(count: 2, label: "Loading your balance")
            }
            if let account = gateway.account {
                SettingsRow("Credits") {
                    HStack(spacing: 8) {
                        Text(verbatim: account.formatQuota(account.quota))
                            .font(Theme.Font.heading.monospacedDigit())
                            .foregroundStyle(account.quota > 0 ? Theme.textPrimary : Theme.warning)
                        Text("\(account.formatQuota(account.usedQuota)) used in total")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                SettingsRow("Plan", alignment: .top) {
                    if let plan = account.subscriptions?.first {
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 6) {
                                Text(plan.name)
                                    .font(Theme.Font.secondary)
                                    .foregroundStyle(Theme.textPrimary)
                                if let price = plan.priceTitle {
                                    Chip(price)
                                }
                            }
                            Text(planDetail(plan, account: account))
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                        }
                    } else {
                        Text("Pay as you go from your credits")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                    }
                }
            }
            HStack(spacing: 8) {
                if gateway.refreshing || gateway.billingLoading {
                    BrailleSpinner(size: 11)
                }
                if let error = gateway.billingError ?? gateway.accountError {
                    Text(error)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.danger)
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
                Button("Open wallet") { appState.openGatewayWallet() }
                Button("Refresh") { Task { await refresh() } }
                    .disabled(gateway.refreshing || gateway.billingLoading)
            }
            .controlSize(.small)
        }
    }

    private func planDetail(_ plan: GatewaySubscription, account: GatewayAccount) -> String {
        var parts: [String] = []
        parts.append(plan.isUnlimited ? "Unlimited usage" : "\(account.formatQuota(plan.remaining)) of \(account.formatQuota(plan.amountTotal)) left")
        if let reset = plan.nextResetDate { parts.append("resets \(reset.formatted(date: .abbreviated, time: .omitted))") }
        if let end = plan.endDate { parts.append("ends \(end.formatted(date: .abbreviated, time: .omitted))") }
        return parts.joined(separator: " · ")
    }

    // MARK: Plans

    @ViewBuilder
    private var plansGroup: some View {
        let gateway = appState.gateway
        if let billing = gateway.billing, !billing.plans.isEmpty {
            SettingsGroup("Plans", footer: billing.hasActiveSubscription ? "You already have an active plan; another can be bought once it ends." : "Card payments open ModelGateway's checkout in your browser; the plan starts when the payment completes.") {
                ForEach(billing.plans) { plan in
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 6) {
                                Text(plan.name)
                                    .font(Theme.Font.secondary.weight(.semibold))
                                    .foregroundStyle(Theme.textPrimary)
                                Chip(plan.priceTitle)
                            }
                            Text(plan.allowanceTitle(quotaPerUnit: billing.quotaPerUnit))
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textSecondary)
                            if !plan.subtitle.isEmpty {
                                Text(plan.subtitle)
                                    .font(Theme.Font.caption)
                                    .foregroundStyle(Theme.textTertiary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                        Spacer(minLength: 8)
                        VStack(alignment: .trailing, spacing: 6) {
                            if plan.stripe {
                                Button("Subscribe…") { subscribe(plan, fromBalance: false) }
                            }
                            if plan.balance {
                                Button("Pay from balance") { confirmBalancePlan = plan }
                            }
                            if !plan.stripe, !plan.balance {
                                Button("On the website…") { appState.openGatewayWallet() }
                            }
                        }
                        .controlSize(.small)
                        .disabled(billing.hasActiveSubscription || gateway.billingBusy)
                    }
                    .padding(.vertical, 2)
                }
            }
        }
    }

    private func subscribe(_ plan: GatewayPlan, fromBalance: Bool) {
        message = nil
        Task {
            do {
                let active = try await appState.subscribeToGatewayPlan(plan, fromBalance: fromBalance)
                message = active ? .success("\(plan.name) is active.") : .info("Finish paying for \(plan.name) in your browser; the plan shows here once the payment completes.")
            } catch {
                message = .error(AppState.gatewayMessage(error))
            }
        }
    }

    // MARK: Preference

    @ViewBuilder
    private var preferenceGroup: some View {
        let gateway = appState.gateway
        if let billing = gateway.billing {
            let selection = Binding<String>(
                get: { billing.billingPreference },
                set: { value in
                    guard let preference = GatewayBillingPreference(rawValue: value), value != billing.billingPreference else { return }
                    message = nil
                    Task {
                        do {
                            try await appState.setGatewayBillingPreference(preference)
                            message = .success("Requests now use: \(preference.title.lowercased()).")
                        } catch {
                            message = .error(AppState.gatewayMessage(error))
                        }
                    }
                }
            )
            SettingsGroup("Use credits or subscription first", footer: "Saved to your ModelGateway account, so it applies to every device and API key.") {
                Picker("Use credits or subscription first", selection: selection) {
                    ForEach(GatewayBillingPreference.allCases) { preference in
                        Text(preference.title).tag(preference.rawValue)
                    }
                }
                .pickerStyle(.radioGroup)
                .labelsHidden()
                .disabled(gateway.billingBusy)
                if let preference = billing.preference {
                    Text(preference.detail)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}

// MARK: - Add funds

private struct AddFundsGroup: View {
    @Environment(AppState.self) private var appState
    @Binding var message: SettingsMessage?

    @State private var amount: Int?
    @State private var custom = ""
    @State private var quote: GatewayTopUpQuote?

    var body: some View {
        let gateway = appState.gateway
        if let topUp = gateway.billing?.topUp {
            SettingsGroup("Add funds", footer: topUp.inApp ? "Opens ModelGateway's checkout in your browser; the credits arrive when the payment completes." : "Adding funds happens on ModelGateway's wallet page.") {
                if !topUp.enabled {
                    InlineMessage(.info("ModelGateway does not take payments right now. Redemption codes can still be used on the wallet page."))
                } else if topUp.inApp {
                    HStack(spacing: 6) {
                        ForEach(options(topUp), id: \.self) { option in
                            Button {
                                amount = option
                                custom = ""
                            } label: {
                                VStack(spacing: 1) {
                                    Text(topUp.label(option))
                                    if let discount = topUp.discountLabel(option) {
                                        Text(discount)
                                            .font(Theme.Font.caption)
                                            .foregroundStyle(Theme.success)
                                    }
                                }
                                .frame(minWidth: 52)
                            }
                            .buttonStyle(.bordered)
                            .tint(selected == option ? Theme.accent : nil)
                        }
                    }
                    SettingsRow("Other amount") {
                        HStack(spacing: 6) {
                            if topUp.isUSD {
                                Text("$")
                                    .font(Theme.Font.secondary)
                                    .foregroundStyle(Theme.textTertiary)
                            }
                            TextField("\(topUp.minAmount)", text: $custom)
                                .settingsField()
                                .frame(width: 100)
                                .onChange(of: custom) { _, _ in amount = nil }
                            Text("\(topUp.label(topUp.minAmount)) to \(topUp.label(topUp.maxAmount))")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                        }
                    }
                    HStack(spacing: 8) {
                        Button("Add funds…") { addFunds(topUp) }
                            .keyboardShortcut(.defaultAction)
                            .disabled(validAmount(topUp) == nil || gateway.billingBusy)
                        if gateway.billingBusy {
                            BrailleSpinner()
                        }
                        if let quote, let chosen = validAmount(topUp), Int(quote.amount) == chosen {
                            Text("You pay \(quote.payMoney.formatted(.currency(code: quote.currency ?? "USD")))")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textSecondary)
                        }
                    }
                    .controlSize(.small)
                    .task(id: validAmount(topUp)) {
                        guard let chosen = validAmount(topUp) else { quote = nil; return }
                        try? await Task.sleep(for: .milliseconds(300))
                        guard !Task.isCancelled else { return }
                        quote = await appState.quoteGatewayTopUp(chosen)
                    }
                } else {
                    Button("Add funds on the website…") { appState.openGatewayWallet() }
                        .controlSize(.small)
                }
            }
        }
    }

    private func options(_ topUp: GatewayTopUpOptions) -> [Int] {
        let listed = topUp.amountOptions.filter { $0 >= topUp.minAmount && $0 <= topUp.maxAmount }
        if !listed.isEmpty { return Array(listed.prefix(6)) }
        return [10, 25, 50, 100].map { max($0, topUp.minAmount) }.reduce(into: []) { if !$0.contains($1) { $0.append($1) } }
    }

    private var selected: Int? { custom.isEmpty ? amount : nil }

    private func validAmount(_ topUp: GatewayTopUpOptions) -> Int? {
        let value = custom.isEmpty ? amount : Int(custom.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "$", with: ""))
        guard let value, value >= topUp.minAmount, value <= topUp.maxAmount else { return nil }
        return value
    }

    private func addFunds(_ topUp: GatewayTopUpOptions) {
        guard let chosen = validAmount(topUp) else { return }
        message = nil
        Task {
            do {
                try await appState.addGatewayFunds(chosen)
                message = .info("Finish paying in your browser; your credits update here once the payment completes.")
            } catch {
                message = .error("ModelGateway could not start the checkout: \(AppState.gatewayMessage(error)) You can still add funds with Open wallet.")
            }
        }
    }
}
