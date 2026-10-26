import SwiftUI
import AppKit

/// The profile page's Account tab (ARCHITECTURE.md "Account"; `ProfileView`
/// holds it next to the settings tabs), opened from the sidebar's
/// bottom-left button. Signed out: the "Sign in with ModelGateway" card
/// (and the device code while a sign-in runs). Signed in, top to bottom:
/// the account header, the Context credit (the welcome offer, then what is
/// left of it), Plan (active subscriptions, or pay as you go),
/// Usage (24h / 7d / 30d bar charts or an all-time GitHub-style heatmap:
/// total, by model, by project; cost / tokens / requests), Settings and
/// Sign out.
struct AccountView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let gateway = appState.gateway
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if gateway.isSignedIn {
                    AccountHeader()
                    ContextCreditCard()
                    if gateway.account?.subscriptions != nil {
                        PlanCard()
                    }
                    UsageCard()
                    GatewaySettingsCard()
                    SignOutCard()
                } else {
                    SignInCard()
                }
            }
            .frame(maxWidth: Theme.Metrics.contentMaxWidth)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .background(Theme.canvas)
        .task(id: gateway.isSignedIn) {
            // Opening the page refreshes a stale account (sign-in refreshes itself).
            if gateway.isSignedIn, gateway.lastRefreshed.map({ Date().timeIntervalSince($0) > 60 }) ?? true {
                await appState.refreshGateway()
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Account")
    }
}

// MARK: - Signed out

private struct SignInCard: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let gateway = appState.gateway
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: Icons.gateway)
                    .font(.system(size: 22, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: 44, height: 44)
                    .background(Theme.accentSoft, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Sign in with ModelGateway")
                        .font(Theme.Font.title)
                        .foregroundStyle(Theme.textPrimary)
                    Text("Optional. Signing in routes model requests through your ModelGateway account and shows your usage here, by model and by project. Context works without it, with any OpenAI-compatible endpoint from the Model tab.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            switch gateway.flow {
            case .starting?:
                HStack(spacing: 8) {
                    BrailleSpinner(size: 12)
                    Text("Contacting ModelGateway…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
            case .waiting(let userCode, let url, let expiresAt)?:
                waiting(userCode: userCode, url: url, expiresAt: expiresAt)
            case .completing?:
                HStack(spacing: 8) {
                    BrailleSpinner(size: 12)
                    Text("Approved. Finishing sign-in…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
            case .failed(let message)?:
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 6) {
                        Image(systemName: Icons.warning)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(Theme.danger)
                            .padding(.top, 1)
                        Text(verbatim: message)
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.danger)
                            .fixedSize(horizontal: false, vertical: true)
                            .selectableText()
                    }
                    signInButtons
                }
            case nil:
                signInButtons
            }
        }
        .card(padding: 20)
    }

    private var signInButtons: some View {
        HStack(spacing: 8) {
            Button {
                appState.dismissGatewayError()
                appState.beginGatewaySignIn()
            } label: {
                Label("Sign in with ModelGateway", systemImage: Icons.signIn)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.regular)
            .disabled(!appState.connection.isReady)
            Button("Create an account…") {
                AppState.openInBrowser(appState.gatewayOrigin + "/register")
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            if !appState.connection.isReady {
                Text("Waiting for the backend…")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
        }
    }

    private func waiting(userCode: String, url: String, expiresAt: Date) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Your browser opened ModelGateway. Confirm that it shows this code, then approve:")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            HStack(spacing: 12) {
                Text(verbatim: userCode)
                    .font(.system(size: 30, weight: .semibold, design: .monospaced))
                    .kerning(3)
                    .foregroundStyle(Theme.textPrimary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
                    .selectableText()
                    .accessibilityLabel("Code \(userCode.map(String.init).joined(separator: " "))")
                CopyButton(text: userCode, label: "Copy code")
            }
            HStack(spacing: 8) {
                BrailleSpinner(size: 11, tint: Theme.textSecondary)
                Text("Waiting for approval · expires \(expiresAt.formatted(date: .omitted, time: .shortened))")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            HStack(spacing: 8) {
                Button {
                    AppState.openInBrowser(url)
                } label: {
                    Label("Open ModelGateway again", systemImage: Icons.externalLink)
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                Button("Cancel") {
                    appState.cancelGatewaySignIn()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
    }
}

// MARK: - Account header

private struct AccountHeader: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let gateway = appState.gateway
        VStack(alignment: .leading, spacing: 14) {
            if let account = gateway.account {
                HStack(alignment: .center, spacing: 14) {
                    Text(verbatim: account.initial)
                        .font(.system(size: 20, weight: .semibold))
                        .foregroundStyle(Theme.accent)
                        .frame(width: 48, height: 48)
                        .background(Theme.accentSoft, in: Circle())
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 8) {
                            Text(verbatim: account.name)
                                .font(Theme.Font.title)
                                .foregroundStyle(Theme.textPrimary)
                                .lineLimit(1)
                            if !account.group.isEmpty {
                                Chip(account.group)
                                    .help("Group")
                            }
                        }
                        Text(verbatim: [account.username.isEmpty ? nil : "@" + account.username, account.email.isEmpty ? nil : account.email].compactMap { $0 }.joined(separator: " · "))
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .selectableText()
                    }
                    Spacer(minLength: 8)
                    HStack(spacing: 4) {
                        if gateway.refreshing {
                            BrailleSpinner(size: 11, tint: Theme.textSecondary)
                                .frame(width: 28, height: 28)
                        } else {
                            IconButton(Icons.refresh, label: "Refresh account and usage") {
                                Task { await appState.refreshGateway() }
                            }
                        }
                        IconButton(Icons.externalLink, label: "Open the ModelGateway dashboard") {
                            appState.openGatewayDashboard()
                        }
                    }
                }
                HStack(spacing: 0) {
                    stat(title: "Balance", value: account.displayInCurrency ? UsageFormat.usd(account.balanceUSD) : UsageFormat.compact(account.quota), detail: "remaining")
                    divider
                    stat(title: "Spent", value: account.displayInCurrency ? UsageFormat.usd(account.usedUSD) : UsageFormat.compact(account.usedQuota), detail: "all time")
                    divider
                    stat(title: "Requests", value: account.requestCount.formatted(), detail: "all time")
                    if let token = account.token {
                        divider
                        stat(title: "This device", value: account.displayInCurrency ? UsageFormat.usd(Double(token.usedQuota) / max(account.quotaPerUnit, 1)) : UsageFormat.compact(token.usedQuota), detail: token.name)
                    }
                }
                .padding(.top, 2)
            }
            if let error = gateway.accountError {
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: Icons.warning)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(Theme.warning)
                        .padding(.top, 1)
                    Text(verbatim: error)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
            }
        }
        .card(padding: 20)
    }

    private func stat(title: String, value: String, detail: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
            Text(verbatim: value)
                .font(.system(size: 18, weight: .semibold, design: .rounded))
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
            Text(verbatim: detail)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private var divider: some View {
        Rectangle()
            .fill(Theme.border)
            .frame(width: 1, height: 40)
            .padding(.horizontal, 12)
    }
}

// MARK: - Context credit

/// The welcome Context credit: an offer the user claims once a card or
/// crypto top-up has gone through, then what is left of it. It pays for
/// requests made with this app's device key only, before any plan or the
/// balance. Hidden once spent, and for gateways that predate it.
private struct ContextCreditCard: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let account = appState.gateway.account, let credit = account.contextCredit,
           credit.claimed ? credit.balance > 0 : credit.offerQuota > 0 {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: Icons.contextCredit)
                    .font(.system(size: 18, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: 40, height: 40)
                    .background(Theme.accentSoft, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
                    .accessibilityHidden(true)
                if credit.claimed {
                    remaining(credit, account: account)
                } else {
                    offer(credit, account: account)
                }
            }
            .card(padding: 20)
        }
    }

    private func offer(_ credit: GatewayContextCredit, account: GatewayAccount) -> some View {
        let claiming = appState.gateway.claimingContextCredit
        return VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: "\(account.formatQuota(credit.offerQuota)) of free Context credit")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Text(credit.eligible
                    ? "Thanks for trying Context! Claim 10$ for use within Context"
                    : "Add funds by card or crypto once to unlock it. Context's requests are then paid from this credit before your balance. It only works in Context.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 8) {
                Button {
                    Task { await appState.claimContextCredit() }
                } label: {
                    if claiming {
                        HStack(spacing: 6) {
                            BrailleSpinner(size: 11, tint: .white)
                            Text("Claiming…")
                        }
                    } else {
                        Text("Claim \(account.formatQuota(credit.offerQuota))")
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.regular)
                .disabled(!credit.eligible || claiming || !appState.connection.isReady)
                .help(credit.eligible ? "Add the credit to this account" : "Available after a card or crypto top-up")
                if !credit.eligible {
                    Button("Add funds…") {
                        appState.openSettings(.billing)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.regular)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func remaining(_ credit: GatewayContextCredit, account: GatewayAccount) -> some View {
        let fraction = credit.granted > 0 ? min(1, Double(credit.balance) / Double(credit.granted)) : 0
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("Context credit")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 8)
                Text(verbatim: "\(account.formatQuota(credit.balance)) of \(account.formatQuota(credit.granted)) left")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
            }
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(Theme.surfaceRaised)
                    Capsule().fill(Theme.accent).frame(width: max(4, geometry.size.width * fraction))
                }
            }
            .frame(height: 6)
            Text("Pays for Context's requests before your balance. It cannot be used with other API keys.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Plan

private struct PlanCard: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let account = appState.gateway.account, let subscriptions = account.subscriptions {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    Image(systemName: Icons.plan)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(Theme.textSecondary)
                        .accessibilityHidden(true)
                    Text(subscriptions.count > 1 ? "Plans" : "Plan")
                        .font(Theme.Font.heading)
                        .foregroundStyle(Theme.textPrimary)
                    Spacer(minLength: 8)
                    Button("Billing settings…") {
                        appState.openSettings(.billing)
                    }
                    .controlSize(.small)
                    .help("Add funds, buy a plan and choose whether credits or the plan are used first")
                }
                if subscriptions.isEmpty {
                    payAsYouGo(account)
                } else {
                    ForEach(subscriptions) { subscription in
                        row(subscription, account: account)
                        if subscription.id != subscriptions.last?.id {
                            Divider()
                        }
                    }
                    if let title = account.billingPreferenceTitle {
                        Text(verbatim: "Billing: \(title) · balance \(account.formatQuota(account.quota))")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .card(padding: 20)
        }
    }

    private func payAsYouGo(_ account: GatewayAccount) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Pay as you go")
                .font(.system(size: 13.5, weight: .semibold))
                .foregroundStyle(Theme.textPrimary)
            Text(verbatim: "No active plan. Requests are billed to your balance (\(account.displayInCurrency ? UsageFormat.usd(account.balanceUSD) : UsageFormat.compact(account.quota)) left).")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func row(_ subscription: GatewaySubscription, account: GatewayAccount) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(verbatim: subscription.name)
                    .font(.system(size: 13.5, weight: .semibold))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                Chip("Active", style: .tinted(Theme.success))
                if let price = subscription.priceTitle {
                    Text(verbatim: price)
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 8)
                if let end = subscription.endDate, let days = subscription.daysLeft() {
                    Text(verbatim: "\(days == 1 ? "1 day" : "\(days) days") left")
                        .font(Theme.Font.caption)
                        .foregroundStyle(days <= 3 ? Theme.warning : Theme.textSecondary)
                        .help("Until \(end.formatted(date: .abbreviated, time: .shortened))")
                }
            }
            if !subscription.subtitle.isEmpty {
                Text(verbatim: subscription.subtitle)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(2)
            }
            if subscription.isUnlimited {
                Text(verbatim: "Unlimited quota · \(account.formatQuota(subscription.amountUsed)) used")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textSecondary)
            } else {
                quotaBar(subscription)
                HStack(spacing: 8) {
                    Text(verbatim: "\(account.formatQuota(subscription.amountUsed)) of \(account.formatQuota(subscription.amountTotal)) used · \(account.formatQuota(subscription.remaining)) left")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .help(subscription.amountReserved > 0 ? "\(account.formatQuota(subscription.amountReserved)) held for requests in flight" : "")
                    Spacer(minLength: 8)
                    if let reset = subscription.nextResetDate {
                        Text(verbatim: "Resets \(reset.formatted(date: .abbreviated, time: .shortened))")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .help(subscription.resetTitle ?? "")
                    }
                }
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func quotaBar(_ subscription: GatewaySubscription) -> some View {
        let fraction = subscription.usedFraction
        let tint: Color = fraction > 0.9 ? Theme.danger : (fraction > 0.7 ? Theme.warning : Theme.accent)
        return GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.surfaceRaised)
                Capsule().fill(tint).frame(width: max(4, geometry.size.width * fraction))
            }
        }
        .frame(height: 6)
        .accessibilityLabel("Plan quota used \(Int(fraction * 100)) percent")
    }
}

// MARK: - Usage

private struct UsageCard: View {
    @Environment(AppState.self) private var appState
    @State private var range: UsageRange = .all
    @State private var scope: UsageScope = .total
    @State private var metric: UsageMetric = .cost
    @State private var showAll = false

    /// Series shown before "Show all".
    private static let collapsedCount = 6

    var body: some View {
        let gateway = appState.gateway
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Image(systemName: Icons.usage)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .accessibilityHidden(true)
                Text("Usage")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                if gateway.usageLoading {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                }
                Spacer(minLength: 8)
                Picker("Period", selection: $range) {
                    ForEach(UsageRange.allCases) { range in
                        Text(range.title).tag(range)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                .help("Period to chart")
            }
            // Side by side, or stacked when the page is narrow (a wide inspector).
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 10) {
                    scopePicker
                    Spacer(minLength: 8)
                    metricPicker
                }
                VStack(alignment: .leading, spacing: 8) {
                    scopePicker
                    metricPicker
                }
            }
            .onChange(of: scope) { _, _ in showAll = false }
            .onChange(of: range) { _, _ in showAll = false }

            if let usage = gateway.usage?[range] {
                content(usage)
            } else if gateway.usageLoading {
                HStack(spacing: 8) {
                    BrailleSpinner(size: 12)
                    Text("Loading usage…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
                .frame(height: 120)
                .frame(maxWidth: .infinity)
            } else if let error = gateway.usageError {
                usageError(error)
            } else {
                Text("Usage appears here once ModelGateway has recorded requests from this account.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
            if let error = gateway.usageError, gateway.usage != nil {
                usageError(error)
            }
        }
        .card(padding: 20)
    }

    private var scopePicker: some View {
        Picker("Scope", selection: $scope) {
            ForEach(UsageScope.allCases) { scope in
                Text(scope.title).tag(scope)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }

    private var metricPicker: some View {
        Picker("Metric", selection: $metric) {
            ForEach(UsageMetric.allCases) { metric in
                Text(metric.title).tag(metric)
            }
        }
        .pickerStyle(.segmented)
        .labelsHidden()
        .fixedSize()
    }

    @ViewBuilder
    private func content(_ usage: UsageBreakdown) -> some View {
        let series = usage.series(for: scope)
        if series.isEmpty || (scope == .total && usage.total.total.isEmpty) {
            VStack(alignment: .leading, spacing: 12) {
                chart(usage.total, of: usage)
                Text(emptyMessage(usage))
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
        } else if scope == .total {
            chart(usage.total, of: usage)
        } else {
            let visible = showAll ? series : Array(series.prefix(Self.collapsedCount))
            VStack(alignment: .leading, spacing: 18) {
                ForEach(visible) { item in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 8) {
                            Text(verbatim: item.title)
                                .font(.system(size: 13.5, weight: .semibold))
                                .foregroundStyle(item.title == UsageBreakdown.unattributed ? Theme.textSecondary : Theme.textPrimary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer(minLength: 8)
                            Text(verbatim: metric.format(item.total, quotaPerUnit: usage.quotaPerUnit))
                                .font(.system(size: 13, weight: .medium, design: .rounded))
                                .foregroundStyle(Theme.textSecondary)
                            share(item, of: usage)
                        }
                        chart(item, of: usage, showsTotal: false)
                    }
                }
                if series.count > Self.collapsedCount {
                    Button(showAll ? "Show fewer" : "Show all \(series.count)") {
                        withAnimation(.easeOut(duration: 0.15)) { showAll.toggle() }
                    }
                    .buttonStyle(.plain)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.accent)
                }
                if scope == .project, series.contains(where: { $0.title == UsageBreakdown.unattributed }) {
                    Text("“Unattributed” is usage without a project name: requests from other clients, or made while “Attribute usage to projects” was off.")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    /// The range's bar chart, or the all-time heatmap.
    @ViewBuilder
    private func chart(_ series: UsageSeries, of usage: UsageBreakdown, showsTotal: Bool = true) -> some View {
        if usage.range == .all {
            UsageHeatmap(series: series, metric: metric, quotaPerUnit: usage.quotaPerUnit, showsTotal: showsTotal)
        } else {
            UsageBarChart(series: series, range: usage.range, buckets: usage.bucketStarts, metric: metric, quotaPerUnit: usage.quotaPerUnit, showsTotal: showsTotal)
        }
    }

    /// "42%" of the scope's total, by the chosen metric.
    private func share(_ item: UsageSeries, of usage: UsageBreakdown) -> some View {
        let total = metric.value(usage.total.total, quotaPerUnit: usage.quotaPerUnit)
        let value = metric.value(item.total, quotaPerUnit: usage.quotaPerUnit)
        let percent = total > 0 ? Int((value / total * 100).rounded()) : 0
        return Chip("\(percent)%")
            .help("Share of all usage in this period, by \(metric.title.lowercased())")
    }

    private func emptyMessage(_ usage: UsageBreakdown) -> String {
        if usage.rowCount == 0 { return "No usage recorded in the last year. Requests routed through ModelGateway show up here within the hour." }
        if usage.total.total.isEmpty { return "No usage in the \(usage.range.caption.lowercased()). Pick a longer period to see earlier requests." }
        return scope == .project ? "No project breakdown yet." : "No model breakdown yet."
    }

    private func usageError(_ error: String) -> some View {
        HStack(alignment: .top, spacing: 6) {
            Image(systemName: Icons.warning)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.warning)
                .padding(.top, 1)
            Text(verbatim: "Usage could not be loaded: \(error)")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .selectableText()
            Button("Retry") {
                Task { await appState.refreshGatewayUsage() }
            }
            .controlSize(.small)
        }
    }
}

// MARK: - Settings

private struct GatewaySettingsCard: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let routing = Binding(
            get: { appState.preferences.gatewayRouteRequests },
            set: { on in Task { await appState.setGatewayRouting(on) } }
        )
        let attribution = Binding(
            get: { appState.preferences.gatewayAttributeProjects },
            set: { on in Task { await appState.setGatewayProjectAttribution(on) } }
        )
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Image(systemName: Icons.settings)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .accessibilityHidden(true)
                Text("Settings")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
            }
            settingRow(
                title: "Route model requests through ModelGateway",
                detail: routingDetail,
                control: Toggle("Route model requests through ModelGateway", isOn: routing).labelsHidden().toggleStyle(.switch).controlSize(.small)
            )
            settingRow(
                title: "Attribute usage to projects",
                detail: "Sends each request's project name as the “X-ModelGateway-Project” header, so Usage can show a per-project breakdown. Nothing else about the project is sent.",
                control: Toggle("Attribute usage to projects", isOn: attribution).labelsHidden().toggleStyle(.switch).controlSize(.small).disabled(!appState.preferences.gatewayRouteRequests)
            )
        }
        .card(padding: 20)
    }

    private var routingDetail: String {
        guard let model = appState.config?.model else { return "The backend is not running." }
        if appState.gatewayRoutingActive {
            return "Model requests go to \(model.baseUrl) with your ModelGateway key. Turning this off restores the previous endpoint from Settings → Model."
        }
        if appState.preferences.gatewayRouteRequests {
            return "Switching the endpoint to \(appState.gatewayModelBaseURL)…"
        }
        return "Off: requests keep using \(model.baseUrl) and your own key. Usage shown here only covers requests that went through ModelGateway."
    }

    private func settingRow<Control: View>(title: String, detail: String, control: Control) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                Text(verbatim: detail)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            control
                .padding(.top, 2)
        }
    }
}

// MARK: - Sign out

private struct SignOutCard: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let gateway = appState.gateway
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Sign out")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                Text("Sign out of Context.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            Button {
                Task { await appState.signOutOfGateway() }
            } label: {
                HStack(spacing: 6) {
                    if gateway.signingOut {
                        BrailleSpinner(size: 10, tint: Theme.textSecondary)
                    } else {
                        Image(systemName: Icons.signOut)
                    }
                    Text("Sign out")
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .tint(Theme.danger)
            .disabled(gateway.signingOut)
        }
        .card(padding: 20)
    }
}

// MARK: - Previews

#Preview("Account · signed out") {
    let state = MainWindowPreview.appState()
    state.sidebarItem = .account
    return AccountView()
        .frame(width: 900, height: 700)
        .environment(state)
}

#Preview("Account · signed in") {
    let state = MainWindowPreview.appState()
    state.sidebarItem = .account
    state.gateway.account = GatewayAccount(id: 7, username: "alex", displayName: "Alex", email: "alex@example.com", group: "default", quota: 12_345_000, usedQuota: 4_200_000, requestCount: 1_284, token: GatewayToken(id: 3, name: "Context · Alex's MacBook", usedQuota: 900_000, createdTime: 1_760_000_000), billingPreference: "subscription_first", subscriptions: [GatewaySubscription(id: 12, planId: 2, title: "Pro", subtitle: "For daily coding", priceAmount: 20, quotaResetPeriod: "monthly", amountTotal: 10_000_000, amountUsed: 6_400_000, endTime: Int64(Date().timeIntervalSince1970) + 19 * 86_400, nextResetTime: Int64(Date().timeIntervalSince1970) + 5 * 86_400)])
    let calendar = Calendar.current
    let today = calendar.startOfDay(for: Date())
    var rows: [GatewayUsageRow] = []
    for back in 0..<300 where back % 4 != 2 {
        if let day = calendar.date(byAdding: .day, value: -back, to: today) {
            let stamp = Int64(day.timeIntervalSince1970) + 3600 * 10
            rows.append(GatewayUsageRow(createdAt: stamp, modelName: back % 3 == 0 ? "claude-sonnet-4.5" : "gpt-4.1", project: back % 5 == 0 ? "" : (back % 2 == 0 ? "context-agent" : "growth-campaign"), count: back % 6 + 1, quota: ((back * 53) % 700 + 40) * 500, tokenUsed: (back * 977) % 30_000 + 400))
        }
    }
    state.gateway.usage = UsageBreakdown.byRange(report: GatewayUsageReport(startTimestamp: 0, endTimestamp: Int64(Date().timeIntervalSince1970), rows: rows))
    state.gateway.lastRefreshed = Date()
    return AccountView()
        .frame(width: 900, height: 900)
        .environment(state)
}
