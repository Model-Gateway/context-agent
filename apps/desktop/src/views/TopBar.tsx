import { useCallback, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Chip } from '../components/Chip'
import { IconButton } from '../components/IconButton'
import { isMac } from '../components/platform'
import { SearchField } from '../components/SearchField'
import { Segmented, type SegmentedOption } from '../components/Segmented'
import { Tooltip } from '../components/Tooltip'
import { permissionProfileIcon } from '../models/permissions'
import { approvalModeLabel, useApp, type AppStore, type SidebarItem } from '../state/app'
import { ChatState } from '../state/chat'
import { useUnsavedCount } from '../state/editor'
import { pendingApprovals, selectedProject, type PendingApproval } from '../state/selectors'
import { Icon } from '../theme/icons'
import { PermissionsPopover } from './Composer/PermissionsPopover'
import { WindowControls } from './WindowControls'
import styles from './TopBar.module.css'

// MARK: - Layouts

/** How the project chip shows: name and branch, the branch only, or not at all. */
type ProjectChipMode = 'nameAndBranch' | 'branch' | 'hidden'

/** One arrangement of the bar, from roomiest to tightest (`TopBar.Layout`). */
interface BarLayout {
  project: ProjectChipMode
  /** Width of the search field; null shows the magnifier button only. */
  paletteWidth: number | null
  /** Titles on the selected tab only. */
  compactTabs: boolean
  /** The approval mode as an icon without its label. */
  compactApproval: boolean
}

/// `TopBar.layouts`: the search field narrows first, then the project chip
/// loses its name and goes, then tab titles and the approval label; the
/// field itself turns into an icon last.
const LAYOUTS: readonly BarLayout[] = [
  { project: 'nameAndBranch', paletteWidth: 320, compactTabs: false, compactApproval: false },
  { project: 'nameAndBranch', paletteWidth: 220, compactTabs: false, compactApproval: false },
  { project: 'nameAndBranch', paletteWidth: 140, compactTabs: false, compactApproval: false },
  { project: 'branch', paletteWidth: 140, compactTabs: false, compactApproval: false },
  { project: 'hidden', paletteWidth: 140, compactTabs: false, compactApproval: false },
  // Near the minimum window width.
  { project: 'hidden', paletteWidth: 120, compactTabs: true, compactApproval: false },
  { project: 'hidden', paletteWidth: 120, compactTabs: true, compactApproval: true },
  { project: 'hidden', paletteWidth: null, compactTabs: true, compactApproval: true },
]

const ROOMIEST: BarLayout = { project: 'nameAndBranch', paletteWidth: 320, compactTabs: false, compactApproval: false }

/** The trailing arrangements the layouts use, by key. */
type TrailingKey = 'full' | 'branch' | 'hidden' | 'hiddenCompact'

function trailingKey(layout: BarLayout): TrailingKey {
  if (layout.project === 'nameAndBranch') return 'full'
  if (layout.project === 'branch') return 'branch'
  return layout.compactApproval ? 'hiddenCompact' : 'hidden'
}

/** `CenteredBarLayout(spacing:)`. */
const SPACING = 16
/** Room for the macOS traffic lights over the bar's leading edge (`TopBar.trafficLightsInset`). */
const TRAFFIC_LIGHTS_INSET = 90
/** The bar's padding before the first control where there are no traffic lights (Windows, Linux). */
const PLAIN_LEADING_INSET = 12
const TRAILING_INSET = 12
/** The magnifier button that replaces the field last (`IconButton` at 16 pt). */
const PALETTE_ICON_WIDTH = 28

interface Measured {
  tabs: number
  tabsCompact: number
  trailing: Record<TrailingKey, number>
}

// MARK: - TopBar

/**
 * `TopBar` (Views/Detail/DetailView.swift): the 52 px bar across the whole
 * window, above the sidebar, centre pane and inspector: after the traffic
 * lights (macOS) the Chat · Changes · Project · Editor tabs; the palette's
 * search field centred in the window, sliding aside only when a neighbour
 * would overlap (`CenteredBarLayout`); at the right the approvals pill, the
 * project / branch chip, the approval-mode chip (opens the permissions
 * popover), the inspector toggle and the Settings gear, then the window
 * controls on Windows and Linux. When space runs out it steps through
 * `LAYOUTS` like SwiftUI's `ViewThatFits`. Empty space drags the window and
 * a double-click maximises it (`data-tauri-drag-region`).
 */
export function TopBar() {
  const leadingInset = isMac ? TRAFFIC_LIGHTS_INSET : PLAIN_LEADING_INSET
  const barRef = useRef<HTMLElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const measureRefs = useRef<Partial<Record<'tabs' | 'tabsCompact' | TrailingKey, HTMLElement | null>>>({})
  const [measured, setMeasured] = useState<Measured | null>(null)
  const [geometry, setGeometry] = useState<{ width: number; center: number } | null>(null)

  const measure = useCallback(() => {
    const refs = measureRefs.current
    const width = (key: keyof typeof refs) => refs[key]?.getBoundingClientRect().width ?? 0
    const next: Measured = {
      tabs: width('tabs'),
      tabsCompact: width('tabsCompact'),
      trailing: { full: width('full'), branch: width('branch'), hidden: width('hidden'), hiddenCompact: width('hiddenCompact') },
    }
    setMeasured((previous) => (previous && sameMeasured(previous, next) ? previous : next))
    const bar = barRef.current
    const content = contentRef.current
    if (bar && content) {
      const available = content.clientWidth - leadingInset - TRAILING_INSET
      // The window's centre, from the start of the content box (after the leading inset).
      const center = bar.clientWidth / 2 - content.offsetLeft - leadingInset
      setGeometry((previous) => (previous && previous.width === available && previous.center === center ? previous : { width: available, center }))
    }
  }, [leadingInset])

  useLayoutEffect(() => {
    measure()
    const observer = new ResizeObserver(() => measure())
    for (const element of Object.values(measureRefs.current)) if (element) observer.observe(element)
    if (barRef.current) observer.observe(barRef.current)
    if (contentRef.current) observer.observe(contentRef.current)
    return () => observer.disconnect()
  }, [measure])

  // The widest arrangement that fits (`ViewThatFits(in: .horizontal)`).
  let layoutIndex = 0
  if (measured && geometry) {
    layoutIndex = LAYOUTS.length - 1
    for (let index = 0; index < LAYOUTS.length; index += 1) {
      const layout = LAYOUTS[index]
      if (!layout) continue
      if (idealWidth(layout, measured) <= geometry.width) {
        layoutIndex = index
        break
      }
    }
  }
  const layout = LAYOUTS[layoutIndex] ?? ROOMIEST

  // `CenteredBarLayout.placeSubviews`: centred in the window, slid aside only
  // as far as needed to keep `SPACING` from the tabs and the trailing group.
  let paletteLeft: number | null = null
  if (measured && geometry) {
    const tabs = layout.compactTabs ? measured.tabsCompact : measured.tabs
    const trailing = measured.trailing[trailingKey(layout)]
    const palette = layout.paletteWidth ?? PALETTE_ICON_WIDTH
    const lowest = tabs + SPACING
    const highest = geometry.width - trailing - SPACING - palette
    paletteLeft = leadingInset + Math.max(lowest, Math.min(geometry.center - palette / 2, highest))
  }

  return (
    <header ref={barRef} className={styles.bar} aria-label="Top bar" data-tauri-drag-region>
      <div ref={contentRef} className={styles.content} style={{ paddingLeft: leadingInset, paddingRight: TRAILING_INSET }} data-tauri-drag-region>
        <TabSwitcher compact={layout.compactTabs} />
        <div className={styles.dragFill} data-tauri-drag-region />
        <Trailing project={layout.project} compactApproval={layout.compactApproval} />
        <div className={styles.palette} style={{ left: paletteLeft ?? undefined, visibility: paletteLeft === null ? 'hidden' : undefined }}>
          {layout.paletteWidth !== null ? <PaletteField width={layout.paletteWidth} /> : <PaletteIconButton />}
        </div>
        {/* What `ViewThatFits` measures: every tab and trailing arrangement at its ideal width. */}
        <div className={styles.measure} aria-hidden="true" inert>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.tabs = element
            }}>
            <TabSwitcher compact={false} measuring />
          </MeasureSlot>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.tabsCompact = element
            }}>
            <TabSwitcher compact measuring />
          </MeasureSlot>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.full = element
            }}>
            <Trailing project="nameAndBranch" compactApproval={false} measuring />
          </MeasureSlot>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.branch = element
            }}>
            <Trailing project="branch" compactApproval={false} measuring />
          </MeasureSlot>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.hidden = element
            }}>
            <Trailing project="hidden" compactApproval={false} measuring />
          </MeasureSlot>
          <MeasureSlot setRef={(element) => {
              measureRefs.current.hiddenCompact = element
            }}>
            <Trailing project="hidden" compactApproval measuring />
          </MeasureSlot>
        </div>
      </div>
      {isMac ? null : <WindowControls />}
    </header>
  )
}

function sameMeasured(a: Measured, b: Measured): boolean {
  return (
    a.tabs === b.tabs &&
    a.tabsCompact === b.tabsCompact &&
    a.trailing.full === b.trailing.full &&
    a.trailing.branch === b.trailing.branch &&
    a.trailing.hidden === b.trailing.hidden &&
    a.trailing.hiddenCompact === b.trailing.hiddenCompact
  )
}

/** `CenteredBarLayout.sizeThatFits`' ideal width: the three ideal widths plus spacing. */
function idealWidth(layout: BarLayout, measured: Measured): number {
  const tabs = layout.compactTabs ? measured.tabsCompact : measured.tabs
  return tabs + (layout.paletteWidth ?? PALETTE_ICON_WIDTH) + measured.trailing[trailingKey(layout)] + SPACING * 2
}

function MeasureSlot({ setRef, children }: { setRef: (element: HTMLDivElement | null) => void; children: ReactNode }) {
  return (
    <div ref={setRef} className={styles.measureSlot}>
      {children}
    </div>
  )
}

// MARK: - Search field

/// Looks like the search field; a click opens the command palette (the
/// field itself never takes clicks or focus).
function PaletteField({ width }: { width: number }) {
  return (
    <Tooltip content="Search chats and actions" shortcut="mod+K" placement="bottom">
      <button
        type="button"
        className={styles.paletteField}
        style={{ width }}
        aria-label="Search chats, opens the command palette"
        onClick={() => useApp.getState().setPaletteVisible(true)}
      >
        <span className={styles.paletteLook} inert aria-hidden="true">
          {/* "Ctrl+K" is twice as wide as "⌘K": in the narrowest field it would leave "S…" of the placeholder, so the hint goes (the tooltip still says it). */}
          <SearchField placeholder="Search chats…" value="" onChange={noop} kbdHint={isMac || width > 120 ? 'mod+K' : undefined} />
        </span>
      </button>
    </Tooltip>
  )
}

function noop() {}

function PaletteIconButton() {
  return (
    <IconButton
      icon="search"
      label="Search chats, opens the command palette"
      shortcut="mod+K"
      tooltipPlacement="bottom"
      onClick={() => useApp.getState().setPaletteVisible(true)}
    />
  )
}

// MARK: - Tabs

const TAB_ITEMS: readonly { item: SidebarItem; title: string; icon: 'chat' | 'changes' | 'project' | 'editor'; shortcut: string }[] = [
  { item: 'chat', title: 'Chat', icon: 'chat', shortcut: 'mod+1' },
  { item: 'changes', title: 'Changes', icon: 'changes', shortcut: 'mod+2' },
  { item: 'project', title: 'Project', icon: 'project', shortcut: 'mod+3' },
  { item: 'editor', title: 'Editor', icon: 'editor', shortcut: 'mod+4' },
]

/// `TabSwitcher`: Chat · Changes · Project · Editor pills; the selected one is
/// `surface` with a hairline and a soft shadow. Disabled without a project.
/// `compact` shows the title on the selected pill only. Changes carries the
/// dirty-file count, Editor the documents with unsaved edits.
function TabSwitcher({ compact, measuring = false }: { compact: boolean; measuring?: boolean }) {
  const sidebarItem = useApp((state) => state.sidebarItem)
  // The project reference (stable while selected) and the primitives drawn
  // here, so streamed events in the project do not redraw the bar.
  const project = useApp(selectedProject)
  const dirty = useApp((state) => selectedProject(state)?.dirtyCount ?? 0)
  const unsaved = useUnsavedCount(project)
  const options: SegmentedOption<SidebarItem>[] = TAB_ITEMS.map((tab) => ({
    value: tab.item,
    label: tab.title,
    icon: tab.icon,
    shortcut: tab.shortcut,
    badge: tab.item === 'changes' ? dirty : tab.item === 'editor' ? unsaved : 0,
  }))
  return (
    <div className={styles.tabs}>
      <Segmented<SidebarItem>
        options={options}
        value={sidebarItem}
        compact={compact}
        disabled={project === null}
        // SF 12 pt symbols draw about 14 px wide; the Icon's optical size lands there at 13.5.
        iconSize={13.5}
        iconStrokeWidth={1.8}
        aria-label="Tabs"
        onChange={(item) => {
          if (measuring) return
          const state = useApp.getState()
          state.setSidebarItem(item)
          if (item === 'chat') state.focusComposer()
        }}
      />
    </div>
  )
}

// MARK: - Trailing group

/** What the approvals pill draws, as one string (the store's objects change in place). */
function approvalsSignature(state: AppStore): string {
  const approvals = pendingApprovals(state)
  const first = approvals[0]
  if (!first) return ''
  return `${approvals.length}|${first.request.id}|${first.request.summary}|${first.chat.projectId}|${first.chat.sessionId}|${first.chat.title}`
}

/// The right end: the approvals pill, the project / branch chip, the
/// approval-mode chip, the inspector toggle and the Settings gear.
function Trailing({ project: mode, compactApproval, measuring = false }: { project: ProjectChipMode; compactApproval: boolean; measuring?: boolean }) {
  const approvalsKey = useApp(approvalsSignature)
  const approvals = useMemo(() => (approvalsKey === '' ? [] : pendingApprovals(useApp.getState())), [approvalsKey])
  const name = useApp((state) => selectedProject(state)?.name ?? null)
  const root = useApp((state) => selectedProject(state)?.root ?? '')
  const branch = useApp((state) => selectedProject(state)?.branch ?? null)
  const profile = useApp((state) => selectedProject(state)?.info.permission_profile ?? null)
  const inspectorVisible = useApp((state) => state.inspectorVisible)
  const first = approvals[0]
  return (
    <div className={styles.trailing} data-tauri-drag-region={measuring ? undefined : true}>
      {first ? <ApprovalsPill count={approvals.length} first={first} /> : null}
      {name !== null && profile !== null ? (
        <>
          {mode !== 'hidden' ? <ProjectChip name={name} root={root} branch={branch} showName={mode === 'nameAndBranch'} /> : null}
          <ApprovalModeChip profile={profile} compact={compactApproval} measuring={measuring} />
        </>
      ) : null}
      <IconButton
        icon="inspector"
        label={inspectorVisible ? 'Hide inspector' : 'Show inspector'}
        isActive={inspectorVisible}
        tooltipPlacement="bottom"
        className={styles.barIcon}
        onClick={() => useApp.getState().toggleInspector()}
      />
      <IconButton icon="settings" label="Settings" shortcut="mod+," tooltipPlacement="bottom" className={styles.barIcon} onClick={() => useApp.getState().openSettings()} />
    </div>
  )
}

/// "1 approval waiting · “<chat>”" in `warning`: a click selects that
/// project and chat.
function ApprovalsPill({ count, first }: { count: number; first: PendingApproval }) {
  const title = count === 1 ? '1 approval waiting' : `${count} approvals waiting`
  const chat = first.chat
  const open = () => {
    const state = useApp.getState()
    state.selectProject(chat.projectId, false)
    void state.selectSession(chat.sessionId)
  }
  return (
    <Tooltip content={`${first.request.summary} — click to open the chat`} placement="bottom">
      <Chip variant="tinted" tint="var(--warning)" icon="approval" aria-label={`${title} in ${chat.title}; open the chat`} onClick={open}>
        {`${title} · “${ChatState.clip(chat.title, 28)}”`}
      </Chip>
    </Tooltip>
  )
}

/** Folder glyph + name (dropped first when the bar is tight) + branch chip. */
function ProjectChip({ name, root, branch, showName }: { name: string; root: string; branch: string | null; showName: boolean }) {
  const hasBranch = branch !== null && branch.length > 0
  return (
    <Tooltip content={root} placement="bottom">
      <div className={styles.projectChip} role="group" aria-label={`Project ${name}${branch !== null ? `, branch ${branch}` : ''}`}>
        <Icon name="folder" size={11} strokeWidth={2} className={styles.projectFolder} />
        {showName ? <span className={styles.projectName}>{name}</span> : null}
        {hasBranch ? <Chip icon="branch">{branch}</Chip> : null}
      </div>
    </Tooltip>
  )
}

/// The approval mode ("✋ Ask"): opens the permissions popover; the composer
/// takes focus again when it closes.
function ApprovalModeChip({ profile, compact, measuring }: { profile: string; compact: boolean; measuring: boolean }) {
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLSpanElement>(null)
  const label = approvalModeLabel(profile)
  const icon = permissionProfileIcon(profile)
  const toggle = () => {
    if (!measuring) setOpen((value) => !value)
  }
  const close = () => {
    setOpen(false)
    useApp.getState().focusComposer()
  }
  return (
    <>
      <Tooltip content={`Approval mode: ${label} · click to change`} placement="bottom" disabled={open}>
        <span ref={anchorRef} className={styles.approvalAnchor}>
          {compact ? (
            <button type="button" className={styles.approvalIcon} aria-label={`Approval mode ${label}`} aria-expanded={open} onClick={toggle}>
              <Icon name={icon} size={13} strokeWidth={2} />
            </button>
          ) : (
            <Chip icon={icon} aria-label={`Approval mode ${label}`} onClick={toggle}>
              {label}
            </Chip>
          )}
        </span>
      </Tooltip>
      {measuring ? null : <PermissionsPopover open={open} onClose={close} anchorRef={anchorRef} />}
    </>
  )
}
