// Port of apps/macos/ContextAgent/Views/Chat/OutcomeCardView.swift.

import { useMemo, useState, type MouseEvent, type ReactNode } from 'react'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { isModKey, modLabel } from '../../components/platform'
import { StatusPill } from '../../components/StatusPill'
import { Tooltip } from '../../components/Tooltip'
import { taskStatusLabel, tokenUsageTotal } from '../../backend/types'
import type { FileReferenceContext } from '../../markdown/fileReferences'
import { MarkdownView } from '../../markdown/MarkdownView'
import { TranscriptFormat, type OutcomeItem } from '../../models/transcript'
import { useApp, type PathAction } from '../../state/app'
import type { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { projectById, selectedProject } from '../../state/selectors'
import { Icon, type IconName } from '../../theme/icons'
import { pathMenuItems } from './ToolTimelineView'
import { useFileReferences as useChatFileReferences } from './TranscriptItemViews'
import styles from './OutcomeCardView.module.css'

export interface OutcomeCardViewProps {
  item: OutcomeItem
  chat: ChatState
}

/**
 * `OutcomeCardView` of the macOS app: the task outcome at the end of a
 * run: status pill, markdown summary (left out when it repeats the reply
 * just above), "Modified files" chips (click → Editor tab; mod-click opens
 * the default editor; path context menu), "Unresolved" bullets in
 * `warning`, and a footer "42.1k tokens · 6m 12s" whose hover shows the
 * absolute finish time. No box: it reads as part of the transcript.
 */
export function OutcomeCardView({ item, chat }: OutcomeCardViewProps) {
  const outcome = item.outcome
  const summary = outcome.summary.trim()
  // The stable project object only (not its version): a committed card must
  // not re-render on every event of the chats in its project.
  const project = useApp((state) => projectById(state, chat.projectId))
  const canRunCommands = useApp((state) => state.selectedProjectId !== null)
  // ChatView's context (its folder listings resolve names the Files tab never
  // listed); a card rendered outside a ChatView builds its own.
  const provided = useChatFileReferences()
  const own = useFileReferences(provided ? null : project)
  const files = provided ?? own

  const reveal = (path: string, action: PathAction) => {
    const state = useApp.getState()
    const target = projectById(state, chat.projectId) ?? selectedProject(state)
    if (!target) return
    state.reveal(path, target, action)
  }

  return (
    <div className={styles.outcome} role="group" aria-label={`Task ${taskStatusLabel(outcome.status).toLowerCase()}`}>
      <div className={styles.pillRow}>
        <StatusPill status={outcome.status} />
      </div>

      {summary.length > 0 && !item.summaryRepeatsReply ? (
        <MarkdownView markdown={summary} files={files} onRunCommand={canRunCommands ? runInTerminal : undefined} />
      ) : null}

      {outcome.modified_files.length > 0 ? (
        <Section title="Modified files" count={outcome.modified_files.length}>
          <div className={styles.flow}>
            {outcome.modified_files.map((path) => (
              <FileChip key={path} path={path} perform={(action) => reveal(path, action)} />
            ))}
          </div>
        </Section>
      ) : null}

      {outcome.unresolved.length > 0 ? (
        <Section title="Unresolved" count={outcome.unresolved.length} tint="var(--warning)" icon="warning">
          <ul className={styles.bullets}>
            {outcome.unresolved.map((line, index) => (
              <li key={index} className={styles.bullet}>
                <span aria-hidden="true">•</span>
                <span className="selectable">{line}</span>
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      <Footer item={item} />
    </div>
  )
}

function runInTerminal(command: string): void {
  useApp.getState().runInTerminal(command)
}

/// The file names in the summary resolve against the chat's project (and
/// its changed files) and open through `reveal`, as ChatView's
/// `fileReferences` environment does for the transcript in Swift.
function useFileReferences(project: ProjectState | null): FileReferenceContext | null {
  return useMemo(() => {
    if (!project) return null
    return {
      root: project.root,
      changedPaths: () => project.gitStatus?.files.map((file) => file.path) ?? [],
      perform: (reference, action) => useApp.getState().reveal(reference.path, project, action, reference.line ?? null),
    }
  }, [project])
}

// MARK: - Sections

function Section({ title, count, tint = 'var(--text-tertiary)', icon, children }: { title: string; count: number | null; tint?: string; icon?: IconName; children: ReactNode }) {
  return (
    <div className={styles.section}>
      <div className={styles.sectionTitle} style={{ color: tint }}>
        {icon ? <Icon name={icon} size={11} strokeWidth={2.25} /> : null}
        <span>{count === null ? title : `${title} · ${count}`}</span>
      </div>
      {children}
    </div>
  )
}

// MARK: - Footer

function Footer({ item }: { item: OutcomeItem }) {
  const [hovering, setHovering] = useState(false)
  const outcome = item.outcome
  // "42.1k tokens · 6m 12s"
  const footerText = [`${TranscriptFormat.tokens(tokenUsageTotal(outcome.usage))} tokens`, TranscriptFormat.durationMs(outcome.duration_ms)].join(' · ')
  const finished = `Finished ${absoluteDate(item.date)}`
  return (
    <Tooltip content={finished}>
      <div
        className={styles.footer}
        aria-label={`${footerText}. ${finished}`}
        onPointerEnter={() => setHovering(true)}
        onPointerLeave={() => setHovering(false)}
      >
        <span className={styles.footerText}>{footerText}</span>
        <span className={styles.footerSpacer} />
        {hovering ? <span className={styles.finished}>{finished}</span> : null}
      </div>
    </Tooltip>
  )
}

const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
const TIME_FORMAT = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' })

/** `date.formatted(date: .abbreviated, time: .shortened)`: "Oct 25, 2026 at 3:42 PM". */
function absoluteDate(date: Date): string {
  return `${DATE_FORMAT.format(date)} at ${TIME_FORMAT.format(date)}`
}

// MARK: - File chip

/**
 * A modified-file chip: click opens the file in the Editor tab, mod-click
 * opens it with the default editor, and the context menu offers those plus
 * Show in Changes, Reveal in Finder (Show in Explorer) and Copy Path.
 */
function FileChip({ path, perform }: { path: string; perform: (action: PathAction) => void }) {
  return (
    <ContextMenu items={() => pathMenuItems(perform)}>
      <Tooltip content={`${path} — click to open in the Editor, ${modLabel}-click to open with the default editor`}>
        <button
          type="button"
          className={styles.fileChip}
          aria-label={`Modified file ${path}. Opens the file in the Editor tab`}
          onClick={(event: MouseEvent<HTMLButtonElement>) => perform(isModKey(event) ? 'openWithEditor' : 'openInEditor')}
        >
          <Chip icon="file">{displayName(path)}</Chip>
        </button>
      </Tooltip>
    </ContextMenu>
  )
}

/** Long paths keep their tail: "…/agent/src/runtime.rs" (`FileChip.displayName`). */
function displayName(path: string, limit = 48): string {
  if (Array.from(path).length <= limit) return path
  const parts = path.split('/').filter((part) => part.length > 0)
  const tail: string[] = []
  let length = 0
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const part = parts[index] ?? ''
    const next = length + Array.from(part).length + (tail.length === 0 ? 0 : 1)
    if (next > limit - 2 && tail.length > 0) break
    tail.unshift(part)
    length = next
  }
  const joined = tail.join('/')
  const chars = Array.from(joined)
  if (chars.length > limit) return '…' + chars.slice(chars.length - (limit - 1)).join('')
  return tail.length === parts.length ? joined : '…/' + joined
}
