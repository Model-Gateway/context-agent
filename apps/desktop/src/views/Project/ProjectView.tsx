// Port of apps/macos/ContextAgent/Views/Project/ProjectView.swift: the
// Project tab, a two-column grid of cards for the selected project — Index
// (stats, progress, Rebuild), Git, Validation commands, Languages, Doctor
// (Run / Run with probe), Memories and Recent tasks — under a header with the
// project's name, root and chips.

import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactElement, type ReactNode } from 'react'
import {
  executionStateLabel,
  parseRFC3339,
  validationCommandId,
  type DoctorReport,
  type LanguageCount,
  type MemoryEntry,
  type TaskSummary,
  type ValidationCommand,
} from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { CopyButton } from '../../components/CopyButton'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { MiddleTruncate, middleTruncated } from '../../components/MiddleTruncate'
import { prefersReducedMotion } from '../../components/overlays'
import { revealLabel } from '../../components/platform'
import { StatusPill } from '../../components/StatusPill'
import { Tooltip } from '../../components/Tooltip'
import { useModelsDevVersion } from '../../models/modelsDev'
import { TranscriptFormat } from '../../models/transcript'
import { approvalModeLabel, useApp, useSelectedProject } from '../../state/app'
import type { ProjectState } from '../../state/project'
import { Icon, type IconName } from '../../theme/icons'
import styles from './ProjectView.module.css'

/** `Int.formatted()`: grouped digits in the user's locale ("4,099"). */
const numberFormat = new Intl.NumberFormat()

function formatted(value: number): string {
  return numberFormat.format(value)
}

/** `language.prefix(1).uppercased() + language.dropFirst()`. */
function capitalized(text: string): string {
  const chars = Array.from(text)
  if (chars.length === 0) return text
  return (chars[0] ?? '').toUpperCase() + chars.slice(1).join('')
}

/**
 * `ProjectView` (Views/Project/ProjectView.swift): the Project tab. Shows the
 * selected project's cards, or "No project open" with Open a project… when
 * none is selected.
 */
export function ProjectView() {
  const project = useSelectedProject()
  return (
    <div className={styles.root}>
      {project ? (
        <ProjectContent project={project} />
      ) : (
        <EmptyPlaceholder
          icon="project"
          title="No project open"
          subtitle="Open a repository to see its index, git state, validation commands and memories."
          actionTitle="Open a project…"
          onAction={() => void useApp.getState().chooseProject()}
        />
      )}
    </div>
  )
}

// MARK: - Content

/// Two columns when they fit (the cards' container is at least 340 × 2 + 16
/// px wide, a container query in the stylesheet), one otherwise. A plain
/// grid: every card is laid out, so the scroll height never changes while
/// scrolling.
function ProjectContent({ project }: { project: ProjectState }) {
  // `.task(id: project.id)`: a fresh `project/status`, then the recent tasks
  // and memories when neither is loaded yet.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      await project.refreshStatus()
      if (cancelled) return
      if (project.tasks.length === 0 && project.memories.length === 0 && !project.extrasLoading) {
        await useApp.getState().loadProjectExtras(project)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [project])

  return (
    <div className={styles.scroll}>
      <div className={styles.content}>
        <ProjectHeader project={project} />
        <div className={styles.cardsContainer}>
          <div className={styles.cards}>
            <IndexCard project={project} />
            <GitCard project={project} />
            <ValidationCard project={project} />
            <LanguagesCard project={project} />
            <DoctorCard project={project} />
            <MemoriesCard project={project} />
            <RecentTasksCard project={project} />
          </div>
        </div>
      </div>
    </div>
  )
}

/** The path row's ideal width: 200 px of path, then the copy (28) and reveal (25) buttons, 4 apart. */
const PATH_IDEAL_WIDTH = 200 + 4 + 28 + 4 + 25
/** Title block → Spacer(minLength: 8) → chips, with the HStack's 16 between each. */
const HEADER_MIN_GAP = 16 + 8 + 16

interface HeaderLayout {
  /** Chips beside the title (true) or under it. */
  horizontal: boolean
  /** Under the title: 0 every chip, 1 without the command-wrapper chip, 2 without the approval chip too. */
  variant: 0 | 1 | 2
}

/// `ProjectHeader`: chips beside the title when the name and ~200 px of path
/// fit next to them; otherwise under it (narrow pane), losing the optional
/// chips if even that row is too wide (`ViewThatFits`, measured here).
function ProjectHeader({ project }: { project: ProjectState }) {
  // The model's display name follows models.dev and the configured endpoint.
  useModelsDevVersion()
  useApp((state) => state.config?.model.base_url ?? '')
  const rootRef = useRef<HTMLDivElement>(null)
  const measureRef = useRef<HTMLDivElement>(null)
  const [layout, setLayout] = useState<HeaderLayout>({ horizontal: true, variant: 0 })

  const name = project.name
  const root = project.root
  const model = useApp.getState().modelDisplayName(project.info.model)
  const approval = approvalModeLabel(project.info.permission_profile)
  const sandbox = project.info.sandbox
  const running = project.isRunning

  useLayoutEffect(() => {
    const element = rootRef.current
    const measure = measureRef.current
    if (!element || !measure) return
    const compute = () => {
      const width = element.clientWidth
      const widthOf = (key: string) => measure.querySelector<HTMLElement>(`[data-measure="${key}"]`)?.getBoundingClientRect().width ?? 0
      const title = Math.max(widthOf('name'), PATH_IDEAL_WIDTH)
      const chips = [widthOf('chips0'), widthOf('chips1'), widthOf('chips2')]
      let next: HeaderLayout
      if (title + HEADER_MIN_GAP + (chips[0] ?? 0) <= width) {
        next = { horizontal: true, variant: 0 }
      } else {
        const fit = chips.findIndex((chipsWidth) => chipsWidth <= width)
        next = { horizontal: false, variant: (fit < 0 ? 2 : fit) as 0 | 1 | 2 }
      }
      setLayout((current) => (current.horizontal === next.horizontal && current.variant === next.variant ? current : next))
    }
    compute()
    const observer = new ResizeObserver(compute)
    observer.observe(element)
    return () => observer.disconnect()
  }, [name, model, approval, sandbox, running])

  const titleBlock = (
    <div className={styles.titleBlock}>
      <div className={styles.title}>{name}</div>
      <PathRow text={root}>
        <CopyButton text={root} label="Copy path" />
        <IconButton icon="folder" label={revealLabel} size={13} onClick={() => useApp.getState().reveal(project.root, project, 'revealInFileManager')} />
      </PathRow>
    </div>
  )

  const chips = (variant: 0 | 1 | 2, measuring = false) => (
    <HeaderChips model={model} approval={variant < 2 ? approval : null} sandbox={variant === 0 && sandbox !== 'none' ? sandbox : null} running={running} measuring={measuring} />
  )

  return (
    <div ref={rootRef} className={styles.headerHost}>
      <div ref={measureRef} className={styles.measure} aria-hidden="true">
        <div data-measure="name" className={styles.title}>
          {name}
        </div>
        <div data-measure="chips0">{chips(0, true)}</div>
        <div data-measure="chips1">{chips(1, true)}</div>
        <div data-measure="chips2">{chips(2, true)}</div>
      </div>
      {layout.horizontal ? (
        <div className={styles.headerRow}>
          {titleBlock}
          {chips(0)}
        </div>
      ) : (
        <div className={styles.headerColumn}>
          {titleBlock}
          {chips(layout.variant)}
        </div>
      )}
    </div>
  )
}

/** `chips(sandbox:approval:)`: model, approval mode, command wrapper, Running / Idle. */
function HeaderChips({ model, approval, sandbox, running, measuring }: { model: string; approval: string | null; sandbox: string | null; running: boolean; measuring: boolean }) {
  const chip = (help: string, node: ReactElement) => (measuring ? node : <Help text={help}>{node}</Help>)
  return (
    <div className={styles.chips}>
      {chip('Project model', <Chip icon="model">{model}</Chip>)}
      {approval !== null ? chip('Approval mode', <Chip icon="approval">{approval}</Chip>) : null}
      {sandbox !== null ? chip('Developer-mode command wrapper from [permissions.sandbox]', <Chip>{`command wrapper: ${sandbox}`}</Chip>) : null}
      <StatusPill running={running} />
    </div>
  )
}

// MARK: - Card chrome

/// `ProjectCard`: a `.card()` with a header row (14 px icon, heading title,
/// trailing controls) and its content, 12 px apart.
function ProjectCard({ title, icon, trailing, children }: { title: string; icon: IconName; trailing?: ReactNode; children?: ReactNode }) {
  return (
    <section className={styles.card} role="group" aria-label={title}>
      <div className={styles.cardHeader}>
        <Icon name={icon} size={14} strokeWidth={2} className={styles.cardIcon} />
        <h2 className={styles.cardTitle}>{title}</h2>
        <span className={styles.cardSpacer} />
        {trailing}
      </div>
      {children}
    </section>
  )
}

/** `EmptyCardText`. */
function EmptyCardText({ text }: { text: string }) {
  return <div className={styles.emptyText}>{text}</div>
}

/** `StatRow`: a label at the left, its value (selectable, truncated in the middle) at the right. */
function StatRow({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.statRow}>
      <span className={styles.statLabel}>{label}</span>
      {/* Fills the row after the 8 px gap (the Spacer) and right-aligns the value, so the fitting width comes from the row. */}
      <MiddleTruncate text={value} className={`${styles.statValue} selectable`} />
    </div>
  )
}

/** "Loading…" with a spinner (Memories and Recent tasks while the extras load). */
function LoadingRow() {
  return (
    <div className={styles.loadingRow}>
      <BrailleSpinner />
      <span className={styles.secondaryText}>Loading…</span>
    </div>
  )
}

/** "and N more" under a capped list. */
function MoreText({ count, indent = false }: { count: number; indent?: boolean }) {
  return <div className={[styles.moreText, indent ? styles.moreIndent : ''].filter(Boolean).join(' ')}>{`and ${count} more`}</div>
}

// MARK: - Index

/// `IndexCard`: the status line (spinner, failure or ready), a progress bar
/// before the first `index/ready`, the six stats, the last full index and
/// the last run; Rebuild in the header.
function IndexCard({ project }: { project: ProjectState }) {
  const index = project.info.index
  const progress = project.indexProgress
  const ready = project.lastIndexReady
  const isIndexing = progress !== null || (project.info.indexing && ready === null)
  const rebuildDisabled = project.isRunning || isIndexing
  const rebuildHelp = project.isRunning ? 'Finishes after the running task' : isIndexing ? 'Indexing is already in progress' : 'Re-index the repository from scratch'
  const lastFull = index.last_full_index_at != null ? parseRFC3339(index.last_full_index_at) : null
  const error = ready?.error ?? null

  let status: ReactNode
  if (isIndexing) {
    status = (
      <div className={styles.statusRow}>
        <BrailleSpinner />
        <span className={styles.secondaryText}>{project.indexSummary}</span>
      </div>
    )
  } else if (error !== null) {
    status = (
      <div className={`${styles.warningRow} ${styles.danger}`}>
        <Icon name="warning" size={12} strokeWidth={2} className={styles.inlineIcon} />
        <span className={styles.wrapText}>{`Indexing failed: ${error} · retrieval falls back to text tools`}</span>
      </div>
    )
  } else {
    status = (
      <div className={styles.readyRow}>
        <Icon name="check" size={12} strokeWidth={2} className={styles.successIcon} />
        <span className={styles.secondaryText}>{project.indexSummary}</span>
      </div>
    )
  }

  return (
    <ProjectCard
      title="Index"
      icon="context"
      trailing={
        <Help text={rebuildHelp}>
          <Button size="small" disabled={rebuildDisabled} onClick={() => void useApp.getState().rebuildIndex()}>
            Rebuild
          </Button>
        </Help>
      }
    >
      {status}
      {progress !== null && progress.total > 0 ? <ProgressBar fraction={progress.done / progress.total} /> : null}
      <div className={styles.statGrid}>
        <StatRow label="Files" value={formatted(index.files)} />
        <StatRow label="Symbols" value={formatted(index.symbols)} />
        <StatRow label="Chunks" value={formatted(index.chunks)} />
        <StatRow label="Relationships" value={formatted(index.relationships)} />
        <StatRow label="Embeddings" value={formatted(index.embeddings)} />
        <StatRow label="Generation" value={String(index.generation)} />
      </div>
      {lastFull !== null ? <div className={styles.captionTertiary}>{`Last full index ${TranscriptFormat.relative(lastFull)}`}</div> : null}
      {ready !== null && ready.error == null ? (
        <div className={styles.captionTertiary}>
          {`Last run: ${ready.files_indexed} indexed · ${ready.files_removed} removed · ${ready.files_unchanged} unchanged · ${TranscriptFormat.durationMs(ready.duration_ms)}`}
        </div>
      ) : null}
    </ProjectCard>
  )
}

/** `ProgressBar`: a thin determinate bar (the app never shows a native progress control). */
function ProgressBar({ fraction }: { fraction: number }) {
  const clamped = Math.max(0, Math.min(1, fraction))
  const percent = Math.round(clamped * 100)
  return (
    <div className={styles.progressTrack} role="progressbar" aria-label="Indexing progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} aria-valuetext={`${percent} percent`}>
      <div className={styles.progressFill} style={{ width: `${clamped * 100}%` }} />
    </div>
  )
}

// MARK: - Git

/** `GitCard`: branch chip (or "detached"), head, changed files and git's error; Open Changes in the header. */
function GitCard({ project }: { project: ProjectState }) {
  const isGit = project.isGitRepository
  const branch = project.branch
  const full = project.gitStatus?.head ?? project.info.git?.head ?? null
  const head = full ? Array.from(full).slice(0, 8).join('') : '—'
  return (
    <ProjectCard
      title="Git"
      icon="branch"
      trailing={
        isGit ? (
          <Button size="small" onClick={() => useApp.getState().setSidebarItem('changes')}>
            Open Changes
          </Button>
        ) : null
      }
    >
      {isGit ? (
        <>
          <div className={styles.statRow}>
            <span className={styles.statLabel}>Branch</span>
            <span className={styles.flexSpacer} />
            {branch ? <Chip icon="branch">{branch}</Chip> : <span className={styles.detached}>detached</span>}
          </div>
          <StatRow label="Head" value={head} />
          <StatRow label="Changed files" value={String(project.dirtyCount)} />
          {project.gitError !== null ? <div className={styles.gitError}>{project.gitError}</div> : null}
        </>
      ) : (
        <EmptyCardText text="Not a git repository." />
      )}
    </ProjectCard>
  )
}

// MARK: - Validation commands

/** How many commands show before "See all N". */
const VALIDATION_SHOWN = 5

/** `ValidationCard`: the first five commands, then "See all N" (expands in place) / "Show fewer". */
function ValidationCard({ project }: { project: ProjectState }) {
  const [showAll, setShowAll] = useState(false)
  const commands = project.info.validation_commands
  const limited = commands.length > VALIDATION_SHOWN
  const shown = showAll || !limited ? commands : commands.slice(0, VALIDATION_SHOWN)
  return (
    <ProjectCard title="Validation commands" icon="validationPass">
      {commands.length === 0 ? (
        <EmptyCardText text="None detected — add [validation] commands to .context-agent.toml" />
      ) : (
        <div className={styles.validationList}>
          {shown.map((command, index) => (
            <ValidationRow key={`${validationCommandId(command)}#${index}`} command={command} />
          ))}
          {limited ? (
            <button type="button" className={styles.linkButton} onClick={() => setShowAll((value) => !value)}>
              {showAll ? 'Show fewer' : `See all ${commands.length}`}
            </button>
          ) : null}
        </div>
      )}
    </ProjectCard>
  )
}

/** "in crates/cli · from crates/cli/Cargo.toml", or "repository root". */
function validationDetail(command: ValidationCommand): string {
  const parts: string[] = []
  if (command.cwd !== '.' && command.cwd.length > 0) parts.push(`in ${command.cwd}`)
  if (command.source.length > 0) parts.push(`from ${command.source}`)
  return parts.length === 0 ? 'repository root' : parts.join(' · ')
}

const ValidationRow = memo(function ValidationRow({ command }: { command: ValidationCommand }) {
  return (
    <div className={styles.validationRow}>
      <div className={styles.validationLine}>
        <span className={`${styles.command} selectable`}>{command.command}</span>
        <span className={styles.flexSpacer} />
        <Chip>{command.kind}</Chip>
      </div>
      <div className={styles.validationDetail}>{validationDetail(command)}</div>
    </div>
  )
})

// MARK: - Languages

/** How many languages get a bar. */
const LANGUAGES_SHOWN = 8

/** `LanguagesCard`: one bar per language (most files first), eight of them, then "and N more". */
function LanguagesCard({ project }: { project: ProjectState }) {
  const info = project.info
  const source = info.languages.length === 0 ? info.index.languages : info.languages
  const languages = source
    .filter((language) => language.files > 0)
    .sort((a, b) => (a.files !== b.files ? b.files - a.files : a.language < b.language ? -1 : a.language > b.language ? 1 : 0))
  const maximum = Math.max(1, languages[0]?.files ?? 1)
  return (
    <ProjectCard title="Languages" icon="chart">
      {languages.length === 0 ? (
        <EmptyCardText text="No files indexed yet." />
      ) : (
        <div className={styles.languageList}>
          {languages.slice(0, LANGUAGES_SHOWN).map((language) => (
            <LanguageRow key={language.language} language={language} maximum={maximum} />
          ))}
          {languages.length > LANGUAGES_SHOWN ? <MoreText count={languages.length - LANGUAGES_SHOWN} /> : null}
        </div>
      )}
    </ProjectCard>
  )
}

const LanguageRow = memo(function LanguageRow({ language, maximum }: { language: LanguageCount; maximum: number }) {
  const label = capitalized(language.language)
  const percent = (language.files / maximum) * 100
  return (
    <div className={styles.languageRow} role="group" aria-label={`${label}: ${language.files} files`}>
      <span className={styles.languageLabel} aria-hidden="true">
        {label}
      </span>
      <span className={styles.languageTrack} aria-hidden="true">
        <span className={styles.languageBar} style={{ width: `max(3px, ${percent}%)` }} />
      </span>
      <span className={styles.languageCount} aria-hidden="true">
        {formatted(language.files)}
      </span>
    </div>
  )
})

// MARK: - Doctor

/// `DoctorCard`: Run / Run with probe (spinner while running), the error,
/// then the report, the "Checking…" line or the explanation.
function DoctorCard({ project }: { project: ProjectState }) {
  const disabled = project.doctorRunning || project.isRunning
  const help = (probe: boolean): string => {
    if (project.isRunning) return 'Finishes after the running task'
    if (project.doctorRunning) return 'The doctor is already running'
    return probe ? 'Also probe the model endpoint (sends real requests, up to 2 min)' : 'Check the project without contacting the model'
  }
  return (
    <ProjectCard
      title="Doctor"
      icon="doctor"
      trailing={
        <div className={styles.doctorButtons}>
          {project.doctorRunning ? <BrailleSpinner /> : null}
          <Help text={help(false)}>
            <Button size="small" disabled={disabled} onClick={() => void useApp.getState().runDoctor(false)}>
              Run
            </Button>
          </Help>
          <Help text={help(true)}>
            <Button size="small" disabled={disabled} onClick={() => void useApp.getState().runDoctor(true)}>
              Run with probe
            </Button>
          </Help>
        </div>
      }
    >
      {project.doctorError !== null ? (
        <div className={`${styles.warningRow} ${styles.danger}`}>
          <Icon name="warning" size={12} strokeWidth={2} className={styles.inlineIcon} />
          <span className={`${styles.wrapText} selectable`}>{project.doctorError}</span>
        </div>
      ) : null}
      {project.doctor !== null ? (
        <DoctorReportView report={project.doctor} />
      ) : project.doctorRunning ? (
        <div className={`${styles.secondaryText} ${styles.wrapText}`}>
          Checking configuration, repository, validation commands, language servers and the model provider…
        </div>
      ) : project.doctorError === null ? (
        <EmptyCardText text="Run the doctor to check the configuration, repository, validation commands, language servers and the model provider. “Run with probe” sends a test request to the model endpoint." />
      ) : null}
    </ProjectCard>
  )
}

/** `DoctorReportView`: the verdict, then sections of label / value rows with ✓ / ✗ marks. */
const DoctorReportView = memo(function DoctorReportView({ report }: { report: DoctorReport }) {
  const clean = report.problems === 0
  return (
    <div className={styles.report}>
      <div className={`${styles.verdict} ${clean ? styles.success : styles.danger}`}>
        <span className={styles.verdictMark}>{clean ? '✓' : '✗'}</span>
        <span className={styles.secondaryFont}>{clean ? 'No problems found' : `${report.problems} problem${report.problems === 1 ? '' : 's'} found`}</span>
      </div>
      {report.sections.map((section, sectionIndex) => (
        <div key={`${section.title}#${sectionIndex}`} className={styles.reportSection}>
          <div className={styles.reportSectionTitle}>{capitalized(section.title)}</div>
          {section.rows.map((row, rowIndex) => (
            <div key={rowIndex} className={styles.reportRow} role="group" aria-label={`${row.ok ? 'OK' : 'Problem'}: ${row.label} ${row.value}`}>
              <span className={`${styles.reportMark} ${row.ok ? styles.success : styles.danger}`} aria-hidden="true">
                {row.ok ? '✓' : '✗'}
              </span>
              <span className={styles.reportLabel}>{row.label}</span>
              <MiddleTruncate lines={3} text={row.value} className={`${styles.reportValue} ${row.ok ? '' : styles.danger} selectable`} />
            </div>
          ))}
        </div>
      ))}
    </div>
  )
})

// MARK: - Memories

/** How many memories are listed. */
const MEMORIES_SHOWN = 10

/** `MemoriesCard`: the count in the header, ten memories, then "and N more". */
function MemoriesCard({ project }: { project: ProjectState }) {
  const memories = project.memories
  return (
    <ProjectCard title="Memories" icon="memory" trailing={memories.length > 0 ? <Chip>{String(memories.length)}</Chip> : null}>
      {memories.length === 0 ? (
        project.extrasLoading ? (
          <LoadingRow />
        ) : (
          <EmptyCardText text="Nothing remembered yet." />
        )
      ) : (
        <div className={styles.memoryList}>
          {memories.slice(0, MEMORIES_SHOWN).map((memory) => (
            <MemoryRow key={memory.id} memory={memory} />
          ))}
          {memories.length > MEMORIES_SHOWN ? <MoreText count={memories.length - MEMORIES_SHOWN} /> : null}
        </div>
      )}
    </ProjectCard>
  )
}

/** How long expanding or collapsing a memory takes (`.easeInOut(duration: 0.18)`). */
const MEMORY_ANIMATION_MS = 180

/// `MemoryRow`: collapsed to three lines; clicking a truncated row shows all
/// of it. Only the expanded text is selectable.
const MemoryRow = memo(function MemoryRow({ memory }: { memory: MemoryEntry }) {
  const [expanded, setExpanded] = useState(false)
  const [clipped, setClipped] = useState(false)
  const contentRef = useRef<HTMLDivElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)
  const fromHeight = useRef<number | null>(null)
  /** While the height animates, the clamp is lifted and must not be re-measured. */
  const animating = useRef(false)

  /** Measured while collapsed; an expanded row can always collapse again. */
  const expandable = expanded || clipped

  // Whether the three-line clamp hides anything, re-measured as the width changes.
  useLayoutEffect(() => {
    if (expanded) return
    const element = contentRef.current
    if (!element) return
    const measure = () => {
      if (animating.current) return
      setClipped(element.scrollHeight > element.clientHeight + 1)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [expanded, memory.content])

  const toggle = useCallback(() => {
    const body = bodyRef.current
    if (body) fromHeight.current = body.getBoundingClientRect().height
    setExpanded((value) => !value)
  }, [])

  // `withAnimation(.easeInOut(duration: 0.18))`: the row's height follows
  // the text. Collapsing keeps the whole text laid out (clipped by the
  // shrinking row) until the end, as SwiftUI does, instead of clamping it
  // to three lines first and then shrinking the empty space.
  useLayoutEffect(() => {
    const body = bodyRef.current
    const content = contentRef.current
    const from = fromHeight.current
    fromHeight.current = null
    if (!body || !content || from === null || prefersReducedMotion()) return
    const to = body.getBoundingClientRect().height
    if (Math.abs(to - from) < 1) return
    const collapsing = !expanded
    animating.current = true
    if (collapsing) {
      content.style.display = 'block'
      content.style.overflow = 'visible'
    }
    body.style.height = `${from}px`
    body.style.overflow = 'hidden'
    void body.offsetHeight
    body.style.transition = `height ${MEMORY_ANIMATION_MS}ms ease-in-out`
    body.style.height = `${to}px`
    const finish = () => {
      body.style.height = ''
      body.style.overflow = ''
      body.style.transition = ''
      content.style.display = ''
      content.style.overflow = ''
      animating.current = false
    }
    const timer = window.setTimeout(finish, MEMORY_ANIMATION_MS + 20)
    return () => {
      window.clearTimeout(timer)
      finish()
    }
  }, [expanded])

  const created = parseRFC3339(memory.created_at)
  const help = expandable ? (expanded ? 'Show less' : 'Show the whole memory') : ''
  const contentClass = [styles.memoryContent, memory.invalidated ? styles.memoryInvalidated : '', expanded ? 'selectable' : styles.memoryClamped, !expanded && expandable ? styles.memoryClickable : '']
    .filter(Boolean)
    .join(' ')

  return (
    <div className={styles.memoryRow} role="group">
      <Help text={help}>
        <button type="button" className={styles.memoryHeader} disabled={!expandable} onClick={toggle} aria-expanded={expandable ? expanded : undefined}>
          <Chip>{memory.category.replace(/_/g, ' ')}</Chip>
          {memory.invalidated ? (
            <Chip variant="tinted" tint="var(--warning)">
              invalidated
            </Chip>
          ) : memory.verification.length > 0 ? (
            <span className={styles.captionTertiary}>{memory.verification.replace(/_/g, ' ')}</span>
          ) : null}
          <span className={styles.memorySpacer} />
          {created !== null ? <span className={styles.captionTertiary}>{TranscriptFormat.relative(created)}</span> : null}
          {expandable ? <Icon name="chevronDown" size={10} strokeWidth={2.5} className={`${styles.memoryChevron} ${expanded ? '' : styles.memoryChevronCollapsed}`} /> : null}
        </button>
      </Help>
      <div ref={bodyRef} className={styles.memoryBody}>
        <div
          ref={contentRef}
          className={contentClass}
          onClick={
            expanded
              ? undefined
              : () => {
                  if (expandable) toggle()
                }
          }
        >
          {memory.content}
        </div>
        {memory.evidence_paths.length > 0 ? (
          expanded ? (
            <div className={`${styles.evidence} ${styles.evidenceExpanded}`}>{memory.evidence_paths.join(' · ')}</div>
          ) : (
            <MiddleTruncate text={memory.evidence_paths.join(' · ')} className={styles.evidence} />
          )
        ) : null}
      </div>
    </div>
  )
})

// MARK: - Recent tasks

/** How many tasks are listed. */
const TASKS_SHOWN = 10

/// `RecentTasksCard`: the project's tasks, newest first, ten of them (each
/// opens its chat); a warning in the header when they could not be loaded.
function RecentTasksCard({ project }: { project: ProjectState }) {
  const tasks = [...project.tasks].sort((a, b) => taskUpdated(b) - taskUpdated(a))
  const open = useCallback((sessionId: string) => void useApp.getState().selectSession(sessionId), [])
  const error = project.extrasError
  return (
    <ProjectCard
      title="Recent tasks"
      icon="queued"
      trailing={
        error !== null ? (
          <Tooltip content={error}>
            <span className={styles.extrasError} role="img" aria-label={`Could not load: ${error}`}>
              <Icon name="warning" size={12} strokeWidth={2} />
            </span>
          </Tooltip>
        ) : null
      }
    >
      {tasks.length === 0 ? (
        project.extrasLoading ? (
          <LoadingRow />
        ) : (
          <EmptyCardText text="No tasks yet." />
        )
      ) : (
        <div className={styles.taskList}>
          {tasks.slice(0, TASKS_SHOWN).map((task) => (
            <TaskRow key={task.id} task={task} isRunning={task.session_id != null && project.isRunningSession(task.session_id) && task.status == null} onOpen={open} />
          ))}
          {tasks.length > TASKS_SHOWN ? <MoreText count={tasks.length - TASKS_SHOWN} indent /> : null}
        </div>
      )}
    </ProjectCard>
  )
}

/** `RFC3339.parse(updatedAt) ?? .distantPast`, as milliseconds. */
function taskUpdated(task: TaskSummary): number {
  return parseRFC3339(task.updated_at)?.getTime() ?? Number.NEGATIVE_INFINITY
}

/** `TaskRow`: objective (two lines), the status pill, when, files touched and unresolved items. */
const TaskRow = memo(function TaskRow({ task, isRunning, onOpen }: { task: TaskSummary; isRunning: boolean; onOpen: (sessionId: string) => void }) {
  const sessionId = task.session_id ?? null
  const date = parseRFC3339(task.completed_at ?? task.updated_at)
  const files = task.modified_files.length
  return (
    <Help text={sessionId === null ? 'This task has no chat' : 'Open the chat'}>
      <button
        type="button"
        className={styles.taskRow}
        disabled={sessionId === null}
        aria-label={`Task: ${task.objective}`}
        onClick={() => {
          if (sessionId !== null) onOpen(sessionId)
        }}
      >
        <span className={styles.taskObjective}>{task.objective}</span>
        <span className={styles.taskMeta}>
          {isRunning ? <StatusPill running /> : task.status != null ? <StatusPill status={task.status} /> : <StatusPill text={executionStateLabel(task.state)} tint="var(--text-secondary)" />}
          {date !== null ? <span className={styles.captionTertiary}>{TranscriptFormat.relative(date)}</span> : null}
          {files > 0 ? <span className={styles.captionTertiary}>{`${files} file${files === 1 ? '' : 's'}`}</span> : null}
          {task.unresolved.length > 0 ? <span className={styles.unresolved}>{`${task.unresolved.length} unresolved`}</span> : null}
        </span>
      </button>
    </Help>
  )
})

// MARK: - Helpers

/// `.help(_:)` on a control that can be disabled: the tooltip hangs off a
/// wrapper, so it still shows while the button is disabled (a disabled
/// button gets no pointer events). An empty text shows no tooltip.
function Help({ text, children }: { text: string; children: ReactElement }) {
  if (text.length === 0) return children
  return (
    <Tooltip content={text}>
      <span className={styles.help}>{children}</span>
    </Tooltip>
  )
}

/// The root path's row: `Text(root).lineLimit(1).truncationMode(.middle)`
/// followed by the copy and reveal buttons, which hug the text (HStack
/// spacing 4). The path drops characters from its middle when the row is
/// too narrow, so the fitting width is the row's minus the buttons', not
/// the text element's own (which hugs the shown text).
function PathRow({ text, children }: { text: string; children: ReactNode }) {
  const rowRef = useRef<HTMLDivElement>(null)
  const textRef = useRef<HTMLSpanElement>(null)
  const [shown, setShown] = useState(text)

  useLayoutEffect(() => {
    const row = rowRef.current
    const label = textRef.current
    if (!row || !label) return
    const fit = () => {
      const style = getComputedStyle(row)
      const gap = parseFloat(style.columnGap) || 0
      let others = 0
      let count = 0
      // The buttons sit in `display: contents` tooltip wrappers, which have no
      // box of their own: their children are the row's flex items.
      const addItems = (parent: Element) => {
        for (const child of Array.from(parent.children)) {
          if (child === label) continue
          if (getComputedStyle(child).display === 'contents') {
            addItems(child)
            continue
          }
          others += child.getBoundingClientRect().width
          count += 1
        }
      }
      addItems(row)
      const width = row.clientWidth - others - gap * count
      if (width <= 0) return
      setShown(middleTruncated(text, width, getComputedStyle(label).font))
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(row)
    return () => observer.disconnect()
  }, [text])

  return (
    <div ref={rowRef} className={styles.pathRow}>
      <span
        ref={textRef}
        className={`${styles.rootPath} selectable`}
        aria-label={shown === text ? undefined : text}
        onCopy={(event) => {
          // Selecting the whole shortened path copies the whole path, as AppKit does.
          if (shown === text || window.getSelection()?.toString() !== shown) return
          event.clipboardData.setData('text/plain', text)
          event.preventDefault()
        }}
      >
        {shown}
      </span>
      {children}
    </div>
  )
}
