/**
 * Port of `MarkdownView.swift`: renders markdown (assistant messages,
 * outcome summaries) with the Theme fonts. Blocks come from `parser.ts`;
 * inline syntax goes through `inline.ts` so emphasis, code spans and links
 * work. Links are tinted `--accent` and open through `onOpenURL` (the
 * `open_url` command when the host passes nothing). File names that resolve
 * to project files render as chips (`fileReferences.ts`) when a
 * `FileReferenceContext` is passed in.
 */
import { createContext, useContext, useMemo, type MouseEvent, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ContextMenu } from '../components/ContextMenu'
import type { MenuItem } from '../components/Menu'
import { isModKey, modLabel, revealLabel } from '../components/platform'
import { Icon } from '../theme/icons'
import styles from './MarkdownView.module.css'
import { CodeBlock } from './CodeBlock'
import { parseInline, renderInline, type FileReferenceNode } from './inline'
import { inlineWithFileReferences, type FileReference, type FileReferenceContext } from './fileReferences'
import { parseMarkdown, type MarkdownBlock, type MarkdownListItem } from './parser'
import { RenderCache } from './renderCache'

/** `MarkdownView(_:)` plus the environment it reads in Swift (`fileReferences`, `openURL`, `AppState` for the run button). */
export interface MarkdownViewProps {
  /** The markdown source; may be half-streamed. */
  markdown: string
  /**
   * Resolves file names to project files; without it names render as plain
   * text. Hosts memoize it (the Swift context compares equal by root so a
   * parent rebuild does not redraw every message; here a new object
   * re-renders the blocks).
   */
  files?: FileReferenceContext | null
  /** Where links go (the Swift app's `openURL` environment); defaults to the `open_url` command. */
  onOpenURL?: (url: string) => void
  /** Runs a shell block's commands in a new terminal tab (CodeBlock's play button). */
  onRunCommand?: (command: string) => void
  className?: string
}

/** The elements one block's inline text renders into. */
type InlineTag = 'p' | 'div' | 'span' | 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6'

/** Parsed blocks by source text (`MarkdownView.parsed`, 512 entries). */
const parsedBlocks = new RenderCache<string, MarkdownBlock[]>(512)

interface Environment {
  files: FileReferenceContext | null
  openURL: (url: string) => void
  onRunCommand?: (command: string) => void
}

const EnvironmentContext = createContext<Environment>({ files: null, openURL: defaultOpenURL })

function defaultOpenURL(url: string): void {
  invoke('open_url', { url }).catch(() => {
    /* Outside Tauri (plain Vite) there is nothing to open with. */
  })
}

/**
 * `MarkdownView` in Swift: the blocks of `markdown` 10 px apart in the
 * Theme fonts, primary text colour, full width. Parsing is memoized by text
 * (a row that scrolls back into view does not parse again).
 */
export function MarkdownView({ markdown, files = null, onOpenURL, onRunCommand, className }: MarkdownViewProps) {
  const blocks = useMemo(() => parsedBlocks.value(markdown, () => parseMarkdown(markdown)), [markdown])
  const environment = useMemo<Environment>(
    () => ({ files, openURL: onOpenURL ?? defaultOpenURL, onRunCommand }),
    [files, onOpenURL, onRunCommand],
  )
  return (
    <EnvironmentContext.Provider value={environment}>
      <div className={cx(styles.root, className)}>
        {blocks.map((block, index) => (
          <BlockView key={index} block={block} />
        ))}
      </div>
    </EnvironmentContext.Provider>
  )
}

// MARK: - Blocks

function BlockView({ block }: { block: MarkdownBlock }) {
  const environment = useContext(EnvironmentContext)
  switch (block.kind) {
    case 'heading':
      return <Heading level={block.level} text={block.text} />
    case 'paragraph':
      return <InlineText as="p" text={block.text} className={styles.paragraph} />
    case 'bulletList':
      return <ListView items={block.items} ordered={false} start={1} />
    case 'orderedList':
      return <ListView items={block.items} ordered start={block.start} />
    case 'blockquote':
      return <QuoteView blocks={block.blocks} />
    case 'codeFence':
      return <CodeBlock code={block.code} language={block.language} onRun={environment.onRunCommand} />
    case 'table':
      return <TableView headers={block.headers} rows={block.rows} />
    case 'horizontalRule':
      return <hr className={styles.rule} aria-hidden="true" />
  }
}

function Heading({ level, text }: { level: number; text: string }) {
  const tag: InlineTag =
    level === 1 ? 'h1' : level === 2 ? 'h2' : level === 3 ? 'h3' : level === 4 ? 'h4' : level === 5 ? 'h5' : 'h6'
  const sizeClass = level === 1 ? styles.h1 : level === 2 ? styles.h2 : level === 3 ? styles.h3 : styles.h4
  return <InlineText as={tag} text={text} className={cx(styles.heading, sizeClass)} />
}

/**
 * `InlineText` in Swift: one block's inline markdown, with file references
 * as chips when a context is in the environment.
 */
function InlineText({ as, text, className }: { as: InlineTag; text: string; className?: string }) {
  const environment = useContext(EnvironmentContext)
  const { files, openURL } = environment
  const content = useMemo<ReactNode[]>(() => {
    const nodes = files ? inlineWithFileReferences(text, files) : parseInline(text)
    return renderInline(nodes, {
      classNames: {
        code: styles.inlineCode,
        link: styles.link,
        strong: styles.strong,
        emphasis: styles.emphasis,
        strikethrough: styles.strikethrough,
      },
      onOpenURL: openURL,
      renderFileReference: files ? (node, key) => <FileChip key={key} node={node} files={files} /> : undefined,
    })
  }, [text, files, openURL])
  const Tag = as
  return <Tag className={cx(styles.inline, className)}>{content}</Tag>
}

function ListView({ items, ordered, start }: { items: MarkdownListItem[]; ordered: boolean; start: number }) {
  const Tag = ordered ? 'ol' : 'ul'
  return (
    <Tag className={styles.list}>
      {items.map((item, index) => (
        <li key={index} className={styles.item}>
          <span className={styles.marker} aria-hidden="true">
            {ordered ? `${start + index}.` : '•'}
          </span>
          <div className={styles.itemBody}>
            {item.text.length > 0 ? <InlineText as="div" text={item.text} /> : null}
            {item.nested.map((block, nestedIndex) => (
              <BlockView key={nestedIndex} block={block} />
            ))}
          </div>
        </li>
      ))}
    </Tag>
  )
}

function QuoteView({ blocks }: { blocks: MarkdownBlock[] }) {
  return (
    <blockquote className={styles.quote}>
      <span className={styles.quoteBar} aria-hidden="true" />
      <div className={styles.quoteBody}>
        {blocks.map((block, index) => (
          <BlockView key={index} block={block} />
        ))}
      </div>
    </blockquote>
  )
}

function TableView({ headers, rows }: { headers: string[]; rows: string[][] }) {
  return (
    <div className={styles.tableWrap}>
      <table className={styles.table}>
        <thead>
          <tr>
            {headers.map((header, column) => (
              <th key={column} className={styles.th} scope="col">
                <InlineText as="span" text={header} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, column) => (
                <td key={column} className={styles.td}>
                  <InlineText as="span" text={cell} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// MARK: - File chips

/**
 * One file reference drawn as the neutral chip of the "Modified files"
 * section: click opens the file in the Editor, mod-click opens it with the
 * default editor, and the context menu offers the path actions
 * (`FileReferenceChipsModifier.target` in Swift).
 */
function FileChip({ node, files }: { node: FileReferenceNode; files: FileReferenceContext }) {
  const reference: FileReference = { path: node.path }
  if (node.line !== undefined) reference.line = node.line
  if (node.column !== undefined) reference.column = node.column
  const onClick = (event: MouseEvent<HTMLButtonElement>) => {
    files.perform(reference, isModKey(event) ? 'openWithEditor' : 'openInEditor')
  }
  const menu = (): MenuItem[] => [
    { label: 'Open in Editor', onSelect: () => files.perform(reference, 'openInEditor') },
    { label: 'Show in Changes', onSelect: () => files.perform(reference, 'showInChanges') },
    { label: 'Open with Default Editor', onSelect: () => files.perform(reference, 'openWithEditor') },
    { type: 'separator' },
    { label: revealLabel, onSelect: () => files.perform(reference, 'revealInFileManager') },
    { label: 'Copy Path', onSelect: () => files.perform(reference, 'copyPath') },
  ]
  return (
    <ContextMenu items={menu} aria-label={`Actions for ${node.path}`}>
      <button
        type="button"
        className={styles.chip}
        onClick={onClick}
        title={`${node.path} — click to open in the Editor, ${modLabel}-click to open with the default editor`}
        aria-label={`File ${node.path}`}
      >
        <span className={styles.chipIcon} aria-hidden="true">
          <Icon name="file" size={10} />
        </span>
        <span>{node.label}</span>
      </button>
    </ContextMenu>
  )
}

function cx(...parts: (string | undefined | false | null)[]): string {
  return parts.filter((part): part is string => typeof part === 'string' && part.length > 0).join(' ')
}
