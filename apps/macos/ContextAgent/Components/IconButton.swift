import SwiftUI

/// A borderless SF Symbol button with a hover highlight, a tooltip and a
/// VoiceOver label (the same `label`). `isActive` tints it `accent` and keeps
/// the highlight on (inspector toggle, context toggle).
struct IconButton: View {
    var systemImage: String
    var label: String
    var isActive: Bool
    var size: CGFloat
    var action: () -> Void

    @State private var hovering = false

    init(_ systemImage: String, label: String, isActive: Bool = false, size: CGFloat = 16, action: @escaping () -> Void) {
        self.systemImage = systemImage
        self.label = label
        self.isActive = isActive
        self.size = size
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: size, weight: .medium))
                .foregroundStyle(foreground)
                .frame(width: size + 12, height: size + 12)
                .background(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(hovering || isActive ? Theme.surfaceRaised : Color.clear)
                )
                .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(label)
        .accessibilityLabel(label)
    }

    private var foreground: Color {
        if isActive { return Theme.accent }
        return hovering ? Theme.textPrimary : Theme.textSecondary
    }
}

#Preview("Icon buttons") {
    HStack(spacing: 8) {
        IconButton(Icons.inspector, label: "Toggle inspector") {}
        IconButton(Icons.inspector, label: "Toggle inspector", isActive: true) {}
        IconButton(Icons.settings, label: "Settings") {}
        IconButton(Icons.refresh, label: "Rebuild index", size: 14) {}
        IconButton(Icons.close, label: "Close project", size: 12) {}
    }
    .padding(24)
    .background(Theme.canvas)
}
