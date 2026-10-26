import SwiftUI

/// The collapsible reasoning row above an assistant message:
/// "Thinking ⠋ 1.2s" while reasoning streams, "Thinking · 1.2s" once the
/// answer has started. It is open while the model thinks and collapses on
/// the first text delta; the user can reopen it at any time, and a click
/// anywhere in the open reasoning closes it again.
struct ThinkingView: View {
    var turn: AssistantTurn

    @State private var isExpanded = false

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(turn: AssistantTurn) {
        self.turn = turn
    }

    var body: some View {
        DisclosureRow(isExpanded: $isExpanded) {
            Image(systemName: Icons.thinking)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            Text("Thinking")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            if turn.isThinking {
                BrailleSpinner()
                if let started = turn.thinkingStarted {
                    TimelineView(.periodic(from: .now, by: 0.1)) { context in
                        elapsedText(context.date.timeIntervalSince(started))
                    }
                }
            } else if let duration = turn.thinkingDuration {
                elapsedText(duration)
            }
        } content: {
            // One `Text` holding the whole reasoning is re-laid out on every
            // 33 ms flush, which stalls the main thread once it runs to tens
            // of KB. While streaming only the tail is shown; once finished
            // each paragraph is its own `Text`, so layout cost stays per
            // paragraph rather than per document.
            VStack(alignment: .leading, spacing: 8) {
                if turn.isThinking {
                    reasoningText(Self.streamingTail(turn.reasoning))
                } else {
                    ForEach(Array(Self.paragraphs(turn.reasoning).enumerated()), id: \.offset) { _, paragraph in
                        reasoningText(paragraph)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.leading, 6)
            .padding(.bottom, 4)
            // A click anywhere in the reasoning collapses it, so a long one
            // can be closed without scrolling back up to the header. It is
            // simultaneous so drag-to-select still reaches the text.
            .contentShape(Rectangle())
            .simultaneousGesture(TapGesture().onEnded { collapse() })
            .help("Click to collapse")
            .accessibilityAction(named: "Collapse") { collapse() }
        }
        .onAppear { isExpanded = turn.isThinking }
        .onChange(of: turn.isThinking) { _, thinking in
            // Auto-collapse once the answer starts; a manual reopen stays open.
            if !thinking {
                withAnimation(.easeInOut(duration: 0.18)) { isExpanded = false }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(accessibilityText)
    }

    private func collapse() {
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { isExpanded = false }
    }

    private func reasoningText(_ text: String) -> some View {
        Text(text)
            .font(Theme.Font.secondary)
            .foregroundStyle(Theme.textSecondary)
            .lineSpacing(2)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .selectableText()
    }

    /// Characters of live reasoning shown while it streams.
    static let streamingTailLength = 1_500

    /// The last `streamingTailLength` characters, starting at a line (else
    /// word) boundary and led by "…" when anything was cut.
    static func streamingTail(_ reasoning: String) -> String {
        guard reasoning.utf8.count > streamingTailLength else { return reasoning }
        var tail = reasoning.suffix(streamingTailLength)
        if let newline = tail.firstIndex(of: "\n") {
            tail = tail[tail.index(after: newline)...]
        } else if let space = tail.firstIndex(of: " ") {
            tail = tail[tail.index(after: space)...]
        }
        return "…" + tail
    }

    /// Blank-line separated paragraphs, trimmed, empties dropped.
    static func paragraphs(_ reasoning: String) -> [String] {
        reasoning
            .components(separatedBy: "\n\n")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    private func elapsedText(_ seconds: TimeInterval) -> some View {
        Text(Self.elapsedLabel(seconds))
            .font(Theme.Font.caption)
            .foregroundStyle(Theme.textTertiary)
            .monospacedDigit()
    }

    private var accessibilityText: String {
        if turn.isThinking { return "Thinking" }
        if let duration = turn.thinkingDuration { return "Thought for \(Self.elapsedLabel(duration))" }
        return "Thinking"
    }

    /// 1.234 → "1.2s"; ≥ 60 s falls back to the shared "1m 05s" style.
    static func elapsedLabel(_ seconds: TimeInterval) -> String {
        let clamped = max(0, seconds)
        if clamped < 60 { return String(format: "%.1fs", clamped) }
        return TranscriptFormat.duration(clamped)
    }
}

#Preview("Thinking") {
    let thinking = AssistantTurn(
        text: "",
        reasoning: "The failing test asserts on the sorted order of the cards. The comparator uses `created_at`, but the fixture only sets `updated_at`…",
        isStreaming: true,
        isThinking: true,
        step: 2,
        thinkingStarted: Date().addingTimeInterval(-1.2)
    )
    let finished = AssistantTurn(
        text: "Here is the fix.",
        reasoning: "The failing test asserts on the sorted order of the cards.",
        isStreaming: false,
        isThinking: false,
        step: 3,
        thinkingStarted: Date().addingTimeInterval(-4),
        thinkingDuration: 2.7
    )
    return VStack(alignment: .leading, spacing: 16) {
        ThinkingView(turn: thinking)
        ThinkingView(turn: finished)
    }
    .frame(width: 560)
    .padding(24)
    .background(Theme.canvas)
}
