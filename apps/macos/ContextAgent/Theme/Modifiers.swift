import SwiftUI

// View modifiers shared by every screen. Styling goes through these and the
// tokens in Theme.swift so the app stays visually consistent.

extension View {
    /// A `surface` card with a hairline border and rounded corners.
    func card(radius: CGFloat = Theme.Metrics.radius, padding: CGFloat = 16) -> some View {
        modifier(CardModifier(radius: radius, padding: padding))
    }

    /// `surfaceRaised` rounded background while the pointer is over the view.
    func hoverHighlight(radius: CGFloat = 8) -> some View {
        modifier(HoverHighlightModifier(radius: radius))
    }

    /// Keyboard-shortcut hint styling ("⌘K").
    func kbd() -> some View {
        modifier(KbdModifier())
    }

    /// 1 px `border` lines on the given edges, drawn as an overlay.
    func hairline(_ edges: Edge.Set) -> some View {
        modifier(HairlineModifier(edges: edges))
    }

    /// Soft shadow for the composer and popovers.
    func floatingShadow() -> some View {
        shadow(color: Color.black.opacity(0.06), radius: 10, x: 0, y: 4)
    }

    /// Makes transcript text, code and tool output selectable.
    func selectableText() -> some View {
        textSelection(.enabled)
    }
}

// MARK: - Implementations

private struct CardModifier: ViewModifier {
    let radius: CGFloat
    let padding: CGFloat

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        content
            .padding(padding)
            .background(Theme.surface, in: shape)
            .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
    }
}

private struct HoverHighlightModifier: ViewModifier {
    let radius: CGFloat
    @State private var hovering = false

    func body(content: Content) -> some View {
        content
            .background(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(hovering ? Theme.surfaceRaised : Color.clear)
            )
            .onHover { inside in
                withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
            }
    }
}

private struct KbdModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            .font(.system(size: 11, weight: .medium, design: .rounded))
            .foregroundStyle(Theme.textTertiary)
            .padding(.horizontal, 5)
            .padding(.vertical, 1.5)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 4, style: .continuous))
            .fixedSize()
    }
}

private struct HairlineModifier: ViewModifier {
    let edges: Edge.Set

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .top) { if edges.contains(.top) { horizontalLine } }
            .overlay(alignment: .bottom) { if edges.contains(.bottom) { horizontalLine } }
            .overlay(alignment: .leading) { if edges.contains(.leading) { verticalLine } }
            .overlay(alignment: .trailing) { if edges.contains(.trailing) { verticalLine } }
    }

    private var horizontalLine: some View {
        Rectangle().fill(Theme.border).frame(height: 1).allowsHitTesting(false)
    }

    private var verticalLine: some View {
        Rectangle().fill(Theme.border).frame(width: 1).allowsHitTesting(false)
    }
}

#Preview("Modifiers") {
    VStack(alignment: .leading, spacing: 16) {
        Text("A card").card()
        Text("Hover me").padding(8).hoverHighlight()
        HStack { Text("Search"); Spacer(); Text("⌘K").kbd() }
            .padding(10)
            .frame(width: 240)
            .background(Theme.surface)
            .hairline(.all)
        Text("Floating").padding(12).background(Theme.surface, in: RoundedRectangle(cornerRadius: 12)).floatingShadow()
        Text("Selectable text").selectableText()
    }
    .padding(24)
    .background(Theme.canvas)
}
