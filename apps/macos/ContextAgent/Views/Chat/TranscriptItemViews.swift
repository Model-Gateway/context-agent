import SwiftUI
import AppKit
import ImageIO

// The plain transcript items: the user's bubble, the assistant's markdown
// message (with its hover toolbar and Thinking row) and the muted notices.
// Tool groups, approvals, validation rows and outcome cards live in their
// own files (V3); `TranscriptRow` in ChatView.swift dispatches to all of them.

// MARK: - User message

/// Right-aligned `surfaceRaised` bubble (max 70 % of the column, 16 radius),
/// timestamp and an Edit pencil on hover, context menu Copy / Resend / Edit
/// message… / Undo from this message…. While the message is being edited
/// (`chat.editingTurnId`) the bubble is an editor with Cancel / Send.
/// Runtime-injected messages (`isRuntime`) render as a muted notice instead.
struct UserMessageView: View {
    var turn: UserTurn
    /// Find-in-chat query; matching runs are highlighted.
    var highlight: String?
    /// The chat the message belongs to; nil disables editing (previews).
    var chat: ChatState?

    @Environment(AppState.self) private var appState
    @State private var hovering = false
    @FocusState private var editorFocused: Bool

    init(turn: UserTurn, highlight: String? = nil, chat: ChatState? = nil) {
        self.turn = turn
        self.highlight = highlight
        self.chat = chat
    }

    var body: some View {
        if turn.isRuntime {
            NoticeView(item: NoticeItem(lines: [turn.text], date: turn.date), highlight: highlight)
        } else if let chat, chat.editingTurnId == turn.id {
            editor(chat: chat)
        } else {
            VStack(alignment: .trailing, spacing: 2) {
                if let prompt = turn.sentPrompt {
                    SentPromptView(prompt: prompt)
                        .padding(.bottom, 4)
                }
                if !turn.images.isEmpty {
                    UserImageStrip(paths: turn.images)
                        .padding(.bottom, 4)
                }
                bubble
                // Under the bubble; it keeps its space while hidden so the
                // strip below the bubble is part of the hover area too.
                HStack(spacing: 6) {
                    Text(TranscriptTime.clock(turn.date))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                    if canEdit {
                        IconButton(Icons.edit, label: "Edit message", size: 12) {
                            if let chat { appState.beginEdit(turn: turn, in: chat) }
                        }
                        .help("Edit this message and send it again (later messages are removed)")
                    }
                }
                .opacity(hovering ? 1 : 0)
                .accessibilityHidden(!hovering)
            }
            .contentShape(Rectangle())
            .onHover { inside in
                withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
            }
            .frame(maxWidth: .infinity, alignment: .trailing)
            .accessibilityElement(children: .contain)
            .accessibilityLabel("You, \(TranscriptTime.clock(turn.date))")
        }
    }

    /// Editing needs the persisted message id and an idle project.
    private var canEdit: Bool {
        guard let chat, turn.isRewindable else { return false }
        return appState.canRewind(chat)
    }

    private var bubble: some View {
        Text(TranscriptHighlight.attributed(turn.text, query: highlight))
            .font(Theme.Font.body)
            .foregroundStyle(Theme.textPrimary)
            .lineSpacing(3)
            .fixedSize(horizontal: false, vertical: true)
            .selectableText()
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous))
            .frame(maxWidth: Theme.Metrics.contentMaxWidth * 0.7, alignment: .trailing)
            .contextMenu {
                Button("Copy") { appState.copyToPasteboard(turn.text) }
                Button("Resend") {
                    let turn = turn
                    Task { await appState.send(OutgoingPrompt(text: turn.text, images: turn.images, files: turn.files)) }
                }
                if let chat {
                    Divider()
                    Button("Edit message…") { appState.beginEdit(turn: turn, in: chat) }
                        .disabled(!canEdit)
                    Button("Undo from this message…") { appState.undo(from: turn, in: chat) }
                        .disabled(!canEdit)
                }
            }
    }

    /// The bubble as an editor: Esc cancels, ⌘⏎ (or Send) asks keep-or-revert and re-sends.
    private func editor(chat: ChatState) -> some View {
        @Bindable var chat = chat
        return VStack(alignment: .trailing, spacing: 8) {
            TextEditor(text: $chat.editingText)
                .font(Theme.Font.body)
                .foregroundStyle(Theme.textPrimary)
                .scrollContentBackground(.hidden)
                .focused($editorFocused)
                .frame(minHeight: 56, maxHeight: 240)
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous)
                        .strokeBorder(Theme.accent, lineWidth: 1)
                )
                .onExitCommand { appState.cancelEdit(in: chat) }
            HStack(spacing: 8) {
                Text("Sending removes this message's reply and everything after it.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                Spacer(minLength: 8)
                Button("Cancel") { appState.cancelEdit(in: chat) }
                Button("Send") { appState.submitEdit(in: chat) }
                    .buttonStyle(.borderedProminent)
                    .disabled(chat.editingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .help("Send the edited message (⌘⏎)")
            }
            .controlSize(.small)
        }
        .frame(maxWidth: Theme.Metrics.contentMaxWidth * 0.7, alignment: .trailing)
        .frame(maxWidth: .infinity, alignment: .trailing)
        .onAppear { editorFocused = true }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Editing your message")
    }
}

/// The images sent with a user message, as thumbnails above the bubble:
/// click opens one in the default viewer. A file deleted since (pasted images
/// are pruned after a month) shows a placeholder.
private struct UserImageStrip: View {
    let paths: [String]

    var body: some View {
        HStack(spacing: 6) {
            ForEach(paths, id: \.self) { path in
                UserImageThumbnail(path: path)
            }
        }
        .frame(maxWidth: Theme.Metrics.contentMaxWidth * 0.7, alignment: .trailing)
    }
}

private struct UserImageThumbnail: View {
    let path: String

    static let height: CGFloat = 96

    @State private var image: NSImage?
    @State private var missing = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Group {
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: Self.height * 2, maxHeight: Self.height)
                    .clipShape(shape)
                    .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
                    .onTapGesture { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
                    .help(path)
            } else {
                shape.fill(Theme.surfaceRaised)
                    .overlay {
                        if missing {
                            Image(systemName: "photo")
                                .foregroundStyle(Theme.textTertiary)
                        }
                    }
                    .frame(width: Self.height, height: Self.height)
                    .help(missing ? "The image is no longer available" : path)
            }
        }
        .accessibilityLabel(missing ? "Image, no longer available" : "Image \((path as NSString).lastPathComponent)")
        .task(id: path) {
            let url = URL(fileURLWithPath: path)
            let loaded = await Task.detached(priority: .utility) { () -> NSImage? in
                guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
                let options: [CFString: Any] = [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: Int(Self.height * 4),
                ]
                guard let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
                return NSImage(cgImage: cgImage, size: NSSize(width: cgImage.width, height: cgImage.height))
            }.value
            image = loaded
            missing = loaded == nil
        }
    }
}

// MARK: - Assistant message

/// Left-aligned markdown without a bubble. A hover toolbar (top-right)
/// carries the timestamp and a `CopyButton` for the raw markdown; the context
/// menu offers Copy as Markdown / Copy as plain text. While streaming a
/// Braille spinner trails the text, and a `ThinkingView` sits above the text
/// whenever the turn exposed reasoning.
struct AssistantMessageView: View {
    var turn: AssistantTurn

    @Environment(AppState.self) private var appState
    @State private var hovering = false

    init(turn: AssistantTurn) {
        self.turn = turn
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !turn.reasoning.isEmpty {
                ThinkingView(turn: turn)
            }
            if !turn.text.isEmpty {
                if isStreamingText, canInlineSpinner {
                    StreamingMarkdown(text: turn.text)
                } else {
                    MarkdownView(turn.text)
                }
            }
            if isStreamingText, turn.text.isEmpty || !canInlineSpinner {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 13, tint: Theme.textSecondary)
                    if turn.text.isEmpty {
                        Text("Writing…")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                .padding(.top, 2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.trailing, 8)
        // The whole message width counts as hovered, not only the glyphs:
        // otherwise moving across the blank space towards the toolbar ends
        // the hover and hides the Copy button before it can be clicked.
        .contentShape(Rectangle())
        .overlay(alignment: .topTrailing) {
            if !turn.isStreaming {
                // Invisible until hovered, but always in the accessibility
                // tree so Copy as Markdown is reachable without a mouse.
                toolbar
                    .opacity(hovering ? 1 : 0)
                    .offset(y: -6)
            }
        }
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .contextMenu {
            Button("Copy as Markdown") { appState.copyToPasteboard(turn.text) }
            Button("Copy as plain text") { appState.copyToPasteboard(TranscriptPlainText.strip(turn.text)) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Assistant, \(TranscriptTime.clock(turn.date))")
    }

    private var isStreamingText: Bool { turn.isStreaming && !turn.isThinking }

    /// The spinner trails the text (contract) by riding along as the last
    /// glyph of the markdown; inside an open code fence it would land in the
    /// code block, so there it falls back to its own line.
    private var canInlineSpinner: Bool {
        (turn.text.components(separatedBy: "```").count - 1) % 2 == 0
    }

    private var toolbar: some View {
        HStack(spacing: 6) {
            Text(TranscriptTime.clock(turn.date))
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .padding(.leading, 6)
            CopyButton(text: turn.text, label: "Copy as Markdown")
        }
        .padding(2)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
    }
}

/// Streaming markdown with the Braille frame appended to the text, so the
/// spinner sits right after the last word (static ⠿ with Reduce Motion).
private struct StreamingMarkdown: View {
    var text: String

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if reduceMotion {
            MarkdownView(text + " ⠿")
        } else {
            TimelineView(.periodic(from: .now, by: BrailleSpinner.frameInterval)) { context in
                MarkdownView(text + " " + BrailleSpinner.frame(at: context.date))
            }
        }
    }
}

// MARK: - Notice

/// Muted 12.5 pt lines (`textTertiary`); `danger` for errors. Low-value
/// `detailLines` (task boundary notes, checkpoints) sit behind a disclosure.
struct NoticeView: View {
    var item: NoticeItem
    /// Find-in-chat query; matching runs are highlighted.
    var highlight: String?

    @State private var detailsExpanded = false

    init(item: NoticeItem, highlight: String? = nil) {
        self.item = item
        self.highlight = highlight
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(Array(item.lines.enumerated()), id: \.offset) { entry in
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    if item.isError {
                        Image(systemName: Icons.warning)
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(Theme.danger)
                            .accessibilityLabel("Error")
                    }
                    line(entry.element)
                }
            }
            if !item.detailLines.isEmpty {
                DisclosureRow(isExpanded: $detailsExpanded) {
                    Text(detailsLabel)
                        .font(Self.font)
                        .foregroundStyle(Theme.textTertiary)
                } content: {
                    VStack(alignment: .leading, spacing: 3) {
                        ForEach(Array(item.detailLines.enumerated()), id: \.offset) { entry in
                            line(entry.element)
                        }
                    }
                    .padding(.leading, 6)
                    .padding(.bottom, 2)
                }
                .padding(.leading, -6)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
    }

    private static let font: Font = .system(size: 12.5)

    private var tint: Color { item.isError ? Theme.danger : Theme.textTertiary }

    /// "2 more notes" under visible lines; with nothing above, the disclosure
    /// is the whole notice and says so.
    private var detailsLabel: String {
        let count = item.detailLines.count
        if item.lines.isEmpty { return "Run notes · \(count)" }
        return "\(count) more note\(count == 1 ? "" : "s")"
    }

    private func line(_ text: String) -> some View {
        Text(TranscriptHighlight.attributed(text, query: highlight))
            .font(Self.font)
            .foregroundStyle(tint)
            .fixedSize(horizontal: false, vertical: true)
            .selectableText()
    }
}

// MARK: - Helpers shared by the transcript views

enum TranscriptTime {
    /// "3:42 PM"
    static func clock(_ date: Date) -> String {
        date.formatted(date: .omitted, time: .shortened)
    }

    /// "Today", "Yesterday", "Oct 12", "Oct 12, 2024".
    static func dayLabel(_ date: Date, now: Date = Date()) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) { return "Today" }
        if calendar.isDateInYesterday(date) { return "Yesterday" }
        if calendar.isDate(date, equalTo: now, toGranularity: .year) {
            return date.formatted(.dateTime.month(.abbreviated).day())
        }
        return date.formatted(.dateTime.month(.abbreviated).day().year())
    }
}

/// Find-in-chat highlighting for plain text.
enum TranscriptHighlight {
    static let matchOptions: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]

    /// `text` with every case-insensitive occurrence of `query` on a `warning` background.
    static func attributed(_ text: String, query: String?) -> AttributedString {
        guard let query, !query.isEmpty else { return AttributedString(text) }
        var result = AttributedString()
        var cursor = text.startIndex
        while cursor < text.endIndex,
              let range = text.range(of: query, options: matchOptions, range: cursor..<text.endIndex),
              !range.isEmpty {
            if cursor < range.lowerBound {
                result.append(AttributedString(String(text[cursor..<range.lowerBound])))
            }
            var hit = AttributedString(String(text[range]))
            hit.backgroundColor = Theme.warning.opacity(0.32)
            result.append(hit)
            cursor = range.upperBound
        }
        if cursor < text.endIndex {
            result.append(AttributedString(String(text[cursor...])))
        }
        return result
    }

    /// Number of case-insensitive occurrences of `query` in `text`.
    static func occurrences(of query: String, in text: String) -> Int {
        guard !query.isEmpty else { return 0 }
        var count = 0
        var cursor = text.startIndex
        while cursor < text.endIndex,
              let range = text.range(of: query, options: matchOptions, range: cursor..<text.endIndex),
              !range.isEmpty {
            count += 1
            cursor = range.upperBound
        }
        return count
    }
}

/// "Copy as plain text": a conservative markdown stripper (emphasis, code
/// spans, headings, links, fences, quote markers). Lists keep their markers.
enum TranscriptPlainText {
    static func strip(_ markdown: String) -> String {
        var out: [String] = []
        var inFence = false
        for rawLine in markdown.split(separator: "\n", omittingEmptySubsequences: false) {
            var line = String(rawLine)
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~") {
                inFence.toggle()
                continue
            }
            if inFence {
                out.append(line)
                continue
            }
            if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                out.append("")
                continue
            }
            line = replacing(line, pattern: #"^\s{0,3}#{1,6}\s+"#, with: "")
            line = replacing(line, pattern: #"^\s{0,3}>\s?"#, with: "")
            line = replacing(line, pattern: #"!?\[([^\]]*)\]\([^)]*\)"#, with: "$1")
            line = replacing(line, pattern: #"`([^`]*)`"#, with: "$1")
            line = replacing(line, pattern: #"\*\*([^*]+)\*\*"#, with: "$1")
            line = replacing(line, pattern: #"__([^_]+)__"#, with: "$1")
            line = replacing(line, pattern: #"(?<![\w*])\*([^*\n]+)\*(?![\w*])"#, with: "$1")
            line = replacing(line, pattern: #"(?<![\w_])_([^_\n]+)_(?![\w_])"#, with: "$1")
            out.append(line)
        }
        return out.joined(separator: "\n")
    }

    private static func replacing(_ text: String, pattern: String, with template: String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.stringByReplacingMatches(in: text, range: range, withTemplate: template)
    }
}

// MARK: - Previews

#Preview("Transcript items") {
    let appState = AppState()
    let user = UserTurn(text: "Fix the failing test in cards.rs and run cargo test to verify", date: Date().addingTimeInterval(-600))
    let runtime = UserTurn(text: "[Context runtime] The previous task was blocked; continuing with the unresolved items.", isRuntime: true)
    let assistant = AssistantTurn(
        text: "The comparator sorted by `created_at` while the fixture only sets `updated_at`.\n\n- Edited `src/cards.rs`\n- Ran `cargo test` (14 passed)\n\n```rust\ncards.sort_by_key(|c| c.updated_at);\n```",
        reasoning: "The failing assertion compares the order of the fixture cards.",
        isStreaming: false,
        isThinking: false,
        step: 3,
        thinkingDuration: 1.2
    )
    let streaming = AssistantTurn(text: "Looking at the failing test now", isStreaming: true, step: 1)
    let notice = NoticeItem(lines: ["Approval mode set to Auto"], detailLines: ["checkpoint: before apply_patch src/cards.rs", "task boundary: continuing task 11"])
    let error = NoticeItem(lines: ["authentication failed: invalid API key"], isError: true)
    return ScrollView {
        VStack(alignment: .leading, spacing: 20) {
            UserMessageView(turn: user, highlight: "cargo")
            UserMessageView(turn: runtime)
            AssistantMessageView(turn: assistant)
            AssistantMessageView(turn: streaming)
            NoticeView(item: notice)
            NoticeView(item: error)
        }
        .padding(24)
        .frame(width: 720)
    }
    .frame(height: 640)
    .background(Theme.canvas)
    .environment(appState)
}
