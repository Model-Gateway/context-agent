import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { BrailleSpinner, useReducedMotion } from '../../components/BrailleSpinner'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { ContextMenu } from '../../components/ContextMenu'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import type { MenuItem } from '../../components/Menu'
import { Tooltip } from '../../components/Tooltip'
import { modLabel } from '../../components/platform'
import { languageDisplayName } from '../../markdown/highlighter'
import { byteCount } from '../../models/attachments'
import { Icon, type IconName } from '../../theme/icons'
import { useApp, useProjectValue } from '../../state/app'
import {
  closeDocument,
  closeDocuments,
  documentFilePath,
  editors,
  load,
  openInEditor,
  refreshCleanDocuments,
  save,
  showFilesTab,
  useDocumentValue,
  useEditorDocument,
  useEditorWorkspace,
  type EditorDocument,
  type EditorWorkspace,
} from '../../state/editor'
import type { ProjectState } from '../../state/project'
import { projectById } from '../../state/selectors'
import { CodeEditor, preloadLanguage } from './CodeEditor'
import { symbolKindIcon } from './CompletionPanel'
import { BinaryDocumentView, ImageDocumentView, PDFDocumentView } from './MediaViews'
import styles from './EditorView.module.css'

// Port of apps/macos/ContextAgent/Views/Editor/EditorView.swift.

/**
 * `EditorView` (Views/Editor/EditorView.swift): the Editor tab (⌘4 /
 * Ctrl+4): the selected project's open documents as a tab strip over the
 * selected one's `CodeEditor`, with a status bar (language, cursor, the
 * symbol under the cursor, save state). Files arrive from the Inspector
 * Files tab and the "Open in Editor" items of every file menu; images and
 * PDFs get viewers instead of the text view.
 */
export function EditorView() {
  // The project object only (its version bumps on every chat event).
  const project = useApp((state) => projectById(state, state.selectedProjectId))
  if (!project) {
    return (
      <div className={styles.root}>
        <EmptyPlaceholder icon="folder" title="No project open" subtitle="Open a project to edit its files." />
      </div>
    )
  }
  return <EditorPane key={project.id} project={project} />
}

/** `EditorPane`: the empty state or the strip and the selected document. */
function EditorPane({ project }: { project: ProjectState }) {
  const workspace = editors(project)
  const snapshot = useEditorWorkspace(workspace)
  const gitStatus = useProjectValue(project, (p) => p.gitStatus, null)
  const lastStatus = useRef<string | null>(null)

  // `.onChange(of: project.gitStatus)`: the agent (or a checkout) changed
  // files: clean documents follow the disk.
  // Also once when the tab appears: the Editor is not mounted while another
  // tab shows, so an edit the agent made meanwhile is caught up here (only
  // documents whose file hash changed are re-read).
  useEffect(() => {
    const json = JSON.stringify(gitStatus)
    if (json === lastStatus.current) return
    lastStatus.current = json
    refreshCleanDocuments(project)
  }, [gitStatus, project])

  const documents = snapshot?.documents ?? []
  const selected = snapshot?.selected ?? null

  return (
    <div className={styles.root} role="group" aria-label="Editor">
      {documents.length === 0 ? (
        <EmptyPlaceholder
          icon="editor"
          title="No file open"
          subtitle="Pick a file in the Inspector Files tab, or choose Open in Editor on a file in a chat or in Changes. Completions come from the project's index."
          actionTitle="Show Files"
          onAction={showFilesTab}
        />
      ) : (
        <>
          <EditorTabStrip project={project} workspace={workspace} documents={documents} selectedPath={snapshot?.selectedPath ?? null} />
          {selected ? <DocumentPane key={selected.path} project={project} document={selected} /> : null}
        </>
      )}
    </div>
  )
}

// MARK: - Tab strip

interface EditorTabStripProps {
  project: ProjectState
  workspace: EditorWorkspace
  documents: readonly EditorDocument[]
  selectedPath: string | null
}

/** `EditorTabStrip`: the open documents in a horizontally scrolling row on `surface`, hairline below. */
function EditorTabStrip({ project, workspace, documents, selectedPath }: EditorTabStripProps) {
  const stripRef = useRef<HTMLDivElement>(null)
  const reduceMotion = useReducedMotion()
  const documentsRef = useRef(documents)
  documentsRef.current = documents
  const previousSelection = useRef(selectedPath)

  // `.onChange(of: workspace.selectedPath)`: scroll the chip into view (0.2 s ease-out).
  useEffect(() => {
    if (previousSelection.current === selectedPath) return
    previousSelection.current = selectedPath
    if (selectedPath === null) return
    const chip = stripRef.current?.querySelector<HTMLElement>(`[data-path="${CSS.escape(selectedPath)}"]`)
    chip?.scrollIntoView({ behavior: reduceMotion ? 'auto' : 'smooth', block: 'nearest', inline: 'nearest' })
  }, [selectedPath, reduceMotion])

  // The strip narrows (the inspector opens, the window shrinks): keep the
  // selected chip in view rather than leaving it scrolled off the end.
  const selectedRef = useRef(selectedPath)
  selectedRef.current = selectedPath
  useEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    let width = strip.clientWidth
    const observer = new ResizeObserver(() => {
      if (strip.clientWidth >= width) {
        width = strip.clientWidth
        return
      }
      width = strip.clientWidth
      const path = selectedRef.current
      if (path === null) return
      const chip = strip.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)
      if (!chip) return
      const bounds = strip.getBoundingClientRect()
      const box = chip.getBoundingClientRect()
      if (box.left < bounds.left) strip.scrollLeft -= bounds.left - box.left + 8
      else if (box.right > bounds.right) strip.scrollLeft += box.right - bounds.right + 8
    })
    observer.observe(strip)
    return () => observer.disconnect()
  }, [])

  const select = useCallback((document: EditorDocument) => workspace.select(document.path), [workspace])
  const close = useCallback((document: EditorDocument) => void closeDocument(document, project), [project])
  const closeOthers = useCallback(
    (document: EditorDocument) => {
      const others = documentsRef.current.filter((candidate) => candidate.path !== document.path)
      void closeDocuments(others, document, project)
    },
    [project],
  )
  const closeToTheRight = useCallback(
    (document: EditorDocument) => {
      const list = documentsRef.current
      const index = list.findIndex((candidate) => candidate.path === document.path)
      void closeDocuments(list.slice(index + 1), document, project)
    },
    [project],
  )

  return (
    <div ref={stripRef} className={`${styles.strip} hairline-bottom`} role="tablist" aria-label="Open files">
      <div className={styles.stripContent}>
        {documents.map((document, index) => (
          <EditorTabChip
            key={document.path}
            document={document}
            selected={document.path === selectedPath}
            canCloseOthers={documents.length > 1}
            canCloseToTheRight={index < documents.length - 1}
            onSelect={select}
            onClose={close}
            onCloseOthers={closeOthers}
            onCloseToTheRight={closeToTheRight}
          />
        ))}
      </div>
    </div>
  )
}

interface EditorTabChipProps {
  document: EditorDocument
  selected: boolean
  /** False when there is nothing to close (the item is then disabled). */
  canCloseOthers: boolean
  canCloseToTheRight: boolean
  onSelect: (document: EditorDocument) => void
  onClose: (document: EditorDocument) => void
  onCloseOthers: (document: EditorDocument) => void
  onCloseToTheRight: (document: EditorDocument) => void
}

/// `EditorTabChip`: file name with a dot while unsaved; the close button
/// shows on hover, the context menu closes it, the others or the tabs to
/// its right.
const EditorTabChip = memo(function EditorTabChip({
  document,
  selected,
  canCloseOthers,
  canCloseToTheRight,
  onSelect,
  onClose,
  onCloseOthers,
  onCloseToTheRight,
}: EditorTabChipProps) {
  const isDirty = useDocumentValue(document, (d) => d.isDirty, false)
  const [hovering, setHovering] = useState(false)
  const [overClose, setOverClose] = useState(false)

  const items = (): MenuItem[] => [
    { label: 'Close', onSelect: () => onClose(document) },
    { label: 'Close Others', onSelect: () => onCloseOthers(document), disabled: !canCloseOthers },
    { label: 'Close Tabs to the Right', onSelect: () => onCloseToTheRight(document), disabled: !canCloseToTheRight },
  ]

  return (
    <ContextMenu items={items}>
      {/* Help tags under the strip (as AppKit shows them below the pointer), not over the top bar. */}
      <Tooltip content={document.path} disabled={overClose} placement="bottom">
        <div
          role="tab"
          aria-selected={selected}
          aria-label={`${document.name}${isDirty ? ', unsaved' : ''}`}
          tabIndex={0}
          data-path={document.path}
          className={[styles.chip, selected ? styles.chipSelected : ''].filter(Boolean).join(' ')}
          onClick={() => onSelect(document)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault()
              onSelect(document)
            }
          }}
          onPointerEnter={() => setHovering(true)}
          onPointerLeave={() => {
            setHovering(false)
            setOverClose(false)
          }}
        >
          <Icon name="file" size={11} className={styles.chipIcon} />
          <span className={styles.chipName}>{document.name}</span>
          <span className={styles.chipAccessory}>
            {hovering ? (
              <Tooltip content={isDirty ? 'Close (unsaved changes)' : 'Close'} placement="bottom">
                <button
                  type="button"
                  className={styles.chipClose}
                  aria-label={`Close ${document.name}`}
                  onClick={(event) => {
                    event.stopPropagation()
                    onClose(document)
                  }}
                  onPointerEnter={() => setOverClose(true)}
                  onPointerLeave={() => setOverClose(false)}
                >
                  <Icon name="close" size={9} strokeWidth={3} />
                </button>
              </Tooltip>
            ) : isDirty ? (
              <Tooltip content="Unsaved changes" placement="bottom">
                <span className={styles.dirtyDot} role="img" aria-label="Unsaved changes" />
              </Tooltip>
            ) : null}
          </span>
        </div>
      </Tooltip>
    </ContextMenu>
  )
})

// MARK: - Document

/** `DocumentPane`: the viewer for the document's kind. */
function DocumentPane({ project, document }: { project: ProjectState; document: EditorDocument }) {
  const kind = useDocumentValue(document, (d) => d.kind, 'text')
  return (
    <div className={styles.document}>
      {kind === 'image' ? (
        <ImageDocumentView path={documentFilePath(document, project)} />
      ) : kind === 'pdf' ? (
        <PDFDocumentView path={documentFilePath(document, project)} />
      ) : kind === 'binary' ? (
        <BinaryDocumentView project={project} document={document} />
      ) : (
        <TextContent project={project} document={document} />
      )}
    </div>
  )
}

/** `DocumentPane.textContent`: the load error, "Opening…", or banners · editor · status bar. */
function TextContent({ project, document }: { project: ProjectState; document: EditorDocument }) {
  const loadError = useDocumentValue(document, (d) => d.loadError, null)
  const loading = useDocumentValue(document, (d) => d.loading, false)
  // Fetch the language package while `file/read` runs, so the text is
  // coloured from its first frame.
  useEffect(() => preloadLanguage(document.path, document.language), [document])
  if (loadError !== null) {
    return (
      <EmptyPlaceholder icon="warning" title={`Could not open ${document.name}`} subtitle={loadError} actionTitle="Try again" onAction={() => void load(document, project)} />
    )
  }
  if (loading) {
    return (
      <div className={styles.opening}>
        <BrailleSpinner size={14} tint="var(--text-tertiary)" />
        <span className={styles.openingText}>{`Opening ${document.name}…`}</span>
      </div>
    )
  }
  return (
    <>
      <EditorBanners project={project} document={document} />
      <CodeEditor project={project} document={document} />
      <EditorStatusBar project={project} document={document} />
    </>
  )
}

/** `DocumentPane.banners`: conflict (Reload · Overwrite) and truncated (read-only). */
function EditorBanners({ project, document }: { project: ProjectState; document: EditorDocument }) {
  const conflict = useDocumentValue(document, (d) => d.conflict, false)
  const truncated = useDocumentValue(document, (d) => d.truncated, false)
  const revision = useDocumentValue(document, (d) => d.revision, 0)
  const shownSize = useMemo(
    () => (truncated ? byteCount(new TextEncoder().encode(document.text).length) : ''),
    // `revision` marks the text being replaced; a truncated buffer is read-only otherwise.
    [truncated, document, revision],
  )
  return (
    <>
      {conflict ? (
        <EditorBanner
          icon="warning"
          tint="var(--warning)"
          text={`${document.name} changed on disk since you opened it. Reload it (your edits are lost) or overwrite it.`}
        >
          <Button variant="bordered" size="small" onClick={() => void load(document, project)}>
            Reload
          </Button>
          <Button variant="bordered" size="small" onClick={() => void save(document, project, true)}>
            Overwrite
          </Button>
        </EditorBanner>
      ) : null}
      {truncated ? (
        <EditorBanner icon="info" tint="var(--text-secondary)" text={`Only the first ${shownSize} of the file are shown; it is read-only here.`}>
          <Button variant="bordered" size="small" onClick={() => useApp.getState().reveal(document.path, project, 'openWithEditor')}>
            Open with Default Editor
          </Button>
        </EditorBanner>
      ) : null}
    </>
  )
}

/** `EditorBanner`: icon · two lines of text · actions, on the tint at 8 %, hairline below. */
function EditorBanner({ icon, tint, text, children }: { icon: IconName; tint: string; text: string; children: ReactNode }) {
  return (
    <div className={`${styles.banner} hairline-bottom`} style={{ '--banner-tint': tint } as CSSProperties}>
      <Icon name={icon} size={12} strokeWidth={2} className={styles.bannerIcon} />
      <span className={styles.bannerText}>{text}</span>
      <span className={styles.spacer} />
      <span className={styles.bannerActions}>{children}</span>
    </div>
  )
}

// MARK: - Status bar

/// `EditorStatusBar`: language (with the language server's name when one
/// runs) · Ln/Col · diagnostics counts · the symbol under the cursor (the
/// server's hover, or the index's definition — click jumps to it) · save
/// state.
function EditorStatusBar({ project, document }: { project: ProjectState; document: EditorDocument }) {
  useEditorDocument(document)
  const { lspServer, cursorLine, cursorColumn, errorCount, warningCount, cursorHover, cursorSymbol, saving, isDirty } = document
  const language = document.languageName !== null && document.languageName.length > 0
    ? document.languageName.charAt(0).toUpperCase() + document.languageName.slice(1)
    : languageDisplayName(document.language)
  const languageLabel = lspServer !== null ? `${language} · ${lastPathComponent(lspServer)}` : language
  const summary = diagnosticsSummary(errorCount, warningCount)

  return (
    <div className={`${styles.statusBar} hairline-top`} role="group" aria-label="Editor status">
      <Tooltip content={lspServer !== null ? `Completion, hover and diagnostics from ${lspServer}` : "Completion from the project's index"}>
        <span className={styles.statusSecondary}>{languageLabel}</span>
      </Tooltip>
      <span className={styles.statusSecondary}>{`Ln ${cursorLine}, Col ${cursorColumn}`}</span>
      {errorCount > 0 || warningCount > 0 ? (
        <Tooltip content={summary}>
          <span className={styles.diagnostics} role="img" aria-label={summary}>
            {errorCount > 0 ? (
              <span className={styles.errorCount}>
                <Icon name="failedFill" size={11} />
                {errorCount}
              </span>
            ) : null}
            {warningCount > 0 ? (
              <span className={styles.warningCount}>
                <Icon name="warningFill" size={11} />
                {warningCount}
              </span>
            ) : null}
          </span>
        </Tooltip>
      ) : null}
      {cursorHover !== null ? (
        <Tooltip content={cursorHover}>
          <span className={styles.symbol}>
            <Icon name="info" size={10} strokeWidth={2} className={styles.symbolIcon} />
            <MiddleTruncate natural text={cursorHover} className={styles.symbolText} />
          </span>
        </Tooltip>
      ) : cursorSymbol !== null ? (
        <Tooltip content={`Go to the definition of ${cursorSymbol.name} (also ${modLabel}-click it)`}>
          <button type="button" className={styles.symbol} onClick={() => openInEditor(cursorSymbol.path, project, cursorSymbol.line)}>
            <Icon name={symbolKindIcon(cursorSymbol.kind)} size={10} strokeWidth={2} className={styles.symbolIcon} />
            <MiddleTruncate natural text={cursorSymbol.signature ?? `${cursorSymbol.kind} ${cursorSymbol.name}`} className={styles.symbolText} />
            <span className={styles.symbolPath}>
              <bdi>{`${cursorSymbol.path}:${cursorSymbol.line}`}</bdi>
            </span>
          </button>
        </Tooltip>
      ) : null}
      <span className={styles.spacer} />
      {saving ? (
        <BrailleSpinner size={10} tint="var(--text-tertiary)" />
      ) : isDirty ? (
        <>
          <Chip variant="tinted" tint="var(--warning)" className={styles.statusChip}>
            unsaved
          </Chip>
          <Tooltip content="Save" shortcut="mod+S">
            <Button variant="bordered" size="small" className={styles.statusButton} onClick={() => void save(document, project)}>
              Save
            </Button>
          </Tooltip>
        </>
      ) : (
        <span className={styles.statusTertiary}>Saved</span>
      )}
    </div>
  )
}

/** `diagnosticsSummary`: "2 errors, 1 warning". */
function diagnosticsSummary(errors: number, warnings: number): string {
  const parts: string[] = []
  if (errors > 0) parts.push(`${errors} error${errors === 1 ? '' : 's'}`)
  if (warnings > 0) parts.push(`${warnings} warning${warnings === 1 ? '' : 's'}`)
  return parts.join(', ')
}

function lastPathComponent(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? path
}
