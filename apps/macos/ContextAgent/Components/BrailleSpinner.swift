import SwiftUI

/// The app's inline loading indicator (whole areas that are still loading
/// use the placeholders in Skeleton.swift): ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ cycling every 80 ms.
/// With Reduce Motion on it is a static ⠿. Either way it is labelled
/// "Working" for VoiceOver.
struct BrailleSpinner: View {
    static let frames: [String] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    static let frameInterval: TimeInterval = 0.08

    var size: CGFloat
    /// nil inherits the surrounding foreground style (a prominent button's label colour).
    var tint: Color?

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(size: CGFloat = 12, tint: Color? = Theme.textSecondary) {
        self.size = size
        self.tint = tint
    }

    var body: some View {
        Group {
            if reduceMotion {
                glyph("⠿")
            } else {
                TimelineView(.periodic(from: .now, by: Self.frameInterval)) { context in
                    glyph(Self.frame(at: context.date))
                }
            }
        }
        .accessibilityLabel("Working")
        .accessibilityAddTraits(.updatesFrequently)
    }

    /// Frame for an absolute date, so re-created views never restart the cycle.
    static func frame(at date: Date) -> String {
        let ticks = Int((date.timeIntervalSinceReferenceDate / frameInterval).rounded(.down))
        let index = ((ticks % frames.count) + frames.count) % frames.count
        return frames[index]
    }

    @ViewBuilder
    private func glyph(_ text: String) -> some View {
        let base = Text(text)
            .font(.system(size: size, weight: .regular, design: .monospaced))
        Group {
            if let tint {
                base.foregroundStyle(tint)
            } else {
                base
            }
        }
        .frame(minWidth: size, minHeight: size)
        .fixedSize()
    }
}

/// Three small pulsing dots for inline "still going" hints (queue rows,
/// streaming tails). Static with Reduce Motion.
struct InlineDots: View {
    var tint: Color

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(tint: Color = Theme.textTertiary) {
        self.tint = tint
    }

    var body: some View {
        Group {
            if reduceMotion {
                dots(active: nil)
            } else {
                TimelineView(.periodic(from: .now, by: 0.3)) { context in
                    let ticks = Int((context.date.timeIntervalSinceReferenceDate / 0.3).rounded(.down))
                    dots(active: ((ticks % 3) + 3) % 3)
                }
            }
        }
        .accessibilityLabel("Working")
    }

    private func dots(active: Int?) -> some View {
        HStack(spacing: 3) {
            ForEach(0..<3, id: \.self) { index in
                Circle()
                    .fill(tint)
                    .frame(width: 4, height: 4)
                    .opacity(active == nil || active == index ? 1 : 0.35)
            }
        }
        .frame(height: 8)
    }
}

#Preview("Spinners") {
    VStack(alignment: .leading, spacing: 16) {
        HStack(spacing: 12) {
            BrailleSpinner()
            BrailleSpinner(size: 16, tint: Theme.accent)
            BrailleSpinner(size: 22, tint: Theme.textPrimary)
        }
        HStack(spacing: 8) {
            Text("Thinking").font(Theme.Font.secondary).foregroundStyle(Theme.textSecondary)
            BrailleSpinner()
            Text("1.2s").font(Theme.Font.caption).foregroundStyle(Theme.textTertiary)
        }
        HStack(spacing: 8) {
            Text("Queued").font(Theme.Font.secondary).foregroundStyle(Theme.textSecondary)
            InlineDots()
        }
    }
    .padding(24)
    .background(Theme.canvas)
}
