import SwiftUI

/// Line-oriented, regex-based syntax colouring. One precompiled alternation
/// per language turns a line into tokens (strings, comments, keywords,
/// numbers, JSON keys); `highlight` paints them with the Theme code colours.
/// Everything works on a single line, so streaming output and `LazyVStack`
/// rows never need context from neighbouring lines.
enum SyntaxHighlighter {
    enum Language: String, CaseIterable, Hashable {
        case json, swift, rust, python, javascript, typescript, go, shell, html, css, markdown, yaml, toml, dotenv, diff, plain

        /// Maps a fence info string, a file extension, a path or a format
        /// label ("JSON", "stdout") onto a language; unknown → `.plain`.
        static func detect(_ hint: String?) -> Language {
            guard let hint else { return .plain }
            var key = hint.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            if key.isEmpty { return .plain }
            if let first = key.split(whereSeparator: { $0 == " " || $0 == "\t" }).first { key = String(first) }
            // `.env`, `.env.local`, `config/.env.production`: the extension says nothing.
            let fileName = key.split(separator: "/").last.map(String.init) ?? key
            if fileName == ".env" || fileName.hasPrefix(".env.") { return .dotenv }
            if fileName == ".envrc" || fileName == ".zshrc" || fileName == ".bashrc" || fileName == ".profile" { return .shell }
            if key.contains("/") || key.contains(".") {
                let name = key.split(separator: "/").last.map(String.init) ?? key
                if let dot = name.lastIndex(of: "."), dot != name.startIndex {
                    key = String(name[name.index(after: dot)...])
                } else {
                    key = name
                }
            }
            switch key {
            case "json", "jsonc", "json5": return .json
            case "swift": return .swift
            case "rs", "rust": return .rust
            case "py", "python", "python3", "pyi": return .python
            case "js", "javascript", "jsx", "mjs", "cjs", "node": return .javascript
            case "ts", "typescript", "tsx", "mts", "cts": return .typescript
            case "go", "golang": return .go
            case "sh", "bash", "zsh", "shell", "console", "fish", "ksh", "shellscript": return .shell
            case "html", "htm", "xhtml", "xml", "svg", "vue", "svelte": return .html
            case "css", "scss", "sass", "less", "pcss", "postcss": return .css
            case "md", "markdown", "mdx", "mdown", "mkd": return .markdown
            case "yaml", "yml": return .yaml
            case "toml": return .toml
            case "env", "dotenv": return .dotenv
            case "diff", "patch": return .diff
            default: return .plain
            }
        }

        /// Label for code block headers.
        var displayName: String {
            switch self {
            case .json: return "JSON"
            case .swift: return "Swift"
            case .rust: return "Rust"
            case .python: return "Python"
            case .javascript: return "JavaScript"
            case .typescript: return "TypeScript"
            case .go: return "Go"
            case .shell: return "Shell"
            case .html: return "HTML"
            case .css: return "CSS"
            case .markdown: return "Markdown"
            case .yaml: return "YAML"
            case .toml: return "TOML"
            case .dotenv: return "Env"
            case .diff: return "Diff"
            case .plain: return "Plain text"
            }
        }
    }

    enum TokenKind: Equatable {
        case string, comment, keyword, number, key
        /// Only from tree-sitter (`TreeSitterHighlighter`); the regex grammars never emit them.
        case type, function
        case diffAdded, diffRemoved, diffHunk, diffMeta
    }

    struct Token: Equatable {
        var range: Range<String.Index>
        var kind: TokenKind
    }

    /// Lines longer than this are left plain: a pathological minified line
    /// must never stall the main thread.
    static let maxScanLength = 4_000

    /// Colours `line` for `language`. The result contains exactly the
    /// characters of `line`; only `foregroundColor` runs are added.
    static func highlight(_ line: String, language: Language) -> AttributedString {
        var result = AttributedString(line)
        for token in tokenize(line, language: language) {
            guard let range = Range(token.range, in: result) else { continue }
            result[range].foregroundColor = color(for: token.kind)
        }
        return result
    }

    /// Non-overlapping tokens in order of appearance.
    static func tokenize(_ line: String, language: Language) -> [Token] {
        if line.isEmpty || line.utf16.count > maxScanLength { return [] }
        switch language {
        case .plain:
            return []
        case .diff:
            return diffTokens(line)
        default:
            guard let grammar = grammars[language] else { return [] }
            return grammar.tokens(in: line)
        }
    }

    /// The words the grammar colours as keywords, for the editor's completion.
    static func keywords(for language: Language) -> [String] {
        switch language {
        case .json: return ["true", "false", "null"]
        case .swift: return swiftKeywords
        case .rust: return rustKeywords
        case .python: return pythonKeywords
        case .javascript: return javascriptKeywords
        case .typescript: return typescriptKeywords
        case .go: return goKeywords
        case .shell: return shellKeywords
        case .html: return htmlKeywords
        case .css: return cssKeywords
        case .yaml: return ["true", "false", "null", "yes", "no", "on", "off"]
        case .toml: return ["true", "false"]
        case .dotenv: return ["export", "true", "false"]
        case .markdown, .diff, .plain: return []
        }
    }

    static func color(for kind: TokenKind) -> Color {
        switch kind {
        case .string: return Theme.codeString
        case .comment: return Theme.codeComment
        case .keyword: return Theme.codeKeyword
        case .number: return Theme.codeNumber
        case .key: return Theme.textSecondary
        case .type: return Theme.codeType
        case .function: return Theme.codeFunction
        case .diffAdded: return Theme.success
        case .diffRemoved: return Theme.danger
        case .diffHunk: return Theme.textTertiary
        case .diffMeta: return Theme.textSecondary
        }
    }

    // MARK: - Grammars

    private struct Grammar {
        let regex: NSRegularExpression
        /// `kinds[i]` is the token kind of capture group `i + 1`.
        let kinds: [TokenKind]

        func tokens(in line: String) -> [Token] {
            let nsLine = line as NSString
            let whole = NSRange(location: 0, length: nsLine.length)
            var tokens: [Token] = []
            regex.enumerateMatches(in: line, options: [], range: whole) { match, _, _ in
                guard let match else { return }
                for (offset, kind) in kinds.enumerated() {
                    let groupRange = match.range(at: offset + 1)
                    if groupRange.location != NSNotFound, groupRange.length > 0 {
                        if let range = Range(groupRange, in: line) {
                            tokens.append(Token(range: range, kind: kind))
                        }
                        break
                    }
                }
            }
            return tokens
        }
    }

    /// Builds one alternation `(a)|(b)|…` from ordered parts. Parts must use
    /// non-capturing groups only, so the capture index identifies the kind.
    private static func grammar(_ parts: [(TokenKind, String)]) -> Grammar? {
        let pattern = parts.map { "(" + $0.1 + ")" }.joined(separator: "|")
        guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else { return nil }
        return Grammar(regex: regex, kinds: parts.map { $0.0 })
    }

    private static func keywords(_ words: [String]) -> String {
        #"\b(?:"# + words.joined(separator: "|") + #")\b"#
    }

    private static let doubleQuoted = #""(?:[^"\\]|\\.)*""#
    private static let singleQuoted = #"'(?:[^'\\]|\\.)*'"#
    private static let backtickQuoted = #"`[^`]*`"#
    private static let slashComment = #"//.*"#
    private static let blockComment = #"/\*(?:.*?\*/|.*)"#
    private static let hashComment = #"#.*"#
    private static let shellComment = #"(?:^|(?<=\s))#.*"#
    private static let decimalNumber = #"\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b"#
    private static let jsonNumber = #"-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b"#
    private static let jsonKey = #""(?:[^"\\]|\\.)*"(?=\s*:)"#

    private static let swiftKeywords = [
        "associatedtype", "class", "deinit", "enum", "extension", "func", "import", "init", "inout", "internal",
        "let", "operator", "private", "fileprivate", "precedencegroup", "protocol", "public", "open", "rethrows",
        "static", "struct", "subscript", "typealias", "var", "break", "case", "catch", "continue", "default",
        "defer", "do", "else", "fallthrough", "for", "guard", "if", "in", "repeat", "return", "throw", "switch",
        "where", "while", "as", "Any", "false", "is", "nil", "self", "Self", "super", "throws", "true", "try",
        "some", "any", "await", "async", "actor", "macro", "consuming", "borrowing", "nonisolated", "weak",
        "unowned", "lazy", "override", "mutating", "nonmutating", "final", "required", "convenience", "indirect",
        "dynamic", "optional", "get", "set", "willSet", "didSet",
    ]
    private static let rustKeywords = [
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
        "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
        "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
        "union", "macro_rules", "Some", "None", "Ok", "Err",
    ]
    private static let pythonKeywords = [
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
        "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
        "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "match", "case", "self",
    ]
    private static let javascriptKeywords = [
        "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete", "do", "else",
        "enum", "export", "extends", "false", "finally", "for", "function", "if", "import", "in", "instanceof",
        "new", "null", "return", "super", "switch", "this", "throw", "true", "try", "typeof", "var", "void",
        "while", "with", "yield", "let", "static", "async", "await", "of", "undefined", "NaN", "get", "set",
    ]
    private static let typescriptKeywords = javascriptKeywords + [
        "interface", "type", "implements", "declare", "namespace", "module", "abstract", "readonly", "private",
        "protected", "public", "as", "keyof", "any", "number", "string", "boolean", "never", "unknown", "object",
        "symbol", "bigint", "is", "infer", "satisfies", "override",
    ]
    private static let goKeywords = [
        "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for", "func",
        "go", "goto", "if", "import", "interface", "map", "package", "range", "return", "select", "struct",
        "switch", "type", "var", "true", "false", "nil", "iota", "append", "len", "cap", "make", "new", "panic",
        "recover", "print", "println", "error", "string", "int", "int8", "int16", "int32", "int64", "uint",
        "uint8", "uint16", "uint32", "uint64", "float32", "float64", "bool", "byte", "rune",
    ]
    private static let shellKeywords = [
        "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in",
        "function", "select", "time", "return", "exit", "export", "local", "readonly", "declare", "unset",
        "shift", "source", "alias", "cd", "echo", "set", "eval", "exec", "trap", "true", "false",
    ]

    private static let htmlKeywords = [
        "html", "head", "body", "title", "meta", "link", "script", "style", "div", "span", "section", "article",
        "header", "footer", "main", "nav", "aside", "button", "input", "label", "select", "option", "textarea",
        "form", "table", "thead", "tbody", "tr", "td", "th", "img", "picture", "source", "video", "audio", "canvas",
        "template", "slot", "dialog", "details", "summary", "figure", "figcaption", "strong", "code", "pre",
        "class", "href", "src", "alt", "type", "name", "value", "placeholder", "disabled", "checked", "hidden",
        "aria-label", "aria-hidden", "role", "tabindex", "target", "rel", "width", "height", "loading", "charset",
        "content", "viewport", "async", "defer", "onclick", "data-",
    ]
    private static let cssKeywords = [
        "display", "position", "relative", "absolute", "fixed", "sticky", "flex", "grid", "inline-flex",
        "inline-block", "block", "none", "flex-direction", "flex-wrap", "justify-content", "align-items",
        "align-self", "gap", "grid-template-columns", "grid-template-rows", "margin", "padding", "border",
        "border-radius", "width", "height", "max-width", "min-width", "max-height", "min-height", "color",
        "background", "background-color", "font", "font-family", "font-size", "font-weight", "line-height",
        "letter-spacing", "text-align", "text-decoration", "text-transform", "white-space", "overflow",
        "opacity", "transform", "transition", "animation", "cursor", "z-index", "top", "right", "bottom", "left",
        "inset", "box-shadow", "box-sizing", "border-box", "content", "outline", "visibility", "pointer-events",
        "user-select", "object-fit", "aspect-ratio", "center", "space-between", "space-around", "wrap", "column",
        "row", "auto", "inherit", "initial", "unset", "transparent", "currentColor", "important", "hover", "focus",
        "focus-visible", "active", "disabled", "before", "after", "first-child", "last-child", "nth-child",
        "media", "supports", "keyframes", "import", "font-face", "layer", "container", "var", "calc", "clamp",
        "min", "max", "rgb", "rgba", "hsl", "linear-gradient", "repeat", "minmax", "fr",
    ]

    private static let grammars: [Language: Grammar] = {
        var table: [Language: Grammar] = [:]
        table[.json] = grammar([
            (.key, jsonKey),
            (.string, doubleQuoted),
            (.number, jsonNumber),
            (.keyword, keywords(["true", "false", "null"])),
        ])
        table[.swift] = grammar([
            (.string, doubleQuoted),
            (.comment, slashComment),
            (.comment, blockComment),
            (.number, decimalNumber),
            (.keyword, keywords(swiftKeywords)),
        ])
        table[.rust] = grammar([
            (.string, doubleQuoted),
            (.comment, slashComment),
            (.comment, blockComment),
            (.number, #"\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?(?:[iuf](?:8|16|32|64|128|size))?)\b"#),
            (.keyword, keywords(rustKeywords)),
        ])
        table[.python] = grammar([
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.comment, hashComment),
            (.number, decimalNumber),
            (.keyword, keywords(pythonKeywords)),
        ])
        table[.javascript] = grammar([
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.string, backtickQuoted),
            (.comment, slashComment),
            (.comment, blockComment),
            (.number, decimalNumber),
            (.keyword, keywords(javascriptKeywords)),
        ])
        table[.typescript] = grammar([
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.string, backtickQuoted),
            (.comment, slashComment),
            (.comment, blockComment),
            (.number, decimalNumber),
            (.keyword, keywords(typescriptKeywords)),
        ])
        table[.go] = grammar([
            (.string, doubleQuoted),
            (.string, backtickQuoted),
            (.string, singleQuoted),
            (.comment, slashComment),
            (.comment, blockComment),
            (.number, decimalNumber),
            (.keyword, keywords(goKeywords)),
        ])
        table[.shell] = grammar([
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.comment, shellComment),
            (.number, #"\b\d+\b"#),
            (.keyword, keywords(shellKeywords)),
        ])
        table[.html] = grammar([
            (.comment, #"<!--(?:.*?-->|.*)"#),
            (.comment, #"<!(?i:doctype)[^>]*>"#),
            (.keyword, #"</?[A-Za-z][A-Za-z0-9\-:]*|/?>"#),
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.key, #"(?<=\s)[A-Za-z_:@#\[(][A-Za-z0-9_:.\-\])]*(?==)"#),
            (.number, #"&[A-Za-z0-9#]+;"#),
        ])
        table[.css] = grammar([
            (.comment, blockComment),
            (.comment, #"(?:^|(?<=\s))//.*"#),
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.keyword, #"@[A-Za-z\-]+|!important\b"#),
            (.key, #"(?:^|(?<=[\s{;]))-{0,2}[A-Za-z_][A-Za-z0-9_\-]*(?=\s*:(?!:))|\$[A-Za-z_][A-Za-z0-9_\-]*"#),
            (.number, #"#[0-9A-Fa-f]{3,8}\b(?![\w\-]*\s*[{,])|(?<![\w\-])-?\d*\.?\d+(?:px|em|rem|%|vh|vw|vmin|vmax|ch|ex|s|ms|deg|rad|turn|fr|dpi|dppx|pt|cm|mm|in)?\b"#),
            (.keyword, keywords(["important", "inherit", "initial", "unset", "revert", "auto", "none"])),
        ])
        table[.markdown] = grammar([
            (.keyword, #"^\s{0,3}#{1,6}(?:\s.*)?$"#),
            (.comment, #"^\s{0,3}>.*"#),
            (.comment, #"<!--(?:.*?-->|.*)"#),
            (.string, #"^\s*(?:```|~~~).*"#),
            (.string, #"`[^`]+`"#),
            (.number, #"!?\[[^\]]*\]\([^)]*\)"#),
            (.key, #"\*\*[^*]+\*\*|__[^_]+__"#),
            (.keyword, #"^\s*(?:[-*+]|\d+[.)])(?=\s)"#),
        ])
        table[.yaml] = grammar([
            (.comment, shellComment),
            (.key, #"^\s*(?:-\s+)?[A-Za-z0-9_.\-/"'$<]+(?=\s*:(?:\s|$))"#),
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.keyword, #"^---$|^\.\.\.$|[&*][A-Za-z0-9_\-]+"#),
            (.number, #"(?<=[:\-\[,]\s)-?\d+(?:\.\d+)?\b"#),
            (.keyword, keywords(["true", "false", "null", "yes", "no", "on", "off", "True", "False", "Null", "TRUE", "FALSE"])),
        ])
        table[.toml] = grammar([
            (.comment, hashComment),
            (.keyword, #"^\s*\[\[?[^\]]*\]\]?"#),
            (.key, #"^\s*[A-Za-z0-9_.\-"']+(?=\s*=)"#),
            (.string, #""{3}.*?(?:"{3}|$)|'{3}.*?(?:'{3}|$)"#),
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.number, #"\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?\b|[+-]?\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b"#),
            (.keyword, keywords(["true", "false", "inf", "nan"])),
        ])
        table[.dotenv] = grammar([
            (.comment, #"^\s*#.*|(?<=\s)#.*"#),
            (.keyword, #"^\s*export(?=\s)"#),
            (.key, #"\G\s*[A-Za-z_][A-Za-z0-9_.\-]*(?=\s*=)"#),
            (.string, doubleQuoted),
            (.string, singleQuoted),
            (.number, #"(?<==)\s*-?\d+(?:\.\d+)?\s*$"#),
            (.keyword, #"\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*"#),
        ])
        return table
    }()

    // MARK: - Diff

    private static func diffTokens(_ line: String) -> [Token] {
        let whole = line.startIndex..<line.endIndex
        let kind: TokenKind?
        if line.hasPrefix("+++ ") || line.hasPrefix("--- ") {
            kind = .diffMeta
        } else if line.hasPrefix("+") {
            kind = .diffAdded
        } else if line.hasPrefix("-") {
            kind = .diffRemoved
        } else if line.hasPrefix("@@") {
            kind = .diffHunk
        } else if line.hasPrefix("diff ") || line.hasPrefix("index ") || line.hasPrefix("new file") || line.hasPrefix("deleted file")
                    || line.hasPrefix("rename ") || line.hasPrefix("similarity ") || line.hasPrefix("Binary files") {
            kind = .diffMeta
        } else {
            kind = nil
        }
        guard let kind else { return [] }
        return [Token(range: whole, kind: kind)]
    }
}

#Preview("Syntax highlighter") {
    let samples: [(SyntaxHighlighter.Language, String)] = [
        (.json, #"{"taskId": "11", "count": 3, "ok": true, "none": null}"#),
        (.swift, #"let greeting = "hi" // comment 42"#),
        (.rust, #"fn main() { let x: u32 = 10u32; println!("{}", x); } // done"#),
        (.python, #"def f(x): return x * 2  # double"#),
        (.javascript, #"const s = `tpl ${x}`; let n = 0x1F; // ok"#),
        (.typescript, #"interface A { readonly id: number } /* c */"#),
        (.go, #"func main() { s := "go"; fmt.Println(s, 3.14) }"#),
        (.shell, #"if [ -f "$f" ]; then echo 'yes' # c; fi"#),
        (.html, #"<a href="/docs" class="nav-link">Docs &amp; more</a> <!-- c -->"#),
        (.css, #".btn-primary:hover { color: #0af; margin: 0 1.5rem; --gap: 4px; } /* c */"#),
        (.markdown, "## Install `ctx` with **cargo**, see [docs](docs/README.md)"),
        (.yaml, #"  image: "postgres:16" # pinned, replicas: 3"#),
        (.toml, #"[workspace.dependencies] # deps"#),
        (.toml, #"serde = { version = "1", features = ["derive"] }"#),
        (.dotenv, #"export DATABASE_URL="postgres://${HOST}/app" # local"#),
        (.diff, "+added line"),
        (.diff, "-removed line"),
        (.diff, "@@ -1,3 +1,4 @@"),
    ]
    return VStack(alignment: .leading, spacing: 6) {
        ForEach(samples.indices, id: \.self) { index in
            Text(SyntaxHighlighter.highlight(samples[index].1, language: samples[index].0))
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textPrimary)
        }
    }
    .padding(24)
    .background(Theme.codeBackground)
}
