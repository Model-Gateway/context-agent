/**
 * Port of `CodeBlockView.swift`: a fenced code block with the language
 * label top-left, the copy button top-right on hover, and horizontally
 * scrolling highlighted lines (`LineList` with the block's grammar) on
 * `--code-background`. Shell blocks also get a play button that runs them
 * in a new terminal tab of the selected project when the host passes
 * `onRun` (the Swift view reads `AppState.selectedProject`; here the chat
 * view decides by passing the callback or not).
 */
import { useMemo } from 'react'
import { CopyButton } from '../components/CopyButton'
import { LineList } from '../components/LineList'
import { Tooltip } from '../components/Tooltip'
import { Icon } from '../theme/icons'
import styles from './CodeBlock.module.css'
import { detectLanguage, languageDisplayName, lineHighlighter, type Language } from './highlighter'

/** `CodeBlockView(code:language:)`, with the terminal action passed in instead of read from `AppState`. */
export interface CodeBlockProps {
  code: string
  /** The fence's info string, a file extension or a format label; `null` → "Code". */
  language?: string | null
  /** Runs the block's commands in a new terminal tab; offered for shell blocks only. */
  onRun?: (command: string) => void
}

/** `CodeBlockView` in Swift. */
export function CodeBlock({ code, language = null, onRun }: CodeBlockProps) {
  const detected = detectLanguage(language)
  const label = blockLabel(detected, language)
  const command = useMemo(() => shellCommand(code), [code])
  const showRun = detected === 'shell' && onRun !== undefined && command.length > 0

  return (
    <div className={styles.block}>
      <div className={styles.header}>
        <span className={styles.label}>{label}</span>
        {/* Invisible until hovered, but always reachable by keyboard and assistive technology. */}
        <div className={styles.actions}>
          {showRun ? <RunButton onClick={() => onRun(command)} /> : null}
          <CopyButton text={code} label="Copy code" />
        </div>
      </div>
      <div className={styles.scroller}>
        <LineList text={code} highlight={lineHighlighter(detected)} wrap={false} className={styles.lines} />
      </div>
    </div>
  )
}

/** The play button beside Copy, styled like `CopyButton` (`RunButton` in Swift). */
function RunButton({ onClick }: { onClick: () => void }) {
  return (
    <Tooltip content="Run in a new terminal tab">
      <button type="button" className={styles.run} onClick={onClick} aria-label="Run in terminal">
        <Icon name="run" size={12} strokeWidth={2.25} />
      </button>
    </Tooltip>
  )
}

/**
 * The block as commands: with `$ ` prompts, only the prompted lines (output
 * lines in console transcripts are dropped).
 */
function shellCommand(code: string): string {
  const lines = code.split('\n')
  const prompted = lines.filter((line) => line.startsWith('$ '))
  const commands = prompted.length === 0 ? lines : prompted.map((line) => line.slice(2))
  return commands.join('\n').trim()
}

/** The header label: the detected language's name, else the fence's word capitalised, else "Code". */
function blockLabel(detected: Language, language: string | null): string {
  if (detected !== 'plain') return languageDisplayName(detected)
  if (language === null) return 'Code'
  const trimmed = language.trim()
  if (trimmed.length === 0) return 'Code'
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1)
}
