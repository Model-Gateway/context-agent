// Port of apps/macos/ContextAgent/Views/Remote/RemoteFolderPicker.swift: the
// remote folder picker (Welcome "Open on a remote host…", the sidebar host
// rows, mod+shift+O): the stand-in for the directory dialog when the folder
// is on another machine. Picks a host, connects to it, and browses its
// directories through `fs/list` — a path field (`~` works), the parent
// folder, repositories marked with a branch glyph — until Open.

import { useCallback, useEffect, useRef, useState } from 'react'
import { errorText } from '../../backend/errors'
import type { FsEntry, FsListing } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { Sheet } from '../../components/Sheet'
import { Tooltip } from '../../components/Tooltip'
import { hostDisplayName, useSSHHosts, type SSHHost } from '../../models/sshHost'
import { remoteConnection, SETTINGS_TAB_REMOTE_HOSTS, useApp } from '../../state/app'
import type { RemotePickerRequest } from '../../state/remote'
import { Icon } from '../../theme/icons'
import { PopUpButton, type PopUpOption } from '../Settings/SettingsControls'
import styles from './RemoteFolderPicker.module.css'

/** Mounted once in MainWindow: shows the picker while `remotePicker` is set. */
export function RemoteFolderPicker() {
  const request = useApp((state) => state.remotePicker)
  if (!request) return null
  return <PickerSheet key={request.id} request={request} />
}

function PickerSheet({ request }: { request: RemotePickerRequest }) {
  const hosts = useSSHHosts()
  const [host, setHost] = useState<SSHHost | null>(request.host)
  const [pathField, setPathField] = useState('')
  const [listing, setListing] = useState<FsListing | null>(null)
  const [selected, setSelected] = useState<FsEntry | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showHidden, setShowHidden] = useState(false)
  const [opening, setOpening] = useState(false)
  const openButtonRef = useRef<HTMLButtonElement>(null)
  // The latest load wins: a slow listing never replaces a newer one.
  const loadCounter = useRef(0)

  const close = () => useApp.getState().setRemotePicker(null)

  const load = useCallback(
    async (target: string | null, on: SSHHost | null = host) => {
      if (!on) return
      const ticket = ++loadCounter.current
      setLoading(true)
      setError(null)
      try {
        const result = await useApp.getState().listRemoteDirectory(target, on)
        if (ticket !== loadCounter.current) return
        setListing(result)
        setSelected(null)
        setPathField(result.path)
      } catch (failure) {
        if (ticket !== loadCounter.current) return
        setError(errorText(failure))
      } finally {
        if (ticket === loadCounter.current) setLoading(false)
      }
    },
    [host],
  )

  // The first listing of the host the picker opened on (once per request:
  // the sheet is keyed by the request id).
  const initialHost = useRef(request.host)
  useEffect(() => {
    const first = initialHost.current
    initialHost.current = null
    if (first) void load(null, first)
  }, [load])

  const chooseHost = (id: string) => {
    const next = hosts.find((candidate) => candidate.id === id) ?? null
    setHost(next)
    setListing(null)
    setSelected(null)
    setError(null)
    if (next) void load(null, next)
  }

  /** The selected folder, else the folder being browsed. */
  const openTarget = (current: FsListing): { path: string; isGit: boolean } => {
    if (selected) return { path: selected.path, isGit: selected.is_git }
    return { path: current.path, isGit: current.entries.some((entry) => entry.name === '.git') }
  }

  const open = async () => {
    if (!host || !listing) return
    setOpening(true)
    try {
      const project = await useApp.getState().openProject(openTarget(listing).path, { host, select: true, restoreSession: false })
      if (project) close()
    } finally {
      setOpening(false)
    }
  }

  const connecting = host !== null && remoteConnection(host.id)?.isReady !== true
  const hostOptions: PopUpOption<string>[] = [{ value: '', label: 'Choose a host…' }, ...hosts.map((candidate) => ({ value: candidate.id, label: hostDisplayName(candidate) }))]
  const target = listing ? openTarget(listing) : null
  const openLabel = target ? `Open ${lastComponent(target.path) || ''}`.trim() : 'Open'
  const entries = listing ? listing.entries.filter((entry) => entry.is_dir && (showHidden || !entry.is_hidden)) : []

  return (
    <Sheet
      open
      onClose={close}
      width={620}
      initialFocus={openButtonRef}
      aria-label="Open a project on a remote host"
      className={styles.sheet}
      footer={
        <>
          {target ? (
            <span className={styles.footerPath}>
              <span className={`selectable ${styles.footerPathText}`}>{target.path}</span>
              {target.isGit ? <Chip icon="branch">git</Chip> : null}
            </span>
          ) : null}
          <span className={styles.footerSpacer} />
          <Button variant="bordered" onClick={close}>
            Cancel
          </Button>
          <Button ref={openButtonRef} variant="prominent" disabled={!listing || opening} loading={opening} onClick={() => void open()}>
            {openLabel}
          </Button>
        </>
      }
    >
      <div className={styles.header}>
        <div className={styles.titleRow}>
          <Icon name="remoteHost" size={16} strokeWidth={2} className={styles.titleIcon} />
          <h2 className={styles.title}>Open a project on a remote host</h2>
          <span className={styles.footerSpacer} />
          <Tooltip content="Hosts are configured in Settings → Remote hosts">
            <span>
              <PopUpButton value={host?.id ?? ''} options={hostOptions} onChange={chooseHost} aria-label="Host" maxWidth={200} />
            </span>
          </Tooltip>
        </div>
        <div className={styles.pathRow}>
          <IconButton icon="send" label="Parent folder" size={11} disabled={!listing?.parent || loading} onClick={() => void load(listing?.parent ?? null)} />
          <input
            className={styles.pathField}
            value={pathField}
            placeholder="Path on the host (~ for home)"
            aria-label="Path on the host"
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            disabled={host === null}
            onChange={(event) => setPathField(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
                event.preventDefault()
                void load(pathField)
              }
            }}
          />
          <Button size="small" disabled={host === null || loading} onClick={() => void load(pathField)}>
            Go
          </Button>
          <IconButton icon="home" label="Home folder" size={11} disabled={!listing || loading} onClick={() => void load(listing?.home ?? null)} />
          <IconButton icon="showHidden" label={showHidden ? 'Hide dotfolders' : 'Show dotfolders'} size={11} isActive={showHidden} onClick={() => setShowHidden((shown) => !shown)} />
        </div>
      </div>
      <div className={styles.content}>
        {host === null ? (
          <EmptyPlaceholder
            icon="remoteHost"
            title="Choose a host"
            subtitle="Pick one of the configured hosts, or add one in Settings → Remote hosts."
            actionTitle="Open Settings"
            onAction={() => {
              close()
              useApp.getState().openSettings(SETTINGS_TAB_REMOTE_HOSTS)
            }}
          />
        ) : error !== null ? (
          <EmptyPlaceholder icon="warning" title="Could not list the folder" subtitle={error} actionTitle="Try again" onAction={() => void load(listing?.path ?? pathField)} />
        ) : listing ? (
          <div className={styles.list} role="listbox" aria-label="Folders">
            {entries.length === 0 ? <p className={styles.noFolders}>No folders here.</p> : null}
            {entries.map((entry) => (
              <FolderRow key={entry.path} entry={entry} selected={selected?.path === entry.path} onSelect={() => setSelected(entry)} onOpen={() => void load(entry.path)} />
            ))}
            {loading ? (
              <span className={styles.loadingOverlay}>
                <BrailleSpinner size={11} />
              </span>
            ) : null}
          </div>
        ) : (
          <div className={styles.connecting}>
            <BrailleSpinner size={14} />
            <span>{connecting ? `Connecting to ${hostDisplayName(host)}…` : 'Listing…'}</span>
          </div>
        )}
      </div>
    </Sheet>
  )
}

function lastComponent(path: string): string {
  const parts = path.split('/').filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? ''
}

/** `FolderRow`: folder glyph (accent, tertiary for dotfolders), name, a branch glyph for repositories; double-click enters. */
function FolderRow({ entry, selected, onSelect, onOpen }: { entry: FsEntry; selected: boolean; onSelect: () => void; onOpen: () => void }) {
  const [hovering, setHovering] = useState(false)
  return (
    <div
      role="option"
      tabIndex={0}
      aria-selected={selected}
      aria-label={`Folder ${entry.name}${entry.is_git ? ', Git repository' : ''}`}
      className={[styles.folderRow, selected ? styles.folderSelected : '', entry.is_hidden ? styles.folderHidden : ''].filter(Boolean).join(' ')}
      onClick={onSelect}
      onDoubleClick={onOpen}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault()
          onOpen()
        } else if (event.key === ' ') {
          event.preventDefault()
          onSelect()
        }
      }}
      onPointerEnter={() => setHovering(true)}
      onPointerLeave={() => setHovering(false)}
    >
      <span className={styles.folderIcon} aria-hidden="true">
        <Icon name={selected ? 'folderFill' : 'folder'} size={12} />
      </span>
      <span className={styles.folderName}>{entry.name}</span>
      {entry.is_git ? (
        <Tooltip content="A Git repository">
          <span className={styles.gitGlyph}>
            <Icon name="branch" size={10} strokeWidth={2} />
          </span>
        </Tooltip>
      ) : null}
      <span className={styles.footerSpacer} />
      <span className={[styles.chevron, hovering ? styles.chevronShown : ''].filter(Boolean).join(' ')} aria-hidden="true">
        <Icon name="chevronRight" size={9} strokeWidth={2.5} />
      </span>
    </div>
  )
}
