/**
 * Port of `SyntaxHighlighter.swift`: line-oriented, regex-based syntax
 * colouring. One precompiled alternation per language turns a line into
 * tokens (strings, comments, keywords, numbers, JSON keys); `highlightLine`
 * turns those into spans carrying `token-*` class names that map onto the
 * `--code-*` tokens (the rules live in `CodeBlock.module.css` as global
 * classes, imported here so every consumer gets them). Everything works on a
 * single line, so streaming output and windowed rows never need context
 * from neighbouring lines.
 */
import { createElement, type ReactNode } from 'react'
import './CodeBlock.module.css'

/** `SyntaxHighlighter.Language` in Swift. */
export type Language =
  | 'json'
  | 'swift'
  | 'rust'
  | 'python'
  | 'javascript'
  | 'typescript'
  | 'go'
  | 'shell'
  | 'html'
  | 'css'
  | 'markdown'
  | 'yaml'
  | 'toml'
  | 'dotenv'
  | 'diff'
  | 'plain'

/** Every language, in the Swift `CaseIterable` order. */
export const LANGUAGES: readonly Language[] = [
  'json',
  'swift',
  'rust',
  'python',
  'javascript',
  'typescript',
  'go',
  'shell',
  'html',
  'css',
  'markdown',
  'yaml',
  'toml',
  'dotenv',
  'diff',
  'plain',
]

/** `SyntaxHighlighter.TokenKind` in Swift. */
export type TokenKind =
  | 'string'
  | 'comment'
  | 'keyword'
  | 'number'
  | 'key'
  /** Only from the editor's tree-sitter highlighting; the regex grammars never emit them. */
  | 'type'
  | 'function'
  | 'diffAdded'
  | 'diffRemoved'
  | 'diffHunk'
  | 'diffMeta'

/** `SyntaxHighlighter.Token`: a UTF-16 range of one line. */
export interface Token {
  start: number
  end: number
  kind: TokenKind
}

/** One run of a highlighted line: plain text (no class) or a token. */
export interface HighlightSpan {
  text: string
  /** `token-string`, `token-keyword`, … (see `tokenClassName`); absent for plain text. */
  className?: string
  kind?: TokenKind
}

/**
 * Lines longer than this are left plain: a pathological minified line must
 * never stall the main thread (`SyntaxHighlighter.maxScanLength`).
 */
export const maxScanLength = 4000

/**
 * `SyntaxHighlighter.Language.detect(_:)`: maps a fence info string, a file
 * extension, a path or a format label ("JSON", "stdout") onto a language;
 * unknown → `'plain'`. Windows paths (`src\app.rs`) are split on both
 * separators.
 */
export function detectLanguage(hint: string | null | undefined): Language {
  if (hint === null || hint === undefined) return 'plain'
  let key = hint.trim().toLowerCase()
  if (key.length === 0) return 'plain'
  const firstWord = key.split(/[ \t]+/).find((word) => word.length > 0)
  if (firstWord) key = firstWord
  // `.env`, `.env.local`, `config/.env.production`: the extension says nothing.
  const fileName = lastPathComponent(key)
  if (fileName === '.env' || fileName.startsWith('.env.')) return 'dotenv'
  if (fileName === '.envrc' || fileName === '.zshrc' || fileName === '.bashrc' || fileName === '.profile') {
    return 'shell'
  }
  if (key.includes('/') || key.includes('\\') || key.includes('.')) {
    const name = lastPathComponent(key)
    const dot = name.lastIndexOf('.')
    key = dot > 0 ? name.slice(dot + 1) : name
  }
  switch (key) {
    case 'json':
    case 'jsonc':
    case 'json5':
      return 'json'
    case 'swift':
      return 'swift'
    case 'rs':
    case 'rust':
      return 'rust'
    case 'py':
    case 'python':
    case 'python3':
    case 'pyi':
      return 'python'
    case 'js':
    case 'javascript':
    case 'jsx':
    case 'mjs':
    case 'cjs':
    case 'node':
      return 'javascript'
    case 'ts':
    case 'typescript':
    case 'tsx':
    case 'mts':
    case 'cts':
      return 'typescript'
    case 'go':
    case 'golang':
      return 'go'
    case 'sh':
    case 'bash':
    case 'zsh':
    case 'shell':
    case 'console':
    case 'fish':
    case 'ksh':
    case 'shellscript':
      return 'shell'
    case 'html':
    case 'htm':
    case 'xhtml':
    case 'xml':
    case 'svg':
    case 'vue':
    case 'svelte':
      return 'html'
    case 'css':
    case 'scss':
    case 'sass':
    case 'less':
    case 'pcss':
    case 'postcss':
      return 'css'
    case 'md':
    case 'markdown':
    case 'mdx':
    case 'mdown':
    case 'mkd':
      return 'markdown'
    case 'yaml':
    case 'yml':
      return 'yaml'
    case 'toml':
      return 'toml'
    case 'env':
    case 'dotenv':
      return 'dotenv'
    case 'diff':
    case 'patch':
      return 'diff'
    default:
      return 'plain'
  }
}

function lastPathComponent(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? path
}

/** `Language.displayName`: the label for code block headers. */
export function languageDisplayName(language: Language): string {
  switch (language) {
    case 'json':
      return 'JSON'
    case 'swift':
      return 'Swift'
    case 'rust':
      return 'Rust'
    case 'python':
      return 'Python'
    case 'javascript':
      return 'JavaScript'
    case 'typescript':
      return 'TypeScript'
    case 'go':
      return 'Go'
    case 'shell':
      return 'Shell'
    case 'html':
      return 'HTML'
    case 'css':
      return 'CSS'
    case 'markdown':
      return 'Markdown'
    case 'yaml':
      return 'YAML'
    case 'toml':
      return 'TOML'
    case 'dotenv':
      return 'Env'
    case 'diff':
      return 'Diff'
    case 'plain':
      return 'Plain text'
  }
}

/** The CSS class a token kind renders with (`SyntaxHighlighter.color(for:)`). */
export function tokenClassName(kind: TokenKind): string {
  switch (kind) {
    case 'string':
      return 'token-string'
    case 'comment':
      return 'token-comment'
    case 'keyword':
      return 'token-keyword'
    case 'number':
      return 'token-number'
    case 'key':
      return 'token-key'
    case 'type':
      return 'token-type'
    case 'function':
      return 'token-function'
    case 'diffAdded':
      return 'token-diff-added'
    case 'diffRemoved':
      return 'token-diff-removed'
    case 'diffHunk':
      return 'token-diff-hunk'
    case 'diffMeta':
      return 'token-diff-meta'
  }
}

/** `SyntaxHighlighter.tokenize(_:language:)`: non-overlapping tokens in order of appearance. */
export function tokenize(line: string, language: Language): Token[] {
  if (line.length === 0 || line.length > maxScanLength) return []
  switch (language) {
    case 'plain':
      return []
    case 'diff':
      return diffTokens(line)
    default: {
      const grammar = grammarFor(language)
      return grammar ? grammarTokens(line, grammar) : []
    }
  }
}

/**
 * `SyntaxHighlighter.highlight(_:language:)`: the spans of one line. Their
 * texts concatenate to exactly `line`.
 */
export function highlightLine(line: string, language: Language): HighlightSpan[] {
  const tokens = tokenize(line, language)
  if (tokens.length === 0) return line.length > 0 ? [{ text: line }] : []
  const spans: HighlightSpan[] = []
  let cursor = 0
  for (const token of tokens) {
    if (token.start > cursor) spans.push({ text: line.slice(cursor, token.start) })
    spans.push({ text: line.slice(token.start, token.end), className: tokenClassName(token.kind), kind: token.kind })
    cursor = token.end
  }
  if (cursor < line.length) spans.push({ text: line.slice(cursor) })
  return spans
}

/**
 * Highlights every line of `text` on its own (the grammar never needs
 * context from neighbours), for `LineList` and `CodeBlock`. A trailing
 * newline does not produce an extra empty line.
 */
export function highlightLines(text: string, language: Language): HighlightSpan[][] {
  return splitLines(text).map((line) => highlightLine(line, language))
}

/** Lines of `text` without their line endings; a trailing newline adds no empty line. */
export function splitLines(text: string): string[] {
  if (text.length === 0) return []
  const lines = text.split('\n').map((line) => (line.endsWith('\r') ? line.slice(0, -1) : line))
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop()
  return lines
}

/**
 * The spans of one line as React nodes: a string for plain text, a
 * `<span class="token-…">` for a token.
 */
export function renderSpans(spans: readonly HighlightSpan[], keyPrefix = ''): ReactNode[] {
  return spans.map((span, index) =>
    span.className
      ? createElement('span', { key: `${keyPrefix}${index}`, className: span.className }, span.text)
      : span.text,
  )
}

/** Stable per-language line renderers, so `LineList`'s memo survives re-renders. */
const lineRenderers = new Map<Language, (line: string) => ReactNode>()

/**
 * The `highlight` callback for `LineList` (`LineList(text:highlight:)` in
 * Swift): renders one line as token spans. `undefined` for plain text, so
 * the list keeps its single-string fast path. The function is the same
 * object for every call with the same language.
 */
export function lineHighlighter(language: Language): ((line: string) => ReactNode) | undefined {
  if (language === 'plain') return undefined
  let renderer = lineRenderers.get(language)
  if (!renderer) {
    renderer = (line: string) => renderSpans(highlightLine(line, language))
    lineRenderers.set(language, renderer)
  }
  return renderer
}

/** The words the grammar colours as keywords, for the editor's completion. */
export function keywordsFor(language: Language): readonly string[] {
  switch (language) {
    case 'json':
      return ['true', 'false', 'null']
    case 'swift':
      return swiftKeywords
    case 'rust':
      return rustKeywords
    case 'python':
      return pythonKeywords
    case 'javascript':
      return javascriptKeywords
    case 'typescript':
      return typescriptKeywords
    case 'go':
      return goKeywords
    case 'shell':
      return shellKeywords
    case 'html':
      return htmlKeywords
    case 'css':
      return cssKeywords
    case 'yaml':
      return ['true', 'false', 'null', 'yes', 'no', 'on', 'off']
    case 'toml':
      return ['true', 'false']
    case 'dotenv':
      return ['export', 'true', 'false']
    case 'markdown':
    case 'diff':
    case 'plain':
      return []
  }
}

// MARK: - Grammars

interface Grammar {
  regex: RegExp
  /** `kinds[i]` is the token kind of capture group `i + 1`. */
  kinds: TokenKind[]
}

function grammarTokens(line: string, grammar: Grammar): Token[] {
  const tokens: Token[] = []
  const regex = grammar.regex
  regex.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = regex.exec(line)) !== null) {
    const whole = match[0]
    if (whole.length === 0) {
      regex.lastIndex += 1
      continue
    }
    for (let group = 0; group < grammar.kinds.length; group += 1) {
      const captured = match[group + 1]
      const kind = grammar.kinds[group]
      if (captured !== undefined && captured.length > 0 && kind !== undefined) {
        tokens.push({ start: match.index, end: match.index + whole.length, kind })
        break
      }
    }
  }
  return tokens
}

type GrammarPart = [TokenKind, string]

/**
 * Builds one alternation `(a)|(b)|…` from ordered parts. Parts must use
 * non-capturing groups only, so the capture index identifies the kind.
 */
function grammar(parts: GrammarPart[]): Grammar | null {
  const pattern = parts.map((part) => '(' + part[1] + ')').join('|')
  try {
    return { regex: new RegExp(pattern, 'g'), kinds: parts.map((part) => part[0]) }
  } catch {
    return null
  }
}

function keywords(words: readonly string[]): string {
  return String.raw`\b(?:` + words.join('|') + String.raw`)\b`
}

const doubleQuoted = String.raw`"(?:[^"\\]|\\.)*"`
const singleQuoted = String.raw`'(?:[^'\\]|\\.)*'`
const backtickQuoted = '`[^`]*`'
const slashComment = String.raw`//.*`
const blockComment = String.raw`/\*(?:.*?\*/|.*)`
const hashComment = String.raw`#.*`
const shellComment = String.raw`(?:^|(?<=\s))#.*`
const decimalNumber = String.raw`\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b`
const jsonNumber = String.raw`-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b`
const jsonKey = String.raw`"(?:[^"\\]|\\.)*"(?=\s*:)`

const swiftKeywords = [
  'associatedtype', 'class', 'deinit', 'enum', 'extension', 'func', 'import', 'init', 'inout', 'internal',
  'let', 'operator', 'private', 'fileprivate', 'precedencegroup', 'protocol', 'public', 'open', 'rethrows',
  'static', 'struct', 'subscript', 'typealias', 'var', 'break', 'case', 'catch', 'continue', 'default',
  'defer', 'do', 'else', 'fallthrough', 'for', 'guard', 'if', 'in', 'repeat', 'return', 'throw', 'switch',
  'where', 'while', 'as', 'Any', 'false', 'is', 'nil', 'self', 'Self', 'super', 'throws', 'true', 'try',
  'some', 'any', 'await', 'async', 'actor', 'macro', 'consuming', 'borrowing', 'nonisolated', 'weak',
  'unowned', 'lazy', 'override', 'mutating', 'nonmutating', 'final', 'required', 'convenience', 'indirect',
  'dynamic', 'optional', 'get', 'set', 'willSet', 'didSet',
]
const rustKeywords = [
  'as', 'async', 'await', 'break', 'const', 'continue', 'crate', 'dyn', 'else', 'enum', 'extern', 'false',
  'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod', 'move', 'mut', 'pub', 'ref', 'return',
  'self', 'Self', 'static', 'struct', 'super', 'trait', 'true', 'type', 'unsafe', 'use', 'where', 'while',
  'union', 'macro_rules', 'Some', 'None', 'Ok', 'Err',
]
const pythonKeywords = [
  'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await', 'break', 'class', 'continue', 'def',
  'del', 'elif', 'else', 'except', 'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda',
  'nonlocal', 'not', 'or', 'pass', 'raise', 'return', 'try', 'while', 'with', 'yield', 'match', 'case', 'self',
]
const javascriptKeywords = [
  'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger', 'default', 'delete', 'do', 'else',
  'enum', 'export', 'extends', 'false', 'finally', 'for', 'function', 'if', 'import', 'in', 'instanceof',
  'new', 'null', 'return', 'super', 'switch', 'this', 'throw', 'true', 'try', 'typeof', 'var', 'void',
  'while', 'with', 'yield', 'let', 'static', 'async', 'await', 'of', 'undefined', 'NaN', 'get', 'set',
]
const typescriptKeywords = [
  ...javascriptKeywords,
  'interface', 'type', 'implements', 'declare', 'namespace', 'module', 'abstract', 'readonly', 'private',
  'protected', 'public', 'as', 'keyof', 'any', 'number', 'string', 'boolean', 'never', 'unknown', 'object',
  'symbol', 'bigint', 'is', 'infer', 'satisfies', 'override',
]
const goKeywords = [
  'break', 'case', 'chan', 'const', 'continue', 'default', 'defer', 'else', 'fallthrough', 'for', 'func',
  'go', 'goto', 'if', 'import', 'interface', 'map', 'package', 'range', 'return', 'select', 'struct',
  'switch', 'type', 'var', 'true', 'false', 'nil', 'iota', 'append', 'len', 'cap', 'make', 'new', 'panic',
  'recover', 'print', 'println', 'error', 'string', 'int', 'int8', 'int16', 'int32', 'int64', 'uint',
  'uint8', 'uint16', 'uint32', 'uint64', 'float32', 'float64', 'bool', 'byte', 'rune',
]
const shellKeywords = [
  'if', 'then', 'else', 'elif', 'fi', 'for', 'while', 'until', 'do', 'done', 'case', 'esac', 'in',
  'function', 'select', 'time', 'return', 'exit', 'export', 'local', 'readonly', 'declare', 'unset',
  'shift', 'source', 'alias', 'cd', 'echo', 'set', 'eval', 'exec', 'trap', 'true', 'false',
]
const htmlKeywords = [
  'html', 'head', 'body', 'title', 'meta', 'link', 'script', 'style', 'div', 'span', 'section', 'article',
  'header', 'footer', 'main', 'nav', 'aside', 'button', 'input', 'label', 'select', 'option', 'textarea',
  'form', 'table', 'thead', 'tbody', 'tr', 'td', 'th', 'img', 'picture', 'source', 'video', 'audio', 'canvas',
  'template', 'slot', 'dialog', 'details', 'summary', 'figure', 'figcaption', 'strong', 'code', 'pre',
  'class', 'href', 'src', 'alt', 'type', 'name', 'value', 'placeholder', 'disabled', 'checked', 'hidden',
  'aria-label', 'aria-hidden', 'role', 'tabindex', 'target', 'rel', 'width', 'height', 'loading', 'charset',
  'content', 'viewport', 'async', 'defer', 'onclick', 'data-',
]
const cssKeywords = [
  'display', 'position', 'relative', 'absolute', 'fixed', 'sticky', 'flex', 'grid', 'inline-flex',
  'inline-block', 'block', 'none', 'flex-direction', 'flex-wrap', 'justify-content', 'align-items',
  'align-self', 'gap', 'grid-template-columns', 'grid-template-rows', 'margin', 'padding', 'border',
  'border-radius', 'width', 'height', 'max-width', 'min-width', 'max-height', 'min-height', 'color',
  'background', 'background-color', 'font', 'font-family', 'font-size', 'font-weight', 'line-height',
  'letter-spacing', 'text-align', 'text-decoration', 'text-transform', 'white-space', 'overflow',
  'opacity', 'transform', 'transition', 'animation', 'cursor', 'z-index', 'top', 'right', 'bottom', 'left',
  'inset', 'box-shadow', 'box-sizing', 'border-box', 'content', 'outline', 'visibility', 'pointer-events',
  'user-select', 'object-fit', 'aspect-ratio', 'center', 'space-between', 'space-around', 'wrap', 'column',
  'row', 'auto', 'inherit', 'initial', 'unset', 'transparent', 'currentColor', 'important', 'hover', 'focus',
  'focus-visible', 'active', 'disabled', 'before', 'after', 'first-child', 'last-child', 'nth-child',
  'media', 'supports', 'keyframes', 'import', 'font-face', 'layer', 'container', 'var', 'calc', 'clamp',
  'min', 'max', 'rgb', 'rgba', 'hsl', 'linear-gradient', 'repeat', 'minmax', 'fr',
]

const grammarParts: Record<Exclude<Language, 'diff' | 'plain'>, GrammarPart[]> = {
  json: [
    ['key', jsonKey],
    ['string', doubleQuoted],
    ['number', jsonNumber],
    ['keyword', keywords(['true', 'false', 'null'])],
  ],
  swift: [
    ['string', doubleQuoted],
    ['comment', slashComment],
    ['comment', blockComment],
    ['number', decimalNumber],
    ['keyword', keywords(swiftKeywords)],
  ],
  rust: [
    ['string', doubleQuoted],
    ['comment', slashComment],
    ['comment', blockComment],
    [
      'number',
      String.raw`\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?(?:[iuf](?:8|16|32|64|128|size))?)\b`,
    ],
    ['keyword', keywords(rustKeywords)],
  ],
  python: [
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['comment', hashComment],
    ['number', decimalNumber],
    ['keyword', keywords(pythonKeywords)],
  ],
  javascript: [
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['string', backtickQuoted],
    ['comment', slashComment],
    ['comment', blockComment],
    ['number', decimalNumber],
    ['keyword', keywords(javascriptKeywords)],
  ],
  typescript: [
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['string', backtickQuoted],
    ['comment', slashComment],
    ['comment', blockComment],
    ['number', decimalNumber],
    ['keyword', keywords(typescriptKeywords)],
  ],
  go: [
    ['string', doubleQuoted],
    ['string', backtickQuoted],
    ['string', singleQuoted],
    ['comment', slashComment],
    ['comment', blockComment],
    ['number', decimalNumber],
    ['keyword', keywords(goKeywords)],
  ],
  shell: [
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['comment', shellComment],
    ['number', String.raw`\b\d+\b`],
    ['keyword', keywords(shellKeywords)],
  ],
  html: [
    ['comment', String.raw`<!--(?:.*?-->|.*)`],
    // ICU's `(?i:doctype)` has no JavaScript equivalent.
    ['comment', String.raw`<![Dd][Oo][Cc][Tt][Yy][Pp][Ee][^>]*>`],
    ['keyword', String.raw`</?[A-Za-z][A-Za-z0-9\-:]*|/?>`],
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['key', String.raw`(?<=\s)[A-Za-z_:@#\[(][A-Za-z0-9_:.\-\])]*(?==)`],
    ['number', String.raw`&[A-Za-z0-9#]+;`],
  ],
  css: [
    ['comment', blockComment],
    ['comment', String.raw`(?:^|(?<=\s))//.*`],
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['keyword', String.raw`@[A-Za-z\-]+|!important\b`],
    ['key', String.raw`(?:^|(?<=[\s{;]))-{0,2}[A-Za-z_][A-Za-z0-9_\-]*(?=\s*:(?!:))|\$[A-Za-z_][A-Za-z0-9_\-]*`],
    [
      'number',
      String.raw`#[0-9A-Fa-f]{3,8}\b(?![\w\-]*\s*[{,])|(?<![\w\-])-?\d*\.?\d+(?:px|em|rem|%|vh|vw|vmin|vmax|ch|ex|s|ms|deg|rad|turn|fr|dpi|dppx|pt|cm|mm|in)?\b`,
    ],
    ['keyword', keywords(['important', 'inherit', 'initial', 'unset', 'revert', 'auto', 'none'])],
  ],
  markdown: [
    ['keyword', String.raw`^\s{0,3}#{1,6}(?:\s.*)?$`],
    ['comment', String.raw`^\s{0,3}>.*`],
    ['comment', String.raw`<!--(?:.*?-->|.*)`],
    ['string', String.raw`^\s*(?:` + '```' + String.raw`|~~~).*`],
    ['string', '`[^`]+`'],
    ['number', String.raw`!?\[[^\]]*\]\([^)]*\)`],
    ['key', String.raw`\*\*[^*]+\*\*|__[^_]+__`],
    ['keyword', String.raw`^\s*(?:[-*+]|\d+[.)])(?=\s)`],
  ],
  yaml: [
    ['comment', shellComment],
    ['key', String.raw`^\s*(?:-\s+)?[A-Za-z0-9_.\-/"'$<]+(?=\s*:(?:\s|$))`],
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['keyword', String.raw`^---$|^\.\.\.$|[&*][A-Za-z0-9_\-]+`],
    ['number', String.raw`(?<=[:\-\[,]\s)-?\d+(?:\.\d+)?\b`],
    [
      'keyword',
      keywords(['true', 'false', 'null', 'yes', 'no', 'on', 'off', 'True', 'False', 'Null', 'TRUE', 'FALSE']),
    ],
  ],
  toml: [
    ['comment', hashComment],
    ['keyword', String.raw`^\s*\[\[?[^\]]*\]\]?`],
    ['key', String.raw`^\s*[A-Za-z0-9_.\-"']+(?=\s*=)`],
    ['string', String.raw`"{3}.*?(?:"{3}|$)|'{3}.*?(?:'{3}|$)`],
    ['string', doubleQuoted],
    ['string', singleQuoted],
    [
      'number',
      String.raw`\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?\b|[+-]?\b(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b`,
    ],
    ['keyword', keywords(['true', 'false', 'inf', 'nan'])],
  ],
  dotenv: [
    ['comment', String.raw`^\s*#.*|(?<=\s)#.*`],
    ['keyword', String.raw`^\s*export(?=\s)`],
    // ICU's `\G` (anchor at the previous match) becomes a lookbehind that
    // only admits a key at the start of the line, after an optional `export`.
    ['key', String.raw`(?<=^(?:\s*export)?\s*)[A-Za-z_][A-Za-z0-9_.\-]*(?=\s*=)`],
    ['string', doubleQuoted],
    ['string', singleQuoted],
    ['number', String.raw`(?<==)\s*-?\d+(?:\.\d+)?\s*$`],
    ['keyword', String.raw`\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*`],
  ],
}

const compiled = new Map<Language, Grammar | null>()

function grammarFor(language: Language): Grammar | null {
  if (language === 'diff' || language === 'plain') return null
  const cached = compiled.get(language)
  if (cached !== undefined) return cached
  const built = grammar(grammarParts[language])
  compiled.set(language, built)
  return built
}

// MARK: - Diff

function diffTokens(line: string): Token[] {
  let kind: TokenKind | null
  if (line.startsWith('+++ ') || line.startsWith('--- ')) {
    kind = 'diffMeta'
  } else if (line.startsWith('+')) {
    kind = 'diffAdded'
  } else if (line.startsWith('-')) {
    kind = 'diffRemoved'
  } else if (line.startsWith('@@')) {
    kind = 'diffHunk'
  } else if (
    line.startsWith('diff ') ||
    line.startsWith('index ') ||
    line.startsWith('new file') ||
    line.startsWith('deleted file') ||
    line.startsWith('rename ') ||
    line.startsWith('similarity ') ||
    line.startsWith('Binary files')
  ) {
    kind = 'diffMeta'
  } else {
    kind = null
  }
  if (kind === null) return []
  return [{ start: 0, end: line.length, kind }]
}
