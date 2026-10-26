import AppKit
import Neon
import RangeState
import SwiftTreeSitter
import SwiftTreeSitterLayer
import TreeSitterClient

/// Incremental tree-sitter colouring for `CodeEditor`: Neon's
/// `TreeSitterClient` reparses after each edit (in the background for big
/// changes, injected languages included) and its `TextSystemStyler` recolours
/// what is visible. Unlike Neon's `TextViewHighlighter` it leaves the text
/// storage's delegate to the editor, which forwards edits here, and it only
/// touches the layout manager's temporary foreground colour, so the
/// diagnostics' temporary underlines stay put.
@MainActor
final class TreeSitterHighlighter {
    private let textView: NSTextView
    private let client: TreeSitterClient
    private let styler: TextSystemStyler<ForegroundColorInterface>
    private let buffer = RangeInvalidationBuffer()

    /// nil when the grammar has no configuration or the client cannot start.
    init?(textView: NSTextView, grammar: String, color: @escaping (SyntaxHighlighter.TokenKind) -> NSColor) {
        guard let storage = textView.textStorage, let layoutManager = textView.layoutManager,
              let configuration = TreeSitterGrammars.configuration(named: grammar) else { return nil }
        self.textView = textView
        let buffer = buffer
        do {
            client = try TreeSitterClient(
                rootLanguageConfig: configuration,
                configuration: .init(
                    languageProvider: { TreeSitterGrammars.configuration(named: $0) },
                    contentProvider: { [storage] in LanguageLayer.Content(string: storage.string, limit: $0) },
                    contentSnapshopProvider: { [storage] in LanguageLayer.ContentSnapshot(string: storage.string, limit: $0) },
                    lengthProvider: { [storage] in storage.length },
                    invalidationHandler: { buffer.invalidate(.set($0)) },
                    locationTransformer: { _ in nil }
                )
            )
        } catch {
            NSLog("tree-sitter: %@ client failed: %@", grammar, String(describing: error))
            return nil
        }
        // Reads the storage at query time, so predicates see the current text.
        let tokens = client.tokenProvider(with: { [storage] in storage.string.predicateTextProvider($0, $1) })
        let interface = ForegroundColorInterface(layoutManager: layoutManager, storage: storage) { token in
            Self.kind(of: token, in: storage).map(color)
        }
        let styler = TextSystemStyler(textSystem: interface, tokenProvider: tokens)
        self.styler = styler
        buffer.invalidationHandler = { [weak textView] target in
            styler.invalidate(target)
            styler.validate(.range(textView?.visibleTextRange ?? NSRange(location: 0, length: storage.length)))
        }
    }

    // MARK: Edits (forwarded from the text storage's delegate)

    /// `willProcessEditing`: hold invalidations until the edit is laid out.
    func willChangeContent(in range: NSRange) {
        buffer.beginBuffering()
        client.willChangeContent(in: range)
    }

    /// `didProcessEditing`, with the edited range as the storage reports it
    /// (the new text) and the change in length.
    func didChangeContent(in range: NSRange, delta: Int) {
        let previous = NSRange(location: range.location, length: range.length - delta)
        client.didChangeContent(in: previous, delta: delta)
        styler.didChangeContent(in: previous, delta: delta)
        // Styling inside `processEditing` is unsafe; the next turn of the run
        // loop is the first point TextKit 1 offers after it.
        DispatchQueue.main.async { [buffer] in buffer.endBuffering() }
    }

    // MARK: Styling

    /// Recolours everything, e.g. after an appearance change.
    func invalidateAll() {
        buffer.invalidate(.all)
    }

    /// Colours what scrolled or resized into view.
    func visibleRangeChanged() {
        styler.validate(.range(textView.visibleTextRange))
    }

    /// A capture name ("keyword.function", "string.special.key") as one of
    /// the theme's token kinds; nil leaves the text in the default colour.
    /// Later captures win, so an unmapped one never undoes an earlier colour.
    static func kind(of token: Token, in storage: NSTextStorage) -> SyntaxHighlighter.TokenKind? {
        let name = token.name
        let head = name.prefix { $0 != "." }
        switch head {
        case "comment": return .comment
        case "string": return name == "string.special.key" ? .key : .string
        case "character", "escape": return .string
        case "keyword", "boolean", "tag", "include", "conditional", "repeat", "exception": return .keyword
        case "number", "float": return .number
        case "type", "constructor", "namespace", "module": return .type
        case "function", "method": return .function
        case "property", "attribute", "label": return .key
        case "variable": return name == "variable.builtin" ? .keyword : nil
        // Rust captures numeric literals as `constant.builtin` too.
        case "constant":
            if name == "constant.numeric" { return .number }
            guard name == "constant.builtin" else { return nil }
            return startsWithDigit(token.range, in: storage) ? .number : .keyword
        // Markdown.
        case "text":
            switch name {
            case "text.title": return .keyword
            case "text.literal", "text.uri", "text.reference": return .string
            default: return nil
            }
        default: return nil
        }
    }

    private static func startsWithDigit(_ range: NSRange, in storage: NSTextStorage) -> Bool {
        let string = storage.string as NSString
        var index = range.location
        guard index < string.length else { return false }
        if string.character(at: index) == 0x2D, index + 1 < string.length { index += 1 }
        let unit = string.character(at: index)
        return unit >= 0x30 && unit <= 0x39
    }
}

/// Applies token colours as layout-manager temporary attributes, touching
/// nothing but `.foregroundColor`.
@MainActor
private struct ForegroundColorInterface: TextSystemInterface {
    let layoutManager: NSLayoutManager
    let storage: NSTextStorage
    let color: (Token) -> NSColor?

    var content: NSTextStorage { storage }

    func applyStyles(for application: TokenApplication) {
        let length = storage.length
        if let range = application.range, application.action == .replace {
            layoutManager.removeTemporaryAttribute(.foregroundColor, forCharacterRange: NSIntersectionRange(range, NSRange(location: 0, length: length)))
        }
        for token in application.tokens {
            guard let color = color(token) else { continue }
            layoutManager.addTemporaryAttribute(.foregroundColor, value: color, forCharacterRange: NSIntersectionRange(token.range, NSRange(location: 0, length: length)))
        }
    }
}
