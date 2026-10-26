import SwiftUI
import AppKit

/// The editable text of one `EditorDocument`: a TextKit 1 `NSTextView`
/// (line numbers need the layout manager) with tree-sitter colouring
/// (`TreeSitterHighlighter`) where a grammar exists, else line-based colouring
/// from `SyntaxHighlighter`, auto-indentation, the completion popup
/// (`CompletionController`: the language server's items when the project
/// has one, else the index's symbols, definitions spotted in the buffer,
/// keywords and words of the buffer), the server's diagnostics underlined,
/// and ⌘-click to the definition of an identifier.
struct CodeEditor: NSViewRepresentable {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var document: EditorDocument

    func makeCoordinator() -> Coordinator {
        Coordinator(document: document)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let coordinator = context.coordinator
        coordinator.appState = appState
        coordinator.project = project

        let storage = NSTextStorage()
        let layoutManager = NSLayoutManager()
        storage.addLayoutManager(layoutManager)
        let container = NSTextContainer(size: NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        layoutManager.addTextContainer(container)

        let textView = CodeTextView(frame: .zero, textContainer: container)
        textView.isEditable = true
        textView.isRichText = false
        textView.allowsUndo = true
        textView.usesFindBar = true
        textView.isIncrementalSearchingEnabled = true
        textView.drawsBackground = false
        textView.font = Theme.Font.nsMono
        textView.textColor = Theme.nsTextPrimary
        textView.insertionPointColor = Theme.nsTextPrimary
        textView.textContainerInset = NSSize(width: 8, height: 10)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isGrammarCheckingEnabled = false
        textView.smartInsertDeleteEnabled = false
        textView.isHorizontallyResizable = true
        textView.isVerticallyResizable = true
        textView.autoresizingMask = [.width]
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.minSize = .zero
        textView.delegate = coordinator
        storage.delegate = coordinator

        let scrollView = NSScrollView()
        scrollView.documentView = textView
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.borderType = .noBorder
        scrollView.drawsBackground = false

        let ruler = LineNumberRuler(scrollView: scrollView, textView: textView)
        scrollView.verticalRulerView = ruler
        scrollView.hasVerticalRuler = true
        scrollView.rulersVisible = true

        coordinator.attach(textView: textView, ruler: ruler, scrollView: scrollView)
        coordinator.replaceText(document.text)
        coordinator.appliedRevision = document.revision
        DispatchQueue.main.async {
            textView.window?.makeFirstResponder(textView)
            coordinator.revealPendingLine()
        }
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.appState = appState
        coordinator.project = project
        if coordinator.appliedRevision != document.revision {
            coordinator.replaceText(document.text)
            coordinator.appliedRevision = document.revision
        }
        coordinator.textView?.isEditable = !document.truncated
        if coordinator.appliedDiagnostics != document.diagnostics {
            coordinator.applyDiagnostics(document.diagnostics)
        }
        if document.pendingLine != nil {
            DispatchQueue.main.async { coordinator.revealPendingLine() }
        }
    }

    // MARK: - Coordinator

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate, NSTextStorageDelegate {
        let document: EditorDocument
        weak var appState: AppState?
        weak var project: ProjectState?
        weak var textView: CodeTextView?
        weak var ruler: LineNumberRuler?
        private weak var scrollView: NSScrollView?
        private var lastRulerOriginY: CGFloat = 0
        let completion = CompletionController()
        var appliedRevision = -1
        var appliedDiagnostics: [LspDiagnostic] = []

        /// Set while the text is replaced from the document, so the change
        /// is not fed back as an edit.
        private var replacing = false
        /// Characters edited since the last highlight pass.
        private var pendingEdit: NSRange?
        /// Whether the edit in flight is one typed identifier character.
        private var typedIdentifierCharacter = false
        /// The typed character when it is one of the server's completion triggers.
        private var typedTrigger: String?
        /// "\t", "  " or "    ", from the file's own indentation.
        private var indentUnit = "    "
        private var colors: [SyntaxHighlighter.TokenKind: NSColor] = [:]
        /// Set when the language has a grammar; the regex pass is then skipped.
        private var treeSitter: TreeSitterHighlighter?
        private var symbolLookup: Task<Void, Never>?
        private var lastSymbolWord: String?
        /// Identifiers and definitions in the buffer for completion, rebuilt
        /// at most once a second.
        private var bufferWords: Set<String> = []
        private var bufferDefinitions: [CompletionItem] = []
        private var bufferScanStamp: Date = .distantPast
        private var bufferScanDirty = true

        init(document: EditorDocument) {
            self.document = document
        }

        func attach(textView: CodeTextView, ruler: LineNumberRuler, scrollView: NSScrollView) {
            self.textView = textView
            self.ruler = ruler
            textView.onAppearanceChange = { [weak self] in
                self?.colors = [:]
                self?.highlightAll()
            }
            textView.onCommandClick = { [weak self] index in
                self?.jumpToDefinition(at: index)
            }
            if let grammar = TreeSitterGrammars.name(forPath: document.path, language: document.language) {
                treeSitter = TreeSitterHighlighter(textView: textView, grammar: grammar) { [weak self] kind in
                    self?.color(for: kind) ?? Theme.nsTextPrimary
                }
            }
            completion.textView = textView
            completion.onAccept = { [weak self] item, range in
                self?.insert(item, replacing: range)
            }
            self.scrollView = scrollView
            lastRulerOriginY = scrollView.contentView.bounds.origin.y
            NotificationCenter.default.addObserver(forName: NSView.boundsDidChangeNotification, object: scrollView.contentView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.clipBoundsChanged() }
            }
            NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: scrollView.contentView, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.treeSitter?.visibleRangeChanged() }
            }
        }

        /// The gutter follows vertical scrolling only; a horizontal scroll
        /// leaves the line numbers where they are.
        private func clipBoundsChanged() {
            guard let scrollView else { return }
            treeSitter?.visibleRangeChanged()
            let originY = scrollView.contentView.bounds.origin.y
            guard originY != lastRulerOriginY else { return }
            lastRulerOriginY = originY
            ruler?.needsDisplay = true
        }

        // MARK: Text in and out

        func replaceText(_ text: String) {
            guard let textView, let storage = textView.textStorage else { return }
            replacing = true
            let selection = textView.selectedRange()
            storage.beginEditing()
            storage.replaceCharacters(in: NSRange(location: 0, length: storage.length), with: text)
            storage.setAttributes(baseAttributes, range: NSRange(location: 0, length: storage.length))
            storage.endEditing()
            textView.typingAttributes = baseAttributes
            textView.undoManager?.removeAllActions()
            indentUnit = Self.detectIndent(in: text)
            replacing = false
            pendingEdit = nil
            bufferScanDirty = true
            highlightAll()
            ruler?.invalidateLines()
            let length = (text as NSString).length
            textView.setSelectedRange(NSRange(location: min(selection.location, length), length: 0))
            applyDiagnostics(document.diagnostics)
            updateCursor()
        }

        private var baseAttributes: [NSAttributedString.Key: Any] {
            [.font: Theme.Font.nsMono, .foregroundColor: Theme.nsTextPrimary]
        }

        func revealPendingLine() {
            guard let line = document.pendingLine, let textView, let ruler else { return }
            document.pendingLine = nil
            let start = ruler.characterIndex(ofLine: line)
            textView.setSelectedRange(NSRange(location: start, length: 0))
            guard let layoutManager = textView.layoutManager, let container = textView.textContainer, let scrollView = textView.enclosingScrollView else { return }
            layoutManager.ensureLayout(for: container)
            let glyph = layoutManager.glyphIndexForCharacter(at: min(start, max(0, layoutManager.numberOfGlyphs - 1)))
            let rect = layoutManager.numberOfGlyphs == 0 ? .zero : layoutManager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
            let y = max(0, rect.midY + textView.textContainerInset.height - scrollView.contentView.bounds.height / 2)
            // The ruler floats over the clip view, whose left edge is at
            // `-contentInsets.left`; x = 0 would hide the first columns under it.
            scrollView.contentView.scroll(to: NSPoint(x: -scrollView.contentView.contentInsets.left, y: y))
            scrollView.reflectScrolledClipView(scrollView.contentView)
            textView.window?.makeFirstResponder(textView)
            updateCursor()
        }

        // MARK: NSTextStorageDelegate

        nonisolated func textStorage(_ textStorage: NSTextStorage, willProcessEditing editedMask: NSTextStorageEditActions, range editedRange: NSRange, changeInLength delta: Int) {
            guard editedMask.contains(.editedCharacters) else { return }
            MainActor.assumeIsolated {
                treeSitter?.willChangeContent(in: editedRange)
            }
        }

        nonisolated func textStorage(_ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorageEditActions, range editedRange: NSRange, changeInLength delta: Int) {
            guard editedMask.contains(.editedCharacters) else { return }
            MainActor.assumeIsolated {
                pendingEdit = pendingEdit.map { NSUnionRange($0, editedRange) } ?? editedRange
                treeSitter?.didChangeContent(in: editedRange, delta: delta)
            }
        }

        // MARK: NSTextViewDelegate

        func textView(_ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange, replacementString: String?) -> Bool {
            typedIdentifierCharacter = false
            typedTrigger = nil
            if let replacementString, replacementString.count == 1, let scalar = replacementString.unicodeScalars.first {
                typedIdentifierCharacter = isWord(scalar)
                if document.lspServer != nil, document.lspTriggers.contains(replacementString) {
                    typedTrigger = replacementString
                }
            }
            return true
        }

        func textDidChange(_ notification: Notification) {
            guard !replacing, let textView else { return }
            document.edited(textView.string)
            bufferScanDirty = true
            if let edit = pendingEdit {
                highlight(edit)
                pendingEdit = nil
            }
            ruler?.invalidateLines()
            updateCursor()
            if let appState, let project {
                appState.scheduleLanguageServerSync(document, in: project)
                appState.scheduleAutoSave(document, in: project)
            }
            offerCompletion()
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard !replacing else { return }
            updateCursor()
            if completion.isVisible, let textView {
                let caret = textView.selectedRange()
                let word = completion.wordRange
                if caret.length > 0 || caret.location < word.location || caret.location > NSMaxRange(word) + 1 {
                    completion.dismiss()
                }
            }
            scheduleSymbolLookup()
        }

        func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            if completion.isVisible {
                switch commandSelector {
                case #selector(NSResponder.moveDown(_:)):
                    completion.move(by: 1)
                    return true
                case #selector(NSResponder.moveUp(_:)):
                    completion.move(by: -1)
                    return true
                case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertTab(_:)):
                    completion.accept()
                    return true
                case #selector(NSResponder.cancelOperation(_:)):
                    completion.dismiss()
                    return true
                default:
                    break
                }
            }
            switch commandSelector {
            case #selector(NSResponder.insertNewline(_:)):
                let indent = currentLineIndent()
                textView.insertText("\n" + indent, replacementRange: textView.selectedRange())
                return true
            case #selector(NSResponder.insertTab(_:)):
                textView.insertText(indentUnit, replacementRange: textView.selectedRange())
                return true
            case #selector(NSResponder.cancelOperation(_:)):
                completion.dismiss()
                return false
            default:
                return false
            }
        }

        // MARK: Highlighting

        private func color(for kind: SyntaxHighlighter.TokenKind) -> NSColor {
            if let cached = colors[kind] { return cached }
            let color = NSColor(SyntaxHighlighter.color(for: kind))
            colors[kind] = color
            return color
        }

        func highlightAll() {
            if let treeSitter {
                treeSitter.invalidateAll()
                return
            }
            guard let storage = textView?.textStorage else { return }
            highlight(NSRange(location: 0, length: storage.length))
        }

        /// Recolours the paragraphs covering `range`; the tokenizer works a
        /// line at a time, so nothing outside them can change.
        private func highlight(_ range: NSRange) {
            guard treeSitter == nil, let storage = textView?.textStorage else { return }
            let string = storage.string as NSString
            let clamped = NSIntersectionRange(range, NSRange(location: 0, length: string.length))
            let paragraphs = string.paragraphRange(for: clamped)
            let language = document.language
            storage.beginEditing()
            storage.addAttribute(.foregroundColor, value: Theme.nsTextPrimary, range: paragraphs)
            string.enumerateSubstrings(in: paragraphs, options: [.byLines]) { [self] line, lineRange, _, _ in
                guard let line, !line.isEmpty else { return }
                for token in SyntaxHighlighter.tokenize(line, language: language) {
                    let local = NSRange(token.range, in: line)
                    storage.addAttribute(.foregroundColor, value: color(for: token.kind), range: NSRange(location: lineRange.location + local.location, length: local.length))
                }
            }
            storage.endEditing()
        }

        // MARK: Diagnostics

        /// Dotted underlines in the severity's colour; the message is the tooltip.
        func applyDiagnostics(_ diagnostics: [LspDiagnostic]) {
            appliedDiagnostics = diagnostics
            guard let textView, let layoutManager = textView.layoutManager, let ruler else { return }
            let length = (textView.string as NSString).length
            let all = NSRange(location: 0, length: length)
            layoutManager.removeTemporaryAttribute(.underlineStyle, forCharacterRange: all)
            layoutManager.removeTemporaryAttribute(.underlineColor, forCharacterRange: all)
            layoutManager.removeTemporaryAttribute(.toolTip, forCharacterRange: all)
            for diagnostic in diagnostics {
                guard let range = characterRange(of: diagnostic, in: ruler, length: length) else { continue }
                let color: NSColor = diagnostic.isError ? NSColor(Theme.danger) : (diagnostic.isWarning ? NSColor(Theme.warning) : NSColor(Theme.accent))
                layoutManager.addTemporaryAttributes([
                    .underlineStyle: NSUnderlineStyle.thick.rawValue | NSUnderlineStyle.patternDot.rawValue,
                    .underlineColor: color,
                    .toolTip: diagnostic.source.map { "\($0): \(diagnostic.message)" } ?? diagnostic.message,
                ], forCharacterRange: range)
            }
        }

        private func characterRange(of diagnostic: LspDiagnostic, in ruler: LineNumberRuler, length: Int) -> NSRange? {
            let start = min(length, ruler.characterIndex(ofLine: diagnostic.line + 1) + diagnostic.character)
            var end = min(length, ruler.characterIndex(ofLine: diagnostic.endLine + 1) + diagnostic.endCharacter)
            if end <= start { end = min(length, start + 1) }
            guard end > start else { return nil }
            return NSRange(location: start, length: end - start)
        }

        // MARK: Cursor and symbols

        private func updateCursor() {
            guard let textView, let ruler else { return }
            let caret = textView.selectedRange().location
            let (line, column) = ruler.position(ofCharacter: caret)
            if document.cursorLine != line { document.cursorLine = line }
            if document.cursorColumn != column { document.cursorColumn = column }
        }

        /// The identifier at the caret, looked up (debounced) for the status
        /// bar: the server's hover when there is one, else the index.
        private func scheduleSymbolLookup() {
            guard let textView else { return }
            let caret = textView.selectedRange().location
            let word = identifierRange(around: caret).map { (textView.string as NSString).substring(with: $0) }
            guard word != lastSymbolWord else { return }
            lastSymbolWord = word
            symbolLookup?.cancel()
            guard let word, word.count >= 2, let ruler else {
                document.cursorSymbol = nil
                document.cursorHover = nil
                return
            }
            let (line, column) = ruler.position(ofCharacter: caret)
            symbolLookup = Task { [weak self] in
                try? await Task.sleep(nanoseconds: 250_000_000)
                guard !Task.isCancelled, let self, let appState, let project else { return }
                if document.lspServer != nil {
                    let hover = await appState.languageServerHover(for: document, line: line - 1, character: column - 1, in: project)
                    guard !Task.isCancelled, lastSymbolWord == word else { return }
                    document.cursorHover = hover.flatMap(Self.firstHoverLine)
                } else {
                    document.cursorHover = nil
                }
                let symbols = await appState.findSymbols(word, mode: .exact, limit: 5, in: project, preferring: document.path)
                guard !Task.isCancelled, lastSymbolWord == word else { return }
                document.cursorSymbol = symbols.first
            }
        }

        /// The signature line of a hover: the first non-empty line that is not a fence.
        static func firstHoverLine(_ contents: String) -> String? {
            contents.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty && !$0.hasPrefix("```") && !$0.hasPrefix("---") }
        }

        private func jumpToDefinition(at index: Int) {
            guard let textView, let appState, let project, let ruler, let range = identifierRange(around: index) else { return }
            let word = (textView.string as NSString).substring(with: range)
            let (line, column) = ruler.position(ofCharacter: index)
            appState.jumpToDefinition(of: word, line: line - 1, character: column - 1, from: document, in: project)
        }

        // MARK: Completion

        private func offerCompletion() {
            guard let textView else { return }
            let caret = textView.selectedRange()
            let hasServer = document.lspServer != nil
            let range = identifierRange(endingAt: caret.location) ?? NSRange(location: caret.location, length: 0)
            let prefix = (textView.string as NSString).substring(with: range)
            let minimum = hasServer ? 1 : 2
            let typed = typedIdentifierCharacter && prefix.count >= minimum
            let triggered = typedTrigger != nil
            let refining = completion.isVisible && prefix.count >= 1
            guard caret.length == 0, typed || triggered || refining else {
                completion.dismiss()
                return
            }
            typedTrigger = nil
            let local = localCandidates(prefix: prefix, hasServer: hasServer)
            guard let ruler else { return }
            let (line, column) = ruler.position(ofCharacter: caret.location)
            completion.present(prefix: prefix, wordRange: range, local: local) { [weak self] in
                guard let self, let appState, let project else { return [] }
                if hasServer {
                    let items = await appState.languageServerCompletions(for: document, line: line - 1, character: column - 1, in: project)
                    let lowered = prefix.lowercased()
                    return items
                        .filter { prefix.isEmpty || $0.label.lowercased().hasPrefix(lowered) || ($0.filterText?.lowercased().hasPrefix(lowered) ?? false) }
                        .map { CompletionItem(server: $0) }
                }
                guard prefix.count >= 2 else { return [] }
                let symbols = await appState.findSymbols(prefix, mode: .prefix, limit: 40, in: project, preferring: document.path)
                return symbols.map { CompletionItem(symbol: $0) }
            }
        }

        /// Definitions in the buffer (unsaved ones included), then keywords
        /// (unless the server supplies them) and words of the buffer.
        private func localCandidates(prefix: String, hasServer: Bool) -> [CompletionItem] {
            let lowered = prefix.lowercased()
            scanBuffer()
            var items: [CompletionItem] = []
            for definition in bufferDefinitions where (prefix.isEmpty || definition.label.lowercased().hasPrefix(lowered)) && definition.label != prefix {
                items.append(definition)
            }
            if !hasServer {
                for keyword in SyntaxHighlighter.keywords(for: document.language) where keyword.lowercased().hasPrefix(lowered) && keyword != prefix {
                    items.append(CompletionItem(keyword: keyword))
                }
            }
            guard !prefix.isEmpty else { return items }
            for word in bufferWords where word.lowercased().hasPrefix(lowered) && word != prefix {
                items.append(CompletionItem(word: word))
            }
            return items
        }

        /// Rebuilds `bufferWords` (identifiers of three or more characters)
        /// and `bufferDefinitions` (`fn name`, `class Name`, `let name`, …).
        private func scanBuffer() {
            guard bufferScanDirty, Date().timeIntervalSince(bufferScanStamp) > 1 else { return }
            let text = document.text
            let hyphens = hyphenatedWords
            var words: Set<String> = []
            var current: [UInt8] = []
            func flush() {
                if current.count >= 3, current.contains(where: { $0 != 0x2D }), let word = String(bytes: current, encoding: .utf8) { words.insert(word) }
                current.removeAll(keepingCapacity: true)
            }
            for byte in text.utf8 {
                let identifier = (byte >= 0x41 && byte <= 0x5A) || (byte >= 0x61 && byte <= 0x7A) || byte == 0x5F || (byte >= 0x30 && byte <= 0x39 && !current.isEmpty) || (hyphens && byte == 0x2D)
                if identifier { current.append(byte) } else { flush() }
            }
            flush()
            bufferWords = words
            bufferDefinitions = Self.definitions(in: text, path: document.path)
            bufferScanStamp = Date()
            bufferScanDirty = false
        }

        private static let definitionPattern: NSRegularExpression? = {
            let modifiers = #"(?:(?:pub(?:\([^)]*\))?|async|static|final|export|default|private|public|protected|abstract|const|unsafe|extern|override|open|fileprivate|internal|mutating|lazy|inline)\s+)*"#
            let keywords = #"(fn|func|function|def|class|struct|enum|trait|interface|protocol|extension|impl|type|typealias|let|var|const|val|mod|module|namespace|macro_rules!)"#
            return try? NSRegularExpression(pattern: "(?m)^[ \\t]*" + modifiers + keywords + #"\s+([A-Za-z_][A-Za-z0-9_]*)"#, options: [])
        }()

        /// Definitions by their introducing keyword, with the line they sit on
        /// as the signature.
        static func definitions(in text: String, path: String) -> [CompletionItem] {
            guard let regex = definitionPattern else { return [] }
            let string = text as NSString
            var items: [CompletionItem] = []
            var seen: Set<String> = []
            var lineStarts: [Int] = [0]
            for (offset, unit) in text.utf16.enumerated() where unit == 0x0A { lineStarts.append(offset + 1) }
            regex.enumerateMatches(in: text, options: [], range: NSRange(location: 0, length: string.length)) { match, _, _ in
                guard let match, match.numberOfRanges >= 3 else { return }
                let keyword = string.substring(with: match.range(at: 1))
                let name = string.substring(with: match.range(at: 2))
                guard seen.insert(name).inserted else { return }
                let kind: String
                switch keyword {
                case "fn", "func", "function", "def", "macro_rules!": kind = "function"
                case "class": kind = "class"
                case "struct", "extension", "impl": kind = "struct"
                case "enum": kind = "enum"
                case "trait", "interface", "protocol": kind = "interface"
                case "type", "typealias": kind = "type_alias"
                case "mod", "module", "namespace": kind = "module"
                default: kind = "variable"
                }
                let lineRange = string.lineRange(for: match.range)
                let signature = string.substring(with: lineRange).trimmingCharacters(in: .whitespacesAndNewlines)
                var low = 0, high = lineStarts.count - 1
                while low < high {
                    let mid = (low + high + 1) / 2
                    if lineStarts[mid] <= match.range.location { low = mid } else { high = mid - 1 }
                }
                items.append(CompletionItem(bufferDefinition: name, kind: kind, signature: String(signature.prefix(80)), path: path, line: low + 1))
            }
            return items
        }

        private func insert(_ item: CompletionItem, replacing range: NSRange) {
            guard let textView else { return }
            var target = range
            if let edit = item.replaceRange, let ruler, edit.startLine == edit.endLine {
                let length = (textView.string as NSString).length
                let start = min(length, ruler.characterIndex(ofLine: edit.startLine + 1) + edit.startCharacter)
                let end = min(length, ruler.characterIndex(ofLine: edit.endLine + 1) + edit.endCharacter)
                if end >= start, start <= range.location, end >= range.location {
                    target = NSRange(location: start, length: max(end, NSMaxRange(range)) - start)
                }
            }
            replacing = true
            textView.insertText(item.insertText, replacementRange: target)
            replacing = false
            document.edited(textView.string)
            if let edit = pendingEdit {
                highlight(edit)
                pendingEdit = nil
            }
            ruler?.invalidateLines()
            bufferScanDirty = true
            updateCursor()
            if let appState, let project {
                appState.scheduleLanguageServerSync(document, in: project)
                appState.scheduleAutoSave(document, in: project)
            }
        }

        // MARK: Text helpers

        private func currentLineIndent() -> String {
            guard let textView else { return "" }
            let string = textView.string as NSString
            let line = string.lineRange(for: NSRange(location: textView.selectedRange().location, length: 0))
            let text = string.substring(with: line)
            return String(text.prefix { $0 == " " || $0 == "\t" })
        }

        /// The identifier whose last character sits right before `location`.
        private func identifierRange(endingAt location: Int) -> NSRange? {
            guard let textView else { return nil }
            let string = textView.string as NSString
            var start = location
            while start > 0, isWord(string.character(at: start - 1)) { start -= 1 }
            return start < location ? NSRange(location: start, length: location - start) : nil
        }

        /// The identifier containing or touching `location`.
        private func identifierRange(around location: Int) -> NSRange? {
            guard let textView else { return nil }
            let string = textView.string as NSString
            var start = location, end = location
            while start > 0, isWord(string.character(at: start - 1)) { start -= 1 }
            while end < string.length, isWord(string.character(at: end)) { end += 1 }
            guard end > start else { return nil }
            let range = NSRange(location: start, length: end - start)
            // Not an identifier when it starts with a digit.
            let first = string.character(at: start)
            return Self.isIdentifierStart(first) || (hyphenatedWords && first == 0x2D) ? range : nil
        }

        /// CSS and HTML names (`btn-primary`, `--brand`, `aria-label`) keep their hyphens.
        private var hyphenatedWords: Bool { document.language == .css || document.language == .html }

        private func isWord(_ unit: unichar) -> Bool {
            Self.isIdentifier(unit) || (hyphenatedWords && unit == 0x2D)
        }

        private func isWord(_ scalar: Unicode.Scalar) -> Bool {
            scalar.value < 0x10000 && isWord(unichar(scalar.value))
        }

        private static func isIdentifier(_ unit: unichar) -> Bool {
            isIdentifierStart(unit) || (unit >= 0x30 && unit <= 0x39)
        }

        private static func isIdentifierStart(_ unit: unichar) -> Bool {
            (unit >= 0x41 && unit <= 0x5A) || (unit >= 0x61 && unit <= 0x7A) || unit == 0x5F
        }

        /// A tab when the first indented line uses one, else its space count (2 or 4).
        private static func detectIndent(in text: String) -> String {
            for line in text.split(separator: "\n", omittingEmptySubsequences: true).prefix(400) {
                if line.hasPrefix("\t") { return "\t" }
                let spaces = line.prefix { $0 == " " }.count
                if spaces > 0 { return spaces == 2 || spaces == 3 ? "  " : "    " }
            }
            return "    "
        }
    }
}

// MARK: - Text view

/// Adds ⌘-click (go to definition) and the appearance hook to `NSTextView`.
final class CodeTextView: NSTextView {
    var onCommandClick: ((Int) -> Void)?
    var onAppearanceChange: (() -> Void)?

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command), let onCommandClick {
            let point = convert(event.locationInWindow, from: nil)
            let index = characterIndexForInsertion(at: point)
            if index < (string as NSString).length {
                onCommandClick(index)
                return
            }
        }
        super.mouseDown(with: event)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        onAppearanceChange?()
    }
}

// MARK: - Line numbers

/// Right-aligned line numbers next to the text, from the layout manager's
/// line fragments; `lineStarts` doubles as the line ↔ character map.
final class LineNumberRuler: NSRulerView {
    private weak var textView: NSTextView?
    /// UTF-16 offsets where each line starts; always begins with 0.
    private var lineStarts: [Int] = [0]

    init(scrollView: NSScrollView, textView: NSTextView) {
        self.textView = textView
        super.init(scrollView: scrollView, orientation: .verticalRuler)
        clientView = textView
        clipsToBounds = true
        ruleThickness = 40
        invalidateLines()
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    func invalidateLines() {
        let string = (textView?.string ?? "") as NSString
        var starts = [0]
        var index = 0
        while index < string.length {
            let line = string.lineRange(for: NSRange(location: index, length: 0))
            let next = NSMaxRange(line)
            if next == index { break }
            if next < string.length || string.character(at: next - 1) == 0x0A || string.character(at: next - 1) == 0x0D {
                starts.append(next)
            }
            index = next
        }
        lineStarts = starts
        let digits = max(2, String(starts.count).count)
        let width = CGFloat(digits) * (Theme.Font.nsMono.pointSize * 0.62) + 20
        if abs(ruleThickness - width) > 0.5 {
            // The clip view's left inset follows the thickness but its origin
            // does not: a view at the left edge would end up partly under the
            // gutter (or with a gap), so it is pinned to the new edge.
            let clip = scrollView?.contentView
            let atLeftEdge = clip.map { $0.bounds.origin.x <= -$0.contentInsets.left + 0.5 } ?? false
            ruleThickness = width
            if atLeftEdge, let scrollView, let clip {
                scrollView.tile()
                clip.scroll(to: NSPoint(x: -clip.contentInsets.left, y: clip.bounds.origin.y))
                scrollView.reflectScrolledClipView(clip)
            }
        }
        needsDisplay = true
    }

    /// 1-based line and column of a UTF-16 offset.
    func position(ofCharacter index: Int) -> (line: Int, column: Int) {
        let line = lineIndex(containing: index)
        return (line + 1, index - lineStarts[line] + 1)
    }

    /// First character of a 1-based line (the last line's for a line past the end).
    func characterIndex(ofLine line: Int) -> Int {
        lineStarts[max(0, min(line - 1, lineStarts.count - 1))]
    }

    private func lineIndex(containing index: Int) -> Int {
        var low = 0, high = lineStarts.count - 1
        while low < high {
            let mid = (low + high + 1) / 2
            if lineStarts[mid] <= index { low = mid } else { high = mid - 1 }
        }
        return low
    }

    /// Hosted in SwiftUI, the dirty rect can span past the ruler, and the
    /// stock separator line then runs up over the editor's header.
    override func draw(_ dirtyRect: NSRect) {
        NSGraphicsContext.saveGraphicsState()
        bounds.clip()
        super.draw(dirtyRect.intersection(bounds))
        NSGraphicsContext.restoreGraphicsState()
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView, let layoutManager = textView.layoutManager, let container = textView.textContainer else { return }
        NSColor.clear.setFill()
        let font = NSFont.monospacedDigitSystemFont(ofSize: Theme.Font.nsMono.pointSize - 1.5, weight: .regular)
        let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.adaptive(light: "#9A9EA6", dark: "#6F7278")]
        let string = textView.string as NSString
        let visible = textView.visibleRect
        let glyphs = layoutManager.glyphRange(forBoundingRect: visible, in: container)
        let characters = layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        let inset = textView.textContainerInset.height
        var line = lineIndex(containing: characters.location)
        while line < lineStarts.count {
            let start = lineStarts[line]
            if start > NSMaxRange(characters), start < string.length { break }
            let fragment: NSRect
            if start >= string.length {
                fragment = layoutManager.extraLineFragmentRect
                if fragment.isEmpty, string.length > 0 { break }
            } else {
                fragment = layoutManager.lineFragmentRect(forGlyphAt: layoutManager.glyphIndexForCharacter(at: start), effectiveRange: nil)
            }
            let y = convert(NSPoint(x: 0, y: fragment.minY + inset), from: textView).y
            if y > rect.maxY { break }
            let label = NSAttributedString(string: String(line + 1), attributes: attributes)
            let size = label.size()
            label.draw(at: NSPoint(x: ruleThickness - size.width - 8, y: y + (fragment.height - size.height) / 2))
            line += 1
            if start >= string.length { break }
        }
    }
}
