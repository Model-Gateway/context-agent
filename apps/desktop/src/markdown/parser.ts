/**
 * Port of `MarkdownParser.swift`: a tolerant, line-based CommonMark-ish
 * block parser written for streaming model output. An unterminated fence is
 * still a code block, a half-written table is a paragraph until its
 * delimiter row arrives, and nothing ever throws. Inline syntax (emphasis,
 * code spans, links) stays in the block text and is rendered by
 * `MarkdownView` through `inline.ts`.
 */

/** `MarkdownBlock` in Swift. */
export type MarkdownBlock =
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'paragraph'; text: string }
  | { kind: 'bulletList'; items: MarkdownListItem[] }
  | { kind: 'orderedList'; items: MarkdownListItem[]; start: number }
  | { kind: 'blockquote'; blocks: MarkdownBlock[] }
  | { kind: 'codeFence'; language: string | null; code: string }
  | { kind: 'table'; headers: string[]; rows: string[][] }
  | { kind: 'horizontalRule' }

/** `MarkdownListItem` in Swift. */
export interface MarkdownListItem {
  /** Inline markdown of the item's first paragraph. */
  text: string
  /** Nested blocks (sub-lists, further paragraphs, code) indented under the item. */
  nested: MarkdownBlock[]
}

/** `MarkdownParser.parse(_:)`: the blocks of a markdown document. */
export function parseMarkdown(markdown: string): MarkdownBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  return parseBlocks(lines)
}

// MARK: - Blocks

function parseBlocks(lines: string[]): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = []
  let i = 0
  const count = lines.length

  while (i < count) {
    const line = lineAt(lines, i)
    if (isBlank(line)) {
      i += 1
      continue
    }

    const fence = fenceOpening(line)
    if (fence) {
      const code: string[] = []
      let j = i + 1
      let closed = false
      while (j < count) {
        if (isFenceClosing(lineAt(lines, j), fence)) {
          closed = true
          break
        }
        code.push(dropIndent(lineAt(lines, j), fence.indent))
        j += 1
      }
      blocks.push({ kind: 'codeFence', language: fence.language, code: code.join('\n') })
      i = closed ? j + 1 : j
      continue
    }

    const heading = atxHeading(line)
    if (heading) {
      blocks.push({ kind: 'heading', level: heading.level, text: heading.text })
      i += 1
      continue
    }

    if (isHorizontalRule(line)) {
      blocks.push({ kind: 'horizontalRule' })
      i += 1
      continue
    }

    if (isBlockquote(line)) {
      const inner: string[] = []
      let j = i
      while (j < count && isBlockquote(lineAt(lines, j))) {
        inner.push(stripQuoteMarker(lineAt(lines, j)))
        j += 1
      }
      blocks.push({ kind: 'blockquote', blocks: parseBlocks(inner) })
      i = j
      continue
    }

    if (i + 1 < count && isTableRow(line) && isTableDelimiter(lineAt(lines, i + 1))) {
      const headers = tableCells(line)
      const rows: string[][] = []
      let j = i + 2
      while (j < count && !isBlank(lineAt(lines, j)) && isTableRow(lineAt(lines, j))) {
        let cells = tableCells(lineAt(lines, j))
        if (cells.length < headers.length) {
          cells = cells.concat(new Array<string>(headers.length - cells.length).fill(''))
        } else if (cells.length > headers.length) {
          cells = cells.slice(0, headers.length)
        }
        rows.push(cells)
        j += 1
      }
      blocks.push({ kind: 'table', headers, rows })
      i = j
      continue
    }

    const marker = listMarker(line)
    if (marker) {
      const parsed = parseList(lines, i, marker)
      blocks.push(parsed.block)
      i = parsed.next
      continue
    }

    // Paragraph: consecutive non-blank lines until another block starts.
    const paragraph: string[] = [trimSpaces(line)]
    let j = i + 1
    let setextLevel: number | null = null
    while (j < count) {
      const next = lineAt(lines, j)
      if (isBlank(next)) break
      const level = setextUnderline(next)
      if (level !== null) {
        setextLevel = level
        j += 1
        break
      }
      if (interruptsParagraph(next)) break
      if (j + 1 < count && isTableRow(next) && isTableDelimiter(lineAt(lines, j + 1))) break
      paragraph.push(trimSpaces(next))
      j += 1
    }
    const text = paragraph.join('\n')
    if (setextLevel !== null) {
      blocks.push({ kind: 'heading', level: setextLevel, text })
    } else {
      blocks.push({ kind: 'paragraph', text })
    }
    i = j
  }
  return blocks
}

// MARK: - Lists

interface ListMarker {
  ordered: boolean
  number: number
  /** Column of the marker. */
  indent: number
  /** Column where the item's content starts; nested lines are stripped to it. */
  contentIndent: number
  text: string
}

function listMarker(line: string): ListMarker | null {
  const indentation = indentOf(line)
  const rest = line.slice(indentation.chars)
  const first = rest.charAt(0)
  if (!first) return null

  if (first === '-' || first === '*' || first === '+') {
    const after = rest.slice(1)
    if (after.length === 0) {
      return { ordered: false, number: 0, indent: indentation.columns, contentIndent: indentation.columns + 2, text: '' }
    }
    const afterFirst = after.charAt(0)
    if (afterFirst !== ' ' && afterFirst !== '\t') return null
    const spaces = leadingCount(after, (c) => c === ' ' || c === '\t')
    const text = after.slice(spaces)
    return {
      ordered: false,
      number: 0,
      indent: indentation.columns,
      contentIndent: indentation.columns + 1 + Math.min(Math.max(spaces, 1), 4),
      text,
    }
  }

  if (/[0-9]/.test(first)) {
    const digitCount = leadingCount(rest, (c) => c >= '0' && c <= '9')
    if (digitCount > 9) return null
    const digits = rest.slice(0, digitCount)
    const number = parseInt(digits, 10)
    if (Number.isNaN(number)) return null
    const afterDigits = rest.slice(digitCount)
    const delimiter = afterDigits.charAt(0)
    if (delimiter !== '.' && delimiter !== ')') return null
    const after = afterDigits.slice(1)
    if (after.length === 0) {
      return {
        ordered: true,
        number,
        indent: indentation.columns,
        contentIndent: indentation.columns + digitCount + 2,
        text: '',
      }
    }
    const afterFirst = after.charAt(0)
    if (afterFirst !== ' ' && afterFirst !== '\t') return null
    const spaces = leadingCount(after, (c) => c === ' ' || c === '\t')
    const text = after.slice(spaces)
    return {
      ordered: true,
      number,
      indent: indentation.columns,
      contentIndent: indentation.columns + digitCount + 1 + Math.min(Math.max(spaces, 1), 4),
      text,
    }
  }
  return null
}

function parseList(lines: string[], start: number, first: ListMarker): { block: MarkdownBlock; next: number } {
  const items: MarkdownListItem[] = []
  let i = start
  const count = lines.length
  const ordered = first.ordered
  const baseIndent = first.indent
  let currentText: string[] = []
  let currentNested: string[] = []
  let hasCurrent = false
  let contentIndent = first.contentIndent
  let previousBlank = false

  const flush = () => {
    if (!hasCurrent) return
    items.push({ text: currentText.join('\n'), nested: parseBlocks(currentNested) })
    currentText = []
    currentNested = []
    hasCurrent = false
  }

  while (i < count) {
    const line = lineAt(lines, i)

    if (isBlank(line)) {
      const nextIndex = nextNonBlank(lines, i + 1)
      if (nextIndex === null) break
      const nextLine = lineAt(lines, nextIndex)
      const nextIndent = indentOf(nextLine).columns
      if (nextIndent > baseIndent) {
        currentNested.push('')
        previousBlank = true
        i += 1
        continue
      }
      const marker = listMarker(nextLine)
      if (marker && marker.ordered === ordered) {
        previousBlank = true
        i += 1
        continue
      }
      break
    }

    const lineIndent = indentOf(line).columns
    if (lineIndent > baseIndent && hasCurrent) {
      currentNested.push(dropIndent(line, Math.min(lineIndent, contentIndent)))
      previousBlank = false
      i += 1
      continue
    }

    const marker = listMarker(line)
    if (marker) {
      if (marker.ordered !== ordered) break
      flush()
      hasCurrent = true
      currentText = [marker.text]
      contentIndent = marker.contentIndent
      previousBlank = false
      i += 1
      continue
    }

    if (hasCurrent && !previousBlank && !isBlockStart(line)) {
      // Lazy continuation of the item's paragraph.
      currentText.push(trimSpaces(line))
      i += 1
      continue
    }
    break
  }
  flush()
  const block: MarkdownBlock = ordered
    ? { kind: 'orderedList', items, start: first.number }
    : { kind: 'bulletList', items }
  return { block, next: i }
}

// MARK: - Fences

interface Fence {
  character: string
  length: number
  indent: number
  language: string | null
}

function fenceOpening(line: string): Fence | null {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return null
  const rest = line.slice(indentation.chars)
  const first = rest.charAt(0)
  if (first !== '`' && first !== '~') return null
  const run = leadingCount(rest, (c) => c === first)
  if (run < 3) return null
  const info = rest.slice(run).trim()
  if (first === '`' && info.includes('`')) return null
  const language = info.split(/[ \t]+/).find((word) => word.length > 0) ?? null
  return { character: first, length: run, indent: indentation.columns, language }
}

function isFenceClosing(line: string, fence: Fence): boolean {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return false
  const rest = line.slice(indentation.chars)
  const run = leadingCount(rest, (c) => c === fence.character)
  if (run < fence.length) return false
  return isBlank(rest.slice(run))
}

// MARK: - Headings and rules

function atxHeading(line: string): { level: number; text: string } | null {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return null
  const rest = line.slice(indentation.chars)
  const hashes = leadingCount(rest, (c) => c === '#')
  if (hashes < 1 || hashes > 6) return null
  const after = rest.slice(hashes)
  const afterFirst = after.charAt(0)
  if (after.length > 0 && afterFirst !== ' ' && afterFirst !== '\t') return null
  let text = trimSpaces(after)
  if (text.endsWith('#')) {
    const withoutClosing = text.replace(/#+$/, '')
    if (withoutClosing.length === 0 || withoutClosing.endsWith(' ') || withoutClosing.endsWith('\t')) {
      text = trimSpaces(withoutClosing)
    }
  }
  return { level: hashes, text }
}

/**
 * `===` → level 1, `---` → level 2 (a `-` underline needs three characters
 * so a half-streamed list marker never flickers into a heading).
 */
function setextUnderline(line: string): number | null {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return null
  const trimmed = trimSpaces(line)
  const first = trimmed.charAt(0)
  if (first !== '=' && first !== '-') return null
  for (const character of trimmed) {
    if (character !== first) return null
  }
  if (first === '-' && trimmed.length < 3) return null
  return first === '=' ? 1 : 2
}

function isHorizontalRule(line: string): boolean {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return false
  const characters = line.slice(indentation.chars).replace(/[ \t]/g, '')
  if (characters.length < 3) return false
  const first = characters.charAt(0)
  if (first !== '-' && first !== '*' && first !== '_') return false
  for (const character of characters) {
    if (character !== first) return false
  }
  return true
}

// MARK: - Blockquotes

function isBlockquote(line: string): boolean {
  const indentation = indentOf(line)
  if (indentation.columns > 3) return false
  return line.charAt(indentation.chars) === '>'
}

function stripQuoteMarker(line: string): string {
  const indentation = indentOf(line)
  let rest = line.slice(indentation.chars)
  if (rest.startsWith('>')) rest = rest.slice(1)
  if (rest.startsWith(' ')) rest = rest.slice(1)
  return rest
}

// MARK: - Tables

function isTableRow(line: string): boolean {
  return line.includes('|')
}

function isTableDelimiter(line: string): boolean {
  const trimmed = trimSpaces(line)
  if (!trimmed.includes('|') || !trimmed.includes('-')) return false
  const cells = tableCells(trimmed)
  if (cells.length === 0) return false
  return cells.every((cell) => cell.includes('-') && /^[-:]+$/.test(cell))
}

/** Splits a row on unescaped pipes; `\|` becomes a literal pipe. */
function tableCells(line: string): string[] {
  const cells: string[] = []
  let current = ''
  let escaped = false
  for (const character of line) {
    if (escaped) {
      if (character === '|') {
        current += '|'
      } else {
        current += '\\' + character
      }
      escaped = false
      continue
    }
    if (character === '\\') {
      escaped = true
      continue
    }
    if (character === '|') {
      cells.push(current)
      current = ''
      continue
    }
    current += character
  }
  if (escaped) current += '\\'
  cells.push(current)

  const trimmed = cells.map(trimSpaces)
  const outer = trimSpaces(line)
  if (outer.startsWith('|') && trimmed.length > 0) trimmed.shift()
  if (outer.endsWith('|') && !outer.endsWith('\\|') && trimmed.length > 0 && trimmed[trimmed.length - 1] === '') {
    trimmed.pop()
  }
  return trimmed
}

// MARK: - Helpers

function lineAt(lines: string[], index: number): string {
  return lines[index] ?? ''
}

function isBlank(line: string): boolean {
  return /^[ \t]*$/.test(line)
}

function isBlockStart(line: string): boolean {
  return (
    fenceOpening(line) !== null ||
    atxHeading(line) !== null ||
    isHorizontalRule(line) ||
    isBlockquote(line) ||
    listMarker(line) !== null
  )
}

/**
 * Like `isBlockStart`, but an empty list item ("-" alone, typically a
 * half-streamed marker) does not interrupt a paragraph (CommonMark 5.2).
 */
function interruptsParagraph(line: string): boolean {
  if (fenceOpening(line) !== null || atxHeading(line) !== null || isHorizontalRule(line) || isBlockquote(line)) {
    return true
  }
  const marker = listMarker(line)
  if (marker) return marker.text.length > 0
  return false
}

function nextNonBlank(lines: string[], start: number): number | null {
  let i = start
  while (i < lines.length) {
    if (!isBlank(lineAt(lines, i))) return i
    i += 1
  }
  return null
}

/** Leading whitespace as (columns with tabs expanded to 4, characters). */
function indentOf(line: string): { columns: number; chars: number } {
  let columns = 0
  let chars = 0
  for (const character of line) {
    if (character === ' ') {
      columns += 1
    } else if (character === '\t') {
      columns += 4 - (columns % 4)
    } else {
      break
    }
    chars += 1
  }
  return { columns, chars }
}

/** Removes up to `columns` columns of leading whitespace. */
function dropIndent(line: string, columns: number): string {
  let column = 0
  let index = 0
  while (index < line.length && column < columns) {
    const character = line.charAt(index)
    if (character === ' ') {
      column += 1
    } else if (character === '\t') {
      column += 4 - (column % 4)
    } else {
      break
    }
    index += 1
  }
  return line.slice(index)
}

function leadingCount(text: string, predicate: (character: string) => boolean): number {
  let count = 0
  while (count < text.length && predicate(text.charAt(count))) count += 1
  return count
}

/** Swift's `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not newlines. */
function trimSpaces(text: string): string {
  return text.replace(/^[\t\p{Zs}]+|[\t\p{Zs}]+$/gu, '')
}
