import SwiftUI

// Placeholders for content that is still loading (the chat list, a project
// reopening at launch, a transcript's history, a settings page): grey bars in the shape of
// what will replace them, pulsing together. Small inline waits keep using
// `BrailleSpinner`.

/// One rounded placeholder bar.
struct SkeletonBar: View {
    var width: CGFloat?
    var height: CGFloat
    var cornerRadius: CGFloat

    init(width: CGFloat? = nil, height: CGFloat = 10, cornerRadius: CGFloat = 4) {
        self.width = width
        self.height = height
        self.cornerRadius = cornerRadius
    }

    var body: some View {
        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
            .fill(Theme.border)
            .frame(width: width, height: height)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
    }
}

/// Fades its content between 45 % and 100 % opacity on a 1.4 s cycle taken
/// from the absolute date, so every skeleton on screen pulses in step however
/// late it appeared. Static with Reduce Motion or while not `active`. Hidden
/// from VoiceOver behind `label`.
private struct SkeletonPulse: ViewModifier {
    var label: String
    var active = true

    static let period: TimeInterval = 1.4

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        Group {
            if reduceMotion || !active {
                content.opacity(0.8)
            } else {
                TimelineView(.animation(minimumInterval: 1 / 30)) { context in
                    content.opacity(Self.opacity(at: context.date))
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
        .accessibilityAddTraits(.updatesFrequently)
    }

    static func opacity(at date: Date) -> Double {
        let phase = date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: period) / period
        return 0.45 + 0.55 * (0.5 + 0.5 * cos(phase * 2 * .pi))
    }
}

extension View {
    /// Pulses a group of `SkeletonBar`s; VoiceOver reads `label` instead.
    /// Inactive, the bars hold still (a skeleton scrolled out of view).
    func skeletonPulse(_ label: String = "Loading", active: Bool = true) -> some View {
        modifier(SkeletonPulse(label: label, active: active))
    }
}

// MARK: - Transcript

/// A conversation's shape while its history loads (and while a remembered
/// chat is restored at launch): a user bubble at the right, then an
/// assistant reply of a few lines, twice.
struct TranscriptSkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            Exchange(question: 0.42, reply: [1, 0.94, 0.62])
            Exchange(question: 0.3, reply: [0.97, 1, 0.88, 0.4])
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .skeletonPulse("Loading history")
    }

    /// A user bubble at the right over a reply of `reply.count` lines
    /// (each a fraction of the width); 132 pt with four lines.
    struct Exchange: View {
        var question: CGFloat
        var reply: [CGFloat]

        var body: some View {
            VStack(alignment: .leading, spacing: 20) {
                GeometryReader { geometry in
                    SkeletonBar(width: geometry.size.width * question, height: 38, cornerRadius: Theme.Metrics.radiusLarge)
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }
                .frame(height: 38)
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(Array(reply.enumerated()), id: \.offset) { _, fraction in
                        GeometryReader { geometry in
                            SkeletonBar(width: geometry.size.width * fraction, height: 11)
                        }
                        .frame(height: 11)
                    }
                }
            }
        }
    }
}

/// A tool-call row's shape: the 24 pt icon square and a label bar; 36 pt.
struct ToolCallSkeleton: View {
    var label: CGFloat

    var body: some View {
        HStack(spacing: 12) {
            SkeletonBar(width: 24, height: 24, cornerRadius: 6)
            GeometryReader { geometry in
                SkeletonBar(width: geometry.size.width * label, height: 10)
                    .frame(maxHeight: .infinity)
            }
        }
        .frame(height: 24)
        .padding(.horizontal, 6)
        .padding(.vertical, 6)
    }
}

/// The Chat tab while the chat it will show is still being looked up (a
/// remembered chat at launch or on switching projects): the transcript
/// skeleton in the transcript's column.
struct ChatLoadingView: View {
    var body: some View {
        ScrollView(.vertical) {
            TranscriptSkeleton()
                .padding(.horizontal, 24)
                .padding(.top, 20)
                .frame(maxWidth: Theme.Metrics.contentMaxWidth + 48)
                .frame(maxWidth: .infinity)
        }
        .scrollDisabled(true)
        .background(Theme.canvas)
    }
}

/// The empty chat's shape (logo, title, composer, three starter cards) while
/// the project it belongs to is still opening at launch.
struct EmptyChatSkeleton: View {
    var body: some View {
        VStack(spacing: 24) {
            VStack(spacing: 14) {
                SkeletonBar(width: 30, height: 30, cornerRadius: 15)
                SkeletonBar(width: 240, height: 20, cornerRadius: 6)
            }
            SkeletonBar(height: 112, cornerRadius: Theme.Metrics.radiusLarge)
            HStack(spacing: 12) {
                ForEach(0..<3, id: \.self) { _ in
                    SkeletonBar(height: 64, cornerRadius: Theme.Metrics.radius)
                }
            }
        }
        .skeletonPulse("Opening project")
        .frame(maxWidth: Theme.Metrics.contentMaxWidth)
        .padding(.horizontal, 24)
        .padding(.bottom, 56)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
    }
}

// MARK: - Settings

/// Label and value bars laid out like `SettingsRow`s (a 128 pt label column).
private struct SkeletonSettingsRows: View {
    var count: Int

    private static let valueWidths: [CGFloat] = [0.55, 0.35, 0.7, 0.45]
    private static let labelWidths: [CGFloat] = [70, 96, 58, 84]

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            ForEach(0..<count, id: \.self) { index in
                HStack(spacing: 12) {
                    SkeletonBar(width: Self.labelWidths[index % Self.labelWidths.count], height: 10)
                        .frame(width: 128, alignment: .trailing)
                    GeometryReader { geometry in
                        SkeletonBar(width: geometry.size.width * Self.valueWidths[index % Self.valueWidths.count], height: 10)
                    }
                    .frame(height: 10)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

/// Rows inside a settings card whose content is still loading (spend this
/// period, the ModelGateway key, the balance).
struct SettingsRowsSkeleton: View {
    var count = 2
    var label = "Loading"

    var body: some View {
        SkeletonSettingsRows(count: count)
            .skeletonPulse(label)
    }
}

/// A whole settings page while its data loads (the backend starting, the
/// configuration not read yet): one card per entry of `rows`, each with a
/// title, that many rows and a footer line, like `SettingsGroup`.
struct SettingsPageSkeleton: View {
    var rows: [Int] = [3, 2]
    var label = "Loading settings"

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, count in
                VStack(alignment: .leading, spacing: 14) {
                    SkeletonBar(width: index.isMultiple(of: 2) ? 120 : 92, height: 13)
                    SkeletonSettingsRows(count: count)
                    GeometryReader { geometry in
                        SkeletonBar(width: geometry.size.width * 0.8, height: 8)
                    }
                    .frame(height: 8)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .card()
            }
        }
        .skeletonPulse(label)
    }
}

#Preview("Settings skeleton") {
    ScrollView {
        SettingsPageSkeleton(rows: [3, 2, 1])
            .padding(20)
    }
    .frame(width: 640, height: 520)
    .background(Theme.canvas)
}

#Preview("Empty chat skeleton") {
    EmptyChatSkeleton()
        .frame(width: 900, height: 640)
}

#Preview("Transcript skeleton") {
    ChatLoadingView()
        .frame(width: 760, height: 480)
}
