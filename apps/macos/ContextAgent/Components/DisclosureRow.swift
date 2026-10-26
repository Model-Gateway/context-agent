import SwiftUI

/// A clickable row with a chevron that rotates when expanded, revealing
/// `content` underneath. Used by the tool timeline header, Thinking rows
/// and noisy notices. The chevron sits right after the label; put a
/// `Spacer()` inside `label` to push trailing elements (elapsed time) right.
struct DisclosureRow<Label: View, Content: View>: View {
    @Binding var isExpanded: Bool
    private let label: Label
    private let content: Content

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(isExpanded: Binding<Bool>, @ViewBuilder label: () -> Label, @ViewBuilder content: () -> Content) {
        self._isExpanded = isExpanded
        self.label = label()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button {
                withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { isExpanded.toggle() }
            } label: {
                HStack(spacing: 6) {
                    label
                    Image(systemName: Icons.chevronRight)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Theme.textTertiary)
                        .rotationEffect(.degrees(isExpanded ? 90 : 0))
                        .accessibilityHidden(true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 6)
                .padding(.vertical, 4)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .hoverHighlight()
            .accessibilityAddTraits(.isButton)
            .accessibilityValue(isExpanded ? "Expanded" : "Collapsed")

            if isExpanded {
                content
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .clipped()
    }
}

#Preview("Disclosure rows") {
    struct Host: View {
        @State private var open = true
        @State private var closed = false
        var body: some View {
            VStack(alignment: .leading, spacing: 12) {
                DisclosureRow(isExpanded: $open) {
                    Icons.LogoView(size: 16)
                    Text("18 tools, edited 5 files, ran 2 commands")
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.textPrimary)
                } content: {
                    Text("Rows go here")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .padding(.leading, 30)
                }
                DisclosureRow(isExpanded: $closed) {
                    Text("Thinking")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                    BrailleSpinner()
                    Text("1.2s")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                } content: {
                    Text("Reasoning text…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
            }
            .frame(width: 480)
            .padding(24)
            .background(Theme.canvas)
        }
    }
    return Host()
}
