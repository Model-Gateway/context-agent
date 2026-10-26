import SwiftUI

/// Developer mode: the collapsed "Full prompt" row above the message that
/// started a run. Open, it lists every message of the run's first model
/// request (each its own collapsed row, so a long repository context is
/// only laid out when asked for) and the tools offered, with Copy.
struct SentPromptView: View {
    var prompt: SentPrompt

    @State private var isExpanded = false

    var body: some View {
        DisclosureRow(isExpanded: $isExpanded) {
            Image(systemName: Icons.prompt)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            Text("Full prompt")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            Text(summary)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
        } content: {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(prompt.messages) { message in
                    SentPromptMessageRow(message: message)
                }
                if !prompt.tools.isEmpty {
                    Text("Tools: " + prompt.tools.joined(separator: ", "))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                        .padding(.horizontal, 6)
                        .padding(.top, 4)
                }
                HStack {
                    Spacer()
                    CopyButton(text: prompt.plainText, label: "Copy the full prompt")
                }
            }
            .padding(.leading, 12)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Full prompt, \(summary)")
    }

    private var summary: String {
        var parts = ["\(prompt.messages.count) message\(prompt.messages.count == 1 ? "" : "s")", "~\(TranscriptFormat.tokens(prompt.estimatedTokens)) tok"]
        if !prompt.tools.isEmpty { parts.append("\(prompt.tools.count) tool\(prompt.tools.count == 1 ? "" : "s")") }
        return parts.joined(separator: " · ")
    }
}

private struct SentPromptMessageRow: View {
    var message: SentPrompt.Message

    @State private var isExpanded = false

    var body: some View {
        DisclosureRow(isExpanded: $isExpanded) {
            Text(title)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textSecondary)
            Text(detail)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
        } content: {
            // One `Text` per block of lines: a single `Text` holding tens of
            // KB of repository context stalls layout.
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(Self.blocks(message.text).enumerated()), id: \.offset) { _, block in
                    Text(verbatim: block)
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .selectableText()
            .padding(10)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        }
    }

    private var title: String {
        message.kind.replacingOccurrences(of: "_", with: " ")
    }

    private var detail: String {
        var parts = ["~\(TranscriptFormat.tokens(message.estimatedTokens)) tok"]
        if message.images > 0 { parts.append("\(message.images) image\(message.images == 1 ? "" : "s")") }
        return parts.joined(separator: " · ")
    }

    static func blocks(_ text: String, linesPerBlock: Int = 40) -> [String] {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        guard lines.count > linesPerBlock else { return [text] }
        return stride(from: 0, to: lines.count, by: linesPerBlock).map { start in
            lines[start..<min(start + linesPerBlock, lines.count)].joined(separator: "\n")
        }
    }
}

#Preview("Full prompt") {
    SentPromptView(prompt: SentPrompt(step: 1, messages: [
        .init(kind: "policy", text: "You are Context, a coding agent working in the user's repository.", estimatedTokens: 1200),
        .init(kind: "context", text: "<<<BEGIN REPOSITORY CONTEXT>>>\nsrc/lib.rs:1-20\npub fn add(a: i32, b: i32) -> i32 { a + b }\n<<<END REPOSITORY CONTEXT>>>", estimatedTokens: 8400),
        .init(kind: "user", text: "Fix the failing test in cards.rs", estimatedTokens: 9, images: 1),
    ], tools: ["read_file", "search_code", "apply_patch", "execute_command"]))
    .padding()
    .frame(width: 640)
}
