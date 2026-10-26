import SwiftUI

/// A profile page tab; the raw value is remembered (`Preferences.settingsTab`).
enum SettingsTab: String, CaseIterable, Identifiable {
    case account, general, model, byok, spending, billing, permissions, appearance

    var id: String { rawValue }

    var title: String {
        switch self {
        case .account: "Account"
        case .general: "General"
        case .model: "Model"
        case .byok: "BYOK"
        case .spending: "Spending"
        case .billing: "Billing"
        case .permissions: "Permissions"
        case .appearance: "Appearance"
        }
        }

    var subtitle: String? {
        switch self {
        case .byok: "Your own endpoint and key"
        case .billing: "ModelGateway"
        default: nil
        }
    }

    var icon: String {
        switch self {
        case .account: Icons.account
        case .general: Icons.settings
        case .model: Icons.model
        case .byok: Icons.byok
        case .spending: Icons.spending
        case .billing: Icons.billing
        case .permissions: Icons.approval
        case .appearance: Icons.appearance
        }
    }
}

/// The profile page, opened from the sidebar's account button and by ⌘,
/// (`AppState.openSettings`): a column of tabs and the selected one. Account
/// is AccountView; the rest are the settings forms, which live in
/// SettingsSections.swift and the per-tab files next to it. Billing is
/// listed only while signed in to ModelGateway. Narrower than
/// `compactWidth` (a small window, or a wide inspector) the column shows
/// icons only, with the titles as tooltips.
struct ProfileView: View {
    @Environment(AppState.self) private var appState
    @State private var compact = false

    private static let compactWidth: CGFloat = 720

    var body: some View {
        let tabs = visibleTabs
        let selected = tabs.first { $0.rawValue == appState.preferences.settingsTab } ?? .account
        HStack(spacing: 0) {
            VStack(alignment: compact ? .center : .leading, spacing: 2) {
                ForEach(tabs) { tab in
                    SettingsTabRow(tab: tab, isSelected: tab == selected, compact: compact) {
                        appState.preferences.settingsTab = tab.rawValue
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(compact ? 8 : 10)
            .frame(width: compact ? 52 : 184)
            .frame(maxHeight: .infinity)
            .background(Theme.sidebar)
            Divider()
            // Never wider than what is left: a wide form must not push the
            // tab column out of the window.
            content(selected)
                .frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
        }
        .onGeometryChange(for: Bool.self) { $0.size.width < Self.compactWidth } action: { compact = $0 }
        .background(Theme.canvas)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Profile")
    }

    private var visibleTabs: [SettingsTab] {
        SettingsTab.allCases.filter { $0 != .billing || appState.gateway.isSignedIn }
    }

    @ViewBuilder
    private func content(_ tab: SettingsTab) -> some View {
        switch tab {
        case .account: AccountView()
        case .general: GeneralSettingsSection()
        case .model: ModelSettingsSection()
        case .byok: BYOKSettingsSection()
        case .spending: SpendingSettingsSection()
        case .billing: BillingSettingsSection()
        case .permissions: PermissionsSettingsSection()
        case .appearance: AppearanceSettingsSection()
        }
    }
}

private struct SettingsTabRow: View {
    let tab: SettingsTab
    let isSelected: Bool
    /// Icon only; the title (and subtitle) becomes the tooltip.
    var compact = false
    let select: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: select) {
            HStack(spacing: 9) {
                Image(systemName: tab.icon)
                    .font(.system(size: compact ? 14 : 13, weight: .medium))
                    .frame(width: 18)
                    .foregroundStyle(isSelected ? Theme.accent : Theme.textSecondary)
                if !compact {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(tab.title)
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textPrimary)
                        if let subtitle = tab.subtitle {
                            Text(subtitle)
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                        }
                    }
                    Spacer(minLength: 0)
                }
            }
            .padding(.horizontal, compact ? 9 : 8)
            .padding(.vertical, compact ? 8 : 6)
            .contentShape(Rectangle())
            .background(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .fill(isSelected ? Theme.accent.opacity(0.12) : hovering ? Theme.textPrimary.opacity(0.05) : .clear)
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .help(compact ? [tab.title, tab.subtitle].compactMap { $0 }.joined(separator: " · ") : "")
        .accessibilityLabel(tab.title)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - Preview

#Preview("Profile · General") {
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
            apiKeySet: false,
            contextWindow: 128_000,
            maxOutputTokens: 16_000,
            reasoningEffort: nil
        ),
        permissions: PermissionSettings(
            profile: "approval_required",
            allowedCommands: ["cargo test", "cargo clippy"],
            deniedCommands: ["git push --force"],
            allowNetworkCommands: false
        ),
        pricing: PricingSettings(inputPerMillion: 2, cachedInputPerMillion: 0.5, outputPerMillion: 8),
        spend: SpendSettings(period: .month, maxUsd: 50, models: ["gpt-4.1": 20])
    )
    appState.config = config
    appState.connection = .ready(config)
    appState.preferences.settingsTab = SettingsTab.general.rawValue
    return ProfileView()
        .frame(width: 900, height: 700)
        .environment(appState)
}
