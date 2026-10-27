import SwiftUI

/// Circular progress for work with a known size, such as a project opening
/// and building its index (the recent cards, the empty chat's index card):
/// a hairline track and an accent arc filled to `fraction`. Until the total
/// is known (`fraction` nil) a quarter arc turns once a second, derived from
/// the clock like `BrailleSpinner` so re-created rings never restart; with
/// Reduce Motion it stands still. Other async waits keep the spinner.
struct ProgressRing: View {
    /// 0…1, or nil while the total is not known.
    var fraction: Double?
    var size: CGFloat
    var lineWidth: CGFloat
    var tint: Color

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(fraction: Double?, size: CGFloat = 16, lineWidth: CGFloat = 2, tint: Color = Theme.accent) {
        self.fraction = fraction
        self.size = size
        self.lineWidth = lineWidth
        self.tint = tint
    }

    var body: some View {
        ZStack {
            Circle()
                .stroke(Theme.borderStrong, lineWidth: lineWidth)
            if let fraction {
                arc(length: min(max(fraction, 0), 1))
                    .rotationEffect(.degrees(-90))
                    .animation(.easeOut(duration: 0.25), value: fraction)
            } else if reduceMotion {
                arc(length: 0.25)
                    .rotationEffect(.degrees(-90))
            } else {
                TimelineView(.animation(minimumInterval: 1.0 / 60)) { context in
                    arc(length: 0.25)
                        .rotationEffect(.degrees(Self.angle(at: context.date)))
                }
            }
        }
        // The stroke is centred on the circle: keep all of it inside the frame.
        .padding(lineWidth / 2)
        .frame(width: size, height: size)
        .accessibilityElement()
        .accessibilityLabel(fraction == nil ? "Working" : "Progress")
        .accessibilityValue(fraction.map { "\(Int(($0 * 100).rounded())) percent" } ?? "")
        .accessibilityAddTraits(.updatesFrequently)
    }

    private func arc(length: Double) -> some View {
        Circle()
            .trim(from: 0, to: length)
            .stroke(tint, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
    }

    /// One turn a second, for an absolute date.
    static func angle(at date: Date) -> Double {
        date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1) * 360
    }
}

#Preview("Progress rings") {
    HStack(spacing: 16) {
        ProgressRing(fraction: nil)
        ProgressRing(fraction: 0)
        ProgressRing(fraction: 0.39)
        ProgressRing(fraction: 1)
        ProgressRing(fraction: 0.62, size: 24, lineWidth: 3)
    }
    .padding(24)
    .background(Theme.canvas)
}
