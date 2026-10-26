import SwiftUI
import AppKit

/// `@` mentions in the composer, as in Claude Code and Codex: typing `@`
/// lists the installed apps (MCP servers, `mcp/status`) and the project's
/// files (`files/search`); ↑/↓ choose, Return or Tab inserts `@name ` or
/// `@path ` (`@"path" ` when it has spaces), Esc hides the list. The
/// backend reads the mentions from the sent text: apps' tools are preferred
/// for the turn, and mentioned files go to the model with the message
/// (docs/configuration.md "MCP servers and apps", "`@` mentions").
///
/// The bridge sits between the `NSTextView` (which finds the `@word` under
/// the cursor and forwards keys) and the SwiftUI menu (which lists the
/// matches and inserts the choice through the text view, so it is one
/// undoable edit that keeps the cursor where it was).
@MainActor
@Observable
final class MentionBridge {
    struct Query: Equatable {
        /// What follows the `@`.
        var text: String
        /// `@` through the cursor, in the text view's string.
        var range: NSRange
    }

    /// The `@word` being typed, if any.
    var query: Query?
    var selected = 0
    /// What accepting each row inserts, in menu order (set by the menu).
    @ObservationIgnored var choices: [String] = []
    /// Every installed app's name, for tinting mentions in the text.
    @ObservationIgnored var installed: [String] = []
    /// An `@` the user dismissed with Esc stays quiet until a new one is typed.
    @ObservationIgnored var dismissedLocation: Int?
    @ObservationIgnored weak var textView: NSTextView?

    var isOpen: Bool { query != nil && !choices.isEmpty }

    /// The `@word` ending at the cursor: a word (no whitespace) that starts
    /// with `@`, after any opening brackets or quotes; `@"…` for a quoted
    /// path. What follows the `@` is the query.
    func update(from textView: NSTextView) {
        let selection = textView.selectedRange()
        guard selection.length == 0, !textView.hasMarkedText() else { return set(nil) }
        let string = textView.string as NSString
        var start = selection.location
        while start > 0, let scalar = UnicodeScalar(string.character(at: start - 1)), !Character(scalar).isWhitespace {
            start -= 1
        }
        var at = start
        while at < selection.location, let scalar = UnicodeScalar(string.character(at: at)), "([{\"'`".contains(Character(scalar)) {
            at += 1
        }
        // `@`, or the full-width `＠` input methods type (accepting a
        // suggestion writes `@`).
        guard at < selection.location, [64, 0xFF20].contains(string.character(at: at)) else { return set(nil) }
        if dismissedLocation == at { return set(nil) }
        dismissedLocation = nil
        var text = string.substring(with: NSRange(location: at + 1, length: selection.location - at - 1))
        if text.hasPrefix("\"") { text.removeFirst() }
        guard !text.contains("@"), !text.contains("＠") else { return set(nil) }
        set(Query(text: text, range: NSRange(location: at, length: selection.location - at)))
    }

    private func set(_ query: Query?) {
        guard self.query != query else { return }
        if self.query?.range.location != query?.range.location { selected = 0 }
        self.query = query
    }

    func move(_ delta: Int) {
        guard !choices.isEmpty else { return }
        selected = (selected + delta + choices.count) % choices.count
    }

    func acceptSelected() {
        guard choices.indices.contains(selected) else { return }
        accept(choices[selected])
    }

    /// Replaces the `@word` with `insertion` (`@github `, `@src/a.rs `).
    func accept(_ insertion: String) {
        guard let textView, let query else { return }
        textView.insertText(insertion, replacementRange: query.range)
        self.query = nil
    }

    func dismiss() {
        dismissedLocation = query?.range.location
        query = nil
    }

    /// Tints `@name` for every installed app in the text view's storage.
    func highlight(in textView: NSTextView) {
        guard let storage = textView.textStorage, !textView.hasMarkedText() else { return }
        let full = NSRange(location: 0, length: storage.length)
        storage.beginEditing()
        storage.removeAttribute(.foregroundColor, range: full)
        storage.addAttribute(.foregroundColor, value: Theme.nsTextPrimary, range: full)
        var patterns: [String] = []
        if !installed.isEmpty {
            let names = installed.map(NSRegularExpression.escapedPattern(for:)).joined(separator: "|")
            patterns.append("(?<![^\\s(\\[{\"'`])@(?:\(names))(?![A-Za-z0-9_-])")
        }
        // Paths: a quoted one, or a word with a `/` or `.` inside.
        patterns.append("(?<![^\\s(\\[{\"'`])@(?:\"[^\"\\n]+\"|[^\\s@\"]*[/.][^\\s@\"]*[^\\s@\".,;:!?)\\]}])")
        for pattern in patterns {
            guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]) else { continue }
            for match in regex.matches(in: storage.string, range: full) {
                storage.addAttribute(.foregroundColor, value: Theme.nsAccent, range: match.range)
            }
        }
        storage.endEditing()
        textView.typingAttributes[.foregroundColor] = Theme.nsTextPrimary
    }
}

/// The list above the composer: installed apps matching the `@word`
/// being typed, then the project's files.
struct MentionMenu: View {
    @Environment(AppState.self) private var appState
    let bridge: MentionBridge
    let projectId: String

    @State private var files: [String] = []

    struct Item: Identifiable, Equatable {
        enum Kind: Equatable { case app(McpCatalogApp?), file }
        var kind: Kind
        /// App name or file path.
        var name: String
        var title: String
        var state: String
        var tint: Color
        /// What accepting the row inserts.
        var insertion: String
        var id: String { (kind == .file ? "file:" : "app:") + name }
    }

    var body: some View {
        let query = bridge.query?.text
        let items = query.map { Self.apps(for: $0, appState: appState) + Self.files(files, query: $0) }.map { Array($0.prefix(10)) } ?? []
        Group {
            if query != nil, !items.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                        if index == 0 || sectionTitle(item) != sectionTitle(items[index - 1]) {
                            Text(sectionTitle(item))
                                .font(Theme.Font.caption.weight(.medium))
                                .foregroundStyle(Theme.textTertiary)
                                .padding(.horizontal, 10)
                                .padding(.top, index == 0 ? 8 : 10)
                                .padding(.bottom, 4)
                        }
                        row(item, selected: index == min(bridge.selected, items.count - 1))
                            .contentShape(Rectangle())
                            .onTapGesture { bridge.accept(item.insertion) }
                            .onHover { if $0 { bridge.selected = index } }
                    }
                    Text("↑↓ choose · ⏎ insert · esc hide")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                }
                .padding(4)
                .frame(width: 380, alignment: .leading)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous).strokeBorder(Theme.border, lineWidth: 1))
                .floatingShadow()
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Mention suggestions")
            }
        }
        .onChange(of: items.map(\.insertion), initial: true) { _, choices in
            bridge.choices = choices
            if bridge.selected >= choices.count { bridge.selected = 0 }
        }
        .task(id: query) {
            guard let query else {
                files = []
                return
            }
            // A short pause so fast typing sends one search, not one per key.
            try? await Task.sleep(nanoseconds: 60_000_000)
            guard !Task.isCancelled, appState.connection.isReady else { return }
            let found = (try? await appState.backend.filesSearch(projectId: projectId, query: query)) ?? []
            guard !Task.isCancelled else { return }
            files = found
        }
    }

    private func sectionTitle(_ item: Item) -> String {
        if case .file = item.kind { return "Files" }
        return "Apps"
    }

    @ViewBuilder
    private func row(_ item: Item, selected: Bool) -> some View {
        HStack(spacing: 10) {
            switch item.kind {
            case .app(let app?):
                AppIcon(app: app, size: 24)
            case .app(nil):
                AppIcon(symbol: Icons.mcp, size: 24)
            case .file:
                Image(systemName: Icons.file)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .frame(width: 24, height: 24)
            }
            switch item.kind {
            case .file:
                let (directory, name) = Self.split(item.name)
                HStack(spacing: 6) {
                    Text(name)
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                    if !directory.isEmpty {
                        Text(directory)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.head)
                    }
                }
            case .app:
                VStack(alignment: .leading, spacing: 1) {
                    Text("@" + item.name)
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    if item.title != item.name {
                        Text(item.title)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                    }
                }
            }
            Spacer(minLength: 8)
            Text(item.state)
                .font(Theme.Font.caption)
                .foregroundStyle(item.tint)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .background(selected ? Theme.accent.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : .isButton)
    }

    /// `src/app/main.rs` → (`src/app`, `main.rs`).
    static func split(_ path: String) -> (directory: String, name: String) {
        guard let slash = path.lastIndex(of: "/") else { return ("", path) }
        return (String(path[..<slash]), String(path[path.index(after: slash)...]))
    }

    /// `@path ` or `@"path" ` (with spaces).
    static func insertion(forPath path: String) -> String {
        path.contains(where: \.isWhitespace) ? "@\"\(path)\" " : "@\(path) "
    }

    /// Installed apps matching `query`: name prefixes first, then names or
    /// titles containing it. Only for name-like queries; an app name typed
    /// in full closes the app part (Return sends).
    static func apps(for query: String, appState: AppState) -> [Item] {
        let q = query.lowercased()
        guard q.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "_" || $0 == "-" }) else { return [] }
        let all: [Item] = appState.mcpServers.map { status in
            let app = appState.mcpCatalog.first { $0.id == (status.app ?? status.name) }
            let (state, tint): (String, Color) = {
                switch status.state {
                case .connected:
                    let n = status.tools.filter(\.enabled).count
                    return ("\(n) tool\(n == 1 ? "" : "s")", Theme.textTertiary)
                case .connecting: return ("Connecting", Theme.warning)
                case .disabled: return ("Paused", Theme.textTertiary)
                case .failed: return ("Failed", Theme.danger)
                }
            }()
            return Item(kind: .app(app), name: status.name, title: app?.name ?? status.name, state: state, tint: tint, insertion: "@\(status.name) ")
        }
        if !q.isEmpty, all.contains(where: { $0.name.lowercased() == q }) { return [] }
        let prefix = all.filter { $0.name.lowercased().hasPrefix(q) }
        let contains = all.filter { !$0.name.lowercased().hasPrefix(q) && ($0.name.lowercased().contains(q) || $0.title.lowercased().contains(q)) }
        return prefix + contains
    }

    /// The backend's matches as rows; a path already typed in full needs no menu.
    static func files(_ paths: [String], query: String) -> [Item] {
        if paths.count == 1, paths[0] == query { return [] }
        return paths.map { Item(kind: .file, name: $0, title: $0, state: "", tint: Theme.textTertiary, insertion: insertion(forPath: $0)) }
    }
}
