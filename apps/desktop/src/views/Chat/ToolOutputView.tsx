// Port of apps/macos/ContextAgent/Views/Chat/ToolOutputView.swift: the
// card that opens under a tool timeline row. The "Open full output" sheet
// is FullOutputSheet.tsx.

import { useMemo, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { CopyButton } from '../../components/CopyButton'
import { capLines, LineList } from '../../components/LineList'
import { Tooltip } from '../../components/Tooltip'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { jsonArray, jsonObject, jsonPrettyPrinted, jsonString, type JSONValue } from '../../backend/types'
import { lineHighlighter, type Language } from '../../markdown/highlighter'
import { pathArgument, rowLabel } from '../../models/toolMetadata'
import { TranscriptFormat, toolOutputOmittedCharacters, type ToolCallItem } from '../../models/transcript'
import { FullOutputSheet } from './FullOutputSheet'
import styles from './ToolOutputView.module.css'

/** `ToolOutputView.Segment`. */
type Segment = 'Arguments' | 'Output'

const SEGMENTS: readonly Segment[] = ['Arguments', 'Output']

/** `ToolOutputView.maxLines` / `maxBytes`: the caps of the inline block. */
const MAX_LINES = 400
const MAX_BYTES = 32_768
/// Blocks up to this many lines sit inline in the transcript; longer ones
/// get their own `maxHeight` (320 px) scroll view, so short cards never hold
/// a nested scroller that captures the wheel from the transcript.
const INLINE_LINE_LIMIT = 18

export interface ToolOutputViewProps {
  call: ToolCallItem
}

/**
 * `ToolOutputView` of the macOS app: `codeBackground`, 14 px radius,
 * *Arguments* | *Output* segments top-left with the format label, a
 * `CopyButton` top-right, capped monospace lines ("+ N more lines · Open
 * full output" opens `FullOutputSheet`), a truncation footer when the
 * backend cut the output, and `apply_patch` edits rendered as a mini diff.
 */
export function ToolOutputView({ call }: ToolOutputViewProps) {
  const [chosenSegment, setChosenSegment] = useState<Segment | null>(null)
  const [showingFull, setShowingFull] = useState(false)

  const output = call.output
  // Output when present, Arguments while running or when nothing came back.
  let segment: Segment
  if (chosenSegment !== null) segment = chosenSegment === 'Output' && output === null ? 'Arguments' : chosenSegment
  else segment = output !== null ? 'Output' : 'Arguments'

  const patchEdits = useMemo(() => (call.name === 'apply_patch' ? patchEditsFrom(call.arguments) : []), [call.name, call.arguments])
  const isPatch = patchEdits.length > 0
  const argumentsText = useMemo(() => (isPatch ? patchDiffText(patchEdits) : jsonPrettyPrinted(call.arguments)), [isPatch, patchEdits, call.arguments])
  const currentText = segment === 'Arguments' ? argumentsText : (output?.content ?? '')

  const formatLabel = segment === 'Arguments' ? (isPatch ? 'diff' : 'JSON') : (output?.format ?? 'text')

  let language: Language = 'plain'
  if (segment === 'Arguments') language = isPatch ? 'diff' : 'json'
  else if (output?.format === 'JSON') language = 'json'
  else if (output?.format === 'diff') language = 'diff'
  const highlight = lineHighlighter(language)

  const label = rowLabel(call)
  const sheetTitle = `${label} · ${segment}`
  const extension = formatLabel === 'JSON' ? 'json' : formatLabel === 'diff' ? 'diff' : 'txt'
  const suggestedFileName = `${call.name.replace(/_/g, '-')}-${segment.toLowerCase()}.${extension}`

  const lineCount = useMemo(() => capLines(currentText, MAX_LINES, MAX_BYTES).lines.length, [currentText])
  const patchPath = segment === 'Arguments' && isPatch ? pathArgument(call) : null
  const omitted = segment === 'Output' && output !== null ? toolOutputOmittedCharacters(output) : 0

  let emptyMessage: string
  if (segment === 'Arguments') emptyMessage = 'No arguments'
  else emptyMessage = call.status === 'running' ? 'Waiting for output…' : 'No output'

  return (
    <div className={styles.card} role="group" aria-label={`${label} details`}>
      <div className={styles.header}>
        <div className={styles.picker} role="tablist" aria-label="Tool details">
          {SEGMENTS.map((candidate) => {
            const selected = candidate === segment
            const available = candidate === 'Arguments' || output !== null
            return (
              <Tooltip key={candidate} content={available ? candidate : 'No output yet'}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-disabled={!available || undefined}
                  aria-label={candidate}
                  className={[styles.segment, selected ? styles.segmentSelected : '', available ? '' : styles.segmentUnavailable].filter(Boolean).join(' ')}
                  onClick={() => {
                    if (available) setChosenSegment(candidate)
                  }}
                >
                  {candidate}
                </button>
              </Tooltip>
            )
          })}
        </div>
        <span className={styles.format}>{formatLabel}</span>
        {patchPath !== null ? (
          <MiddleTruncate
            text={`${patchPath} · ${patchEdits.length} edit${patchEdits.length === 1 ? '' : 's'}`}
            className={styles.patchPath}
          />
        ) : null}
        {segment === 'Output' && output?.isError === true ? (
          <Chip variant="tinted" tint="var(--danger)">
            error
          </Chip>
        ) : null}
        <span className={patchPath !== null ? styles.spacerFixed : styles.spacer} />
        {call.status === 'running' ? (
          <span className={styles.running}>
            <BrailleSpinner size={11} />
            <span className={styles.runningLabel}>Running</span>
          </span>
        ) : call.durationMs !== null ? (
          <span className={styles.duration}>{TranscriptFormat.durationMs(call.durationMs)}</span>
        ) : null}
        <CopyButton text={currentText} label={`Copy ${segment.toLowerCase()}`} />
      </div>

      {currentText.length === 0 ? (
        <span className={styles.empty}>{emptyMessage}</span>
      ) : lineCount > INLINE_LINE_LIMIT ? (
        <div className={styles.scroller}>
          <LineList
            text={currentText}
            maxLines={MAX_LINES}
            maxBytes={MAX_BYTES}
            highlight={highlight}
            cacheKey={highlight ? language : undefined}
            onOpenFull={() => setShowingFull(true)}
            className={styles.lines}
          />
        </div>
      ) : (
        <LineList
          text={currentText}
          maxLines={MAX_LINES}
          maxBytes={MAX_BYTES}
          highlight={highlight}
          cacheKey={highlight ? language : undefined}
          onOpenFull={() => setShowingFull(true)}
          className={styles.lines}
        />
      )}

      {omitted > 0 ? (
        <Tooltip content="The runtime truncated this output before the model saw it">
          <span className={styles.omitted}>+ {omitted.toLocaleString()} more characters</span>
        </Tooltip>
      ) : null}

      {showingFull ? (
        <FullOutputSheet open onClose={() => setShowingFull(false)} title={sheetTitle} text={currentText} suggestedFileName={suggestedFileName} />
      ) : null}
    </div>
  )
}

// MARK: - apply_patch edits

/** One `{old_text, new_text}` entry of an `apply_patch` call (`old` / `new` are accepted too). */
interface PatchEdit {
  id: number
  oldText: string
  newText: string
}

function patchEditsFrom(args: JSONValue): PatchEdit[] {
  const list = jsonArray(jsonObject(args)?.edits)
  if (!list) return []
  return list.map((entry, index) => {
    const edit = jsonObject(entry)
    return {
      id: index,
      oldText: jsonString(edit?.old_text) ?? jsonString(edit?.old) ?? '',
      newText: jsonString(edit?.new_text) ?? jsonString(edit?.new) ?? '',
    }
  })
}

/** Unified-diff-like text (`@@ edit 1 of 2 @@`, `-old`, `+new`) for the diff highlighter. */
function patchDiffText(edits: readonly PatchEdit[]): string {
  const out: string[] = []
  for (const edit of edits) {
    if (edits.length > 1) out.push(`@@ edit ${edit.id + 1} of ${edits.length} @@`)
    if (edit.oldText.length === 0) out.push('@@ insert @@')
    else for (const line of linesOf(edit.oldText)) out.push('-' + line)
    if (edit.newText.length === 0) out.push('@@ delete @@')
    else for (const line of linesOf(edit.newText)) out.push('+' + line)
  }
  return out.join('\n')
}

/** `split(separator: "\n", omittingEmptySubsequences: false)` minus one trailing empty line. */
function linesOf(text: string): string[] {
  const parts = text.split('\n')
  if (parts.length > 1 && parts[parts.length - 1] === '') parts.pop()
  return parts
}
