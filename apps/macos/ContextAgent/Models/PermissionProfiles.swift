import Foundation

/// One permission profile with its icon and the description shown by the
/// composer's permissions popover and Settings → Permissions.
struct PermissionProfileOption: Identifiable {
    let id: String
    let title: String
    let systemImage: String
    let description: String

    /// Titles come from `AppState.approvalModeLabel` (main actor); these are only read from view bodies.
    @MainActor static let standard: [PermissionProfileOption] = [
        PermissionProfileOption(
            id: "approval_required",
            title: AppState.approvalModeLabel("approval_required"),
            systemImage: Icons.approval,
            description: "Asks for edits and commands."
        ),
        PermissionProfileOption(
            id: "workspace_autonomous",
            title: AppState.approvalModeLabel("workspace_autonomous"),
            systemImage: Icons.permissionAuto,
            description: "Asks for risky commands."
        ),
    ]

    @MainActor static let bypass = PermissionProfileOption(
        id: "bypass_permissions",
        title: AppState.approvalModeLabel("bypass_permissions"),
        systemImage: Icons.permissionBypass,
        description: "Never asks."
    )

    /// Every profile the app offers, as in the TUI's `/approval` list: Ask
    /// and Auto, then Bypass.
    @MainActor static var all: [PermissionProfileOption] { standard + [bypass] }

    /// `all`, plus an unknown configured profile name when it is the current value.
    @MainActor static func options(current: String) -> [PermissionProfileOption] {
        if all.contains(where: { $0.id == current }) { return all }
        return all + [PermissionProfileOption(id: current, title: AppState.approvalModeLabel(current), systemImage: Icons.approval, description: "Set in config.toml.")]
    }

    /// The icon for a profile name (the composer and top-bar chips).
    @MainActor static func systemImage(for profile: String) -> String {
        all.first { $0.id == profile }?.systemImage ?? Icons.approval
    }
}
