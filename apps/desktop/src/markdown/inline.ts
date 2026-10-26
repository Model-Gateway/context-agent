/**
 * Inline markdown. The macOS app hands block text to
 * `AttributedString(markdown:)` with `.inlineOnlyPreservingWhitespace`
 * (`MarkdownInline.render` in MarkdownView.swift); this is the equivalent
 * parser for the web app: emphasis, strong, code spans, links, autolinks,
 * strikethrough, backslash escapes, entities and line breaks (newlines are
 * preserved, as they are in the Swift app). `renderInline` turns the nodes
 * into React nodes with the app's code and link styling; `**strong**` is
 * semibold (600) rather than bold, which reads too heavy beside 14 px text.
 */
import { createElement, Fragment, type MouseEvent, type ReactNode } from 'react'
import { RenderCache } from './renderCache'

/**
 * One run of inline markdown: the web counterpart of the runs of the
 * `AttributedString` that `MarkdownInline.render` builds (presentation
 * intents `.code`, `.stronglyEmphasized`, `.emphasized`,
 * `.strikethrough`, a `link`, or a file chip from FileReferences.swift).
 */
export type InlineNode =
  | TextNode
  | CodeNode
  | StrongNode
  | EmphasisNode
  | StrikethroughNode
  | LinkNode
  | LineBreakNode
  | FileReferenceNode

/** Plain characters. */
export interface TextNode {
  kind: 'text'
  text: string
}

/** A code span (`` `code` ``); mono on `--code-background`. */
export interface CodeNode {
  kind: 'code'
  text: string
}

/** `**strong**` / `__strong__`: semibold, not bold (`.stronglyEmphasized`). */
export interface StrongNode {
  kind: 'strong'
  children: InlineNode[]
}

/** `*emphasis*` / `_emphasis_`: italic (`.emphasized`). */
export interface EmphasisNode {
  kind: 'emphasis'
  children: InlineNode[]
}

/** `~~struck~~` / `~struck~` (GFM). */
export interface StrikethroughNode {
  kind: 'strikethrough'
  children: InlineNode[]
}

/** `[text](href "title")`, `<https://…>` or a bare URL (`autolink`); an image is rendered as a link to its source. */
export interface LinkNode {
  kind: 'link'
  href: string
  title: string | null
  children: InlineNode[]
  autolink: boolean
  image: boolean
}

/** A newline inside a paragraph (preserved whitespace) or a backslash hard break. */
export interface LineBreakNode {
  kind: 'lineBreak'
}

/**
 * A file name that `fileReferences.ts` resolved to a project file; rendered
 * as a chip by `MarkdownView`. `label` is the text it replaced.
 */
export interface FileReferenceNode {
  kind: 'fileReference'
  label: string
  path: string
  line?: number
  column?: number
}

/** Parsed inline nodes by source text (`MarkdownInline.renderCache`, 2048 entries). The result is shared: never mutate it. */
const parsed = new RenderCache<string, InlineNode[]>(2048)

/** Parses one block's inline markdown. Memoized per text. */
export function parseInline(text: string): InlineNode[] {
  return parsed.value(text, () => parseUncached(text))
}

/** The characters of the nodes without any markup (the chip label of a link, "Copy as plain text"). */
export function plainText(nodes: readonly InlineNode[]): string {
  let out = ''
  for (const node of nodes) {
    switch (node.kind) {
      case 'text':
      case 'code':
        out += node.text
        break
      case 'lineBreak':
        out += '\n'
        break
      case 'fileReference':
        out += node.label
        break
      default:
        out += plainText(node.children)
    }
  }
  return out
}

// MARK: - Parsing

interface Delimiter {
  node: TextNode
  char: '*' | '_' | '~'
  count: number
  origCount: number
  canOpen: boolean
  canClose: boolean
  removed: boolean
}

interface Bracket {
  node: TextNode
  image: boolean
  active: boolean
  /** `delimiters.length` when the bracket was pushed: the stack bottom for its emphasis. */
  delimiterFloor: number
}

const plainRun = /[^\\\n`<*_~[\]!&hH]+/y
const schemeAutolink = /<([A-Za-z][A-Za-z0-9+.-]{1,31}:[^\s<>]*)>/y
const emailAutolink =
  /<([A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)*)>/y
// A `]` ends a bare URL so `[https://a.b](https://a.b)` stays one link.
const bareURL = /https?:\/\/[^\s<\]]+/iy
const entity = /&(?:#(\d{1,7})|#[xX]([0-9a-fA-F]{1,6})|([A-Za-z][A-Za-z0-9]{1,31}));/y
const asciiPunctuation = /[!-/:-@[-`{-~]/
const unicodePunctuation = /[\p{P}\p{S}]/u

function parseUncached(source: string): InlineNode[] {
  const nodes: InlineNode[] = []
  const delimiters: Delimiter[] = []
  const brackets: Bracket[] = []
  let buffer = ''
  const flush = () => {
    if (buffer.length > 0) {
      nodes.push({ kind: 'text', text: buffer })
      buffer = ''
    }
  }

  const length = source.length
  let i = 0
  while (i < length) {
    plainRun.lastIndex = i
    const run = plainRun.exec(source)
    if (run) {
      buffer += run[0]
      i += run[0].length
      continue
    }

    const ch = source.charAt(i)
    switch (ch) {
      case '\\': {
        const next = source.charAt(i + 1)
        if (next === '\n') {
          flush()
          nodes.push({ kind: 'lineBreak' })
          i += 2
        } else if (next && asciiPunctuation.test(next)) {
          buffer += next
          i += 2
        } else {
          buffer += '\\'
          i += 1
        }
        break
      }
      case '\n': {
        flush()
        nodes.push({ kind: 'lineBreak' })
        i += 1
        break
      }
      case '`': {
        const span = codeSpan(source, i)
        if (span) {
          flush()
          nodes.push({ kind: 'code', text: span.text })
          i = span.end
        } else {
          const ticks = runLength(source, i, '`')
          buffer += source.slice(i, i + ticks)
          i += ticks
        }
        break
      }
      case '<': {
        schemeAutolink.lastIndex = i
        const scheme = schemeAutolink.exec(source)
        if (scheme && scheme[1] !== undefined) {
          flush()
          nodes.push(autolinkNode(scheme[1], scheme[1]))
          i += scheme[0].length
          break
        }
        emailAutolink.lastIndex = i
        const email = emailAutolink.exec(source)
        if (email && email[1] !== undefined) {
          flush()
          nodes.push(autolinkNode('mailto:' + email[1], email[1]))
          i += email[0].length
          break
        }
        buffer += '<'
        i += 1
        break
      }
      case 'h':
      case 'H': {
        const previous = i > 0 ? source.charAt(i - 1) : ''
        if (previous === '' || !/[A-Za-z0-9]/.test(previous)) {
          bareURL.lastIndex = i
          const match = bareURL.exec(source)
          if (match) {
            const url = trimBareURL(match[0])
            if (url.length > 0 && /^https?:\/\/./i.test(url)) {
              flush()
              nodes.push(autolinkNode(url, url))
              i += url.length
              break
            }
          }
        }
        buffer += ch
        i += 1
        break
      }
      case '*':
      case '_':
      case '~': {
        const count = runLength(source, i, ch)
        if (ch === '~' && count > 2) {
          buffer += source.slice(i, i + count)
          i += count
          break
        }
        const before = i > 0 ? source.charAt(i - 1) : '\n'
        const after = i + count < length ? source.charAt(i + count) : '\n'
        const leftFlanking = !isWhitespace(after) && (!isPunctuation(after) || isWhitespace(before) || isPunctuation(before))
        const rightFlanking = !isWhitespace(before) && (!isPunctuation(before) || isWhitespace(after) || isPunctuation(after))
        let canOpen: boolean
        let canClose: boolean
        if (ch === '_') {
          canOpen = leftFlanking && (!rightFlanking || isPunctuation(before))
          canClose = rightFlanking && (!leftFlanking || isPunctuation(after))
        } else {
          canOpen = leftFlanking
          canClose = rightFlanking
        }
        flush()
        const node: TextNode = { kind: 'text', text: source.slice(i, i + count) }
        nodes.push(node)
        delimiters.push({ node, char: ch, count, origCount: count, canOpen, canClose, removed: false })
        i += count
        break
      }
      case '!': {
        if (source.charAt(i + 1) === '[') {
          flush()
          const node: TextNode = { kind: 'text', text: '![' }
          nodes.push(node)
          brackets.push({ node, image: true, active: true, delimiterFloor: delimiters.length })
          i += 2
        } else {
          buffer += '!'
          i += 1
        }
        break
      }
      case '[': {
        flush()
        const node: TextNode = { kind: 'text', text: '[' }
        nodes.push(node)
        brackets.push({ node, image: false, active: true, delimiterFloor: delimiters.length })
        i += 1
        break
      }
      case ']': {
        flush()
        const opener = brackets[brackets.length - 1]
        if (!opener) {
          buffer += ']'
          i += 1
          break
        }
        if (!opener.active) {
          brackets.pop()
          buffer += ']'
          i += 1
          break
        }
        const destination = linkDestination(source, i + 1)
        if (!destination) {
          brackets.pop()
          buffer += ']'
          i += 1
          break
        }
        processEmphasis(nodes, delimiters, opener.delimiterFloor)
        const start = nodes.indexOf(opener.node)
        const inner = start >= 0 ? nodes.splice(start, nodes.length - start).slice(1) : []
        nodes.push({
          kind: 'link',
          href: destination.href,
          title: destination.title,
          children: unwrapLinks(inner),
          autolink: false,
          image: opener.image,
        })
        brackets.pop()
        if (!opener.image) {
          for (const bracket of brackets) {
            if (!bracket.image) bracket.active = false
          }
        }
        i = destination.end
        break
      }
      case '&': {
        entity.lastIndex = i
        const match = entity.exec(source)
        const decoded = match ? decodeEntity(match) : null
        if (match && decoded !== null) {
          buffer += decoded
          i += match[0].length
        } else {
          buffer += '&'
          i += 1
        }
        break
      }
      default: {
        buffer += ch
        i += 1
      }
    }
  }
  flush()
  processEmphasis(nodes, delimiters, 0)
  return mergeText(nodes)
}

function autolinkNode(href: string, label: string): LinkNode {
  return { kind: 'link', href, title: null, children: [{ kind: 'text', text: label }], autolink: true, image: false }
}

function runLength(source: string, from: number, character: string): number {
  let end = from
  while (end < source.length && source.charAt(end) === character) end += 1
  return end - from
}

function isWhitespace(character: string): boolean {
  return character === '' || /\s/u.test(character)
}

function isPunctuation(character: string): boolean {
  return character !== '' && unicodePunctuation.test(character)
}

/** A code span opened at `from`: the closing run must have the same length. */
function codeSpan(source: string, from: number): { text: string; end: number } | null {
  const ticks = runLength(source, from, '`')
  let position = from + ticks
  while (position < source.length) {
    const index = source.indexOf('`', position)
    if (index < 0) return null
    const run = runLength(source, index, '`')
    if (run === ticks) {
      let text = source.slice(from + ticks, index).replace(/\n/g, ' ')
      if (text.length >= 2 && text.startsWith(' ') && text.endsWith(' ') && /[^ ]/.test(text)) {
        text = text.slice(1, -1)
      }
      return { text, end: index + run }
    }
    position = index + run
  }
  return null
}

/** GFM trailing punctuation never belongs to a bare URL; a `)` only when it closes a `(` inside it. */
function trimBareURL(url: string): string {
  let trimmed = url
  for (;;) {
    const last = trimmed.charAt(trimmed.length - 1)
    if (last === '' ) break
    if ('?!.,:*_~\'"'.includes(last)) {
      trimmed = trimmed.slice(0, -1)
      continue
    }
    if (last === ')') {
      let depth = 0
      for (const character of trimmed) {
        if (character === '(') depth += 1
        else if (character === ')') depth -= 1
      }
      if (depth < 0) {
        trimmed = trimmed.slice(0, -1)
        continue
      }
    }
    break
  }
  return trimmed
}

/** `(destination "title")` starting at `from`; `null` when the text is not an inline link. */
function linkDestination(source: string, from: number): { href: string; title: string | null; end: number } | null {
  if (source.charAt(from) !== '(') return null
  let position = skipWhitespace(source, from + 1)
  let href: string
  if (source.charAt(position) === '<') {
    const close = source.indexOf('>', position + 1)
    if (close < 0) return null
    const inner = source.slice(position + 1, close)
    if (inner.includes('<') || inner.includes('\n')) return null
    href = unescape(inner)
    position = close + 1
  } else {
    const start = position
    let depth = 0
    while (position < source.length) {
      const character = source.charAt(position)
      if (character === '\\' && asciiPunctuation.test(source.charAt(position + 1))) {
        position += 2
        continue
      }
      if (character === '(') {
        depth += 1
      } else if (character === ')') {
        if (depth === 0) break
        depth -= 1
      } else if (/\s/.test(character)) {
        break
      }
      position += 1
    }
    href = unescape(source.slice(start, position))
  }
  position = skipWhitespace(source, position)
  let title: string | null = null
  const quote = source.charAt(position)
  if (quote === '"' || quote === "'" || quote === '(') {
    const closer = quote === '(' ? ')' : quote
    let scan = position + 1
    let found = -1
    while (scan < source.length) {
      const character = source.charAt(scan)
      if (character === '\\') {
        scan += 2
        continue
      }
      if (character === closer) {
        found = scan
        break
      }
      scan += 1
    }
    if (found < 0) return null
    title = unescape(source.slice(position + 1, found))
    position = skipWhitespace(source, found + 1)
  }
  if (source.charAt(position) !== ')') return null
  return { href, title, end: position + 1 }
}

function skipWhitespace(source: string, from: number): number {
  let position = from
  while (position < source.length && /\s/.test(source.charAt(position))) position += 1
  return position
}

function unescape(text: string): string {
  return text.replace(/\\([!-/:-@[-`{-~])/g, '$1')
}

/** Nested links are not allowed: the inner ones dissolve into their text. */
function unwrapLinks(nodes: InlineNode[]): InlineNode[] {
  const out: InlineNode[] = []
  for (const node of nodes) {
    switch (node.kind) {
      case 'link':
        out.push(...unwrapLinks(node.children))
        break
      case 'strong':
      case 'emphasis':
      case 'strikethrough':
        out.push({ ...node, children: unwrapLinks(node.children) })
        break
      default:
        out.push(node)
    }
  }
  return out
}

/**
 * CommonMark's "process emphasis" over the delimiter stack above `bottom`,
 * with GFM strikethrough (`~` runs of one or two, matched by equal length).
 */
function processEmphasis(nodes: InlineNode[], delimiters: Delimiter[], bottom: number): void {
  const openersBottom = new Map<string, number>()
  let closerIndex = bottom
  while (closerIndex < delimiters.length) {
    const closer = delimiters[closerIndex]
    if (!closer || closer.removed || !closer.canClose) {
      closerIndex += 1
      continue
    }
    const key = closer.char + (closer.canOpen ? 'o' : 'c') + (closer.origCount % 3)
    const floor = Math.max(bottom - 1, openersBottom.get(key) ?? bottom - 1)
    let openerIndex = closerIndex - 1
    let opener: Delimiter | null = null
    while (openerIndex > floor) {
      const candidate = delimiters[openerIndex]
      if (candidate && !candidate.removed && candidate.char === closer.char && candidate.canOpen) {
        const oddMatch =
          (closer.canOpen || candidate.canClose) &&
          closer.origCount % 3 !== 0 &&
          (candidate.origCount + closer.origCount) % 3 === 0
        const lengthMatch = closer.char !== '~' || candidate.count === closer.count
        if (!oddMatch && lengthMatch) {
          opener = candidate
          break
        }
      }
      openerIndex -= 1
    }

    if (opener) {
      const use = closer.char === '~' ? closer.count : closer.count >= 2 && opener.count >= 2 ? 2 : 1
      const openerPosition = nodes.indexOf(opener.node)
      const closerPosition = nodes.indexOf(closer.node)
      if (openerPosition >= 0 && closerPosition > openerPosition) {
        const inner = nodes.splice(openerPosition + 1, closerPosition - openerPosition - 1)
        const wrapped: InlineNode =
          closer.char === '~'
            ? { kind: 'strikethrough', children: mergeText(inner) }
            : use === 2
              ? { kind: 'strong', children: mergeText(inner) }
              : { kind: 'emphasis', children: mergeText(inner) }
        nodes.splice(openerPosition + 1, 0, wrapped)
      }
      for (let k = openerIndex + 1; k < closerIndex; k += 1) {
        const between = delimiters[k]
        if (between) between.removed = true
      }
      opener.count -= use
      opener.node.text = opener.node.text.slice(use)
      closer.count -= use
      closer.node.text = closer.node.text.slice(use)
      if (opener.count === 0) {
        opener.removed = true
        removeNode(nodes, opener.node)
      }
      if (closer.count === 0) {
        closer.removed = true
        removeNode(nodes, closer.node)
        closerIndex += 1
      }
    } else {
      openersBottom.set(key, closerIndex - 1)
      if (!closer.canOpen) closer.removed = true
      closerIndex += 1
    }
  }
  delimiters.length = bottom
}

function removeNode(nodes: InlineNode[], node: InlineNode): void {
  const index = nodes.indexOf(node)
  if (index >= 0) nodes.splice(index, 1)
}

/** Joins adjacent text nodes and drops empty ones, recursively. */
function mergeText(nodes: InlineNode[]): InlineNode[] {
  const out: InlineNode[] = []
  for (const node of nodes) {
    if (node.kind === 'text') {
      if (node.text.length === 0) continue
      const last = out[out.length - 1]
      if (last && last.kind === 'text') {
        out[out.length - 1] = { kind: 'text', text: last.text + node.text }
      } else {
        out.push({ kind: 'text', text: node.text })
      }
      continue
    }
    if (node.kind === 'strong' || node.kind === 'emphasis' || node.kind === 'strikethrough' || node.kind === 'link') {
      out.push({ ...node, children: mergeText(node.children) })
      continue
    }
    out.push(node)
  }
  return out
}

const namedEntities: Record<string, string> = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', copy: '©', reg: '®', trade: '™',
  hellip: '…', mdash: '—', ndash: '–', laquo: '«', raquo: '»', ldquo: '“', rdquo: '”', lsquo: '‘',
  rsquo: '’', bull: '•', middot: '·', times: '×', divide: '÷', deg: '°', plusmn: '±', para: '¶',
  sect: '§', euro: '€', pound: '£', yen: '¥', cent: '¢', larr: '←', rarr: '→', uarr: '↑', darr: '↓',
  harr: '↔', ensp: ' ', emsp: ' ', thinsp: ' ', zwj: '‍', zwnj: '‌', shy: '­',
  check: '✓', cross: '✗', hearts: '♥', star: '☆', starf: '★', infin: '∞', ne: '≠', le: '≤', ge: '≥',
}

function decodeEntity(match: RegExpExecArray): string | null {
  const decimal = match[1]
  const hex = match[2]
  const name = match[3]
  let code: number | null = null
  if (decimal !== undefined) code = parseInt(decimal, 10)
  else if (hex !== undefined) code = parseInt(hex, 16)
  else if (name !== undefined) return namedEntities[name] ?? null
  if (code === null || Number.isNaN(code)) return null
  if (code === 0 || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) return '�'
  return String.fromCodePoint(code)
}

// MARK: - Rendering

/** How `renderInline` styles and wires the nodes (the fonts and colours `MarkdownInline.render` applies). */
export interface InlineRenderOptions {
  /** CSS Module classes of the host (`MarkdownView.module.css`). */
  classNames: {
    code: string | undefined
    link: string | undefined
    strong?: string | undefined
    emphasis?: string | undefined
    strikethrough?: string | undefined
  }
  /** Links open through this (the macOS app's `openURL` environment). */
  onOpenURL: (url: string) => void
  /** Renders a resolved file reference as a chip; without it the label is plain text. */
  renderFileReference?: (node: FileReferenceNode, key: string) => ReactNode
}

/** Inline nodes → React nodes (`MarkdownInline.render(_:font:)`). */
export function renderInline(nodes: readonly InlineNode[], options: InlineRenderOptions, keyPrefix = ''): ReactNode[] {
  return nodes.map((node, index) => renderNode(node, `${keyPrefix}${index}`, options))
}

/**
 * Script-bearing schemes have no meaning outside a browser and are never
 * handed to the opener (the Swift app has no script context to guard).
 */
function isOpenable(href: string): boolean {
  const scheme = /^\s*([A-Za-z][A-Za-z0-9+.-]*):/.exec(href)?.[1]?.toLowerCase()
  return scheme !== 'javascript' && scheme !== 'vbscript' && scheme !== 'data'
}

function renderNode(node: InlineNode, key: string, options: InlineRenderOptions): ReactNode {
  switch (node.kind) {
    case 'text':
      return createElement(Fragment, { key }, node.text)
    case 'lineBreak':
      return createElement('br', { key })
    case 'code':
      return createElement('code', { key, className: options.classNames.code }, node.text)
    case 'strong':
      return createElement(
        'strong',
        { key, className: options.classNames.strong },
        ...renderInline(node.children, options, `${key}.`),
      )
    case 'emphasis':
      return createElement('em', { key, className: options.classNames.emphasis }, ...renderInline(node.children, options, `${key}.`))
    case 'strikethrough':
      return createElement(
        's',
        { key, className: options.classNames.strikethrough },
        ...renderInline(node.children, options, `${key}.`),
      )
    case 'link': {
      const href = node.href
      const openable = isOpenable(href)
      // The webview must never navigate itself: every click (and a middle
      // click, which would open a window) goes to `onOpenURL` instead.
      const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
        event.preventDefault()
        if (openable) options.onOpenURL(href)
      }
      const onAuxClick = (event: MouseEvent<HTMLAnchorElement>) => {
        event.preventDefault()
      }
      return createElement(
        'a',
        {
          key,
          href: openable ? href : undefined,
          className: options.classNames.link,
          title: node.title ?? undefined,
          rel: 'noreferrer',
          onClick,
          onAuxClick,
        },
        ...renderInline(node.children, options, `${key}.`),
      )
    }
    case 'fileReference':
      return options.renderFileReference
        ? options.renderFileReference(node, key)
        : createElement(Fragment, { key }, node.label)
  }
}
