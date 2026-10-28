import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { ContextMenu } from '../../components/ContextMenu'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import type { MenuItem } from '../../components/Menu'
import { revealLabel } from '../../components/platform'
import { gitFileCode, type GitStatus } from '../../backend/types'
import { useApp, useSelectedProject, type PathAction } from '../../state/app'
import type { FileNode, ProjectState } from '../../state/project'
import { Icon } from '../../theme/icons'
import { ChangesStatus } from '../Changes/ChangesView'
import { MiddleText } from './MiddleText'
import { cx } from './inspectorShared'
import styles from './FilesTab.module.css'

/** Row height and the list's vertical padding (`.frame(height: 24)`, `.padding(.vertical, 4)`). */
const ROW_HEIGHT = 24
const LIST_PADDING = 4
/** Rows rendered beyond the visible ones on each side. */
const OVERSCAN = 12
/** How long after a folder toggle new rows fade in (`withAnimation(.easeOut(duration: 0.15))`); the folder may still be loading. */
const TOGGLE_WINDOW_MS = 1000
const ENTER_MS = 150
const NO_PATHS: ReadonlySet<string> = new Set()

/**
 * Port of `FilesTab` (Views/Inspector/FilesTab.swift): the selected
 * project's tree, read from disk one folder at a time as it is expanded
 * (`ProjectState.fileTree`). Changed files carry their git status letter
 * and folders holding changes a dot; a click opens a file in the Editor
 * tab; the context menu has Show in Changes · Open in Editor · Open with
 * Default Editor · Reveal in Finder (Show in Explorer) · Copy Path. For a
 * project on a remote host the tree comes from the host (`fs/list`) and the
 * local actions (`FileRow.localActions`) are left out.
 */
export function FilesTab() {
  const project = useSelectedProject()
  if (!project) return <EmptyPlaceholder icon="folder" title="No project open" subtitle="Open a project to browse its files." />
  return <FileTreePane key={project.id} project={project} />
}

// MARK: - Pane

interface Changes {
  /** Changed path → status letter. */
  files: Map<string, string>
  /** Every folder above a changed path. */
  folders: Set<string>
}

function changesOf(status: GitStatus | null): Changes {
  const files = new Map<string, string>()
  const folders = new Set<string>()
  for (const file of status?.files ?? []) {
    const path = file.path.endsWith('/') ? file.path.slice(0, -1) : file.path
    files.set(path, gitFileCode(file))
    let parent = path
    for (let slash = parent.lastIndexOf('/'); slash >= 0; slash = parent.lastIndexOf('/')) {
      parent = parent.slice(0, slash)
      folders.add(parent)
    }
  }
  return { files, folders }
}

/** `FileTreePane`: the toolbar over the (windowed) rows; reloads on every git status change. */
function FileTreePane({ project }: { project: ProjectState }) {
  const tree = project.fileTree
  const treeVersion = tree.version
  // `tree.version` is the change signal of the tree's mutable listings.
  const rows = useMemo(() => tree.visibleRows, [tree, treeVersion])
  const gitStatus = project.gitStatus
  const changes = useMemo(() => changesOf(gitStatus), [gitStatus])
  // `.onChange(of: project.gitStatus)`: compared by value, as the Swift `Equatable` status is.
  const statusKey = useMemo(() => (gitStatus ? JSON.stringify(gitStatus) : ''), [gitStatus])
  const lastStatusKey = useRef(statusKey)

  useEffect(() => {
    tree.loadRootIfNeeded()
  }, [tree])

  useEffect(() => {
    if (statusKey === lastStatusKey.current) return
    lastStatusKey.current = statusKey
    void tree.reload()
  }, [statusKey, tree])

  // Rows a folder toggle brought in fade in, as the Swift rows do inside the
  // toggle's animation; rows that merely scroll into the window do not.
  const animateUntil = useRef(0)
  const previousPaths = useRef<Set<string> | null>(null)
  const [entering, setEntering] = useState<ReadonlySet<string>>(NO_PATHS)
  useLayoutEffect(() => {
    const current = new Set(rows.map((row) => row.node.path))
    const previous = previousPaths.current
    previousPaths.current = current
    if (previous === null || performance.now() > animateUntil.current) return
    // An expanded folder that is still loading changes nothing yet: keep waiting for its rows.
    const added = rows.filter((row) => !previous.has(row.node.path)).map((row) => row.node.path)
    if (added.length === 0) return
    animateUntil.current = 0
    setEntering(new Set(added))
  }, [rows])
  useEffect(() => {
    if (entering.size === 0) return
    const timer = window.setTimeout(() => setEntering(NO_PATHS), ENTER_MS + 50)
    return () => window.clearTimeout(timer)
  }, [entering])

  const open = useCallback(
    (node: FileNode) => {
      tree.select(node.path)
      if (node.isDirectory) {
        // Expanding brings rows in; collapsing only removes them.
        animateUntil.current = tree.expanded.has(node.path) ? 0 : performance.now() + TOGGLE_WINDOW_MS
        tree.toggle(node)
      } else useApp.getState().reveal(node.path, project, 'openInEditor')
    },
    [project, tree],
  )

  const perform = useCallback((node: FileNode, action: PathAction) => useApp.getState().reveal(node.path, project, action), [project])

  // Windowing (the LazyVStack): only the rows in view, plus a margin, are rendered.
  const scrollRef = useRef<HTMLDivElement>(null)
  const [viewport, setViewport] = useState({ top: 0, height: 0 })
  const frame = useRef(0)
  const measure = useCallback(() => {
    frame.current = 0
    const element = scrollRef.current
    if (!element) return
    setViewport((current) =>
      current.top === element.scrollTop && current.height === element.clientHeight ? current : { top: element.scrollTop, height: element.clientHeight },
    )
  }, [])
  const scheduleMeasure = useCallback(() => {
    if (frame.current === 0) frame.current = requestAnimationFrame(measure)
  }, [measure])

  useLayoutEffect(() => {
    measure()
    const element = scrollRef.current
    if (!element || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(scheduleMeasure)
    observer.observe(element)
    return () => {
      observer.disconnect()
      if (frame.current !== 0) cancelAnimationFrame(frame.current)
      frame.current = 0
    }
  }, [measure, scheduleMeasure])

  const height = viewport.height > 0 ? viewport.height : 800
  const first = Math.max(0, Math.floor((viewport.top - LIST_PADDING) / ROW_HEIGHT) - OVERSCAN)
  const last = Math.min(rows.length, Math.ceil((viewport.top + height - LIST_PADDING) / ROW_HEIGHT) + OVERSCAN)

  return (
    <div className={styles.pane}>
      <div className={styles.toolbar}>
        <Icon name="folder" size={11} className={styles.toolbarIcon} />
        <MiddleText text={project.name} grow className={styles.projectName} />
        <IconButton
          icon="collapseAll"
          label="Collapse all folders"
          size={11}
          className={styles.toolbarButton}
          disabled={tree.expanded.size === 0}
          onClick={() => tree.collapseAll()}
        />
        <IconButton icon="refresh" label="Reload files" size={11} className={styles.toolbarButton} onClick={() => void tree.reload()} />
      </div>
      <div ref={scrollRef} className={styles.scroll} onScroll={scheduleMeasure}>
        <div className={styles.list} style={{ height: rows.length * ROW_HEIGHT + LIST_PADDING * 2 }}>
          <div className={styles.window} style={{ top: LIST_PADDING + first * ROW_HEIGHT }}>
            {rows.slice(first, last).map((row) => (
              <FileRow
                key={row.node.path}
                node={row.node}
                depth={row.depth}
                expanded={tree.expanded.has(row.node.path)}
                selected={tree.selectedPath === row.node.path}
                statusCode={changes.files.get(row.node.path) ?? null}
                containsChanges={changes.folders.has(row.node.path)}
                entering={entering.has(row.node.path)}
                localActions={!project.isRemote}
                onOpen={open}
                onAction={perform}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

// MARK: - Row

interface FileRowProps {
  node: FileNode
  depth: number
  expanded: boolean
  selected: boolean
  statusCode: string | null
  containsChanges: boolean
  /** Just brought in by a folder toggle: fades in. */
  entering: boolean
  /** The file is on this machine: Open with Default Editor and the file manager apply. */
  localActions: boolean
  onOpen: (node: FileNode) => void
  onAction: (node: FileNode, action: PathAction) => void
}

/// Rows are compared on what they draw, so toggling one folder or a git
/// status change rebuilds only the rows it changed (the callbacks carry
/// nothing but the node).
function sameRow(lhs: FileRowProps, rhs: FileRowProps): boolean {
  return (
    lhs.node.path === rhs.node.path &&
    lhs.node.name === rhs.node.name &&
    lhs.node.isDirectory === rhs.node.isDirectory &&
    lhs.depth === rhs.depth &&
    lhs.expanded === rhs.expanded &&
    lhs.selected === rhs.selected &&
    lhs.statusCode === rhs.statusCode &&
    lhs.containsChanges === rhs.containsChanges &&
    lhs.entering === rhs.entering &&
    lhs.localActions === rhs.localActions &&
    lhs.onOpen === rhs.onOpen &&
    lhs.onAction === rhs.onAction
  )
}

/** `FileRow`: chevron, folder / file glyph, name tinted by its git status, status letter or a folder's dot. */
const FileRow = memo(function FileRow({ node, depth, expanded, selected, statusCode, containsChanges, entering, localActions, onOpen, onAction }: FileRowProps) {
  const tint = statusCode !== null ? ChangesStatus.tint(statusCode) : undefined
  const menu = (): MenuItem[] => {
    const items: MenuItem[] = []
    if (statusCode !== null) items.push({ label: 'Show in Changes', onSelect: () => onAction(node, 'showInChanges') })
    if (!node.isDirectory) {
      items.push({ label: 'Open in Editor', onSelect: () => onAction(node, 'openInEditor') })
      if (localActions) items.push({ label: 'Open with Default Editor', onSelect: () => onAction(node, 'openWithEditor') })
    }
    if (localActions) items.push({ label: revealLabel, onSelect: () => onAction(node, 'revealInFileManager') })
    items.push({ label: 'Copy Path', onSelect: () => onAction(node, 'copyPath') })
    return items
  }
  let label = node.isDirectory ? `Folder ${node.name}, ${expanded ? 'expanded' : 'collapsed'}` : node.name
  if (statusCode !== null) label += `, ${ChangesStatus.label(statusCode)}`
  else if (containsChanges) label += ', contains changes'

  return (
    <ContextMenu items={menu}>
      <button
        type="button"
        className={cx(styles.row, selected && styles.selected, entering && styles.entering)}
        style={{ paddingLeft: 8 + depth * 12 }}
        aria-label={label}
        aria-current={selected || undefined}
        onClick={() => onOpen(node)}
      >
        <span className={styles.chevronFrame} style={node.isDirectory ? undefined : { opacity: 0 }} aria-hidden="true">
          <Icon name={expanded ? 'chevronDown' : 'chevronRight'} size={11} strokeWidth={2.5} />
        </span>
        <span className={cx(styles.iconFrame, node.isDirectory ? styles.folderIcon : styles.fileIcon)} aria-hidden="true">
          <Icon name={node.isDirectory ? (expanded ? 'folderFill' : 'folder') : 'file'} size={12} />
        </span>
        <MiddleText text={node.name} grow className={cx(styles.name, statusCode === 'D' && styles.deleted)} style={tint ? { color: tint } : undefined} />
        <span className={styles.rowSpacer} />
        {statusCode !== null ? (
          <span className={styles.code} style={{ color: tint }} aria-hidden="true">
            {statusCode === '?' ? 'U' : statusCode}
          </span>
        ) : containsChanges ? (
          <span className={styles.dot} aria-hidden="true" />
        ) : null}
      </button>
    </ContextMenu>
  )
}, sameRow)
