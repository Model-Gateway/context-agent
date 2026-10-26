// Port of apps/macos/ContextAgent/Views/Chat/ToolTimelineView.swift: one
// tool group of a run (the header "18 tools, edited 5 files, ran 2
// commands ⌄" with the elapsed time, one row per call joined by a 1 px
// connector, the ending row when the group stopped / failed / blocked) and
// the small pieces the other cards share (`Fade`, `LiveDuration`,
// `pathMenuItems`).

import { memo, useCallback, useEffect, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { Fade } from '../../components/Fade'
import type { MenuItem } from '../../components/Menu'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { revealLabel } from '../../components/platform'
import { Tooltip } from '../../components/Tooltip'
import { abbreviate, lineChanges, pathArgument, rowLabel, toolInfo } from '../../models/toolMetadata'
import {
  TranscriptFormat,
  toolGroupFailedCount,
  toolGroupHeadline,
  toolGroupIsOpen,
  type ToolCallItem,
  type ToolGroup,
  type ToolGroupTerminal,
} from '../../models/transcript'
import { useApp, type PathAction } from '../../state/app'
import type { ChatState } from '../../state/chat'
import { projectById, selectedProject } from '../../state/selectors'
import { Icon, type IconName } from '../../theme/icons'
import { ToolOutputView } from './ToolOutputView'
import styles from './ToolTimelineView.module.css'

/// Groups that ended within this window start expanded, so a run the user
/// just watched keeps showing its rows once it is committed; older history
/// starts collapsed (`ToolTimelineView.recentWindow`, seconds).
const RECENT_WINDOW_MS = 120_000

/// The row icon. Swift draws the SF Symbol at 12 pt medium; a lucide glyph
/// fills less of its box, so 14 px matches it on screen (running.png).
const ROW_ICON_SIZE = 12

function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(' ')
}

/** Open groups, groups with a failed call and recently finished groups start expanded. */
function startsExpanded(group: ToolGroup): boolean {
  if (toolGroupIsOpen(group) || toolGroupFailedCount(group) > 0) return true
  if (group.endedAt === null) return true
  return Date.now() - group.endedAt.getTime() < RECENT_WINDOW_MS
}

export interface ToolTimelineViewProps {
  group: ToolGroup
  /**
   * False shows the rows without the collapsible "N tools" header (the
   * Inspector's agent view, which shows a slice of the agent's calls).
   */
  showsHeader?: boolean
  /** The chat the group belongs to; path actions resolve against its project (else the selected one). */
  chat?: ChatState
}

/**
 * `ToolTimelineView` of the macOS app: a header "18 tools, edited 5 files,
 * ran 2 commands ⌄" with the elapsed time at the right (ticking while the
 * group is open), then one row per call (24 px rounded-square icon, verb +
 * target label, category chip) joined by a 1 px connector, and a last
 * "Stopped" / "Failed" / "Blocked" row when the group ended that way.
 * Clicking a row toggles its `ToolOutputView` card; rows whose target is a
 * path get a context menu. A group of one call is just that call's row.
 */
export function ToolTimelineView({ group, showsHeader = true, chat }: ToolTimelineViewProps) {
  const [isExpanded, setExpanded] = useState(() => startsExpanded(group))
  const [expandedCalls, setExpandedCalls] = useState<ReadonlySet<string>>(() => new Set())

  const isSingleCall = group.calls.length === 1
  const headline = toolGroupHeadline(group)
  // The ending row, only when the group ended other than normally; a
  // finished group shows no "Done" row.
  const shownTerminal: ToolGroupTerminal | null = group.terminal !== null && group.terminal !== 'done' ? group.terminal : null

  const toggle = useCallback((callId: string) => {
    setExpandedCalls((current) => {
      const next = new Set(current)
      if (next.has(callId)) next.delete(callId)
      else next.add(callId)
      return next
    })
  }, [])

  const projectId = chat?.projectId ?? null
  const reveal = useCallback(
    (path: string, action: PathAction) => {
      const state = useApp.getState()
      const project = (projectId !== null ? projectById(state, projectId) : null) ?? selectedProject(state)
      if (!project) return
      state.reveal(path, project, action)
    },
    [projectId],
  )

  const rows = (
    <div className={styles.rows}>
      {/* A lone call with no ending row has nothing to connect. */}
      {!(isSingleCall && shownTerminal === null) ? (
        <div className={cx(styles.connector, shownTerminal !== null && styles.connectorToEnd)} aria-hidden="true" />
      ) : null}
      {group.calls.map((call) => {
        const expanded = expandedCalls.has(call.id)
        return (
          <div key={call.id} className={styles.callBlock}>
            <ToolRow call={call} isExpanded={expanded} onToggle={toggle} onReveal={reveal} />
            <Fade show={expanded}>
              <div className={styles.card}>
                <ToolOutputView call={call} />
              </div>
            </Fade>
          </div>
        )
      })}
      {shownTerminal !== null ? <TerminalRow terminal={shownTerminal} /> : null}
    </div>
  )

  return (
    <section className={styles.timeline} aria-label={`Tool timeline, ${headline}`}>
      {isSingleCall || !showsHeader ? (
        rows
      ) : (
        <>
          <button
            type="button"
            className={cx(styles.header, !isExpanded && styles.collapsed)}
            aria-expanded={isExpanded}
            aria-label={`${headline}, ${isExpanded ? 'Expanded' : 'Collapsed'}. Shows or hides the tool calls`}
            onClick={() => setExpanded((value) => !value)}
          >
            <span className={styles.headline}>{headline}</span>
            <Icon name="chevronDown" size={13.5} strokeWidth={1.75} className={styles.chevron} />
            <span className={styles.spacer} />
            <span className={styles.elapsed} aria-label="Elapsed">
              <LiveDuration startedAt={group.startedAt} endedAt={group.endedAt} running={toolGroupIsOpen(group)} />
            </span>
          </button>
          <Fade show={isExpanded} className={styles.underHeader}>
            {rows}
          </Fade>
        </>
      )}
    </section>
  )
}

// MARK: - Call row

interface ToolRowProps {
  call: ToolCallItem
  isExpanded: boolean
  onToggle: (callId: string) => void
  onReveal: (path: string, action: PathAction) => void
}

/// One call. A group re-renders on every streamed event and whenever a
/// sibling toggles; rows whose call did not change keep their output (the
/// Swift row is `Equatable` on `call` and `isExpanded`; calls are replaced,
/// never mutated, so identity is the equality here).
const ToolRow = memo(
  function ToolRow({ call, isExpanded, onToggle, onReveal }: ToolRowProps) {
    const meta = toolInfo(call.name)
    const isRunning = call.status === 'running'
    const isFailed = call.status === 'failed'
    const waitingForApproval = isRunning && call.approvalRequestId !== null
    const label = rowLabel(call)
    // "+12 −3" for writes and edits; a failed call changed nothing.
    const changes = isFailed ? null : lineChanges(call)

    // Second line: "failed · <summary>" for failures, "Waiting for approval" while an approval is pending.
    let detail: { text: string; tint: string } | null = null
    if (isFailed) {
      const summary = abbreviate(call.summary, 160)
      detail = { text: summary.length === 0 ? 'failed' : `failed · ${summary}`, tint: 'var(--danger)' }
    } else if (waitingForApproval) {
      detail = { text: 'Waiting for approval', tint: 'var(--warning)' }
    }

    let status: string
    switch (call.status) {
      case 'running':
        status = waitingForApproval ? 'waiting for approval' : 'running'
        break
      case 'succeeded':
        status = 'succeeded'
        break
      case 'failed':
        status = 'failed'
        break
    }
    const lines = changes ? `, ${changes.added} lines added, ${changes.removed} removed` : ''
    const accessibilityText = `${label}${lines}, ${meta.category}, ${status}`

    const button = (
      <button
        type="button"
        className={cx(styles.row, isExpanded && styles.expanded)}
        aria-expanded={isExpanded}
        aria-label={accessibilityText}
        onClick={() => onToggle(call.id)}
      >
        <span className={cx(styles.iconSlot, isFailed && styles.iconSlotFailed)} aria-hidden="true">
          {isRunning ? (
            <BrailleSpinner size={11} tint={waitingForApproval ? 'var(--warning)' : 'var(--text-secondary)'} />
          ) : (
            <Icon name={meta.icon} size={ROW_ICON_SIZE} strokeWidth={2} />
          )}
        </span>
        <span className={styles.labels}>
          <MiddleTruncate text={label} className={styles.label} />
          {detail ? (
            <span className={styles.detail} style={{ color: detail.tint }}>
              {detail.text}
            </span>
          ) : null}
        </span>
        <span className={styles.rowGap} />
        {!isRunning && call.durationMs !== null ? <span className={styles.duration}>{TranscriptFormat.durationMs(call.durationMs)}</span> : null}
        {changes ? <LineChangeCounts added={changes.added} removed={changes.removed} /> : null}
        <Chip>{meta.category}</Chip>
      </button>
    )

    const path = pathArgument(call)
    const withTooltip = <Tooltip content={call.summary.length === 0 ? label : call.summary}>{button}</Tooltip>
    if (path === null) return withTooltip
    return <ContextMenu items={() => pathMenuItems((action) => onReveal(path, action))}>{withTooltip}</ContextMenu>
  },
  (previous, next) => previous.call === next.call && previous.isExpanded === next.isExpanded,
)

// MARK: - Line change counts

/** "+12 −3" in the diff colours; a zero side is left out ("+40" for a new file). */
function LineChangeCounts({ added, removed }: { added: number; removed: number }) {
  return (
    <span className={styles.counts} aria-hidden="true">
      {added > 0 || removed === 0 ? <span className={styles.added}>+{added}</span> : null}
      {removed > 0 ? <span className={styles.removed}>−{removed}</span> : null}
    </span>
  )
}

// MARK: - Terminal row

const TERMINALS: Record<ToolGroupTerminal, { label: string; icon: IconName; tint: string }> = {
  done: { label: 'Done', icon: 'check', tint: 'var(--text-secondary)' },
  stopped: { label: 'Stopped', icon: 'stopped', tint: 'var(--text-secondary)' },
  failed: { label: 'Failed', icon: 'failed', tint: 'var(--danger)' },
  blocked: { label: 'Blocked', icon: 'approval', tint: 'var(--warning)' },
}

function TerminalRow({ terminal }: { terminal: ToolGroupTerminal }) {
  const { label, icon, tint } = TERMINALS[terminal]
  return (
    <div className={styles.terminalRow} role="note" aria-label={`Tools ${label.toLowerCase()}`}>
      <span className={styles.terminalIcon} style={{ color: tint }} aria-hidden="true">
        <Icon name={icon} size={15} strokeWidth={1.5} />
      </span>
      <span className={styles.terminalLabel}>{label}</span>
    </div>
  )
}

// MARK: - Shared pieces

/// The path context menu of timeline rows and modified-file chips
/// (`timelinePathContextMenu` / `FileChip.contextMenu`): Show in Changes,
/// Open in Editor, Reveal in Finder (Show in Explorer on Windows), Open with
/// Default Editor, then Copy Path.
export function pathMenuItems(perform: (action: PathAction) => void): MenuItem[] {
  return [
    { label: 'Show in Changes', onSelect: () => perform('showInChanges') },
    { label: 'Open in Editor', onSelect: () => perform('openInEditor') },
    { label: revealLabel, onSelect: () => perform('revealInFileManager') },
    { label: 'Open with Default Editor', onSelect: () => perform('openWithEditor') },
    { type: 'separator' },
    { label: 'Copy Path', onSelect: () => perform('copyPath') },
  ]
}

/** Re-renders every `intervalMs` while `active` (`TimelineView(.periodic(by: 1))`); returns the time. */
function useNow(active: boolean, intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs)
    return () => window.clearInterval(timer)
  }, [active, intervalMs])
  return now
}

export interface LiveDurationProps {
  startedAt: Date
  endedAt: Date | null
  /** Ticks once a second while true (the open group's / running agent's clock). */
  running: boolean
}

/**
 * The elapsed-time text of a timeline header or agent row:
 * `TranscriptFormat.duration` of the running clock (once a second), or of
 * the final duration ("<1s" for groups that finished in milliseconds).
 * Only this text re-renders on each tick.
 */
export function LiveDuration({ startedAt, endedAt, running }: LiveDurationProps) {
  const now = useNow(running)
  const end = running ? now : (endedAt?.getTime() ?? now)
  return <>{TranscriptFormat.duration(Math.max(0, (end - startedAt.getTime()) / 1000))}</>
}

// `Fade` lives in components/Fade (DisclosureRow uses it too); re-exported
// for the views that import it from here.
export { Fade, type FadeProps } from '../../components/Fade'
