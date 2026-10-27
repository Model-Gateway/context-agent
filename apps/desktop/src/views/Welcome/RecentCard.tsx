import { memo, useEffect, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { IconButton } from '../../components/IconButton'
import type { MenuItem } from '../../components/Menu'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { revealLabel } from '../../components/platform'
import { ProgressRing } from '../../components/ProgressRing'
import { Tooltip } from '../../components/Tooltip'
import { usePref } from '../../models/preferences'
import { abbreviatedPath, RecentProjects, recentProjectExists, type RecentProject } from '../../models/recentProject'
import { TranscriptFormat } from '../../models/transcript'
import { useApp } from '../../state/app'
import { Icon } from '../../theme/icons'
import { invoke } from '@tauri-apps/api/core'
import { toasts } from '../../components/toasts'
import { errorText } from '../../backend/errors'
import styles from './RecentCard.module.css'

export interface RecentCardProps {
  recent: RecentProject
  /** The project is open in the app ("Open" chip). */
  isOpen: boolean
}

/// `RecentProject.exists` answers synchronously in Swift; here the folder
/// check (`fs_file_info`) is asynchronous, so answers are cached per path
/// and a card counts as existing until the check says otherwise.
/// `checkRecentFolders` fills the cache at launch, so the Welcome cards
/// usually draw their "Missing" state on their first frame.
const existence = new Map<string, boolean>()

/** Checks the given folders now and caches the answers for the cards. */
export function checkRecentFolders(paths: readonly string[]): void {
  for (const path of paths) {
    if (existence.has(path)) continue
    void recentProjectExists({ path, name: '', lastOpened: new Date(0), pinned: false }).then((value) => existence.set(path, value))
  }
}

function useFolderExists(path: string): boolean {
  const [exists, setExists] = useState(() => existence.get(path) ?? true)
  useEffect(() => {
    let cancelled = false
    setExists(existence.get(path) ?? true)
    void recentProjectExists({ path, name: '', lastOpened: new Date(0), pinned: false }).then((value) => {
      existence.set(path, value)
      if (!cancelled) setExists(value)
    })
    return () => {
      cancelled = true
    }
  }, [path])
  return exists
}

/** Shows the folder in Finder / Explorer. */
function revealFolder(path: string): void {
  void invoke<void>('reveal_in_file_manager', { path }).catch((error: unknown) => {
    toasts.push({ message: `Could not show ${path}: ${errorText(error)}`, level: 'error' })
  })
}

/** "Indexing 42%", or "Indexing…" until the pass knows its total. */
function indexingLabel(fraction: number | null): string {
  return fraction === null ? 'Indexing…' : `Indexing ${Math.floor(fraction * 100)}%`
}

/**
 * `RecentCard` (Views/Welcome/WelcomeView.swift), shared with the Projects
 * screen: a recent project as a card with its name, abbreviated path and
 * last-opened time; pinned ones carry a pin; hover reveals Remove; the
 * footer says "Opening…", "Indexing 42%", "Open" or "Missing", and while
 * the project opens and builds its index a progress ring takes the
 * folder's place; the context menu has Open, Pin / Unpin, Reveal in Finder,
 * Copy Path and Remove from Recents.
 */
export const RecentCard = memo(function RecentCard({ recent, isOpen }: RecentCardProps) {
  const isOpening = useApp((state) => state.openingProjectPaths.includes(recent.path))
  const ready = useApp((state) => state.connection.kind === 'ready')
  // The open project while its index pass runs; on a first open that pass
  // builds the whole index. `undefined` when it is not indexing.
  const indexFraction = useApp((state) => {
    if (!isOpen) return undefined
    const project = state.projects.find((candidate) => candidate.root === recent.path)
    return project?.isIndexing ? project.indexFraction : undefined
  })
  const indexing = indexFraction !== undefined
  // False for a folder being opened for the first time (not a recent yet): nothing to pin or remove.
  const isRemembered = usePref('recents').some((item) => item.path === recent.path)
  const exists = useFolderExists(recent.path)
  const [hovering, setHovering] = useState(false)
  /** The pointer is over a control with a tooltip of its own: the card's stands down. */
  const [overControl, setOverControl] = useState(false)
  const canOpen = ready && exists && !isOpening
  const path = abbreviatedPath(recent.path)

  const open = () => {
    if (!canOpen) return
    void useApp.getState().openProject(recent.path)
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      open()
    }
  }

  const remove = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation()
    RecentProjects.remove(recent.path)
  }

  const menu = (): MenuItem[] => [
    { label: 'Open', disabled: !canOpen, onSelect: () => void useApp.getState().openProject(recent.path) },
    { label: recent.pinned ? 'Unpin' : 'Pin', disabled: !isRemembered, onSelect: () => RecentProjects.togglePin(recent.path) },
    { label: revealLabel, disabled: !exists, onSelect: () => revealFolder(recent.path) },
    { label: 'Copy Path', onSelect: () => useApp.getState().copyToPasteboard(recent.path) },
    { type: 'separator' },
    { label: 'Remove from Recents', destructive: true, disabled: !isRemembered, onSelect: () => RecentProjects.remove(recent.path) },
  ]

  const status = isOpening ? ', opening' : indexing ? `, ${indexingLabel(indexFraction).toLowerCase()}` : isOpen ? ', open' : ''
  const active = hovering && canOpen
  const showRemove = hovering && isRemembered

  return (
    <ContextMenu items={menu}>
      <Tooltip content={exists ? recent.path : `Missing: ${recent.path}`} placement="bottom" disabled={overControl}>
        <div
          role="button"
          tabIndex={0}
          aria-disabled={!canOpen || undefined}
          aria-label={`${recent.name}, ${path}${recent.pinned ? ', pinned' : ''}${status}`}
          className={[styles.card, active ? styles.active : ''].filter(Boolean).join(' ')}
          onClick={open}
          onKeyDown={onKeyDown}
          onPointerEnter={() => setHovering(true)}
          onPointerLeave={() => {
            setHovering(false)
            setOverControl(false)
          }}
        >
          <div className={styles.titleRow}>
            {/* While the project opens and builds its index the folder gives way to the progress ring. */}
            {isOpening || indexing ? (
              <span className={styles.ringSlot} aria-hidden>
                <ProgressRing fraction={isOpening ? null : (indexFraction ?? null)} />
              </span>
            ) : (
              <Icon name="folder" size={15} strokeWidth={1.85} className={[styles.folder, exists ? '' : styles.missingFolder].filter(Boolean).join(' ')} />
            )}
            <MiddleTruncate text={recent.name} className={styles.name} />
            {recent.pinned ? <Icon name="pinned" size={10} strokeWidth={2} className={styles.pin} aria-label="Pinned" /> : null}
            {/* Always laid out, shown on hover and held to the row's 20 px, so hovering never resizes the card. */}
            <span
              className={[styles.remove, showRemove ? styles.visible : ''].filter(Boolean).join(' ')}
              aria-hidden={!showRemove || undefined}
              onPointerEnter={() => setOverControl(true)}
              onPointerLeave={() => setOverControl(false)}
            >
              <IconButton icon="close" label="Remove from recents" size={11} tabIndex={showRemove ? 0 : -1} onClick={remove} />
            </span>
          </div>
          <MiddleTruncate text={path} className={styles.path} />
          <div className={styles.footer}>
            <span className={styles.opened}>Opened {TranscriptFormat.relative(recent.lastOpened)}</span>
            <span className={styles.spacer} />
            {isOpening ? (
              <span className={styles.busy}>Opening…</span>
            ) : indexing ? (
              <span className={styles.busy}>{indexingLabel(indexFraction)}</span>
            ) : isOpen ? (
              <Chip variant="tinted" tint="var(--success)">
                Open
              </Chip>
            ) : !exists ? (
              <span className={styles.chipSlot} onPointerEnter={() => setOverControl(true)} onPointerLeave={() => setOverControl(false)}>
                <Tooltip content="The folder no longer exists at this path">
                  <Chip variant="tinted" tint="var(--warning)">
                    Missing
                  </Chip>
                </Tooltip>
              </span>
            ) : null}
          </div>
        </div>
      </Tooltip>
    </ContextMenu>
  )
})
