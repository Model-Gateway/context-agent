import SwiftUI

/// Small rounded label: a neutral grey chip ("script", "main"), a tinted
/// one (status, counts) or an attention badge ("New", unseen counts).
struct Chip: View {
    enum Style {
        case neutral
        case tinted(Color)
        case badge
    }

    var text: String
    var systemImage: String?
    var style: Style

    init(_ text: String, systemImage: String? = nil, style: Style = .neutral) {
        self.text = text
        self.systemImage = systemImage
        self.style = style
    }

    var body: some View {
        HStack(spacing: 4) {
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: iconSize, weight: .medium))
            }
            Text(text)
                .lineLimit(1)
        }
        .font(font)
        .foregroundStyle(foreground)
        .padding(.horizontal, horizontalPadding)
        .padding(.vertical, verticalPadding)
        .background(background, in: shape)
        .fixedSize()
        .accessibilityElement(children: .combine)
    }

    private var isBadge: Bool {
        if case .badge = style { return true }
        return false
    }

    private var font: Font {
        isBadge ? .system(size: 11, weight: .semibold) : .system(size: 12, weight: .medium)
    }

    private var iconSize: CGFloat { isBadge ? 9 : 10 }
    private var horizontalPadding: CGFloat { isBadge ? 6 : 7 }
    private var verticalPadding: CGFloat { isBadge ? 2 : 2.5 }

    private var shape: some InsettableShape {
        RoundedRectangle(cornerRadius: isBadge ? 20 : 6, style: .continuous)
    }

    private var foreground: Color {
        switch style {
        case .neutral: return Theme.textSecondary
        case .tinted(let color): return color
        case .badge: return Theme.badgeText
        }
    }

    private var background: Color {
        switch style {
        case .neutral: return Theme.surfaceRaised
        case .tinted(let color): return color.opacity(0.14)
        case .badge: return Theme.badgeBackground
        }
    }
}

#Preview("Chips") {
    VStack(alignment: .leading, spacing: 12) {
        HStack(spacing: 8) {
            Chip("script")
            Chip("file")
            Chip("main", systemImage: Icons.branch)
            Chip("gpt-4.1", systemImage: Icons.model)
        }
        HStack(spacing: 8) {
            Chip("3", style: .tinted(Theme.accent))
            Chip("1 approval waiting", systemImage: Icons.approval, style: .tinted(Theme.warning))
            Chip("failed", style: .tinted(Theme.danger))
            Chip("PASSED", style: .tinted(Theme.success))
        }
        HStack(spacing: 8) {
            Chip("New", style: .badge)
            Chip("12", style: .badge)
        }
    }
    .padding(24)
    .background(Theme.canvas)
}
