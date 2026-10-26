import { useCallback, useEffect, useRef, useState } from 'react'
import {
  acceptCompletion,
  autocompletion,
  startCompletion,
  closeCompletion,
  completionStatus,
  currentCompletions,
  moveCompletionSelection,
  pickedCompletion,
  selectedCompletionIndex,
  setSelectedCompletion,
  type Completion,
} from '@codemirror/autocomplete'
import { history, historyKeymap, standardKeymap } from '@codemirror/commands'
import { indentUnit, LanguageDescription, LanguageSupport, StreamLanguage, syntaxHighlighting, syntaxTree, type StreamParser } from '@codemirror/language'
import { setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { search, searchKeymap } from '@codemirror/search'
import { Compartment, EditorSelection, EditorState, Prec, RangeSetBuilder, StateField, type Extension } from '@codemirror/state'
import { Decoration, drawSelection, dropCursor, EditorView, keymap, lineNumbers, ViewPlugin, type Command, type DecorationSet, type KeyBinding, type ViewUpdate } from '@codemirror/view'
import { tagHighlighter, tags } from '@lezer/highlight'
import { lspDiagnosticIsError, lspDiagnosticIsWarning, type LspDiagnostic } from '../../backend/types'
import { isMac, isModKey } from '../../components/platform'
import { keywordsFor, type Language } from '../../markdown/highlighter'
import {
  findSymbols,
  jumpToDefinition,
  languageServerCompletions,
  languageServerHover,
  save,
  scheduleAutoSave,
  scheduleLanguageServerSync,
  type EditorDocument,
} from '../../state/editor'
import type { ProjectState } from '../../state/project'
import {
  bufferDefinitionItem,
  CompletionPanel,
  CompletionSession,
  keywordCompletionItem,
  serverCompletionItem,
  symbolCompletionItem,
  wordCompletionItem,
  type CompletionHost,
  type CompletionItem,
} from './CompletionPanel'
import { createFindPanel } from './EditorFindBar'
import styles from './CodeEditor.module.css'

// Port of apps/macos/ContextAgent/Views/Editor/CodeEditor.swift with
// CodeMirror 6 in place of the TextKit 1 `NSTextView`: the line-number
// ruler (`LineNumberRuler`) is CodeMirror's gutter at the Swift ruler's
// width and type, the tree-sitter / `SyntaxHighlighter` colouring is the
// CodeMirror language package for the file type mapped onto the same token
// colours (`TreeSitterHighlighter.kind(of:in:)`), and the Coordinator's
// behaviour is `EditorController` below: the file's own indentation on ⏎ /
// ⇥, undo, the find bar, ⌘S / Ctrl+S, ⌘-click / Ctrl-click to the
// definition, the completion popup (CompletionPanel.tsx), the server's
// diagnostics underlined (`@codemirror/lint`), the caret's line and column
// and the symbol under it for the status bar.

export interface CodeEditorProps {
  project: ProjectState
  document: EditorDocument
}

/** What the popup draws (`CompletionModel`), published by the controller. */
interface PanelSnapshot {
  view: EditorView
  options: readonly Completion[]
  items: readonly CompletionItem[]
  selectedIndex: number
  prefix: string
  from: number
}

/**
 * `CodeEditor` (Views/Editor/CodeEditor.swift): the editable text of one
 * `EditorDocument`. Recreated per document (the Editor gives it the
 * document's path as its key), like the Swift `NSViewRepresentable`.
 */
export function CodeEditor({ project, document }: CodeEditorProps) {
  const hostRef = useRef<HTMLDivElement>(null)
  const controllerRef = useRef<EditorController | null>(null)
  const [panel, setPanel] = useState<PanelSnapshot | null>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const controller = new EditorController(host, project, document, setPanel)
    controllerRef.current = controller
    return () => {
      controller.destroy()
      controllerRef.current = null
      setPanel(null)
    }
  }, [project, document])

  const pick = useCallback((index: number) => controllerRef.current?.pick(index), [])

  return (
    <>
      <div ref={hostRef} className={styles.editor} />
      {panel ? (
        <CompletionPanel view={panel.view} items={panel.items} selectedIndex={panel.selectedIndex} prefix={panel.prefix} from={panel.from} onPick={pick} />
      ) : null}
    </>
  )
}

// MARK: - Languages

/** The CodeMirror package a file gets (the tree-sitter grammar or regex highlighter of the Swift app). */
type LanguageKey =
  | 'rust'
  | 'python'
  | 'javascript'
  | 'jsx'
  | 'typescript'
  | 'tsx'
  | 'go'
  | 'java'
  | 'json'
  | 'markdown'
  | 'yaml'
  | 'html'
  | 'css'
  | 'toml'
  | 'shell'
  | 'swift'
  | 'dotenv'
  | 'diff'

/** `TreeSitterGrammars.name(forPath:language:)`'s counterpart. */
function languageKey(path: string, language: Language): LanguageKey | null {
  const ext = pathExtension(path)
  if (ext === 'java') return 'java'
  if (ext === 'tsx') return 'tsx'
  if (ext === 'jsx') return 'jsx'
  switch (language) {
    case 'rust':
    case 'python':
    case 'javascript':
    case 'typescript':
    case 'go':
    case 'json':
    case 'markdown':
    case 'yaml':
    case 'html':
    case 'css':
    case 'toml':
    case 'shell':
    case 'swift':
    case 'dotenv':
    case 'diff':
      return language
    case 'plain':
      return null
  }
}

/** Starts loading the package a file will need (the Editor calls it while the file is read). */
export function preloadLanguage(path: string, language: Language): void {
  const key = languageKey(path, language)
  if (key !== null) void loadLanguage(key)
}

/** Loaded once per session; the packages are split out of the main bundle. */
const languageLoads = new Map<LanguageKey, Promise<LanguageSupport>>()
const loadedLanguages = new Map<LanguageKey, LanguageSupport>()

function loadLanguage(key: LanguageKey): Promise<LanguageSupport> {
  let load = languageLoads.get(key)
  if (!load) {
    load = importLanguage(key).then((support) => {
      loadedLanguages.set(key, support)
      return support
    })
    languageLoads.set(key, load)
  }
  return load
}

async function importLanguage(key: LanguageKey): Promise<LanguageSupport> {
  switch (key) {
    case 'rust':
      return (await import('@codemirror/lang-rust')).rust()
    case 'python':
      return (await import('@codemirror/lang-python')).python()
    case 'javascript':
      return (await import('@codemirror/lang-javascript')).javascript()
    case 'jsx':
      return (await import('@codemirror/lang-javascript')).javascript({ jsx: true })
    case 'typescript':
      return (await import('@codemirror/lang-javascript')).javascript({ typescript: true })
    case 'tsx':
      return (await import('@codemirror/lang-javascript')).javascript({ typescript: true, jsx: true })
    case 'go':
      return (await import('@codemirror/lang-go')).go()
    case 'java':
      return (await import('@codemirror/lang-java')).java()
    case 'json':
      return (await import('@codemirror/lang-json')).json()
    case 'markdown': {
      // Fenced code is coloured in its own language (tree-sitter's injections).
      const { markdown, markdownLanguage } = await import('@codemirror/lang-markdown')
      return markdown({ base: markdownLanguage, codeLanguages: markdownCodeLanguages() })
    }
    case 'yaml':
      return (await import('@codemirror/lang-yaml')).yaml()
    case 'html':
      return (await import('@codemirror/lang-html')).html()
    case 'css':
      return (await import('@codemirror/lang-css')).css()
    case 'toml':
      return new LanguageSupport(StreamLanguage.define(treeSitterLikeToml((await import('@codemirror/legacy-modes/mode/toml')).toml)))
    case 'shell':
      return new LanguageSupport(StreamLanguage.define(treeSitterLikeShell((await import('@codemirror/legacy-modes/mode/shell')).shell)))
    case 'swift':
      return new LanguageSupport(StreamLanguage.define(treeSitterLikeSwift((await import('@codemirror/legacy-modes/mode/swift')).swift)))
    case 'dotenv':
      return new LanguageSupport(StreamLanguage.define((await import('@codemirror/legacy-modes/mode/properties')).properties))
    case 'diff':
      return new LanguageSupport(StreamLanguage.define((await import('@codemirror/legacy-modes/mode/diff')).diff))
  }
}

interface SwiftState<S> {
  base: S
  /** The previous token was a member access's dot. */
  afterDot: boolean
  /** The last keyword, so the name after `func` is told from the one after `class` or `import`. */
  keyword: string | null
}

/** Words the legacy mode colours as keywords that tree-sitter-swift leaves alone (or calls types). */
const SWIFT_PLAIN_WORDS = new Set(['init', 'new', 'left', 'right', 'none', 'assignment', 'associativity', 'precedence', 'dynamicType', '_'])
const SWIFT_TYPE_WORDS = new Set(['Any', 'AnyObject', 'Type', 'Protocol'])
const SWIFT_KEYWORD_ATTRIBUTES = new Set(['@escaping', '@autoclosure'])

/// The legacy Swift mode re-tokenised the way tree-sitter-swift's highlight
/// queries colour Swift in the macOS editor (`TreeSitterHighlighter.kind`):
/// a call is a function (a capitalised one, `Foo(`, a constructor in the
/// type colour), other capitalised names are types, a member after a dot is
/// plain unless it is called (the dot itself never coloured), the name after
/// `func` is a function, the names after `class`, `let` or `import` and an
/// `init` declaration stay plain.
function treeSitterLikeSwift<S>(base: StreamParser<S>): StreamParser<SwiftState<S>> {
  // StreamLanguage's own default: a copy of the object with its arrays (the token stack) copied.
  const copyBase = (state: S): S => {
    if (base.copyState) return base.copyState(state)
    if (typeof state !== 'object' || state === null) return state
    const copy: Record<string, unknown> = {}
    for (const [key, value] of Object.entries(state)) copy[key] = Array.isArray(value) ? value.slice() : value
    return copy as S
  }
  const called = (stream: { match: (pattern: RegExp, consume?: boolean) => unknown }) => Boolean(stream.match(/^[?!]?\s*\(/, false))
  const callOrName = (name: string, isCalled: boolean, otherwise: string | null): string | null => {
    if (isCalled) return /^[A-Z][a-z0-9]/.test(name) ? 'typeName' : 'variableName.function'
    return /^[A-Z]/.test(name) ? 'typeName' : otherwise
  }
  return {
    name: 'swift',
    startState: (indent) => ({ base: base.startState ? base.startState(indent) : ({} as S), afterDot: false, keyword: null }),
    copyState: (state) => ({ base: copyBase(state.base), afterDot: state.afterDot, keyword: state.keyword }),
    indent: base.indent ? (state, textAfter, context) => base.indent?.(state.base, textAfter, context) ?? null : undefined,
    languageData: base.languageData,
    token(stream, state) {
      const afterDot = state.afterDot
      state.afterDot = false
      const style = base.token(stream, state.base)
      const text = stream.current()
      switch (style) {
        case 'property':
          // `.name`: the dot alone now, the name on the next call.
          if (text.length > 1) stream.backUp(text.length - 1)
          state.afterDot = true
          return 'punctuation'
        case 'variable':
        case 'keyword':
        case 'atom':
        case 'type':
        case 'def': {
          if (afterDot) return callOrName(text.replace(/`/g, ''), called(stream), null)
          if (style === 'keyword') {
            state.keyword = text
            if (SWIFT_TYPE_WORDS.has(text)) return 'typeName'
            if (!SWIFT_PLAIN_WORDS.has(text)) return 'keyword'
            return called(stream) && text !== 'init' ? 'variableName.function' : null
          }
          if (style === 'atom') return text === '_' ? null : 'keyword'
          if (style === 'type') return 'typeName'
          if (style === 'def') return state.keyword === 'func' ? 'variableName.function' : null
          return callOrName(text, called(stream), null)
        }
        case 'attribute':
          return SWIFT_KEYWORD_ATTRIBUTES.has(text) ? 'keyword' : 'attribute'
        default:
          return style ?? null
      }
    },
  }
}

/** The fields of the legacy TOML mode's state that the wrapper reads. */
interface LegacyTomlState {
  inString?: boolean
  inArray?: number
  lhs?: boolean
}

interface TomlState {
  base: unknown
  /** Inside a `[table]` / `[[array]]` header: 0 outside, 1 after the brackets, 2 after the name. */
  header: 0 | 1 | 2
}

/// The legacy TOML mode re-tokenised like tree-sitter-toml's queries: a
/// table header's name is a type with plain brackets (the legacy mode
/// colours the whole header as an atom), a date is a special string, keys
/// keep the property colour.
function treeSitterLikeToml(base: StreamParser<unknown>): StreamParser<TomlState> {
  return {
    name: 'toml',
    startState: (indent) => ({ base: base.startState ? base.startState(indent) : {}, header: 0 }),
    copyState: (state) => ({ base: { ...(state.base as LegacyTomlState) }, header: state.header }),
    languageData: base.languageData,
    token(stream, state) {
      if (stream.sol()) state.header = 0
      const legacy = state.base as LegacyTomlState
      const top = legacy.inString !== true && (legacy.inArray ?? 0) === 0
      if (state.header === 0 && top && (stream.sol() || legacy.lhs !== false) && stream.match(/^\s*\[\[?/)) {
        state.header = 1
        return null
      }
      if (state.header === 1) {
        if (stream.match(/^[^\]#]+/)) {
          state.header = 2
          return 'typeName'
        }
        state.header = 2
      }
      if (state.header === 2) {
        if (stream.match(/^\]\]?/)) return null
        state.header = 0
      }
      const style = base.token(stream, state.base)
      if (style === 'atom' && /^\d/.test(stream.current())) return 'string'
      return style
    },
  }
}

interface ShellState {
  base: unknown
  /** The next word is a command name (line start, after `;` `|` `&` `(` or `then` / `do` …). */
  commandPosition: boolean
  /** The previous line ended with a backslash: this one continues its command. */
  continued: boolean
  /** Inside `NAME=value` before a command: the value is not the command. */
  inAssignment: boolean
}

const SHELL_KEYWORDS = new Set(['if', 'then', 'else', 'elif', 'fi', 'for', 'while', 'until', 'do', 'done', 'case', 'esac', 'in', 'function', 'select', 'time', '!', '{', '}'])
/** Keywords after which a command follows. */
const SHELL_COMMAND_KEYWORDS = new Set(['if', 'then', 'else', 'elif', 'while', 'until', 'do', 'time', '!', '{'])
const SHELL_DECLARATIONS = new Set(['export', 'local', 'declare', 'readonly', 'typeset', 'unset'])
/** After these a command follows (`)` ends a `case` pattern). */
const SHELL_SEPARATORS = new Set([';', '|', '&', '(', ')', '{', '`'])

/// The legacy shell mode re-tokenised like tree-sitter-bash's queries: the
/// command name of every command is a function (not only the legacy list of
/// well-known commands, and those only in command position), flags and
/// other arguments are plain, `$NAME` and assignment names are properties
/// (the key colour), declaration commands are keywords.
function treeSitterLikeShell(base: StreamParser<unknown>): StreamParser<ShellState> {
  const atTop = (state: ShellState) => ((state.base as { tokens?: unknown[] }).tokens?.length ?? 0) === 0
  return {
    name: 'shell',
    startState: (indent) => ({ base: base.startState ? base.startState(indent) : {}, commandPosition: true, continued: false, inAssignment: false }),
    copyState: (state) => {
      const legacy = state.base as { tokens?: unknown[] }
      return { ...state, base: { ...legacy, tokens: legacy.tokens ? legacy.tokens.slice() : [] } }
    },
    languageData: base.languageData,
    token(stream, state) {
      if (stream.sol()) {
        if (!state.continued && atTop(state)) state.commandPosition = true
        state.continued = false
        state.inAssignment = false
      }
      if (atTop(state)) {
        if (stream.eatSpace()) {
          state.inAssignment = false
          return null
        }
        if (state.commandPosition && !state.inAssignment) {
          const assignment = stream.match(/^[A-Za-z_]\w*(?=\+?=)/)
          if (assignment) {
            state.inAssignment = true
            return 'propertyName'
          }
          const word = stream.match(/^[^\s;|&()<>'"`$#\\]+/)
          if (word) {
            const text = stream.current()
            if (SHELL_KEYWORDS.has(text)) {
              state.commandPosition = SHELL_COMMAND_KEYWORDS.has(text)
              return 'keyword'
            }
            state.commandPosition = false
            if (SHELL_DECLARATIONS.has(text)) return 'keyword'
            if (/^\d+$/.test(text)) return 'number'
            // A flag, a test bracket or a `case` pattern (`*)`, `-h|--help)`) is not a command.
            if (text.startsWith('-') || text === '[' || text === '[[' || stream.match(/^\s*[)|]/, false)) return null
            return 'variableName.function'
          }
        }
      }
      const style = base.token(stream, state.base)
      const text = stream.current()
      if (stream.eol() && /\\$/.test(stream.string)) state.continued = true
      if (atTop(state) && style === null && SHELL_SEPARATORS.has(text)) {
        state.commandPosition = true
        state.inAssignment = false
        return null
      }
      switch (style) {
        case 'builtin':
        case 'attribute':
          return null
        case 'def':
          return 'propertyName'
        case 'keyword':
          if (SHELL_COMMAND_KEYWORDS.has(text)) state.commandPosition = true
          return 'keyword'
        default:
          return style ?? null
      }
    },
  }
}

let codeLanguagesCache: LanguageDescription[] | null = null

/** The languages a Markdown fence may name (`TreeSitterGrammars.aliases`). */
function markdownCodeLanguages(): LanguageDescription[] {
  if (codeLanguagesCache) return codeLanguagesCache
  const entries: Array<[LanguageKey, string[]]> = [
    ['rust', ['rs']],
    ['python', ['py', 'python3']],
    ['javascript', ['js', 'mjs', 'cjs', 'node']],
    ['jsx', []],
    ['typescript', ['ts']],
    ['tsx', []],
    ['go', ['golang']],
    ['java', []],
    ['json', ['jsonc', 'json5']],
    ['yaml', ['yml']],
    ['html', ['htm', 'xml', 'svg', 'xhtml']],
    ['css', ['scss', 'less']],
    ['toml', []],
    ['shell', ['sh', 'bash', 'zsh', 'console', 'fish']],
    ['swift', []],
    ['dotenv', ['env']],
    ['diff', ['patch']],
  ]
  codeLanguagesCache = entries.map(([key, alias]) => LanguageDescription.of({ name: key, alias, load: () => loadLanguage(key) }))
  return codeLanguagesCache
}

/// The token colours of `SyntaxHighlighter.color(for:)`, by the capture
/// mapping of `TreeSitterHighlighter.kind(of:in:)`; the classes are styled
/// in CodeEditor.module.css.
const codeHighlighter = tagHighlighter([
  { tag: tags.comment, class: 'ctx-tok-comment' },
  {
    tag: [tags.string, tags.character, tags.escape, tags.regexp, tags.attributeValue, tags.url, tags.link, tags.monospace],
    class: 'ctx-tok-string',
  },
  {
    tag: [tags.keyword, tags.bool, tags.null, tags.atom, tags.self, tags.tagName, tags.heading, tags.standard(tags.variableName)],
    class: 'ctx-tok-keyword',
  },
  { tag: tags.number, class: 'ctx-tok-number' },
  // Not `namespace`: Rust's lower-case path segments (`std::`) stay plain like
  // tree-sitter-rust's; capitalised ones are coloured by `namingConventions`.
  { tag: [tags.typeName, tags.className], class: 'ctx-tok-type' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.macroName], class: 'ctx-tok-function' },
  { tag: [tags.propertyName, tags.attributeName, tags.labelName, tags.meta], class: 'ctx-tok-key' },
  { tag: tags.inserted, class: 'ctx-tok-diff-added' },
  { tag: tags.deleted, class: 'ctx-tok-diff-removed' },
])

// MARK: - Naming conventions

/// What tree-sitter's queries colour by the name alone and the Lezer
/// grammars leave plain: in the Python, JavaScript / TypeScript and Rust
/// grammars a capitalised identifier is a type or constructor (`PRODUCTS`,
/// `None`, `ExitCode`, `Math`; the all-caps constant capture is unmapped, so
/// it does not undo that), a call keeps the function colour; JavaScript's
/// `console` / `window` / `document` / `module` / `arguments` are builtins
/// (keyword colour); a Rust lifetime is a label (the key colour); a
/// Markdown heading's `#` marks are punctuation (plain) in its title colour.
const CONVENTION_NODES: Partial<Record<LanguageKey, ReadonlySet<string>>> = {
  markdown: new Set(['HeaderMark']),
  python: new Set(['VariableName']),
  javascript: new Set(['VariableName', 'VariableDefinition']),
  jsx: new Set(['VariableName', 'VariableDefinition']),
  typescript: new Set(['VariableName', 'VariableDefinition']),
  tsx: new Set(['VariableName', 'VariableDefinition']),
  rust: new Set(['Identifier', 'ScopeIdentifier', 'BoundIdentifier', 'Lifetime']),
}
const JAVASCRIPT_BUILTINS = new Set(['arguments', 'module', 'console', 'window', 'document'])
/** Parents whose identifier child is a name with its own colour (a function, a class, a macro). */
const DEFINITION_PARENTS = new Set(['FunctionItem', 'FunctionDeclaration', 'FunctionDefinition', 'ClassDeclaration', 'ClassDefinition', 'MacroInvocation'])
const typeMark = Decoration.mark({ class: 'ctx-tok-type' })
const keywordMark = Decoration.mark({ class: 'ctx-tok-keyword' })
const keyMark = Decoration.mark({ class: 'ctx-tok-key' })
const plainMark = Decoration.mark({ class: 'ctx-tok-plain' })

function namingConventions(language: () => LanguageKey | null): Extension {
  const build = (view: EditorView): DecorationSet => {
    const key = language()
    const names = key !== null ? CONVENTION_NODES[key] : undefined
    const builder = new RangeSetBuilder<Decoration>()
    if (!names) return builder.finish()
    const tree = syntaxTree(view.state)
    for (const { from, to } of view.visibleRanges) {
      tree.iterate({
        from,
        to,
        enter: (node) => {
          if (!names.has(node.name)) return
          // One token only: error recovery can stretch a node over a stray
          // word and the lines after it, which must keep their own colours.
          if (node.to - node.from > 128) return
          const text = view.state.sliceDoc(node.from, node.to)
          if (node.name === 'Lifetime' || node.name === 'HeaderMark') {
            if (/^(?:'[A-Za-z_]\w*|#{1,6})$/.test(text)) builder.add(node.from, node.to, node.name === 'Lifetime' ? keyMark : plainMark)
            return false
          }
          if (!/^[A-Za-z_$][\w$]*$/.test(text)) return
          const parent = node.node.parent
          if (parent && DEFINITION_PARENTS.has(parent.name)) return
          // The callee keeps the function colour.
          if (parent && parent.name === 'CallExpression' && parent.from === node.from) return
          const first = text.charCodeAt(0)
          if (first >= 0x41 && first <= 0x5a) builder.add(node.from, node.to, typeMark)
          else if (key !== 'python' && key !== 'rust' && JAVASCRIPT_BUILTINS.has(text)) builder.add(node.from, node.to, keywordMark)
        },
      })
    }
    return builder.finish()
  }
  // Above the syntax highlighter's Prec.high, so these marks are the inner
  // elements and their colour wins where they overlap a highlighted range.
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = build(view)
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged || syntaxTree(update.state) !== syntaxTree(update.startState)) {
          this.decorations = build(update.view)
        }
      }
    },
    { decorations: (value) => value.decorations },
  )
  return Prec.highest(plugin)
}

// MARK: - Typed character (shouldChangeTextIn)

/// The single character the last edit typed (`typedIdentifierCharacter` /
/// `typedTrigger` are derived from it), null after any other change.
const lastTypedField = StateField.define<string | null>({
  create: () => null,
  update(value, tr) {
    if (!tr.docChanged) return value
    if (!tr.isUserEvent('input.type')) return null
    const typed: { text: string | null; count: number } = { text: null, count: 0 }
    tr.changes.iterChanges((_fromA, _toA, _fromB, _toB, inserted) => {
      typed.count += 1
      typed.text = inserted.length === 1 && inserted.lines === 1 ? inserted.toString() : null
    })
    return typed.count === 1 ? typed.text : null
  },
})

// MARK: - Keys

/** The find bar's keys (NSTextView `usesFindBar`): find, next / previous, close. */
const FIND_KEYS = new Set(['Mod-f', 'F3', 'Mod-g', 'Escape'])
/** Undo / redo (`allowsUndo`), without CodeMirror's selection-history keys. */
const UNDO_KEYS = new Set(['Mod-u', 'Alt-u'])

/** ↑ ↓ ⏎ ⇥ Esc while the popup is up (`textView(_:doCommandBy:)`'s first switch). */
const completionKeys: KeyBinding[] = [
  { key: 'ArrowDown', run: moveCompletionSelection(true) },
  { key: 'ArrowUp', run: moveCompletionSelection(false) },
  { key: 'Enter', run: acceptCompletion },
  { key: 'Tab', run: acceptCompletion },
  { key: 'Escape', run: closeCompletion },
]

/** ⏎: a newline with the current line's indentation (`currentLineIndent()`). */
const insertNewlineKeepingIndent: Command = (view) => {
  const { state } = view
  if (state.readOnly) return false
  view.dispatch(
    state.changeByRange((range) => {
      const line = state.doc.lineAt(range.from)
      const indent = /^[ \t]*/.exec(line.text)?.[0] ?? ''
      return {
        changes: { from: range.from, to: range.to, insert: state.lineBreak + indent },
        range: EditorSelection.cursor(range.from + 1 + indent.length),
      }
    }),
    { scrollIntoView: true, userEvent: 'input' },
  )
  return true
}

// MARK: - Controller (`CodeEditor.Coordinator`)

const MONO_FONT_SIZE = 12.5
/** `ScrollView` / the gutter follow `Theme.Font.nsMono`. */
const SYMBOL_LOOKUP_DELAY_MS = 250

/** `definitionPattern`: `fn name`, `class Name`, `let name`, … with their modifiers. */
const DEFINITION_PATTERN = new RegExp(
  '^[ \\t]*' +
    '(?:(?:pub(?:\\([^)]*\\))?|async|static|final|export|default|private|public|protected|abstract|const|unsafe|extern|override|open|fileprivate|internal|mutating|lazy|inline)\\s+)*' +
    '(fn|func|function|def|class|struct|enum|trait|interface|protocol|extension|impl|type|typealias|let|var|const|val|mod|module|namespace|macro_rules!)' +
    '\\s+([A-Za-z_][A-Za-z0-9_]*)',
  'gm',
)

/**
 * The Coordinator of `CodeEditor`: owns the `EditorView`, takes the
 * document's text over when it is replaced from outside (`revision`),
 * feeds edits back (`document.edited`), schedules `lsp/change` and auto
 * save, underlines diagnostics, tracks the caret for the status bar and
 * drives the completion popup.
 */
class EditorController {
  readonly view: EditorView
  private readonly host: HTMLElement
  private readonly project: ProjectState
  private readonly document: EditorDocument
  private readonly publishPanel: (snapshot: PanelSnapshot | null) => void
  private readonly session: CompletionSession
  private readonly editable = new Compartment()
  private readonly languageCompartment = new Compartment()
  private readonly indentCompartment = new Compartment()
  private unsubscribe: () => void
  private destroyed = false

  private appliedRevision: number
  private appliedDiagnostics: LspDiagnostic[] = []
  private appliedTruncated: boolean
  private appliedLanguage: LanguageKey | null
  /** "\t", "  " or "    ", from the file's own indentation. */
  private indent = '    '
  private rulerDigits = 0
  private panel: PanelSnapshot | null = null
  private revealQueued = false

  private symbolTimer: ReturnType<typeof setTimeout> | null = null
  private symbolGeneration = 0
  private lastSymbolWord: string | null = null

  /** Identifiers and definitions in the buffer for completion, rebuilt at most once a second. */
  private bufferWords = new Set<string>()
  private bufferDefinitions: CompletionItem[] = []
  private bufferScanStamp = 0
  private bufferScanDirty = true

  constructor(host: HTMLElement, project: ProjectState, document: EditorDocument, publishPanel: (snapshot: PanelSnapshot | null) => void) {
    this.host = host
    this.project = project
    this.document = document
    this.publishPanel = publishPanel
    this.session = new CompletionSession(this.completionHost())
    this.appliedRevision = document.revision
    this.appliedTruncated = document.truncated
    this.appliedLanguage = languageKey(document.path, document.language)
    this.indent = detectIndent(document.text)
    this.view = new EditorView({ parent: host, state: this.createState(document.text, 0) })
    this.updateRulerWidth()
    this.applyLanguage()
    this.applyDiagnostics(document.diagnostics)
    this.updateCursor()
    this.unsubscribe = document.subscribe(() => this.documentChanged())
    requestAnimationFrame(() => {
      if (this.destroyed) return
      this.view.focus()
      this.revealPendingLine()
    })
  }

  destroy(): void {
    this.destroyed = true
    this.unsubscribe()
    this.session.dismiss()
    this.symbolGeneration += 1
    if (this.symbolTimer !== null) clearTimeout(this.symbolTimer)
    this.view.destroy()
  }

  /** A click on a popup row: select it and insert it. */
  pick(index: number): void {
    this.view.dispatch({ effects: setSelectedCompletion(index) })
    acceptCompletion(this.view)
    this.view.focus()
  }

  // MARK: State

  private createState(text: string, caret: number): EditorState {
    const truncated = this.document.truncated
    const state = EditorState.create({
      doc: text,
      extensions: [
        // Keep the file's line endings: CRLF files stay CRLF on save.
        EditorState.lineSeparator.of(text.includes('\r\n') ? '\r\n' : '\n'),
        EditorView.clipboardInputFilter.of((input, current) => input.replace(/\r\n|\r|\n/g, current.lineBreak)),
        lineNumbers(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.tabSize.of(4),
        this.indentCompartment.of(indentUnit.of(this.indent)),
        this.languageCompartment.of(this.appliedLanguage !== null ? (loadedLanguages.get(this.appliedLanguage) ?? []) : []),
        syntaxHighlighting(codeHighlighter),
        namingConventions(() => this.appliedLanguage),
        this.editable.of(this.editableExtension(truncated)),
        search({ top: true, createPanel: createFindPanel }),
        autocompletion({
          override: [this.session.source],
          activateOnTyping: true,
          activateOnTypingDelay: 0,
          interactionDelay: 0,
          defaultKeymap: false,
          icons: false,
          closeOnBlur: true,
          maxRenderedOptions: 80,
          selectOnOpen: true,
        }),
        Prec.highest(keymap.of(completionKeys)),
        keymap.of([
          { key: 'Enter', run: insertNewlineKeepingIndent },
          { key: 'Tab', run: (view) => this.insertIndent(view) },
          { key: 'Mod-s', run: () => this.saveDocument(), preventDefault: true },
          ...searchKeymap.filter((binding) => binding.key !== undefined && FIND_KEYS.has(binding.key)),
          ...historyKeymap.filter((binding) => binding.key === undefined || !UNDO_KEYS.has(binding.key)),
          ...standardKeymap,
          // Ask for the popup: Ctrl+Space, and on macOS Esc (NSTextView's
          // `cancelOperation:` → `complete:`) once the find bar is closed.
          { key: 'Ctrl-Space', run: (view) => this.requestCompletion(view), preventDefault: true },
          ...(isMac ? [{ key: 'Escape', run: (view: EditorView) => this.requestCompletion(view) }] : []),
        ]),
        lastTypedField,
        EditorView.contentAttributes.of({ spellcheck: 'false', autocorrect: 'off', autocapitalize: 'off', 'aria-label': this.document.name }),
        EditorView.domEventHandlers({ mousedown: (event, view) => this.mouseDown(event, view) }),
        EditorView.updateListener.of((update) => this.updated(update)),
      ],
    })
    if (caret === 0) return state
    return state.update({ selection: { anchor: Math.min(caret, state.doc.length) } }).state
  }

  /// `isEditable = !document.truncated`: a truncated read stays focusable
  /// and selectable (find, copy), it only refuses edits.
  private editableExtension(truncated: boolean): Extension {
    return EditorState.readOnly.of(truncated)
  }

  /** The language package arrives (or was loaded already): colour the text with it. */
  private applyLanguage(): void {
    const key = this.appliedLanguage
    if (key === null || loadedLanguages.has(key)) return
    void loadLanguage(key).then((support) => {
      if (this.destroyed || this.appliedLanguage !== key) return
      this.view.dispatch({ effects: this.languageCompartment.reconfigure(support) })
    })
  }

  /** `updateNSView`: the document changed from outside the text view. */
  private documentChanged(): void {
    const document = this.document
    if (this.appliedRevision !== document.revision) {
      this.appliedRevision = document.revision
      this.replaceText(document.text)
    }
    if (this.appliedTruncated !== document.truncated) {
      this.appliedTruncated = document.truncated
      this.view.dispatch({ effects: this.editable.reconfigure(this.editableExtension(document.truncated)) })
    }
    const key = languageKey(document.path, document.language)
    if (key !== this.appliedLanguage) {
      this.appliedLanguage = key
      this.view.dispatch({ effects: this.languageCompartment.reconfigure(key !== null ? (loadedLanguages.get(key) ?? []) : []) })
      this.applyLanguage()
    }
    if (this.appliedDiagnostics !== document.diagnostics) this.applyDiagnostics(document.diagnostics)
    if (document.pendingLine !== null && !this.revealQueued) {
      this.revealQueued = true
      requestAnimationFrame(() => {
        this.revealQueued = false
        if (!this.destroyed) this.revealPendingLine()
      })
    }
  }

  /// `replaceText(_:)`: the buffer takes the document's text (a load or a
  /// reload); the undo history starts over and the caret keeps its offset.
  private replaceText(text: string): void {
    const caret = this.view.state.selection.main.head
    const scroll = this.view.scrollSnapshot()
    this.indent = detectIndent(text)
    this.view.setState(this.createState(text, caret))
    this.view.dispatch({ effects: scroll })
    this.bufferScanDirty = true
    this.updateRulerWidth()
    this.applyDiagnostics(this.document.diagnostics)
    this.updateCursor()
    this.lastSymbolWord = null
    this.scheduleSymbolLookup()
  }

  /** `revealPendingLine()`: the caret to the start of the line, scrolled to the middle. */
  private revealPendingLine(): void {
    const line = this.document.pendingLine
    if (line === null) return
    this.document.pendingLine = null
    const doc = this.view.state.doc
    const start = doc.line(Math.max(1, Math.min(line, doc.lines))).from
    this.view.dispatch({ selection: { anchor: start }, effects: EditorView.scrollIntoView(start, { y: 'center', x: 'start' }) })
    this.view.focus()
    this.updateCursor()
  }

  // MARK: Edits (textDidChange / textViewDidChangeSelection)

  private updated(update: ViewUpdate): void {
    if (update.docChanged) {
      this.document.edited(update.state.sliceDoc())
      this.bufferScanDirty = true
      this.updateRulerWidth()
      scheduleLanguageServerSync(this.document, this.project)
      scheduleAutoSave(this.document, this.project)
    }
    if (update.docChanged || update.selectionSet) {
      this.updateCursor()
      this.scheduleSymbolLookup()
    }
    this.publishCompletion(update.state)
  }

  /** ⇥: the file's indent unit in place of the selection. */
  private insertIndent(view: EditorView): boolean {
    if (view.state.readOnly) return false
    view.dispatch(view.state.replaceSelection(this.indent), { scrollIntoView: true, userEvent: 'input' })
    return true
  }

  /** Opens the completion popup at the caret on request (with no word typed, keywords and definitions). */
  private requestCompletion(view: EditorView): boolean {
    if (!view.state.selection.main.empty) return false
    this.session.request()
    return startCompletion(view)
  }

  /** ⌘S / Ctrl+S (`saveSelectedDocument`). */
  private saveDocument(): boolean {
    if (this.document.isDirty) void save(this.document, this.project)
    return true
  }

  /** `CodeTextView.mouseDown`: ⌘-click / Ctrl-click opens the definition of the identifier under the pointer. */
  private mouseDown(event: MouseEvent, view: EditorView): boolean {
    if (event.button !== 0 || !isModKey(event)) return false
    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
    if (pos === null || pos >= view.state.doc.length) return false
    event.preventDefault()
    this.jumpToDefinition(pos)
    return true
  }

  // MARK: Ruler

  /// `LineNumberRuler.invalidateLines()`'s thickness: digits × (font size ×
  /// 0.62) + 20, at least two digits.
  private updateRulerWidth(): void {
    const digits = Math.max(2, String(this.view.state.doc.lines).length)
    if (digits === this.rulerDigits) return
    this.rulerDigits = digits
    this.host.style.setProperty('--ruler-width', `${digits * MONO_FONT_SIZE * 0.62 + 20}px`)
  }

  // MARK: Diagnostics

  /** `applyDiagnostics(_:)`: dotted underlines in the severity's colour; the message is the tooltip. */
  private applyDiagnostics(diagnostics: LspDiagnostic[]): void {
    this.appliedDiagnostics = diagnostics
    const doc = this.view.state.doc
    const length = doc.length
    const list: Diagnostic[] = []
    for (const diagnostic of diagnostics) {
      const start = Math.min(length, this.lineStart(diagnostic.line + 1) + diagnostic.character)
      let end = Math.min(length, this.lineStart(diagnostic.end_line + 1) + diagnostic.end_character)
      if (end <= start) end = Math.min(length, start + 1)
      if (end <= start) continue
      list.push({
        from: start,
        to: end,
        severity: lspDiagnosticIsError(diagnostic) ? 'error' : lspDiagnosticIsWarning(diagnostic) ? 'warning' : 'info',
        message: diagnostic.source != null ? `${diagnostic.source}: ${diagnostic.message}` : diagnostic.message,
      })
    }
    this.view.dispatch(setDiagnostics(this.view.state, list))
  }

  /** `LineNumberRuler.characterIndex(ofLine:)`: first character of a 1-based line (the last line's past the end). */
  private lineStart(line: number): number {
    const doc = this.view.state.doc
    return doc.line(Math.max(1, Math.min(line, doc.lines))).from
  }

  // MARK: Cursor and symbols

  /** `position(ofCharacter:)`: 1-based line and column. */
  private position(pos: number): { line: number; column: number } {
    const line = this.view.state.doc.lineAt(pos)
    return { line: line.number, column: pos - line.from + 1 }
  }

  private updateCursor(): void {
    const { line, column } = this.position(this.view.state.selection.main.head)
    if (this.document.cursorLine === line && this.document.cursorColumn === column) return
    this.document.update((d) => {
      d.cursorLine = line
      d.cursorColumn = column
    })
  }

  /// `scheduleSymbolLookup()`: the identifier at the caret, looked up
  /// (debounced) for the status bar: the server's hover when there is one,
  /// else the index.
  private scheduleSymbolLookup(): void {
    const state = this.view.state
    const caret = state.selection.main.head
    const range = this.identifierAround(caret)
    const word = range ? state.sliceDoc(range.from, range.to) : null
    if (word === this.lastSymbolWord) return
    this.lastSymbolWord = word
    if (this.symbolTimer !== null) clearTimeout(this.symbolTimer)
    this.symbolTimer = null
    this.symbolGeneration += 1
    if (word === null || word.length < 2) {
      if (this.document.cursorSymbol !== null || this.document.cursorHover !== null) {
        this.document.update((d) => {
          d.cursorSymbol = null
          d.cursorHover = null
        })
      }
      return
    }
    const { line, column } = this.position(caret)
    const generation = this.symbolGeneration
    const document = this.document
    const project = this.project
    const current = () => generation === this.symbolGeneration && this.lastSymbolWord === word
    this.symbolTimer = setTimeout(() => {
      this.symbolTimer = null
      void (async () => {
        if (document.lspServer !== null) {
          const hover = await languageServerHover(document, line - 1, column - 1, project)
          if (!current()) return
          document.update((d) => (d.cursorHover = hover !== null ? firstHoverLine(hover) : null))
        } else if (document.cursorHover !== null) {
          document.update((d) => (d.cursorHover = null))
        }
        const symbols = await findSymbols(word, 'exact', 5, project, document.path)
        if (!current()) return
        document.update((d) => (d.cursorSymbol = symbols[0] ?? null))
      })()
    }, SYMBOL_LOOKUP_DELAY_MS)
  }

  private jumpToDefinition(pos: number): void {
    const range = this.identifierAround(pos)
    if (!range) return
    const word = this.view.state.sliceDoc(range.from, range.to)
    const { line, column } = this.position(pos)
    jumpToDefinition(word, line - 1, column - 1, this.document, this.project)
  }

  // MARK: Completion

  private completionHost(): CompletionHost {
    return {
      view: () => (this.destroyed ? null : this.view),
      hasServer: () => this.document.lspServer !== null,
      triggers: () => this.document.lspTriggers,
      isWordCharacter: (character) => character.length === 1 && this.isWord(character.charCodeAt(0)),
      lastTyped: (state) => state.field(lastTypedField, false) ?? null,
      wordStart: (state, pos) => this.identifierStart(state, pos),
      localCandidates: (prefix, hasServer) => this.localCandidates(prefix, hasServer),
      remote: (prefix, pos, hasServer) => this.remoteCandidates(prefix, pos, hasServer),
      insert: (view, item, from, to, completion) => this.insertCompletion(view, item, from, to, completion),
    }
  }

  /** The `remote` closure of `offerCompletion()`. */
  private async remoteCandidates(prefix: string, pos: number, hasServer: boolean): Promise<CompletionItem[]> {
    if (this.destroyed) return []
    if (hasServer) {
      const { line, column } = this.position(Math.min(pos, this.view.state.doc.length))
      const items = await languageServerCompletions(this.document, line - 1, column - 1, this.project)
      const lowered = prefix.toLowerCase()
      return items
        .filter((item) => prefix.length === 0 || item.label.toLowerCase().startsWith(lowered) || (item.filter_text?.toLowerCase().startsWith(lowered) ?? false))
        .map(serverCompletionItem)
    }
    if (prefix.length < 2) return []
    const symbols = await findSymbols(prefix, 'prefix', 40, this.project, this.document.path)
    return symbols.map(symbolCompletionItem)
  }

  /// `localCandidates(prefix:hasServer:)`: definitions in the buffer (unsaved
  /// ones included), then keywords (unless the server supplies them) and
  /// words of the buffer.
  private localCandidates(prefix: string, hasServer: boolean): CompletionItem[] {
    const lowered = prefix.toLowerCase()
    this.scanBuffer()
    const items: CompletionItem[] = []
    for (const definition of this.bufferDefinitions) {
      if ((prefix.length === 0 || definition.label.toLowerCase().startsWith(lowered)) && definition.label !== prefix) items.push(definition)
    }
    if (!hasServer) {
      for (const keyword of keywordsFor(this.document.language)) {
        if (keyword.toLowerCase().startsWith(lowered) && keyword !== prefix) items.push(keywordCompletionItem(keyword))
      }
    }
    if (prefix.length === 0) return items
    for (const word of this.bufferWords) {
      if (word.toLowerCase().startsWith(lowered) && word !== prefix) items.push(wordCompletionItem(word))
    }
    return items
  }

  /// `scanBuffer()`: rebuilds `bufferWords` (identifiers of three or more
  /// characters) and `bufferDefinitions` (`fn name`, `class Name`, `let
  /// name`, …), at most once a second.
  private scanBuffer(): void {
    const now = Date.now()
    if (!this.bufferScanDirty || now - this.bufferScanStamp <= 1000) return
    const text = this.document.text
    const hyphens = this.hyphenatedWords
    const words = new Set<string>()
    let start = -1
    let hasNonHyphen = false
    const flush = (end: number) => {
      if (start >= 0 && end - start >= 3 && hasNonHyphen) words.add(text.slice(start, end))
      start = -1
      hasNonHyphen = false
    }
    for (let i = 0; i < text.length; i += 1) {
      const code = text.charCodeAt(i)
      const letter = (code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a) || code === 0x5f
      const digit = code >= 0x30 && code <= 0x39 && start >= 0
      const hyphen = hyphens && code === 0x2d
      if (letter || digit || hyphen) {
        if (start < 0) start = i
        if (!hyphen) hasNonHyphen = true
      } else {
        flush(i)
      }
    }
    flush(text.length)
    this.bufferWords = words
    this.bufferDefinitions = bufferDefinitions(text, this.document.path)
    this.bufferScanStamp = now
    this.bufferScanDirty = false
  }

  /** `insert(_:replacing:)`: the item's text over the word (or the server's replace range). */
  private insertCompletion(view: EditorView, item: CompletionItem, from: number, to: number, completion: Completion): void {
    const state = view.state
    let target = { from, to }
    const edit = item.replaceRange
    if (edit && edit.start_line === edit.end_line) {
      const length = state.doc.length
      const start = Math.min(length, this.lineStart(edit.start_line + 1) + edit.start_character)
      const end = Math.min(length, this.lineStart(edit.end_line + 1) + edit.end_character)
      if (end >= start && start <= from && end >= from) target = { from: start, to: Math.max(end, to) }
    }
    const insert = item.insertText.replace(/\r\n|\r|\n/g, state.lineBreak)
    view.dispatch({
      changes: { from: target.from, to: target.to, insert },
      selection: { anchor: target.from + state.toText(insert).length },
      annotations: pickedCompletion.of(completion),
      userEvent: 'input.complete',
      scrollIntoView: true,
    })
  }

  /// Hands the popup its rows: CodeMirror's open completion (its order is the
  /// session's), kept while a refresh is pending so the popup does not blink.
  private publishCompletion(state: EditorState): void {
    const status = completionStatus(state)
    if (status === null) {
      this.session.closed()
      this.setPanel(null)
      return
    }
    if (status === 'pending') return
    const options = currentCompletions(state)
    if (options.length === 0) {
      this.setPanel(null)
      return
    }
    const selectedIndex = selectedCompletionIndex(state) ?? 0
    const previous = this.panel
    if (previous && previous.options === options && previous.selectedIndex === selectedIndex) return
    const items =
      previous && previous.options === options
        ? previous.items
        : options.map((option) => this.session.itemOf(option)).filter((item): item is CompletionItem => item !== null)
    this.setPanel({ view: this.view, options, items, selectedIndex, prefix: this.session.prefix, from: this.session.from })
  }

  private setPanel(snapshot: PanelSnapshot | null): void {
    if (snapshot === null && this.panel === null) return
    this.panel = snapshot
    this.publishPanel(snapshot)
  }

  // MARK: Text helpers

  /** CSS and HTML names (`btn-primary`, `--brand`, `aria-label`) keep their hyphens. */
  private get hyphenatedWords(): boolean {
    return this.document.language === 'css' || this.document.language === 'html'
  }

  private isWord(code: number): boolean {
    return isIdentifier(code) || (this.hyphenatedWords && code === 0x2d)
  }

  /** `identifierRange(endingAt:)`'s start: the identifier whose last character sits right before `pos`. */
  private identifierStart(state: EditorState, pos: number): number {
    const line = state.doc.lineAt(pos)
    let start = pos - line.from
    while (start > 0 && this.isWord(line.text.charCodeAt(start - 1))) start -= 1
    return line.from + start
  }

  /** `identifierRange(around:)`: the identifier containing or touching `pos` (not one starting with a digit). */
  private identifierAround(pos: number): { from: number; to: number } | null {
    const line = this.view.state.doc.lineAt(pos)
    const text = line.text
    let start = pos - line.from
    let end = start
    while (start > 0 && this.isWord(text.charCodeAt(start - 1))) start -= 1
    while (end < text.length && this.isWord(text.charCodeAt(end))) end += 1
    if (end <= start) return null
    const first = text.charCodeAt(start)
    if (!isIdentifierStart(first) && !(this.hyphenatedWords && first === 0x2d)) return null
    return { from: line.from + start, to: line.from + end }
  }
}

// MARK: - Helpers

function isIdentifierStart(code: number): boolean {
  return (code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a) || code === 0x5f
}

function isIdentifier(code: number): boolean {
  return isIdentifierStart(code) || (code >= 0x30 && code <= 0x39)
}

/** `detectIndent(in:)`: a tab when the first indented line uses one, else its space count (2 or 4). */
function detectIndent(text: string): string {
  let lineStart = 0
  let seen = 0
  while (lineStart < text.length && seen < 400) {
    let lineEnd = text.indexOf('\n', lineStart)
    if (lineEnd < 0) lineEnd = text.length
    if (lineEnd > lineStart) {
      seen += 1
      if (text.charCodeAt(lineStart) === 0x09) return '\t'
      let spaces = 0
      while (lineStart + spaces < lineEnd && text.charCodeAt(lineStart + spaces) === 0x20) spaces += 1
      if (spaces > 0) return spaces === 2 || spaces === 3 ? '  ' : '    '
    }
    lineStart = lineEnd + 1
  }
  return '    '
}

/** `firstHoverLine(_:)`: the signature line of a hover, the first non-empty line that is not a fence. */
function firstHoverLine(contents: string): string | null {
  for (const raw of contents.split('\n')) {
    const line = raw.trim()
    if (line.length > 0 && !line.startsWith('```') && !line.startsWith('---')) return line
  }
  return null
}

/** `definitions(in:path:)`: definitions by their introducing keyword, with the line they sit on as the signature. */
function bufferDefinitions(text: string, path: string): CompletionItem[] {
  const items: CompletionItem[] = []
  const seen = new Set<string>()
  const lineStarts = [0]
  for (let i = 0; i < text.length; i += 1) if (text.charCodeAt(i) === 0x0a) lineStarts.push(i + 1)
  DEFINITION_PATTERN.lastIndex = 0
  for (const match of text.matchAll(DEFINITION_PATTERN)) {
    const keyword = match[1]
    const name = match[2]
    if (keyword === undefined || name === undefined || seen.has(name)) continue
    seen.add(name)
    const index = match.index
    const lineFrom = text.lastIndexOf('\n', index - 1) + 1
    let lineTo = text.indexOf('\n', index + match[0].length)
    if (lineTo < 0) lineTo = text.length
    const signature = text.slice(lineFrom, lineTo).trim().slice(0, 80)
    let low = 0
    let high = lineStarts.length - 1
    while (low < high) {
      const mid = (low + high + 1) >> 1
      if ((lineStarts[mid] ?? 0) <= index) low = mid
      else high = mid - 1
    }
    items.push(bufferDefinitionItem(name, definitionKind(keyword), signature, path, low + 1))
  }
  return items
}

function definitionKind(keyword: string): string {
  switch (keyword) {
    case 'fn':
    case 'func':
    case 'function':
    case 'def':
    case 'macro_rules!':
      return 'function'
    case 'class':
      return 'class'
    case 'struct':
    case 'extension':
    case 'impl':
      return 'struct'
    case 'enum':
      return 'enum'
    case 'trait':
    case 'interface':
    case 'protocol':
      return 'interface'
    case 'type':
    case 'typealias':
      return 'type_alias'
    case 'mod':
    case 'module':
    case 'namespace':
      return 'module'
    default:
      return 'variable'
  }
}

function pathExtension(path: string): string {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  const name = slash < 0 ? path : path.slice(slash + 1)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}
