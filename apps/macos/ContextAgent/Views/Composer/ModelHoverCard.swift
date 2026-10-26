import SwiftUI
import AppKit

/// The card shown beside the model picker while a row is hovered: the lab's
/// logo, models.dev's name and one-line description, context / output
/// limits, capability chips and price, then ModelGateway's 24 h error rate
/// and savings. Content comes from `ModelsDevInfo` and `ModelStat` (either
/// may be missing, not both); the logo from `ModelsDevCatalog.logo(for:)`.
struct ModelHoverCard: View {
    let modelId: String
    /// Display name used when models.dev has no entry.
    let name: String
    let info: ModelsDevInfo?
    let stat: ModelStat?
    let catalog: ModelsDevCatalog

    static let width: CGFloat = 280

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        VStack(alignment: .leading, spacing: 10) {
            header
            if let info {
                if let description = info.description {
                    Text(verbatim: description)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textSecondary)
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                limits(info)
                capabilities(info)
                if let cost = info.costLine {
                    Text(verbatim: cost)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            if let stat, stat.errorRate24h != nil || stat.savingsLabel != nil {
                gateway(stat)
            }
        }
        .padding(12)
        .frame(width: Self.width, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .floatingShadow()
        // Room for the shadow inside the borderless panel.
        .padding(12)
    }

    private var header: some View {
        HStack(spacing: 10) {
            LabLogo(image: info.flatMap { catalog.logo(for: $0.labId) }, size: 18, tint: Theme.textPrimary)
                .frame(width: 32, height: 32)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(verbatim: info?.name ?? name)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                Text(verbatim: info.map { "\($0.labName) · \($0.id)" } ?? modelId)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }

    /// ModelGateway's 24 h error rate (green under 5%, amber under 20%) and savings.
    private func gateway(_ stat: ModelStat) -> some View {
        HStack(spacing: 14) {
            if let rate = stat.errorRate24h, let label = stat.errorRateLabel {
                let tint = rate < 5 ? Theme.success : rate < 20 ? Theme.warning : Theme.danger
                VStack(alignment: .leading, spacing: 1) {
                    Text("Errors (24h)")
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                    HStack(spacing: 5) {
                        Circle().fill(tint).frame(width: 6, height: 6)
                        Text(verbatim: label)
                            .font(.system(size: 12.5, weight: .semibold))
                            .foregroundStyle(Theme.textPrimary)
                    }
                }
            }
            if let savings = stat.savingsLabel {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Cost")
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                    Text(verbatim: savings)
                        .font(.system(size: 12.5, weight: .semibold))
                        .foregroundStyle(Theme.success)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private func limits(_ info: ModelsDevInfo) -> some View {
        if info.contextWindow != nil || info.maxOutput != nil || info.knowledge != nil {
            HStack(spacing: 14) {
                if let context = info.contextWindow {
                    stat("Context", ModelsDevInfo.tokens(context))
                }
                if let output = info.maxOutput {
                    stat("Output", ModelsDevInfo.tokens(output))
                }
                if let knowledge = info.knowledge {
                    stat("Knowledge", knowledge)
                }
            }
        }
    }

    private func stat(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label)
                .font(.system(size: 10.5, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            Text(verbatim: value)
                .font(.system(size: 12.5, weight: .semibold))
                .foregroundStyle(Theme.textPrimary)
        }
    }

    /// Agent runs need tool calling, so its absence is the one warning chip.
    private func capabilities(_ info: ModelsDevInfo) -> some View {
        AutoScrollingRow {
            capabilityChips(info)
        }
    }

    private func capabilityChips(_ info: ModelsDevInfo) -> some View {
        HStack(spacing: 6) {
            if info.toolCall {
                Chip("Tools", systemImage: "wrench.adjustable")
            } else {
                Chip("No tool calling", systemImage: Icons.warning, style: .tinted(Theme.warning))
            }
            if info.reasoning { Chip("Reasoning", systemImage: Icons.thinking) }
            if info.vision { Chip("Vision", systemImage: "eye") }
            if info.openWeights { Chip("Open", systemImage: "lock.open") }
        }
    }
}

/// A single line that may be wider than its container. When it overflows
/// it slides slowly to its end and back (the hover card never takes the
/// pointer, so it cannot be scrolled by hand), with faded edges; with
/// Reduce Motion it stays put and fades out at the trailing edge.
struct AutoScrollingRow<Content: View>: View {
    @ViewBuilder let content: Content

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var contentWidth: CGFloat = 0
    @State private var containerWidth: CGFloat = 0
    @State private var offset: CGFloat = 0

    /// Points per second while sliding.
    private static var speed: CGFloat { 24 }
    private static var fade: CGFloat { 14 }

    private var overflow: CGFloat { max(0, contentWidth - containerWidth) }

    var body: some View {
        content
            .fixedSize()
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { contentWidth = $0 }
            .offset(x: offset)
            // `minWidth: 0`: otherwise the frame takes the (wider) content
            // as its minimum and never overflows.
            .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { containerWidth = $0 }
            .clipped()
            .mask(edgeFade)
            .task(id: overflow) { await slide() }
    }

    @ViewBuilder
    private var edgeFade: some View {
        if overflow > 0 {
            let edge = Self.fade / max(containerWidth, 1)
            LinearGradient(
                stops: [
                    .init(color: offset < 0 ? .clear : .black, location: 0),
                    .init(color: .black, location: edge),
                    .init(color: .black, location: 1 - edge),
                    .init(color: offset <= -overflow ? .black : .clear, location: 1),
                ],
                startPoint: .leading,
                endPoint: .trailing
            )
        } else {
            Color.black
        }
    }

    /// Pause, slide to the end, pause, slide back; repeats while visible.
    private func slide() async {
        offset = 0
        guard overflow > 0, !reduceMotion else { return }
        let duration = Double(overflow / Self.speed)
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(1.2))
            guard !Task.isCancelled else { return }
            withAnimation(.easeInOut(duration: duration)) { offset = -overflow }
            try? await Task.sleep(for: .seconds(duration + 1.2))
            guard !Task.isCancelled else { return }
            withAnimation(.easeInOut(duration: duration)) { offset = 0 }
            try? await Task.sleep(for: .seconds(duration))
        }
    }
}

// MARK: - Logos

/// The lab's logo for a model id (models.dev), else the ✳ model glyph.
/// Used by the composer's model chip and the status line above it.
struct ModelLogo: View {
    @Environment(AppState.self) private var appState
    let modelId: String
    var size: CGFloat = 12
    var tint: Color = Theme.textSecondary

    var body: some View {
        let image = appState.modelInfo(modelId).flatMap { appState.modelsDev.logo(for: $0.labId) }
        LabLogo(image: image, size: size, tint: tint)
            .accessibilityHidden(true)
    }
}

/// A template logo drawn at `size`, or the ✳ glyph when there is none.
struct LabLogo: View {
    let image: NSImage?
    let size: CGFloat
    let tint: Color

    var body: some View {
        if let image {
            Image(nsImage: image)
                .resizable()
                .renderingMode(.template)
                .interpolation(.high)
                .scaledToFit()
                .frame(width: size, height: size)
                .foregroundStyle(tint)
        } else {
            Image(systemName: Icons.model)
                .font(.system(size: size * 0.8, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: size, height: size)
        }
    }
}

// MARK: - Panel

/// One borderless, non-activating panel that floats a hover card beside an
/// anchor (the hovered row). It never becomes key, so the picker's search
/// field keeps focus, and it ignores the mouse.
@MainActor
final class HoverCardPanel {
    static let shared = HoverCardPanel()

    private var panel: NSPanel?
    private var hosting: NSHostingView<AnyView>?
    /// The row the visible card belongs to.
    private weak var anchor: NSView?

    enum Placement {
        /// Beside the anchor's window (the model picker), tops aligned with the row.
        case besideWindow
        /// Under the anchor, leading edges aligned; above it when the screen ends first.
        case below
    }

    /// Shows `content` beside `anchor` (the row's view) at once.
    func show<Content: View>(_ content: Content, beside anchor: NSView, placement: Placement = .besideWindow) {
        self.anchor = anchor
        present(AnyView(content), beside: anchor, placement: placement)
    }

    /// Hides the card, or with `anchor` only while it is that row's card:
    /// moving between rows can deliver the new row's enter before the old
    /// row's exit.
    func hide(from anchor: NSView? = nil) {
        if let anchor, anchor !== self.anchor { return }
        self.anchor = nil
        panel?.orderOut(nil)
    }

    private func present(_ content: AnyView, beside anchor: NSView, placement: Placement) {
        guard let window = anchor.window else { return }
        let panel = self.panel ?? makePanel()
        let hosting = self.hosting ?? NSHostingView(rootView: AnyView(EmptyView()))
        if self.hosting == nil {
            self.hosting = hosting
            panel.contentView = hosting
        }
        hosting.rootView = content
        let size = hosting.fittingSize
        let row = window.convertToScreen(anchor.convert(anchor.bounds, to: nil))
        let host = window.frame
        let screen = (window.screen ?? NSScreen.main)?.visibleFrame ?? host
        // The 12 pt shadow margin is part of `size`.
        var x: CGFloat
        var y: CGFloat
        switch placement {
        case .besideWindow:
            // Right of the picker when it fits, else left; the card's top
            // lines up with the row.
            x = host.maxX - 6
            if x + size.width > screen.maxX { x = host.minX - size.width + 6 }
            y = row.maxY - size.height + 12
        case .below:
            x = min(max(row.minX - 12, screen.minX), screen.maxX - size.width)
            y = row.minY - size.height + 6
            if y < screen.minY { y = row.maxY - 6 }
        }
        y = min(max(y, screen.minY), screen.maxY - size.height)
        panel.setFrame(NSRect(x: x, y: y, width: size.width, height: size.height), display: true)
        panel.orderFront(nil)
    }

    private func makePanel() -> NSPanel {
        let panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isFloatingPanel = true
        panel.level = .popUpMenu
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = true
        panel.collectionBehavior = [.transient, .ignoresCycle, .fullScreenAuxiliary]
        self.panel = panel
        return panel
    }
}

/// Hands the hosting `NSView` of a row to `HoverCardPanel` (the anchor).
final class HoverAnchor {
    weak var view: NSView?
}

struct HoverAnchorView: NSViewRepresentable {
    let anchor: HoverAnchor

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        anchor.view = view
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        anchor.view = nsView
    }
}

// MARK: - Preview

#Preview("Hover card") {
    ModelHoverCard(
        modelId: "gpt-4.1",
        name: "GPT-4.1",
        info: ModelsDevInfo(
            id: "gpt-4.1", name: "GPT-4.1",
            description: "Long-lived GPT workhorse for coding, instruction following, and production apps",
            labId: "openai", labName: "OpenAI", contextWindow: 1_047_576, maxOutput: 32_768,
            toolCall: true, reasoning: false, vision: true, openWeights: false,
            knowledge: "2024-04", releaseDate: "2025-04-14", inputCost: 2, outputCost: 8
        ),
        stat: ModelStat(savingsPercent: 98, errorRate24h: 1.5),
        catalog: ModelsDevCatalog()
    )
}
