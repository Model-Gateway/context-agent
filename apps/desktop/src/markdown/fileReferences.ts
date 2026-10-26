/**
 * Port of `FileReferences.swift`. File names in rendered markdown
 * ("`index.html` — layout …", `src/app.rs:42`, relative links) become small
 * chips styled like the outcome card's "Modified files" chips: click opens
 * the file in the Editor tab, mod-click opens it in the default editor, and
 * the context menu offers those plus Show in Changes, Reveal and Copy Path.
 *
 * This file finds and resolves the references in the inline nodes of
 * `inline.ts`; `MarkdownView` draws the chips. The macOS app checks the
 * disk synchronously to resolve a name; the web app cannot, so the chat
 * views back `isFile` with what they know (the project's file index or
 * tree) and the changed paths from `git/status`.
 */
import type { FileReferenceNode, InlineNode } from './inline'
import { parseInline, plainText } from './inline'
import { RenderCache } from './renderCache'

/** `PathAction` in AppState.swift (`revealInFinder` reads "Show in Explorer" on Windows). */
export type PathAction = 'openInEditor' | 'showInChanges' | 'openWithEditor' | 'revealInFileManager' | 'copyPath'

/** What a chip carries to the host: the resolved path and the `:line[:column]` / `#L42` it was written with. */
export interface FileReference {
  /** Repository-relative, or absolute when the text named an absolute path. */
  path: string
  line?: number
  column?: number
}

/**
 * `FileReferenceContext` in Swift: resolves file names to files in the
 * project and performs the path actions on them. `ChatView` provides one;
 * without it `MarkdownView` renders file names as plain text. Memoized
 * results are keyed by `root`, so a context whose data changed must call
 * `invalidateFileReferences()`.
 */
export interface FileReferenceContext {
  /** Repository root. */
  root: string
  /** Repository-relative paths with changes: a bare `index.html` also matches a changed `web/index.html`. */
  changedPaths: () => readonly string[]
  /**
   * Whether `path` (absolute, as joined with `root`) names an existing file.
   * Without it only the changed paths resolve.
   */
  isFile?: (path: string) => boolean
  /**
   * Click → `openInEditor`, mod-click → `openWithEditor`; the chip's context
   * menu offers every `PathAction` (`AppState.perform(_:on:)` in Swift).
   */
  perform: (reference: FileReference, action: PathAction) => void
}

/**
 * `FileReferenceContext.rejected`: whitespace and code punctuation. The
 * Swift set also rejects `\`; here a backslash is a Windows separator and
 * becomes `/` (git and the backend both accept forward slashes there).
 */
const rejected = /[\s()[\]{}<>=,;'"*?|$`]/

/**
 * `FileReferenceContext.normalized`: drops `./`, splits off a
 * `:line[:column]` or `#L42` suffix and rejects anything with spaces, a
 * scheme or code punctuation, and words with neither a slash nor an
 * extension. `~/` is kept as written (the host's `isFile` expands it).
 */
export function parseFileReference(candidate: string): FileReference | null {
  let path = candidate.replace(/^[\t\p{Zs}]+|[\t\p{Zs}]+$/gu, '')
  if (path.length === 0 || path.length >= 512 || rejected.test(path) || path.includes('://')) return null
  path = path.replace(/\\/g, '/')
  let line: number | undefined
  let column: number | undefined
  const colon = /:(\d+)(?::(\d+))?$/.exec(path)
  if (colon) {
    line = parseInt(colon[1] ?? '', 10)
    if (colon[2] !== undefined) column = parseInt(colon[2], 10)
    path = path.slice(0, colon.index)
  } else {
    const hash = /#L(\d+)(?:-L?\d+)?$/.exec(path)
    if (hash) {
      line = parseInt(hash[1] ?? '', 10)
      path = path.slice(0, hash.index)
    }
  }
  if (path.startsWith('./')) path = path.slice(2)
  if (path.length === 0 || path.endsWith('/')) return null
  if (!path.includes('/') && pathExtension(path).length === 0) return null
  const reference: FileReference = { path }
  if (line !== undefined && !Number.isNaN(line)) reference.line = line
  if (column !== undefined && !Number.isNaN(column)) reference.column = column
  return reference
}

/** `NSString.pathExtension`: the part after the last dot of the last component, unless the dot leads. */
function pathExtension(path: string): string {
  const name = path.slice(path.lastIndexOf('/') + 1)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1) : ''
}

/** Absolute on any platform: `/…`, `//server/…`, `C:/…`, or `~/…` (home-relative, expanded by the host). */
function isAbsolute(path: string): boolean {
  return path.startsWith('/') || path.startsWith('~/') || /^[A-Za-z]:\//.test(path)
}

/** `root` + `path` with the root's separator (`\\` for a Windows root). */
function join(root: string, path: string): string {
  if (root.length === 0) return path
  const windows = root.includes('\\') && !root.includes('/')
  const relative = windows ? path.replace(/\//g, '\\') : path
  if (root.endsWith('/') || root.endsWith('\\')) return root + relative
  return root + (windows ? '\\' : '/') + relative
}

/**
 * `FileReferenceContext.resolve`: the reference `candidate` names, or
 * `null` when it is not a path or no such file exists.
 */
export function resolveFileReference(files: FileReferenceContext, candidate: string): FileReference | null {
  const reference = parseFileReference(candidate)
  if (!reference) return null
  const { path } = reference
  if (isAbsolute(path)) {
    if (files.isFile?.(path)) return reference
    const root = files.root.replace(/\\/g, '/')
    const prefix = root.endsWith('/') ? root : root + '/'
    if (!files.isFile && path.startsWith(prefix) && files.changedPaths().includes(path.slice(prefix.length))) {
      return reference
    }
    return null
  }
  if (files.isFile?.(join(files.root, path))) return reference
  const changed = files.changedPaths()
  if (changed.includes(path)) return reference
  const suffix = '/' + path
  const backslashSuffix = '\\' + path.replace(/\//g, '\\')
  const matches = changed.filter((changedPath) => changedPath.endsWith(suffix) || changedPath.endsWith(backslashSuffix))
  const only = matches[0]
  return matches.length === 1 && only !== undefined ? { ...reference, path: only } : null
}

/**
 * `index.html`, `src/app.rs:42`, `~/notes.md` in running text; the
 * extension starts with a letter so "v1.2" and "e.g" stay text (and only
 * names that resolve to a file become chips). Unlike the Swift pattern a
 * `\` also separates components and a drive letter may lead (`C:\src\app.rs`),
 * since model output on Windows writes paths that way.
 */
const barePath =
  /(?<![\w./\\~-])(?:~?[/\\]|[A-Za-z]:[/\\])?(?:[\w.-]+[/\\])*[\w-][\w.-]*\.[A-Za-z][A-Za-z0-9]{0,9}(?::\d+(?::\d+)?)?(?![\w/\\-])/g

function hasScheme(href: string): boolean {
  return /^[A-Za-z][A-Za-z0-9+.-]*:/.test(href)
}

/**
 * `MarkdownInline.fileReferences(in:files:)`: code spans, relative links
 * and bare `name.ext` words that resolve to a file become
 * `fileReference` nodes. Never mutates its input.
 */
export function applyFileReferences(nodes: readonly InlineNode[], files: FileReferenceContext): InlineNode[] {
  const out: InlineNode[] = []
  for (const node of nodes) {
    switch (node.kind) {
      case 'code': {
        const reference = resolveFileReference(files, node.text)
        out.push(reference ? chip(node.text, reference) : node)
        break
      }
      case 'link': {
        if (node.autolink || node.image) {
          out.push(node)
          break
        }
        let candidate: string | null = null
        if (node.href.startsWith('file://')) {
          try {
            candidate = decodeURIComponent(node.href.slice('file://'.length))
          } catch {
            candidate = null
          }
        } else if (!hasScheme(node.href)) {
          candidate = node.href
        }
        const reference = candidate !== null ? resolveFileReference(files, candidate) : null
        out.push(reference ? chip(plainText(node.children), reference) : node)
        break
      }
      case 'text': {
        let cursor = 0
        for (const match of node.text.matchAll(barePath)) {
          const reference = resolveFileReference(files, match[0])
          if (!reference) continue
          if (match.index > cursor) out.push({ kind: 'text', text: node.text.slice(cursor, match.index) })
          out.push(chip(match[0], reference))
          cursor = match.index + match[0].length
        }
        if (cursor === 0) {
          out.push(node)
        } else if (cursor < node.text.length) {
          out.push({ kind: 'text', text: node.text.slice(cursor) })
        }
        break
      }
      case 'strong':
      case 'emphasis':
      case 'strikethrough':
        out.push({ ...node, children: applyFileReferences(node.children, files) })
        break
      default:
        out.push(node)
    }
  }
  return out
}

function chip(label: string, reference: FileReference): FileReferenceNode {
  const node: FileReferenceNode = { kind: 'fileReference', label, path: reference.path }
  if (reference.line !== undefined) node.line = reference.line
  if (reference.column !== undefined) node.column = reference.column
  return node
}

/** Resolved inline nodes by root and text (`MarkdownInline.renderedCache`, 2048 entries). */
const resolved = new RenderCache<string, InlineNode[]>(2048)

/**
 * `MarkdownInline.rendered(_:font:files:)`: the inline nodes of `text` with
 * file references turned into chips. Memoized per project root: resolving
 * references asks the host, and rows re-render whenever they scroll back
 * into view. The result is shared: never mutate it.
 */
export function inlineWithFileReferences(text: string, files: FileReferenceContext): InlineNode[] {
  return resolved.value(files.root + '\0' + text, () => applyFileReferences(parseInline(text), files))
}

/**
 * `MarkdownInline.invalidateFileReferences()`: forgets resolved references;
 * call when the working tree changed (a file now exists, or a bare name now
 * matches a changed path).
 */
export function invalidateFileReferences(): void {
  resolved.removeAll()
}
