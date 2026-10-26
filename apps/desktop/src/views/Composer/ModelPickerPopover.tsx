// Port of apps/macos/ContextAgent/Views/Composer/ModelPickerPopover.swift:
// the model picker (apps/macos/ARCHITECTURE.md "Model picker popover").

import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from 'react'
import { errorText } from '../../backend/errors'
import { modelStatSavingsLabel, type CapabilitySummary } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { Popover } from '../../components/Popover'
import { SearchField } from '../../components/SearchField'
import { Tooltip } from '../../components/Tooltip'
import { toasts } from '../../components/toasts'
import { matchSegments, type ModelMatch } from '../../models/modelCatalog'
import { useModelsDevVersion } from '../../models/modelsDev'
import { usePref } from '../../models/preferences'
import { selectedProject, useApp, useProjectValue } from '../../state/app'
import type { ProjectState } from '../../state/project'
import { Icon, type IconName } from '../../theme/icons'
import { FloatingCard, MiddleRuns, ModelHoverCard, ModelLogo, modelCard, type ModelHoverCardProps, type TextRun } from './ModelHoverCard'
import { isPlainReturn, useNativeKeyDown } from './nativeKeyDown'
import styles from './ModelPickerPopover.module.css'

export interface ModelPickerPopoverProps {
  /** `appState.modelPickerVisible`. */
  open: boolean
  /** Esc, an outside press, or a choice: sets `modelPickerVisible` false. */
  onClose: () => void
  /** The composer's model chip. */
  anchorRef: RefObject<HTMLElement | null>
}

/// `ModelPickerPopover` (ModelPickerPopover.swift): 320 × 400, auto-focused
/// search, a "Recent" section, prefix groups past 12 entries, fuzzy matches
/// with highlighted runs, ↑↓ Return Esc handled here, a trailing "Use
/// “<typed>” as the model name" row, loading / list-error / switching /
/// capability states and a read-only mode while the project runs. Reads the
/// selected project. Each row floats a models.dev hover card beside the
/// picker.
export function ModelPickerPopover({ open, onClose, anchorRef }: ModelPickerPopoverProps) {
  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="below" align="center" width={322} returnFocus={false} aria-label="Choose a model">
      <ModelPicker onClose={onClose} />
    </Popover>
  )
}

/** One line of the list (`Row`). */
type Row =
  | { id: string; kind: 'header'; title: string }
  | { id: string; kind: 'model'; match: ModelMatch; recent: boolean; runs: TextRun[] }
  | { id: string; kind: 'custom'; typed: string; runs: TextRun[] }

/** The row under the pointer whose card is shown. */
interface HoverState {
  rowId: string
  anchor: HTMLElement
  card: ModelHoverCardProps
}

/** `capabilityLine(_:)`: the probe's blocking issue for a model that cannot run agent tasks. */
function capabilityLine(capability: CapabilitySummary): string {
  const issue = capability.issues[0]
  return 'Not usable for agent runs' + (issue !== undefined ? `: ${issue}` : '')
}

/** `rows(for:)`. */
function buildRows(project: ProjectState, query: string): Row[] {
  const catalog = project.models
  const app = useApp.getState()
  const out: Row[] = []
  for (const section of catalog.sections(query, (id) => app.modelDisplayName(id))) {
    if (section.items.length === 0) continue
    out.push({ id: `header:${section.id}`, kind: 'header', title: section.title })
    for (const item of section.items) {
      out.push({ id: `${section.id}:${item.name}`, kind: 'model', match: item, recent: section.kind === 'recent', runs: highlighted(item) })
    }
  }
  if (catalog.offersCustomName(query)) {
    const typed = query.replace(/^[\p{Zs}\t]+|[\p{Zs}\t]+$/gu, '')
    out.push({ id: 'custom', kind: 'custom', typed, runs: [{ text: `Use “${typed}” as the model name`, className: styles.plainRun }] })
  }
  return out
}

function ModelPicker({ onClose }: { onClose: () => void }) {
  const project = useApp(selectedProject)
  const [query, setQuery] = useState('')
  /** Index into the selectable rows (headers are skipped). */
  const [selected, setSelected] = useState(0)
  /// Bumped by ↑↓ and typing; only these scroll the selection into view
  /// (hover selects without scrolling, so the list never moves under the cursor).
  const [scrollRequest, setScrollRequest] = useState(0)
  /// Set once a model is chosen: rows stop floating hover cards while the
  /// popover fades out under the cursor.
  const [closing, setClosing] = useState(false)
  const [hover, setHover] = useState<HoverState | null>(null)
  /// Screen location of the last hover that changed the selection. A hover
  /// at the same location comes from rows scrolling under a still cursor
  /// and is ignored, so arrow keys keep their selection.
  const lastHoverLocation = useRef<{ x: number; y: number } | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const rowElements = useRef(new Map<string, HTMLElement>())

  // What the rows are built from; each changes identity when it changes.
  const info = useProjectValue(project, (p) => p.info, null)
  const switchingModel = useProjectValue(project, (p) => p.switchingModel, null)
  const running = useProjectValue(project, (p) => p.isRunning, false)
  const models = useProjectValue(project, (p) => p.models.models, [] as string[])
  const loading = useProjectValue(project, (p) => p.models.loading, false)
  const listError = useProjectValue(project, (p) => p.models.error, null)
  const switchError = useProjectValue(project, (p) => p.models.switchError, null)
  const lastSwitch = useProjectValue(project, (p) => p.models.lastSwitch, null)
  const stats = useProjectValue(project, (p) => p.models.stats, null)
  const current = useProjectValue(project, (p) => p.models.current, null)
  const recents = usePref('modelRecents')
  const modelsDevVersion = useModelsDevVersion()
  const baseURL = useApp((state) => state.config?.model.base_url ?? '')

  const rows = useMemo(
    () => (project ? buildRows(project, query) : []),
    // The catalogue is mutated in place; these are its change signals.
    [project, query, models, current, listError, recents, modelsDevVersion, baseURL],
  )
  const ids = useMemo(() => rows.filter((row) => row.kind !== 'header').map((row) => row.id), [rows])
  const selectedId = ids.length === 0 ? null : (ids[Math.min(Math.max(selected, 0), ids.length - 1)] ?? null)
  const interactive = !running && switchingModel === null

  // The search field is focused by the popover (its first focusable element).
  // The list is normally prefetched when the project opened; this fetches
  // only when it is missing or stale.
  useEffect(() => {
    void useApp.getState().refreshModels(false)
  }, [])

  // No anchor: scroll only as far as needed to show the row.
  useLayoutEffect(() => {
    if (scrollRequest === 0 || selectedId === null) return
    rowElements.current.get(selectedId)?.scrollIntoView({ block: 'nearest' })
    // Only ↑↓ and typing scroll (not hover selection).
  }, [scrollRequest])

  const changeQuery = (value: string) => {
    setQuery(value)
    setSelected(0)
    setScrollRequest((n) => n + 1)
  }

  const move = (delta: number) => {
    const count = ids.length
    if (count === 0) return
    setSelected((index) => Math.min(Math.max(index + delta, 0), count - 1))
    setScrollRequest((n) => n + 1)
  }

  /// Closes the picker and runs `models/switch` in the background: the
  /// composer chip shows "Switching…", and a failure or a model the probe
  /// rules out for agent runs comes back as a toast. Choosing the current
  /// model again re-runs the switch unless it last passed the probe, so a
  /// model that failed validation gets probed again.
  const choose = (name: string) => {
    const target = selectedProject(useApp.getState())
    if (!target || target.isRunning || target.switchingModel !== null) return
    // The chosen row never gets a hover exit: hide its card now.
    setClosing(true)
    setHover(null)
    useApp.getState().setModelPickerVisible(false)
    onClose()
    const last = target.models.lastSwitch
    const passed = last !== null && last.model === name && last.capability.agent_capable
    if (name === target.info.model && passed) return
    const display = useApp.getState().modelDisplayName(name)
    void (async () => {
      try {
        const result = await useApp.getState().switchModel(name)
        if (!result.capability.agent_capable) {
          toasts.push({ message: `${display}: ${capabilityLine(result.capability)}`, level: 'warning' })
        }
      } catch (error) {
        const message = errorText(error)
        toasts.push({ message: `Could not switch to ${display}: ${message}`, level: 'error', details: message })
      }
    })()
  }

  /** Return: the selected row (model or custom name). */
  const activateSelection = () => {
    if (selectedId === null) return
    const row = rows.find((candidate) => candidate.id === selectedId)
    if (!row) return
    if (row.kind === 'model') choose(row.match.name)
    else if (row.kind === 'custom') choose(row.typed)
  }

  // On the container (an ancestor of the focused search field), so the
  // arrows move the selection instead of the caret.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    }
  }

  // Return in the search field takes the selected row (natively: the focus
  // trap stops Return before React's listener, see nativeKeyDown.ts).
  useNativeKeyDown(rootRef, (event) => {
    if (!isPlainReturn(event)) return
    event.preventDefault()
    event.stopPropagation()
    activateSelection()
  })

  // The rows' callbacks stay the same across renders (rows are memoised);
  // they read the latest values through this ref.
  const latest = useRef({ ids, interactive, closing, choose })
  useLayoutEffect(() => {
    latest.current = { ids, interactive, closing, choose }
  })

  const onRowHover = useCallback((rowId: string, event: PointerEvent<HTMLElement>, card: ModelHoverCardProps | null) => {
    const state = latest.current
    if (state.interactive) {
      const location = { x: event.screenX, y: event.screenY }
      const last = lastHoverLocation.current
      if (!last || last.x !== location.x || last.y !== location.y) {
        lastHoverLocation.current = location
        const index = state.ids.indexOf(rowId)
        if (index >= 0) setSelected(index)
      }
    }
    const anchor = event.currentTarget
    setHover(card && !state.closing ? { rowId, anchor, card } : null)
  }, [])

  const onRowLeave = useCallback((rowId: string) => {
    // Moving between rows can deliver the new row's enter before the old row's exit.
    setHover((state) => (state && state.rowId === rowId ? null : state))
  }, [])

  const onChoose = useCallback((value: string) => latest.current.choose(value), [])

  const onRemoveRecent = useCallback(
    (name: string) => {
      project?.models.removeRecent(name)
    },
    [project],
  )

  const registerRow = useCallback((rowId: string, element: HTMLElement | null) => {
    if (element) rowElements.current.set(rowId, element)
    else rowElements.current.delete(rowId)
  }, [])

  const ruledOut = lastSwitch !== null && info !== null && lastSwitch.model === info.model && !lastSwitch.capability.agent_capable
  const showsLoading = loading && models.length === 0
  const hasStatus = running || switchingModel !== null || switchError !== null || showsLoading || listError !== null || ruledOut

  return (
    <div ref={rootRef} className={styles.picker} onKeyDown={onKeyDown}>
      <div className={styles.search}>
        <SearchField placeholder="Search models…" aria-label="Search models" value={query} onChange={changeQuery} />
      </div>
      <div className={styles.divider} aria-hidden="true" />
      {project && info ? (
        <>
          <div className={styles.list} role="listbox" aria-label="Models">
            <div className={styles.rows}>
              {rows.map((row) => {
                if (row.kind === 'header') {
                  return (
                    <div key={row.id} className={styles.header} role="heading" aria-level={3}>
                      {row.title}
                    </div>
                  )
                }
                if (row.kind === 'model') {
                  const name = row.match.name
                  const isCurrent = name === info.model
                  const stat = stats !== null ? useApp.getState().modelStat(name, project) : null
                  const savings = !isCurrent && stat ? modelStatSavingsLabel(stat) : null
                  return (
                    <ModelRowView
                      key={row.id}
                      rowId={row.id}
                      value={name}
                      runs={row.runs}
                      logoModelId={name}
                      icon={null}
                      // The current model keeps "current"; the others show the gateway's savings.
                      trailing={isCurrent ? 'current' : savings}
                      trailingTint={isCurrent ? 'var(--accent)' : 'var(--success)'}
                      isSelected={row.id === selectedId}
                      interactive={interactive}
                      tooltip={row.match.display === name ? null : name}
                      onRemoveRecent={row.recent ? onRemoveRecent : null}
                      hasCard={!closing}
                      onChoose={onChoose}
                      onHover={onRowHover}
                      onLeave={onRowLeave}
                      register={registerRow}
                    />
                  )
                }
                return (
                  <ModelRowView
                    key={row.id}
                    rowId={row.id}
                    value={row.typed}
                    runs={row.runs}
                    logoModelId={null}
                    icon="addCustom"
                    trailing={null}
                    trailingTint="var(--accent)"
                    isSelected={row.id === selectedId}
                    interactive={interactive}
                    tooltip={null}
                    onRemoveRecent={null}
                    hasCard={false}
                    onChoose={onChoose}
                    onHover={onRowHover}
                    onLeave={onRowLeave}
                    register={registerRow}
                  />
                )
              })}
              {rows.length === 0 ? <div className={styles.emptyRows}>{showsLoading ? 'Loading models…' : 'No models listed'}</div> : null}
            </div>
          </div>
          {hasStatus ? (
            <StatusFooter
              running={running}
              switching={switchingModel !== null}
              switchError={switchError}
              showsLoading={showsLoading}
              listError={listError}
              refreshing={loading}
              ruledOutLine={ruledOut && lastSwitch ? capabilityLine(lastSwitch.capability) : null}
            />
          ) : null}
        </>
      ) : (
        <div className={styles.placeholder}>
          <EmptyPlaceholder icon="model" title="No project selected" subtitle="Open a project to choose its model." />
        </div>
      )}
      {hover && !closing ? (
        <FloatingCard anchor={hover.anchor} host={rootRef.current} placement="besideWindow">
          <ModelHoverCard {...hover.card} />
        </FloatingCard>
      ) : null}
    </div>
  )
}

/** `highlighted(_:)`: matched runs in `accent` semibold. */
function highlighted(match: ModelMatch): TextRun[] {
  return matchSegments(match).map((segment) => ({ text: segment.text, className: segment.matched ? styles.matchedRun : styles.plainRun }))
}

// MARK: - Footer (states)

interface StatusFooterProps {
  running: boolean
  switching: boolean
  switchError: string | null
  showsLoading: boolean
  listError: string | null
  /** `catalog.loading` (the Refresh button's spinner). */
  refreshing: boolean
  ruledOutLine: string | null
}

/// The footer only appears with something to report: a running task, a
/// switch in progress or failed, a first load, a list error or a model the
/// probe ruled out (no idle "N models · Refresh" line).
function StatusFooter({ running, switching, switchError, showsLoading, listError, refreshing, ruledOutLine }: StatusFooterProps) {
  return (
    <div className={`${styles.footer} hairline-top`} role="status">
      {running ? <FooterLine icon="warning" tint="var(--warning)" text="Finish or cancel the running task to switch" /> : null}
      {switching ? (
        <div className={styles.footerSpinnerLine}>
          <BrailleSpinner size={11} tint="var(--text-secondary)" />
          <span>Switching… probing the endpoint can take up to 2 min</span>
        </div>
      ) : null}
      {switchError !== null ? <FooterLine icon="failed" tint="var(--danger)" text={switchError} /> : null}
      {showsLoading ? (
        <div className={styles.footerSpinnerLine}>
          <BrailleSpinner size={11} tint="var(--text-secondary)" />
          <span>Loading models…</span>
        </div>
      ) : listError !== null ? (
        <div className={styles.footerErrorLine}>
          <FooterLine icon="warning" tint="var(--danger)" text={listError} />
          <span className={styles.footerSpacer} />
          {refreshing ? (
            <span role="img" aria-label="Refreshing models">
              <BrailleSpinner size={11} tint="var(--text-secondary)" />
            </span>
          ) : (
            <Tooltip content="Fetch the model list again">
              <button type="button" className={styles.refresh} onClick={() => void useApp.getState().refreshModels(true)}>
                Refresh
              </button>
            </Tooltip>
          )}
        </div>
      ) : null}
      {/* Capabilities live on the hover cards; only a probe that rules the model out is worth a line. */}
      {ruledOutLine !== null ? <FooterLine icon="failed" tint="var(--danger)" text={ruledOutLine} /> : null}
    </div>
  )
}

function FooterLine({ icon, tint, text }: { icon: IconName; tint: string; text: string }) {
  return (
    <div className={styles.footerLine} style={{ color: tint }}>
      <Icon name={icon} size={10} strokeWidth={2.25} className={styles.footerIcon} />
      <span className={styles.footerText}>{text}</span>
    </div>
  )
}

// MARK: - Row

interface ModelRowViewProps {
  rowId: string
  /** The model name (or typed custom name) choosing the row switches to. */
  value: string
  runs: TextRun[]
  /** Draws the model's lab logo (models.dev, ✳ when unknown) in front of the title. */
  logoModelId: string | null
  icon: IconName | null
  trailing: string | null
  trailingTint: string
  isSelected: boolean
  interactive: boolean
  /** The model id when it differs from the shown name (`.help`). */
  tooltip: string | null
  /** "Remove from Recent" (context menu of a Recent row); null elsewhere. */
  onRemoveRecent: ((name: string) => void) | null
  /** Whether hovering floats `value`'s models.dev card beside the picker. */
  hasCard: boolean
  onChoose: (value: string) => void
  onHover: (rowId: string, event: PointerEvent<HTMLElement>, card: ModelHoverCardProps | null) => void
  onLeave: (rowId: string) => void
  register: (rowId: string, element: HTMLElement | null) => void
}

/** `ModelRow`: 28 px, logo or glyph, the title, the trailing chip; dimmed and inert while read-only. */
const ModelRowView = memo(function ModelRowView({
  rowId,
  value,
  runs,
  logoModelId,
  icon,
  trailing,
  trailingTint,
  isSelected,
  interactive,
  tooltip,
  onRemoveRecent,
  hasCard,
  onChoose,
  onHover,
  onLeave,
  register,
}: ModelRowViewProps) {
  const button = (
    <button
      ref={(element) => register(rowId, element)}
      type="button"
      role="option"
      aria-selected={isSelected}
      aria-disabled={!interactive || undefined}
      tabIndex={-1}
      className={[styles.row, isSelected ? styles.selected : '', interactive ? '' : styles.inert].filter(Boolean).join(' ')}
      onClick={() => {
        if (interactive) onChoose(value)
      }}
      onPointerEnter={(event) => onHover(rowId, event, hasCard ? modelCard(value) : null)}
      onPointerLeave={() => onLeave(rowId)}
    >
      {logoModelId !== null ? (
        <ModelLogo modelId={logoModelId} size={14} tint="var(--text-primary)" />
      ) : icon !== null ? (
        <span className={styles.rowIcon} aria-hidden="true">
          <Icon name={icon} size={11} strokeWidth={2} />
        </span>
      ) : null}
      <MiddleRuns runs={runs} className={styles.rowTitle} />
      <span className={styles.rowSpacer} />
      {trailing !== null ? (
        <Chip variant="tinted" tint={trailingTint}>
          {trailing}
        </Chip>
      ) : null}
    </button>
  )
  const withTooltip = tooltip !== null ? <Tooltip content={tooltip}>{button}</Tooltip> : button
  if (onRemoveRecent === null) return withTooltip
  return <ContextMenu items={[{ label: 'Remove from Recent', onSelect: () => onRemoveRecent(value) }]}>{withTooltip}</ContextMenu>
})
