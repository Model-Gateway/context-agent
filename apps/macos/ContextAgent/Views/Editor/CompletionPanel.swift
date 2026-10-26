import SwiftUI
import AppKit

/// One row of the completion popup.
struct CompletionItem: Identifiable, Hashable {
    enum Source: Hashable {
        /// The language server's item (`lsp/completion`).
        case server(kind: String)
        /// A definition the index knows (`symbols/find`), or one found in the
        /// buffer itself (unsaved yet).
        case symbol(kind: String, path: String, line: Int)
        case keyword
        /// A word already in the buffer.
        case word
    }

    let label: String
    let detail: String?
    let source: Source
    /// What goes into the text; the label unless the server said otherwise.
    var insertText: String
    /// The server's replace range (0-based) when it gave one.
    var replaceRange: LspRange? = nil

    var id: String {
        switch source {
        case .server(let kind): return "server:\(kind):\(label):\(insertText)"
        case .symbol(_, let path, let line): return "symbol:\(path):\(line):\(label)"
        case .keyword: return "keyword:\(label)"
        case .word: return "word:\(label)"
        }
    }

    init(server item: LspCompletionItem) {
        label = item.label
        detail = item.detail ?? (item.kind == "text" ? nil : item.kind.replacingOccurrences(of: "_", with: " "))
        source = .server(kind: item.kind)
        insertText = item.insertText
        replaceRange = item.range
    }

    init(symbol: SymbolInfo) {
        label = symbol.name
        detail = symbol.signature ?? (symbol.parent.map { "\(symbol.kind) in \($0)" } ?? symbol.kind)
        source = .symbol(kind: symbol.kind, path: symbol.path, line: symbol.line)
        insertText = symbol.name
    }

    /// A definition spotted in the buffer (`fn name`, `class Name`, …).
    init(bufferDefinition name: String, kind: String, signature: String, path: String, line: Int) {
        label = name
        detail = signature
        source = .symbol(kind: kind, path: path, line: line)
        insertText = name
    }

    init(keyword: String) {
        label = keyword
        detail = "keyword"
        source = .keyword
        insertText = keyword
    }

    init(word: String) {
        label = word
        detail = nil
        source = .word
        insertText = word
    }

    var systemImage: String {
        switch source {
        case .server(let kind): return Self.systemImage(forSymbolKind: kind)
        case .symbol(let kind, _, _): return Self.systemImage(forSymbolKind: kind)
        case .keyword: return "textformat"
        case .word: return "text.word.spacing"
        }
    }

    var path: String? {
        if case .symbol(_, let path, _) = source { return path }
        return nil
    }

    /// `label` or the server's `filterText`, what the typed prefix is matched against.
    var filterText: String { label }

    static func systemImage(forSymbolKind kind: String) -> String {
        switch kind {
        case "function", "method", "macro", "constructor": return "function"
        case "struct", "class", "interface", "trait", "impl", "component", "type_parameter": return "shippingbox"
        case "enum", "enum_member": return "list.bullet"
        case "type_alias": return "t.square"
        case "variable", "field", "constant", "property", "value": return "v.square"
        case "module", "namespace", "section", "file", "folder": return "folder"
        case "keyword": return "textformat"
        case "snippet": return "text.insert"
        case "text": return "text.word.spacing"
        default: return "circle.hexagongrid"
        }
    }
}

/// The popup's rows and selection, shared with the SwiftUI list.
@MainActor
@Observable
final class CompletionModel {
    var prefix = ""
    var items: [CompletionItem] = []
    var selectedIndex = 0
    var onPick: ((Int) -> Void)?
}

/// Shows candidates under the word being typed in a borderless child panel
/// of the text view's window; the editor forwards ↑ ↓ ⏎ ⇥ Esc while it is
/// up. Local candidates (keywords, words of the buffer) show at once and the
/// index's symbols are merged in when `remote` returns, symbols first.
@MainActor
final class CompletionController {
    weak var textView: NSTextView?
    let model = CompletionModel()
    private(set) var wordRange = NSRange(location: 0, length: 0)
    var onAccept: ((CompletionItem, NSRange) -> Void)?

    private var panel: NSPanel?
    private var remoteTask: Task<Void, Never>?
    private var generation = 0
    private var localItems: [CompletionItem] = []
    private var remoteItems: [CompletionItem] = []

    private static let rowHeight: CGFloat = 24
    private static let maxRows = 8
    private static let width: CGFloat = 440

    init() {
        model.onPick = { [weak self] index in
            self?.model.selectedIndex = index
            self?.accept()
        }
    }

    var isVisible: Bool { panel?.isVisible == true }

    func present(prefix: String, wordRange: NSRange, local: [CompletionItem], remote: @escaping () async -> [CompletionItem]) {
        let prefixChanged = model.prefix != prefix
        self.wordRange = wordRange
        model.prefix = prefix
        generation += 1
        let generation = generation
        localItems = local
        // Items from the previous prefix still fit while the user keeps typing it.
        remoteItems = remoteItems.filter { prefix.isEmpty || $0.label.lowercased().hasPrefix(prefix.lowercased()) }
        show(resetSelection: prefixChanged)
        remoteTask?.cancel()
        remoteTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 60_000_000)
            guard !Task.isCancelled else { return }
            let symbols = await remote()
            guard !Task.isCancelled, let self, self.generation == generation else { return }
            remoteItems = symbols
            show(resetSelection: false)
        }
    }

    func move(by delta: Int) {
        guard !model.items.isEmpty else { return }
        model.selectedIndex = (model.selectedIndex + delta + model.items.count) % model.items.count
    }

    func accept() {
        guard model.items.indices.contains(model.selectedIndex) else {
            dismiss()
            return
        }
        let item = model.items[model.selectedIndex]
        let range = wordRange
        dismiss()
        onAccept?(item, range)
    }

    func dismiss() {
        remoteTask?.cancel()
        generation += 1
        guard let panel else { return }
        panel.parent?.removeChildWindow(panel)
        panel.orderOut(nil)
    }

    // MARK: Rows and panel

    private func show(resetSelection: Bool) {
        let prefix = model.prefix
        var seen: Set<String> = []
        var items: [CompletionItem] = []
        for item in remoteItems + localItems {
            let key = item.label + (item.path ?? "")
            guard seen.insert(key).inserted else { continue }
            items.append(item)
        }
        // Case-sensitive prefix matches before the case-insensitive ones, stably.
        let exact = items.filter { $0.label.hasPrefix(prefix) }
        let loose = items.filter { !$0.label.hasPrefix(prefix) }
        items = Array((exact + loose).prefix(80))
        guard !items.isEmpty else {
            dismiss()
            return
        }
        let previous = model.items.indices.contains(model.selectedIndex) ? model.items[model.selectedIndex].id : nil
        model.items = items
        if resetSelection {
            model.selectedIndex = 0
        } else {
            model.selectedIndex = items.firstIndex { $0.id == previous } ?? 0
        }
        place()
    }

    private func place() {
        guard let textView, let window = textView.window else { return }
        let panel = panel ?? makePanel()
        let rows = min(model.items.count, Self.maxRows)
        let height = CGFloat(rows) * Self.rowHeight + 10
        var anchor = textView.firstRect(forCharacterRange: wordRange, actualRange: nil)
        if anchor.isEmpty {
            anchor = textView.firstRect(forCharacterRange: NSRange(location: wordRange.location, length: 0), actualRange: nil)
        }
        let screen = window.screen?.visibleFrame ?? NSScreen.main?.visibleFrame ?? .zero
        var x = anchor.minX - 30
        x = max(screen.minX, min(x, screen.maxX - Self.width))
        var y = anchor.minY - height - 4
        if y < screen.minY { y = anchor.maxY + 4 }
        panel.setFrame(NSRect(x: x, y: y, width: Self.width, height: height), display: true)
        if panel.parent == nil {
            window.addChildWindow(panel, ordered: .above)
        }
        panel.orderFront(nil)
    }

    private func makePanel() -> NSPanel {
        let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: Self.width, height: Self.rowHeight), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.isFloatingPanel = true
        panel.level = .popUpMenu
        panel.hasShadow = true
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hidesOnDeactivate = true
        panel.becomesKeyOnlyIfNeeded = true
        panel.isReleasedWhenClosed = false
        let hosting = NSHostingView(rootView: CompletionListView(model: model))
        hosting.sizingOptions = []
        panel.contentView = hosting
        self.panel = panel
        return panel
    }
}

// MARK: - List

private struct CompletionListView: View {
    var model: CompletionModel

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                LazyVStack(spacing: 0) {
                    ForEach(Array(model.items.enumerated()), id: \.element.id) { index, item in
                        CompletionRow(item: item, prefix: model.prefix, selected: index == model.selectedIndex)
                            .id(item.id)
                            .onTapGesture { model.onPick?(index) }
                    }
                }
                .padding(.vertical, 5)
            }
            .scrollIndicators(.never)
            .onChange(of: model.selectedIndex) { _, index in
                if model.items.indices.contains(index) { proxy.scrollTo(model.items[index].id) }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous).strokeBorder(Theme.borderStrong, lineWidth: 1))
        .padding(1)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Completions")
    }
}

private struct CompletionRow: View {
    var item: CompletionItem
    var prefix: String
    var selected: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: item.systemImage)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(selected ? Theme.textPrimary : Theme.accent)
                .frame(width: 16)
                .accessibilityHidden(true)
            label
                .font(.system(size: 12.5, design: .monospaced))
                .lineLimit(1)
            if let detail = item.detail {
                Text(verbatim: detail)
                    .font(.system(size: 11.5, design: .monospaced))
                    .foregroundStyle(selected ? Theme.textPrimary.opacity(0.75) : Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            Spacer(minLength: 8)
            if let path = item.path {
                Text(verbatim: path)
                    .font(.system(size: 11))
                    .foregroundStyle(selected ? Theme.textPrimary.opacity(0.6) : Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.head)
                    .frame(maxWidth: 160, alignment: .trailing)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 24)
        .background(selected ? Theme.accentSoft : Color.clear, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
        .padding(.horizontal, 4)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(item.label)\(item.detail.map { ", \($0)" } ?? "")")
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    /// The typed prefix in bold.
    private var label: Text {
        if item.label.lowercased().hasPrefix(prefix.lowercased()) {
            let head = String(item.label.prefix(prefix.count))
            let tail = String(item.label.dropFirst(prefix.count))
            return Text("\(Text(verbatim: head).bold())\(Text(verbatim: tail))").foregroundColor(Theme.textPrimary)
        }
        return Text(verbatim: item.label).foregroundColor(Theme.textPrimary)
    }
}
