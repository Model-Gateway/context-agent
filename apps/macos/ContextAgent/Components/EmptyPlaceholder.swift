import SwiftUI

/// Centered icon + title + subtitle with an optional action button, for
/// empty and error states ("Working tree clean", "No tasks yet").
struct EmptyPlaceholder: View {
    var systemImage: String
    var title: String
    var subtitle: String?
    var actionTitle: String?
    var action: (() -> Void)?

    init(systemImage: String, title: String, subtitle: String? = nil, actionTitle: String? = nil, action: (() -> Void)? = nil) {
        self.systemImage = systemImage
        self.title = title
        self.subtitle = subtitle
        self.actionTitle = actionTitle
        self.action = action
    }

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: systemImage)
                .font(.system(size: 28, weight: .regular))
                .foregroundStyle(Theme.textTertiary)
                .padding(.bottom, 4)
                .accessibilityHidden(true)
            Text(title)
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
                .multilineTextAlignment(.center)
            if let subtitle {
                Text(subtitle)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 360)
            }
            if let actionTitle, let action {
                Button(actionTitle, action: action)
                    .buttonStyle(.bordered)
                    .controlSize(.regular)
                    .padding(.top, 8)
            }
        }
        .padding(32)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .contain)
    }
}

#Preview("Empty placeholders") {
    HStack(spacing: 0) {
        EmptyPlaceholder(systemImage: Icons.changes, title: "Working tree clean", subtitle: "Changes made by the agent show up here.")
        EmptyPlaceholder(systemImage: Icons.warning, title: "Not a git repository", subtitle: "Open a folder with a .git directory to see changes.", actionTitle: "Retry") {}
    }
    .frame(width: 720, height: 320)
    .background(Theme.canvas)
}
