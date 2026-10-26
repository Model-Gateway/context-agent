import SwiftUI

/// Renders markdown (assistant messages, outcome summaries) with the Theme
/// fonts. Blocks come from `MarkdownParser`; inline syntax goes through
/// `AttributedString(markdown:)` so emphasis, code spans and links work.
/// Links are tinted `accent` and open through the `openURL` environment.
struct MarkdownView: View {
    private let blocks: [MarkdownBlock]

    /// Parsed blocks by source text; rows rebuild whenever they scroll back into view.
    private static let parsed = RenderCache<String, [MarkdownBlock]>(countLimit: 512)

    init(_ markdown: String) {
        blocks = Self.parsed.value(for: markdown) { MarkdownParser.parse(markdown) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(blocks.indices, id: \.self) { index in
                MarkdownBlockView(block: blocks[index])
            }
        }
        .foregroundStyle(Theme.textPrimary)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Inline markdown → `AttributedString` with the app's code and link styling.
enum MarkdownInline {
    /// `**strong**` runs are drawn in `font` at semibold rather than the
    /// default bold, which reads too heavy beside 14 pt body text.
    static func render(_ text: String, font: Font = Theme.Font.body) -> AttributedString {
        renderCache.value(for: RenderKey(text: text, font: font)) { uncachedRender(text, font: font) }
    }

    private struct RenderKey: Hashable {
        var text: String
        var font: Font
    }

    private static let renderCache = RenderCache<RenderKey, AttributedString>(countLimit: 2048)

    private static func uncachedRender(_ text: String, font: Font) -> AttributedString {
        var result: AttributedString
        if let parsed = try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)) {
            result = parsed
        } else {
            result = AttributedString(text)
        }
        let runs = result.runs.map { ($0.range, $0.inlinePresentationIntent, $0.link) }
        for (range, intent, link) in runs {
            if let intent, intent.contains(.code) {
                result[range].font = Theme.Font.mono
                result[range].backgroundColor = Theme.codeBackground
            } else if let intent, intent.contains(.stronglyEmphasized) {
                let strong = font.weight(.semibold)
                result[range].font = intent.contains(.emphasized) ? strong.italic() : strong
                result[range].inlinePresentationIntent = intent.subtracting([.stronglyEmphasized, .emphasized])
            }
            if link != nil {
                result[range].foregroundColor = Theme.accent
                result[range].underlineStyle = .single
            }
        }
        return result
    }
}

// MARK: - Blocks

private struct MarkdownBlockView: View {
    let block: MarkdownBlock

    var body: some View {
        switch block {
        case .heading(let level, let text):
            InlineText(text, font: headingFont(level))
                .padding(.top, level <= 2 ? 6 : 2)
        case .paragraph(let text):
            InlineText(text, font: Theme.Font.body)
        case .bulletList(let items):
            MarkdownListView(items: items, ordered: false, start: 1)
        case .orderedList(let items, let start):
            MarkdownListView(items: items, ordered: true, start: start)
        case .blockquote(let blocks):
            MarkdownQuoteView(blocks: blocks)
        case .codeFence(let language, let code):
            CodeBlockView(code: code, language: language)
        case .table(let headers, let rows):
            MarkdownTableView(headers: headers, rows: rows)
        case .horizontalRule:
            Rectangle()
                .fill(Theme.border)
                .frame(height: 1)
                .padding(.vertical, 4)
                .accessibilityHidden(true)
        }
    }

    private func headingFont(_ level: Int) -> Font {
        switch level {
        case 1: return Theme.Font.title
        case 2: return .system(size: 18, weight: .semibold)
        case 3: return Theme.Font.heading
        default: return .system(size: 14, weight: .semibold)
        }
    }
}

/// File names that resolve to project files render as chips
/// (FileReferences.swift) when a `FileReferenceContext` is in the environment.
private struct InlineText: View {
    let text: String
    let font: Font

    @Environment(\.fileReferences) private var files
    @State private var hoveredReference: Int?

    init(_ text: String, font: Font) {
        self.text = text
        self.font = font
    }

    var body: some View {
        let rendered = MarkdownInline.rendered(text, font: font, files: files)
        rendered.text
            .font(font)
            .lineSpacing(3)
            .fixedSize(horizontal: false, vertical: true)
            .selectableText()
            .fileReferenceChips(paths: rendered.paths, files: files, hovered: $hoveredReference)
    }
}

private struct MarkdownListView: View {
    let items: [MarkdownListItem]
    let ordered: Bool
    let start: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(items.indices, id: \.self) { index in
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(ordered ? "\(start + index)." : "•")
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.textSecondary)
                        .frame(minWidth: 16, alignment: .trailing)
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 6) {
                        if !items[index].text.isEmpty {
                            InlineText(items[index].text, font: Theme.Font.body)
                        }
                        ForEach(items[index].nested.indices, id: \.self) { nestedIndex in
                            MarkdownBlockView(block: items[index].nested[nestedIndex])
                        }
                    }
                }
            }
        }
        .padding(.leading, 4)
    }
}

private struct MarkdownQuoteView: View {
    let blocks: [MarkdownBlock]

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(Theme.borderStrong)
                .frame(width: 3)
            VStack(alignment: .leading, spacing: 8) {
                ForEach(blocks.indices, id: \.self) { index in
                    MarkdownBlockView(block: blocks[index])
                }
            }
            .foregroundStyle(Theme.textSecondary)
        }
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct MarkdownTableView: View {
    let headers: [String]
    let rows: [[String]]

    var body: some View {
        Grid(alignment: .topLeading, horizontalSpacing: 0, verticalSpacing: 0) {
            GridRow {
                ForEach(headers.indices, id: \.self) { column in
                    cell(headers[column], font: .system(size: 13, weight: .semibold))
                        .background(Theme.surfaceRaised)
                }
            }
            ForEach(rows.indices, id: \.self) { row in
                GridRow {
                    ForEach(rows[row].indices, id: \.self) { column in
                        cell(rows[row][column], font: Theme.Font.secondary)
                    }
                }
            }
        }
        .background(Theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
    }

    private func cell(_ text: String, font: Font) -> some View {
        InlineText(text, font: font)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .overlay(alignment: .bottom) {
                Rectangle().fill(Theme.border).frame(height: 1)
            }
    }
}

#Preview("Markdown") {
    let sample = """
    # Summary

    The parser handles **bold**, *italic*, `inline code` and [links](https://example.com).
    A second line in the same paragraph.

    ## Changes
    - Edited `src/app.rs`
      - nested item with `code`
      - another
    - Ran the tests

    1. First
    2. Second
       - sub bullet

    > A quoted note with **emphasis**.

    ```rust
    fn main() {
        println!("hello"); // greet
    }
    ```

    | File | Status |
    |---|---|
    | `src/app.rs` | modified |
    | `README.md` | added |

    ---

    ```json
    {"unterminated": true
    """
    return ScrollView {
        MarkdownView(sample)
            .padding(24)
            .frame(width: 640)
    }
    .frame(height: 720)
    .background(Theme.canvas)
}
