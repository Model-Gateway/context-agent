import Foundation
import SwiftTreeSitter
import TreeSitterBash
import TreeSitterCSS
import TreeSitterGo
import TreeSitterHTML
import TreeSitterJavaScript
import TreeSitterJSON
import TreeSitterMarkdown
import TreeSitterMarkdownInline
import TreeSitterPython
import TreeSitterRust
import TreeSitterSwift
import TreeSitterTOML
import TreeSitterTSX
import TreeSitterTypeScript
import TreeSitterYAML

/// The tree-sitter grammars the editor highlights with, each packaged with
/// its `queries/*.scm` (Swift packages from the grammars' own repositories).
/// Configurations are built on first use and cached; the lookup also serves
/// injected languages (`<script>` in HTML, fenced code in Markdown), so it
/// accepts their aliases and can run off the main thread.
enum TreeSitterGrammars {
    /// The grammar for a file, or nil when there is none and the regex
    /// highlighter keeps the file.
    static func name(forPath path: String, language: SyntaxHighlighter.Language) -> String? {
        let ext = (path as NSString).pathExtension.lowercased()
        switch language {
        case .typescript: return ext == "tsx" ? "tsx" : "typescript"
        case .javascript: return "javascript"
        case .json: return "json"
        case .swift: return "swift"
        case .rust: return "rust"
        case .python: return "python"
        case .go: return "go"
        case .shell: return "bash"
        case .html: return ["xml", "svg"].contains(ext) ? nil : "html"
        case .css: return ext == "css" ? "css" : nil
        case .markdown: return "markdown"
        case .yaml: return "yaml"
        case .toml: return "toml"
        case .dotenv, .diff, .plain: return nil
        }
    }

    /// The configuration for a grammar name or an injection's language
    /// ("js", "sh", "py", …); nil for one without a grammar.
    static func configuration(named name: String) -> LanguageConfiguration? {
        let key = aliases[name.lowercased()] ?? name.lowercased()
        lock.lock()
        defer { lock.unlock() }
        if let cached = cache[key] { return cached }
        let configuration = make(key)
        cache[key] = configuration
        return configuration
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var cache: [String: LanguageConfiguration?] = [:]

    private static let aliases: [String: String] = [
        "js": "javascript", "jsx": "javascript", "mjs": "javascript", "cjs": "javascript", "node": "javascript",
        "ts": "typescript", "mts": "typescript", "cts": "typescript",
        "py": "python", "python3": "python",
        "rs": "rust", "golang": "go",
        "sh": "bash", "shell": "bash", "zsh": "bash", "console": "bash", "shellscript": "bash",
        "htm": "html", "xhtml": "html",
        "jsonc": "json", "json5": "json",
        "yml": "yaml", "md": "markdown",
    ]

    private static func make(_ name: String) -> LanguageConfiguration? {
        let javascript = "TreeSitterJavaScript_TreeSitterJavaScript"
        let typescript = "TreeSitterTypeScript_TreeSitterTypeScript"
        switch name {
        // TypeScript's own queries only add to JavaScript's, as its
        // tree-sitter.json lists them.
        case "javascript":
            return build(tree_sitter_javascript(), name, highlights: [(javascript, "highlights.scm"), (javascript, "highlights-jsx.scm"), (javascript, "highlights-params.scm")], injections: [(javascript, "injections.scm")])
        case "typescript":
            return build(tree_sitter_typescript(), name, highlights: [(typescript, "highlights.scm"), (javascript, "highlights.scm")], injections: [(javascript, "injections.scm")])
        case "tsx":
            return build(tree_sitter_tsx(), name, highlights: [(typescript, "highlights.scm"), (javascript, "highlights-jsx.scm"), (javascript, "highlights.scm")], injections: [(javascript, "injections.scm")])
        case "rust": return standard(tree_sitter_rust(), name, bundle: "TreeSitterRust_TreeSitterRust")
        case "python": return standard(tree_sitter_python(), name, bundle: "TreeSitterPython_TreeSitterPython")
        case "go": return standard(tree_sitter_go(), name, bundle: "TreeSitterGo_TreeSitterGo")
        case "bash": return standard(tree_sitter_bash(), name, bundle: "TreeSitterBash_TreeSitterBash")
        case "html": return standard(tree_sitter_html(), name, bundle: "TreeSitterHTML_TreeSitterHTML")
        case "css": return standard(tree_sitter_css(), name, bundle: "TreeSitterCSS_TreeSitterCSS")
        case "json": return standard(tree_sitter_json(), name, bundle: "TreeSitterJSON_TreeSitterJSON")
        case "yaml": return standard(tree_sitter_yaml(), name, bundle: "TreeSitterYAML_TreeSitterYAML")
        case "toml": return standard(tree_sitter_toml(), name, bundle: "TreeSitterTOML_TreeSitterTOML")
        case "swift": return standard(tree_sitter_swift(), name, bundle: "TreeSitterSwift_TreeSitterSwift")
        case "markdown": return standard(tree_sitter_markdown(), name, bundle: "TreeSitterMarkdown_TreeSitterMarkdown")
        case "markdown_inline": return standard(tree_sitter_markdown_inline(), name, bundle: "TreeSitterMarkdown_TreeSitterMarkdownInline")
        default: return nil
        }
    }

    private static func standard(_ language: OpaquePointer, _ name: String, bundle: String) -> LanguageConfiguration? {
        build(language, name, highlights: [(bundle, "highlights.scm")], injections: [(bundle, "injections.scm")])
    }

    /// Joins query files (missing ones are skipped) into one query per kind.
    private static func build(_ pointer: OpaquePointer, _ name: String, highlights: [(String, String)], injections: [(String, String)]) -> LanguageConfiguration? {
        let language = Language(pointer)
        var queries: [Query.Definition: Query] = [:]
        for (definition, files) in [(Query.Definition.highlights, highlights), (.injections, injections)] {
            let source = files.compactMap { bundle, file in
                queriesDirectory(bundle).flatMap { try? Data(contentsOf: $0.appendingPathComponent(file)) }
            }.reduce(Data()) { $0 + $1 + Data("\n".utf8) }
            guard !source.isEmpty else { continue }
            do {
                queries[definition] = try Query(language: language, data: source)
            } catch {
                NSLog("tree-sitter: %@ %@ query failed: %@", name, definition.name, String(describing: error))
            }
        }
        guard queries[.highlights] != nil else { return nil }
        return LanguageConfiguration(language, name: name, queries: queries)
    }

    /// `<bundle>.bundle/Contents/Resources/queries` in the app (Xcode), or
    /// `<bundle>.bundle/queries` next to a SwiftPM executable.
    private static func queriesDirectory(_ bundle: String) -> URL? {
        guard let url = Bundle.main.url(forResource: bundle, withExtension: "bundle") else { return nil }
        let short = url.appendingPathComponent("queries", isDirectory: true)
        if FileManager.default.fileExists(atPath: short.path) { return short }
        return url.appendingPathComponent("Contents/Resources/queries", isDirectory: true)
    }
}
