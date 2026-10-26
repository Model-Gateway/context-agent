import SwiftUI
import AppKit

/// The permissions popover, opened from the composer's mode chip and the
/// top bar's approval chip: one row per profile (Ask · Auto · Bypass, as in
/// the TUI's `/approval`) with its icon and
/// description, the current one checked. A click or Return switches the
/// selected project's mode for its next runs (`permissions/set` through
/// `appState.setApprovalMode`) and closes the popover; ↑↓ move, Esc closes.
struct PermissionsPopover: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    /// The profile a `permissions/set` is in flight for.
    @State private var pendingProfile: String?
    /// Keyboard / hover highlight, an index into the options.
    @State private var highlighted: Int?
    @FocusState private var focused: Bool

    var body: some View {
        let project = appState.selectedProject
        let current = project?.info.permissionProfile ?? appState.config?.permissions.profile ?? "approval_required"
        let options = PermissionProfileOption.options(current: current)
        let enabled = project != nil && pendingProfile == nil
        VStack(alignment: .leading, spacing: 0) {
            VStack(spacing: 2) {
                ForEach(Array(options.enumerated()), id: \.element.id) { index, option in
                    PermissionRow(
                        option: option,
                        isCurrent: option.id == current,
                        isPending: option.id == pendingProfile,
                        isHighlighted: highlighted == index,
                        enabled: enabled
                    ) {
                        select(option.id)
                    } onHover: {
                        highlighted = index
                    }
                }
            }

        }
        .padding(6)
        .frame(width: 260)
        .background(Theme.surface)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onKeyPress(.upArrow) {
            move(-1, count: options.count)
            return .handled
        }
        .onKeyPress(.downArrow) {
            move(1, count: options.count)
            return .handled
        }
        .onKeyPress(.return) {
            if let highlighted, options.indices.contains(highlighted) { select(options[highlighted].id) }
            return .handled
        }
        .onKeyPress(.escape) {
            dismiss()
            return .handled
        }
        .onExitCommand { dismiss() }
        .task {
            highlighted = options.firstIndex { $0.id == current }
            // Popover content is attached a beat after creation; focusing then sticks.
            try? await Task.sleep(for: .milliseconds(60))
            focused = true
        }
    }

    private func move(_ delta: Int, count: Int) {
        guard count > 0 else { return }
        highlighted = min(max((highlighted ?? -1) + delta, 0), count - 1)
    }

    private func select(_ profile: String) {
        guard pendingProfile == nil, let project = appState.selectedProject else { return }
        guard project.info.permissionProfile != profile else {
            dismiss()
            return
        }
        pendingProfile = profile
        Task {
            await appState.setApprovalMode(profile)
            pendingProfile = nil
            dismiss()
        }
    }
}

// MARK: - Row

private struct PermissionRow: View {
    let option: PermissionProfileOption
    let isCurrent: Bool
    let isPending: Bool
    let isHighlighted: Bool
    let enabled: Bool
    let action: () -> Void
    let onHover: () -> Void

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        // Bypass never asks, so its icon stays in the warning tint.
        let risky = option.id == PermissionProfileOption.bypass.id
        let iconTint = risky ? Theme.warning : (isCurrent ? Theme.accent : Theme.textSecondary)
        let iconBackground = risky ? Theme.warning.opacity(0.12) : (isCurrent ? Theme.accentSoft : Theme.surfaceRaised)
        Button(action: action) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: option.systemImage)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(iconTint)
                    .frame(width: 26, height: 26)
                    .background(iconBackground, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(option.title)
                        .font(.system(size: 13, weight: isCurrent ? .semibold : .medium))
                        .foregroundStyle(Theme.textPrimary)
                    Text(option.description)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                Group {
                    if isPending {
                        BrailleSpinner(size: 11, tint: Theme.textSecondary)
                    } else if isCurrent {
                        Image(systemName: "checkmark")
                            .font(.system(size: 11, weight: .bold))
                            .foregroundStyle(Theme.accent)
                            .accessibilityHidden(true)
                    }
                }
                .frame(width: 14)
                .padding(.top, 6)
            }
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(isHighlighted ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .onHover { inside in
            if inside { onHover() }
        }
        .accessibilityLabel("\(option.title): \(option.description)")
        .accessibilityAddTraits(isCurrent ? .isSelected : [])
    }
}

// MARK: - Previews

@MainActor
private enum PermissionsPopoverPreview {
    static func make(profile: String) -> AppState {
        let defaults = UserDefaults(suiteName: "dev.contextagent.desktop.preview") ?? .standard
        let preferences = Preferences(defaults: defaults)
        let app = AppState(preferences: preferences, backend: BackendClient(), recents: RecentProjectStore(), toasts: ToastCenter(), notifier: TaskNotifier())
        let info = ProjectInfo(
            projectId: "a1b2c3d4e5f6a1b2c3d4e5f6", root: "/Users/me/Projects/context-agent", name: "context-agent",
            git: nil,
            index: IndexStats(generation: 12, files: 310, symbols: 0, chunks: 0, relationships: 0, embeddings: 0, lastFullIndexAt: nil, languages: []),
            indexing: false, model: "gpt-4.1", permissionProfile: profile, sandbox: "none",
            validationCommands: [], languages: [], buildFiles: [], fileCount: 310, runningTask: nil
        )
        let project = ProjectState(info: info, models: ModelCatalog(preferences: preferences, baseURL: ""))
        app.projects = [project]
        app.selectedProjectId = project.id
        return app
    }
}

#Preview("Ask") {
    PermissionsPopover()
        .environment(PermissionsPopoverPreview.make(profile: "approval_required"))
}

#Preview("Bypass active") {
    PermissionsPopover()
        .environment(PermissionsPopoverPreview.make(profile: "bypass_permissions"))
}
