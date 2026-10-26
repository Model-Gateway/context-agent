import SwiftUI
import Foundation

/// Block-level markdown. Inline syntax (emphasis, code spans, links) stays in
/// the block text and is rendered by `MarkdownView` with
/// `AttributedString(markdown:)`.
enum MarkdownBlock: Equatable {
    case heading(level: Int, text: String)
    case paragraph(text: String)
    case bulletList(items: [MarkdownListItem])
    case orderedList(items: [MarkdownListItem], start: Int)
    case blockquote(blocks: [MarkdownBlock])
    case codeFence(language: String?, code: String)
    case table(headers: [String], rows: [[String]])
    case horizontalRule
}

struct MarkdownListItem: Equatable {
    /// Inline markdown of the item's first paragraph.
    var text: String
    /// Nested blocks (sub-lists, further paragraphs, code) indented under the item.
    var nested: [MarkdownBlock]

    init(text: String, nested: [MarkdownBlock] = []) {
        self.text = text
        self.nested = nested
    }
}

/// A tolerant, line-based CommonMark-ish block parser. It is written for
/// streaming model output: an unterminated fence is still a code block, a
/// half-written table is a paragraph until its delimiter row arrives, and
/// nothing ever throws.
enum MarkdownParser {
    static func parse(_ markdown: String) -> [MarkdownBlock] {
        let normalized = markdown.replacingOccurrences(of: "\r\n", with: "\n")
        let lines = normalized.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        return parseBlocks(lines)
    }

    // MARK: - Blocks

    private static func parseBlocks(_ lines: [String]) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        var i = 0
        let count = lines.count

        while i < count {
            let line = lines[i]
            if isBlank(line) {
                i += 1
                continue
            }

            if let fence = fenceOpening(line) {
                var code: [String] = []
                var j = i + 1
                var closed = false
                while j < count {
                    if isFenceClosing(lines[j], fence) {
                        closed = true
                        break
                    }
                    code.append(dropIndent(lines[j], columns: fence.indent))
                    j += 1
                }
                blocks.append(.codeFence(language: fence.language, code: code.joined(separator: "\n")))
                i = closed ? j + 1 : j
                continue
            }

            if let heading = atxHeading(line) {
                blocks.append(.heading(level: heading.level, text: heading.text))
                i += 1
                continue
            }

            if isHorizontalRule(line) {
                blocks.append(.horizontalRule)
                i += 1
                continue
            }

            if isBlockquote(line) {
                var inner: [String] = []
                var j = i
                while j < count, isBlockquote(lines[j]) {
                    inner.append(stripQuoteMarker(lines[j]))
                    j += 1
                }
                blocks.append(.blockquote(blocks: parseBlocks(inner)))
                i = j
                continue
            }

            if i + 1 < count, isTableRow(line), isTableDelimiter(lines[i + 1]) {
                let headers = tableCells(line)
                var rows: [[String]] = []
                var j = i + 2
                while j < count, !isBlank(lines[j]), isTableRow(lines[j]) {
                    var cells = tableCells(lines[j])
                    if cells.count < headers.count {
                        cells.append(contentsOf: Array(repeating: "", count: headers.count - cells.count))
                    } else if cells.count > headers.count {
                        cells = Array(cells.prefix(headers.count))
                    }
                    rows.append(cells)
                    j += 1
                }
                blocks.append(.table(headers: headers, rows: rows))
                i = j
                continue
            }

            if let marker = listMarker(line) {
                let parsed = parseList(lines, from: i, first: marker)
                blocks.append(parsed.block)
                i = parsed.next
                continue
            }

            // Paragraph: consecutive non-blank lines until another block starts.
            var paragraph: [String] = [line.trimmingCharacters(in: .whitespaces)]
            var j = i + 1
            var setextLevel: Int?
            while j < count {
                let next = lines[j]
                if isBlank(next) { break }
                if let level = setextUnderline(next) {
                    setextLevel = level
                    j += 1
                    break
                }
                if interruptsParagraph(next) { break }
                if j + 1 < count, isTableRow(next), isTableDelimiter(lines[j + 1]) { break }
                paragraph.append(next.trimmingCharacters(in: .whitespaces))
                j += 1
            }
            let text = paragraph.joined(separator: "\n")
            if let setextLevel {
                blocks.append(.heading(level: setextLevel, text: text))
            } else {
                blocks.append(.paragraph(text: text))
            }
            i = j
        }
        return blocks
    }

    // MARK: - Lists

    private struct ListMarker {
        let ordered: Bool
        let number: Int
        /// Column of the marker.
        let indent: Int
        /// Column where the item's content starts; nested lines are stripped to it.
        let contentIndent: Int
        let text: String
    }

    private static func listMarker(_ line: String) -> ListMarker? {
        let indentation = indent(of: line)
        let rest = line.dropFirst(indentation.chars)
        guard let first = rest.first else { return nil }

        if first == "-" || first == "*" || first == "+" {
            let after = rest.dropFirst()
            if after.isEmpty {
                return ListMarker(ordered: false, number: 0, indent: indentation.columns, contentIndent: indentation.columns + 2, text: "")
            }
            guard after.first == " " || after.first == "\t" else { return nil }
            let spaces = after.prefix(while: { $0 == " " || $0 == "\t" }).count
            let text = String(after.dropFirst(spaces))
            return ListMarker(ordered: false, number: 0, indent: indentation.columns,
                              contentIndent: indentation.columns + 1 + min(max(spaces, 1), 4), text: text)
        }

        if first.isASCII, first.isNumber {
            let digits = rest.prefix(while: { $0.isASCII && $0.isNumber })
            guard digits.count <= 9, let number = Int(digits) else { return nil }
            let afterDigits = rest.dropFirst(digits.count)
            guard let delimiter = afterDigits.first, delimiter == "." || delimiter == ")" else { return nil }
            let after = afterDigits.dropFirst()
            if after.isEmpty {
                return ListMarker(ordered: true, number: number, indent: indentation.columns,
                                  contentIndent: indentation.columns + digits.count + 2, text: "")
            }
            guard after.first == " " || after.first == "\t" else { return nil }
            let spaces = after.prefix(while: { $0 == " " || $0 == "\t" }).count
            let text = String(after.dropFirst(spaces))
            return ListMarker(ordered: true, number: number, indent: indentation.columns,
                              contentIndent: indentation.columns + digits.count + 1 + min(max(spaces, 1), 4), text: text)
        }
        return nil
    }

    private static func parseList(_ lines: [String], from start: Int, first: ListMarker) -> (block: MarkdownBlock, next: Int) {
        var items: [MarkdownListItem] = []
        var i = start
        let count = lines.count
        let ordered = first.ordered
        let baseIndent = first.indent
        var currentText: [String] = []
        var currentNested: [String] = []
        var hasCurrent = false
        var contentIndent = first.contentIndent
        var previousBlank = false

        func flush() {
            guard hasCurrent else { return }
            items.append(MarkdownListItem(text: currentText.joined(separator: "\n"), nested: parseBlocks(currentNested)))
            currentText = []
            currentNested = []
            hasCurrent = false
        }

        while i < count {
            let line = lines[i]

            if isBlank(line) {
                guard let nextIndex = nextNonBlank(lines, from: i + 1) else { break }
                let nextLine = lines[nextIndex]
                let nextIndent = indent(of: nextLine).columns
                if nextIndent > baseIndent {
                    currentNested.append("")
                    previousBlank = true
                    i += 1
                    continue
                }
                if let marker = listMarker(nextLine), marker.ordered == ordered {
                    previousBlank = true
                    i += 1
                    continue
                }
                break
            }

            let lineIndent = indent(of: line).columns
            if lineIndent > baseIndent, hasCurrent {
                currentNested.append(dropIndent(line, columns: min(lineIndent, contentIndent)))
                previousBlank = false
                i += 1
                continue
            }

            if let marker = listMarker(line) {
                if marker.ordered != ordered { break }
                flush()
                hasCurrent = true
                currentText = [marker.text]
                contentIndent = marker.contentIndent
                previousBlank = false
                i += 1
                continue
            }

            if hasCurrent, !previousBlank, !isBlockStart(line) {
                // Lazy continuation of the item's paragraph.
                currentText.append(line.trimmingCharacters(in: .whitespaces))
                i += 1
                continue
            }
            break
        }
        flush()
        let block: MarkdownBlock = ordered ? .orderedList(items: items, start: first.number) : .bulletList(items: items)
        return (block, i)
    }

    // MARK: - Fences

    private struct Fence {
        let character: Character
        let length: Int
        let indent: Int
        let language: String?
    }

    private static func fenceOpening(_ line: String) -> Fence? {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return nil }
        let rest = line.dropFirst(indentation.chars)
        guard let first = rest.first, first == "`" || first == "~" else { return nil }
        let run = rest.prefix(while: { $0 == first }).count
        guard run >= 3 else { return nil }
        let info = rest.dropFirst(run).trimmingCharacters(in: .whitespaces)
        if first == "`", info.contains("`") { return nil }
        let language = info.split(whereSeparator: { $0 == " " || $0 == "\t" }).first.map(String.init)
        return Fence(character: first, length: run, indent: indentation.columns, language: language)
    }

    private static func isFenceClosing(_ line: String, _ fence: Fence) -> Bool {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return false }
        let rest = line.dropFirst(indentation.chars)
        let run = rest.prefix(while: { $0 == fence.character }).count
        guard run >= fence.length else { return false }
        return rest.dropFirst(run).allSatisfy { $0 == " " || $0 == "\t" }
    }

    // MARK: - Headings and rules

    private static func atxHeading(_ line: String) -> (level: Int, text: String)? {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return nil }
        let rest = line.dropFirst(indentation.chars)
        let hashes = rest.prefix(while: { $0 == "#" }).count
        guard hashes >= 1, hashes <= 6 else { return nil }
        let after = rest.dropFirst(hashes)
        guard after.isEmpty || after.first == " " || after.first == "\t" else { return nil }
        var text = after.trimmingCharacters(in: .whitespaces)
        if text.hasSuffix("#") {
            let withoutClosing = String(text.reversed().drop(while: { $0 == "#" }).reversed())
            if withoutClosing.isEmpty || withoutClosing.hasSuffix(" ") || withoutClosing.hasSuffix("\t") {
                text = withoutClosing.trimmingCharacters(in: .whitespaces)
            }
        }
        return (hashes, text)
    }

    /// `===` → level 1, `---` → level 2 (a `-` underline needs three
    /// characters so a half-streamed list marker never flickers into a heading).
    private static func setextUnderline(_ line: String) -> Int? {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return nil }
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard let first = trimmed.first, first == "=" || first == "-", trimmed.allSatisfy({ $0 == first }) else { return nil }
        if first == "-", trimmed.count < 3 { return nil }
        return first == "=" ? 1 : 2
    }

    private static func isHorizontalRule(_ line: String) -> Bool {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return false }
        let characters = line.dropFirst(indentation.chars).filter { $0 != " " && $0 != "\t" }
        guard characters.count >= 3, let first = characters.first, first == "-" || first == "*" || first == "_" else { return false }
        return characters.allSatisfy { $0 == first }
    }

    // MARK: - Blockquotes

    private static func isBlockquote(_ line: String) -> Bool {
        let indentation = indent(of: line)
        guard indentation.columns <= 3 else { return false }
        return line.dropFirst(indentation.chars).first == ">"
    }

    private static func stripQuoteMarker(_ line: String) -> String {
        let indentation = indent(of: line)
        var rest = line.dropFirst(indentation.chars)
        if rest.first == ">" { rest = rest.dropFirst() }
        if rest.first == " " { rest = rest.dropFirst() }
        return String(rest)
    }

    // MARK: - Tables

    private static func isTableRow(_ line: String) -> Bool {
        line.contains("|")
    }

    private static func isTableDelimiter(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.contains("|"), trimmed.contains("-") else { return false }
        let cells = tableCells(trimmed)
        guard !cells.isEmpty else { return false }
        return cells.allSatisfy { cell in
            cell.contains("-") && cell.allSatisfy { $0 == "-" || $0 == ":" }
        }
    }

    /// Splits a row on unescaped pipes; `\|` becomes a literal pipe.
    private static func tableCells(_ line: String) -> [String] {
        var cells: [String] = []
        var current = ""
        var escaped = false
        for character in line {
            if escaped {
                if character == "|" {
                    current.append("|")
                } else {
                    current.append("\\")
                    current.append(character)
                }
                escaped = false
                continue
            }
            if character == "\\" {
                escaped = true
                continue
            }
            if character == "|" {
                cells.append(current)
                current = ""
                continue
            }
            current.append(character)
        }
        if escaped { current.append("\\") }
        cells.append(current)

        var trimmed = cells.map { $0.trimmingCharacters(in: .whitespaces) }
        let outer = line.trimmingCharacters(in: .whitespaces)
        if outer.hasPrefix("|"), !trimmed.isEmpty { trimmed.removeFirst() }
        if outer.hasSuffix("|"), !outer.hasSuffix("\\|"), let last = trimmed.last, last.isEmpty { trimmed.removeLast() }
        return trimmed
    }

    // MARK: - Helpers

    private static func isBlank(_ line: String) -> Bool {
        line.allSatisfy { $0 == " " || $0 == "\t" }
    }

    private static func isBlockStart(_ line: String) -> Bool {
        fenceOpening(line) != nil || atxHeading(line) != nil || isHorizontalRule(line) || isBlockquote(line) || listMarker(line) != nil
    }

    /// Like `isBlockStart`, but an empty list item ("-" alone, typically a
    /// half-streamed marker) does not interrupt a paragraph (CommonMark 5.2).
    private static func interruptsParagraph(_ line: String) -> Bool {
        if fenceOpening(line) != nil || atxHeading(line) != nil || isHorizontalRule(line) || isBlockquote(line) { return true }
        if let marker = listMarker(line) { return !marker.text.isEmpty }
        return false
    }

    private static func nextNonBlank(_ lines: [String], from start: Int) -> Int? {
        var i = start
        while i < lines.count {
            if !isBlank(lines[i]) { return i }
            i += 1
        }
        return nil
    }

    /// Leading whitespace as (columns with tabs expanded to 4, characters).
    private static func indent(of line: String) -> (columns: Int, chars: Int) {
        var columns = 0
        var chars = 0
        for character in line {
            if character == " " {
                columns += 1
            } else if character == "\t" {
                columns += 4 - (columns % 4)
            } else {
                break
            }
            chars += 1
        }
        return (columns, chars)
    }

    /// Removes up to `columns` columns of leading whitespace.
    private static func dropIndent(_ line: String, columns: Int) -> String {
        var column = 0
        var index = line.startIndex
        while index < line.endIndex, column < columns {
            let character = line[index]
            if character == " " {
                column += 1
            } else if character == "\t" {
                column += 4 - (column % 4)
            } else {
                break
            }
            index = line.index(after: index)
        }
        return String(line[index...])
    }
}
