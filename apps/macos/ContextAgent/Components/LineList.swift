import SwiftUI

/// Monospace lines as a single `Text`, optionally syntax-highlighted.
/// Rendering is capped at `maxLines` / `maxBytes`; beyond that a footer says
/// "+ N more lines · Open full output" and calls `onOpenFull`.
///
/// One `Text` for the whole block (not one per line): a transcript with a
/// few expanded tool cards used to hold hundreds of selectable text views,
/// each a platform view AppKit had to track while scrolling. One view draws
/// in one pass, selection runs across lines, and the highlighted string is
/// memoized so a row that scrolls back into view does not tokenize again.
///
/// Inside a vertical scroll view the lines wrap; inside a horizontal
/// `ScrollView` (see `CodeBlockView`) they stay on one line.
struct LineList: View {
    private let lines: [String]
    private let omittedLines: Int
    /// A line longer than the remaining byte budget was cut short.
    private let clipped: Bool
    private let highlight: SyntaxHighlighter.Language?
    private let onOpenFull: (() -> Void)?

    /// Rendered blocks by text and language; a block is rebuilt whenever its
    /// row scrolls back into view or a sibling toggles, so the cache pays off.
    private static let rendered = RenderCache<RenderKey, AttributedString>(countLimit: 256)

    private struct RenderKey: Hashable {
        var lines: [String]
        var highlight: SyntaxHighlighter.Language?
    }

    init(text: String, maxLines: Int = 400, maxBytes: Int = 32_768, highlight: SyntaxHighlighter.Language? = nil, onOpenFull: (() -> Void)? = nil) {
        let capped = LineList.cap(text, maxLines: maxLines, maxBytes: maxBytes)
        self.lines = capped.lines
        self.omittedLines = capped.omitted
        self.clipped = capped.clipped
        self.highlight = highlight
        self.onOpenFull = onOpenFull
    }

    /// Lines that will be drawn (after the caps), so a host can decide
    /// whether the block needs its own scroll view.
    var lineCount: Int { lines.count }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(rendered)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textPrimary)
                .lineSpacing(2)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .selectableText()
            if omittedLines > 0 || clipped {
                footer
            }
        }
    }

    private var rendered: AttributedString {
        Self.rendered.value(for: RenderKey(lines: lines, highlight: highlight)) {
            Self.render(lines, highlight: highlight)
        }
    }

    private var footer: some View {
        Button {
            onOpenFull?()
        } label: {
            HStack(spacing: 0) {
                Text(omittedLines > 0 ? "+ \(omittedLines.formatted()) more line\(omittedLines == 1 ? "" : "s")" : "Long lines clipped")
                    .foregroundStyle(Theme.textSecondary)
                if onOpenFull != nil {
                    Text(" · ")
                        .foregroundStyle(Theme.textTertiary)
                    Text("Open full output")
                        .foregroundStyle(Theme.accent)
                }
            }
            .font(Theme.Font.caption)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(onOpenFull == nil)
        .padding(.top, 6)
        .accessibilityLabel(onOpenFull == nil ? "\(omittedLines) more lines" : "\(omittedLines) more lines, open full output")
    }

    /// The lines joined with newlines; each line is tokenized on its own so
    /// the grammar never needs context from its neighbours.
    private static func render(_ lines: [String], highlight: SyntaxHighlighter.Language?) -> AttributedString {
        var result = AttributedString()
        for (index, line) in lines.enumerated() {
            if index > 0 { result.append(AttributedString("\n")) }
            // An empty line still needs a glyph run to keep its height.
            let shown = line.isEmpty ? " " : line
            if let highlight {
                result.append(SyntaxHighlighter.highlight(shown, language: highlight))
            } else {
                result.append(AttributedString(shown))
            }
        }
        return result
    }

    /// Splits `text` into lines and keeps at most `maxLines` lines / `maxBytes`
    /// bytes; a line longer than the remaining budget is clipped (never inside
    /// a UTF-8 sequence) so one huge line can never reach `Text`. Works on the
    /// UTF-8 view: splitting a multi-hundred-kilobyte output as `Character`s
    /// on every render is what made large cards slow to open. A trailing
    /// newline does not count as an extra (empty) line.
    static func cap(_ text: String, maxLines: Int, maxBytes: Int) -> (lines: [String], omitted: Int, clipped: Bool) {
        let utf8 = text.utf8
        var kept: [String] = []
        var budget = max(0, maxBytes)
        var total = 0
        var clipped = false
        var cursor = utf8.startIndex
        while cursor < utf8.endIndex {
            let newline = utf8[cursor...].firstIndex(of: 0x0A) ?? utf8.endIndex
            var lineEnd = newline
            if lineEnd > cursor, utf8[utf8.index(before: lineEnd)] == 0x0D { lineEnd = utf8.index(before: lineEnd) }
            total += 1
            if kept.count < maxLines, budget > 0 {
                let length = utf8.distance(from: cursor, to: lineEnd)
                if length > budget {
                    var clipEnd = utf8.index(cursor, offsetBy: budget)
                    while clipEnd > cursor, (utf8[clipEnd] & 0b1100_0000) == 0b1000_0000 { clipEnd = utf8.index(before: clipEnd) }
                    kept.append(String(decoding: utf8[cursor..<clipEnd], as: UTF8.self) + "…")
                    clipped = true
                    budget = 0
                } else {
                    kept.append(String(decoding: utf8[cursor..<lineEnd], as: UTF8.self))
                    budget -= length + 1
                }
            }
            cursor = newline == utf8.endIndex ? utf8.endIndex : utf8.index(after: newline)
        }
        return (kept, max(0, total - kept.count), clipped)
    }
}

#Preview("Line list") {
    let json = """
    {
      "taskId": "11",
      "status": "completed",
      "files": [1, 2, 3],
      "ok": true
    }
    """
    let long = (1...30).map { "line \($0): echo \"hello\" # comment" }.joined(separator: "\n")
    return ScrollView {
        VStack(alignment: .leading, spacing: 16) {
            LineList(text: json, highlight: .json)
                .padding(12)
                .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: 12))
            LineList(text: long, maxLines: 8, highlight: .shell, onOpenFull: {})
                .padding(12)
                .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: 12))
            LineList(text: long, maxLines: 5)
                .padding(12)
                .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: 12))
        }
        .padding(24)
    }
    .frame(width: 560, height: 520)
    .background(Theme.canvas)
}
