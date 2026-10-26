// Port of apps/macos/ContextAgent/App/EditorState.swift: the Editor tab's
// state, one `EditorWorkspace` per project (the documents open in its tab
// strip) and the `AppState` actions that load, save and close them through
// `file/read` / `file/write`, keep the project's language server in step
// with the buffer (`lsp/*`) and look up symbols in the index
// (`symbols/find`) for the editor's completion and go-to-definition.
//
// `EditorDocument` and `EditorWorkspace` are plain classes mutated in place,
// like `ProjectState`: every change ends with `changed()` (or goes through
// `update(fn)`), which bumps the object's `version` and tells its
// subscribers; `useEditorWorkspace` / `useEditorDocument` /
// `useDocumentValue` subscribe a view. A document whose unsaved state flips
// also bumps the app store, so `useApp` selectors reading
// `existingEditor(id)?.unsavedCount` (the Editor tab's badge) follow.
//
// The module plugs into the app store through `extendApp` (`lsp/diagnostics`
// routing and `PathAction.openInEditor`) and `onProjectClosed`
// (`editors.removeValue(forKey:)`), as the Foundation notes say.
//
// `Models/EditorApp.swift` (the installed-editors list behind Settings →
// General "Default editor") is AppKit only; the preference it fills,
// `defaultEditorPath`, is read by `reveal(…, 'openWithEditor')` in state/app.ts.

import { useCallback, useSyncExternalStore } from 'react'
import { backend } from '../backend/client'
import { BackendError, errorText } from '../backend/errors'
import { debug } from '../backend/log'
import {
  lspDiagnosticIsError,
  lspDiagnosticIsWarning,
  type FileContent,
  type LspCompletionItem,
  type LspDiagnostic,
  type LspDiagnostics,
  type SymbolInfo,
  type SymbolsFindMode,
} from '../backend/types'
import { toasts } from '../components/toasts'
import { detectLanguage, type Language } from '../markdown/highlighter'
import { getPref, setPref } from '../models/preferences'
import { askToSaveChanges } from '../views/Editor/SaveChangesDialog'
import { extendApp, onProjectClosed, relativePath, touch, useApp } from './app'
import type { ProjectState } from './project'
import { projectById, selectedProject } from './selectors'

// MARK: - EditorDocument

/// `EditorDocument.Kind`: what the Editor shows for the file: the text view,
/// an image, a PDF or a placeholder for other binary files.
export type EditorDocumentKind = 'text' | 'image' | 'pdf' | 'binary'

/** `EditorDocument.imageExtensions`. */
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'heic', 'heif', 'tiff', 'tif', 'bmp', 'svg', 'ico', 'icns'])

type Timer = ReturnType<typeof setTimeout>

/**
 * `EditorDocument` (App/EditorState.swift): one open file. `text` follows the
 * text view; `savedText` / `hash` are what the disk held at the last read or
 * write. Mutate through `update(fn)` (or follow a change with `changed()`).
 */
export class EditorDocument {
  /** Repository-relative, `/`-separated. */
  readonly path: string
  kind: EditorDocumentKind

  text = ''
  savedText = ''
  hash: string | null = null
  language: Language
  /** The registry's name ("rust", "markdown", …) for the status bar. */
  languageName: string | null = null
  loading = true
  loadError: string | null = null
  /** `file/read` returned only the first part: the document is read-only. */
  truncated = false
  saving = false
  /** The last save was refused because the file changed on disk. */
  conflict = false
  cursorLine = 1
  cursorColumn = 1
  /** Line to reveal once the text view exists (go to definition, open at line). */
  pendingLine: number | null = null
  /// Bumped whenever `text` was replaced from outside the text view (load,
  /// reload), so the view knows to take it over.
  revision = 0
  isDirty = false

  /** Symbol under the cursor from the index, for the status bar. */
  cursorSymbol: SymbolInfo | null = null
  /// What the language server says about the identifier under the cursor
  /// (its first line: the signature or type), preferred over `cursorSymbol`.
  cursorHover: string | null = null

  /** The language server's executable once `lsp/open` found one. */
  lspServer: string | null = null
  /** Characters after which the server wants a completion request. */
  lspTriggers: string[] = []
  /** Set by `lsp/diagnostics`; the text view underlines them. */
  diagnostics: LspDiagnostic[] = []
  /** The buffer changed since the server last saw it (not observed). */
  lspDirty = false
  /** The pending debounced `lsp/change` (not observed). */
  lspSync: Timer | null = null
  /** The pending "after a delay" auto save (not observed). */
  autoSave: Timer | null = null

  /** Bumped by every `changed()`. */
  version = 0
  private readonly listeners = new Set<() => void>()

  constructor(path: string) {
    this.path = path
    this.language = detectLanguage(path)
    const ext = pathExtension(path)
    if (IMAGE_EXTENSIONS.has(ext)) this.kind = 'image'
    else if (ext === 'pdf') this.kind = 'pdf'
    else this.kind = 'text'
  }

  /** `id`. */
  get id(): string {
    return this.path
  }

  /** The file name (`lastPathComponent`). */
  get name(): string {
    const slash = Math.max(this.path.lastIndexOf('/'), this.path.lastIndexOf('\\'))
    return slash < 0 ? this.path : this.path.slice(slash + 1)
  }

  get errorCount(): number {
    let count = 0
    for (const diagnostic of this.diagnostics) if (lspDiagnosticIsError(diagnostic)) count += 1
    return count
  }

  get warningCount(): number {
    let count = 0
    for (const diagnostic of this.diagnostics) if (lspDiagnosticIsWarning(diagnostic)) count += 1
    return count
  }

  /** The text view's edits (notifies). */
  edited(newText: string): void {
    this.text = newText
    this.isDirty = newText !== this.savedText
    this.lspDirty = true
    this.changed()
  }

  /** Content from disk replaces the buffer (call inside `update`). */
  load(content: FileContent): void {
    this.text = content.content
    this.savedText = content.content
    this.hash = content.hash ?? null
    this.truncated = content.truncated
    this.languageName = content.language ?? null
    if (content.language != null && detectLanguage(this.path) === 'plain') {
      this.language = detectLanguage(content.language)
    }
    this.loading = false
    this.loadError = null
    this.conflict = false
    this.isDirty = false
    this.lspDirty = true
    this.revision += 1
  }

  /** Runs `change` on the document, then notifies. */
  update(change: (document: EditorDocument) => void): void {
    change(this)
    this.changed()
  }

  /** Marks the document changed: bumps `version` and tells subscribers. */
  changed(): void {
    this.version += 1
    for (const listener of [...this.listeners]) listener()
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  /** Stops the pending `lsp/change` and auto save. */
  cancelTimers(): void {
    if (this.lspSync !== null) clearTimeout(this.lspSync)
    if (this.autoSave !== null) clearTimeout(this.autoSave)
    this.lspSync = null
    this.autoSave = null
  }
}

// MARK: - EditorWorkspace

/** What the tab strip reads (`useEditorWorkspace`). */
export interface EditorWorkspaceSnapshot {
  documents: readonly EditorDocument[]
  selectedPath: string | null
  selected: EditorDocument | null
  unsavedCount: number
  version: number
}

/** `EditorWorkspace` (App/EditorState.swift): the documents open in one project's Editor tab. */
export class EditorWorkspace {
  readonly projectId: string
  /** Replaced, never mutated in place. */
  documents: EditorDocument[] = []
  selectedPath: string | null = null

  private version = 0
  private readonly listeners = new Set<() => void>()
  private readonly documentSubscriptions = new Map<string, () => void>()
  private snapshotCache: EditorWorkspaceSnapshot | null = null

  constructor(projectId: string) {
    this.projectId = projectId
  }

  get selected(): EditorDocument | null {
    if (this.selectedPath === null) return null
    return this.document(this.selectedPath)
  }

  get unsavedCount(): number {
    let count = 0
    for (const document of this.documents) if (document.isDirty) count += 1
    return count
  }

  /** `document(for:)`. */
  document(path: string): EditorDocument | null {
    return this.documents.find((document) => document.path === path) ?? null
  }

  /** Selects the document, adding it (after the selected one) when new. */
  open(path: string): { document: EditorDocument; isNew: boolean } {
    const existing = this.document(path)
    if (existing) {
      this.select(path)
      return { document: existing, isNew: false }
    }
    const document = new EditorDocument(path)
    const index = this.selectedPath === null ? -1 : this.documents.findIndex((candidate) => candidate.path === this.selectedPath)
    if (index >= 0) this.documents = [...this.documents.slice(0, index + 1), document, ...this.documents.slice(index + 1)]
    else this.documents = [...this.documents, document]
    this.watch(document)
    this.selectedPath = path
    this.changed()
    return { document, isNew: true }
  }

  /** Drops the document; a neighbour takes the selection. */
  close(document: EditorDocument): void {
    const index = this.documents.findIndex((candidate) => candidate.path === document.path)
    if (index < 0) return
    const wasDirty = this.documents[index]?.isDirty ?? false
    this.documentSubscriptions.get(document.path)?.()
    this.documentSubscriptions.delete(document.path)
    const documents = this.documents.filter((_, i) => i !== index)
    this.documents = documents
    if (this.selectedPath === document.path) {
      this.selectedPath = documents[index]?.path ?? documents[documents.length - 1]?.path ?? null
    }
    this.changed()
    if (wasDirty) touch()
  }

  /** `selectedPath = path`. */
  select(path: string | null): void {
    if (this.selectedPath === path) return
    this.selectedPath = path
    this.changed()
  }

  /** Lets go of every document (the project closed). */
  dispose(): void {
    for (const document of this.documents) document.cancelTimers()
    for (const unsubscribe of this.documentSubscriptions.values()) unsubscribe()
    this.documentSubscriptions.clear()
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getSnapshot(): EditorWorkspaceSnapshot {
    if (!this.snapshotCache) {
      this.snapshotCache = {
        documents: this.documents,
        selectedPath: this.selectedPath,
        selected: this.selected,
        unsavedCount: this.unsavedCount,
        version: this.version,
      }
    }
    return this.snapshotCache
  }

  /// Follows a document's unsaved state: the strip's count and the app's
  /// Editor badge change only when it flips, not on every keystroke.
  private watch(document: EditorDocument): void {
    let dirty = document.isDirty
    this.documentSubscriptions.set(
      document.path,
      document.subscribe(() => {
        if (document.isDirty === dirty) return
        dirty = document.isDirty
        this.changed()
        touch()
      }),
    )
  }

  private changed(): void {
    this.version += 1
    this.snapshotCache = null
    for (const listener of [...this.listeners]) listener()
  }
}

// MARK: - Workspaces (`AppState.editors`)

/** projectId → the Editor tab's documents (`AppState.editors`). */
const workspaces = new Map<string, EditorWorkspace>()

/** `AppState.editor(for:)`: the project's editor workspace, created on first use. */
export function editors(project: ProjectState): EditorWorkspace {
  const existing = workspaces.get(project.id)
  if (existing) return existing
  const workspace = new EditorWorkspace(project.id)
  workspaces.set(project.id, workspace)
  // A view may ask for it while rendering: tell the store afterwards.
  queueMicrotask(() => touch())
  return workspace
}

/** `editors[projectId]` without creating one (the Editor tab's badge). */
export function existingEditor(projectId: string | null | undefined): EditorWorkspace | null {
  if (projectId == null) return null
  return workspaces.get(projectId) ?? null
}

// MARK: - Hooks

/** Re-renders with the workspace's documents and selection; null for no workspace. */
export function useEditorWorkspace(workspace: EditorWorkspace | null): EditorWorkspaceSnapshot | null {
  const subscribe = useCallback((listener: () => void) => (workspace ? workspace.subscribe(listener) : () => undefined), [workspace])
  return useSyncExternalStore(subscribe, () => (workspace ? workspace.getSnapshot() : null))
}

/** Re-renders whenever the document changes; returns its version (-1 for none). */
export function useEditorDocument(document: EditorDocument | null): number {
  const subscribe = useCallback((listener: () => void) => (document ? document.subscribe(listener) : () => undefined), [document])
  return useSyncExternalStore(subscribe, () => (document ? document.version : -1))
}

/// One value of `document`, re-rendering only when it changes: `select`
/// must return a primitive or a reference the document keeps.
export function useDocumentValue<T>(document: EditorDocument | null, select: (document: EditorDocument) => T, fallback: T): T {
  const subscribe = useCallback((listener: () => void) => (document ? document.subscribe(listener) : () => undefined), [document])
  return useSyncExternalStore(subscribe, () => (document ? select(document) : fallback))
}

/// `editors[project.id]?.unsavedCount ?? 0` (DetailView's Editor badge),
/// re-rendering when a document's unsaved state flips.
export function useUnsavedCount(project: ProjectState | null): number {
  const workspace = useApp(() => (project ? (workspaces.get(project.id) ?? null) : null))
  const subscribe = useCallback((listener: () => void) => (workspace ? workspace.subscribe(listener) : () => undefined), [workspace])
  return useSyncExternalStore(subscribe, () => workspace?.unsavedCount ?? 0)
}

// MARK: - Actions (the `AppState` extension)

/// `openInEditor(path:line:in:)`: shows `path` in the Editor tab, loading it
/// on first open, and reveals `line` (1-based) when given. Images and PDFs
/// are shown straight from disk; everything else goes through `file/read`.
export function openInEditor(path: string, project: ProjectState, line: number | null = null): void {
  const relative = isAbsolutePath(path) ? relativePath(path, project.root) : path.replace(/\\/g, '/')
  const workspace = editors(project)
  const { document, isNew } = workspace.open(relative)
  if (line !== null) document.update((d) => (d.pendingLine = line))
  const app = useApp.getState()
  app.selectProject(project.id, false)
  app.setSidebarItem('editor')
  if (document.kind !== 'text') {
    if (document.loading) document.update((d) => (d.loading = false))
    return
  }
  if (isNew || document.loadError !== null) void load(document, project)
}

/** `fileURL(of:in:)`: the document's absolute path, for the image and PDF views. */
export function documentFilePath(document: EditorDocument, project: ProjectState): string {
  return joinPath(project.root, document.path)
}

/** `load(_:in:)`: `file/read`; a file the backend refuses as binary becomes a `binary` document. */
export async function load(document: EditorDocument, project: ProjectState): Promise<void> {
  document.update((d) => (d.loading = true))
  try {
    const content = await backend.fileRead(project.id, document.path)
    document.update((d) => d.load(content))
    connectLanguageServer(document, project)
  } catch (error) {
    const message = errorText(error)
    document.update((d) => {
      d.loading = false
      if (message.includes('binary file')) {
        // Not text: the placeholder offers the default app instead.
        d.kind = 'binary'
        d.loadError = null
      } else {
        d.loadError = message
      }
    })
  }
}

/// `refreshCleanDocuments(in:)`: re-reads a clean document whose file
/// changed on disk (the agent edited it, a git checkout); an edited one keeps
/// its buffer.
export function refreshCleanDocuments(project: ProjectState): void {
  const workspace = workspaces.get(project.id)
  if (!workspace) return
  for (const document of workspace.documents) {
    if (document.kind !== 'text' || document.isDirty || document.loading || document.loadError !== null) continue
    void (async () => {
      let content: FileContent
      try {
        content = await backend.fileRead(project.id, document.path)
      } catch {
        return
      }
      if ((content.hash ?? null) !== document.hash && !document.isDirty) {
        document.update((d) => d.load(content))
        scheduleLanguageServerSync(document, project)
      }
    })()
  }
}

/// `save(_:in:overwrite:)`: `file/write` with the hash of the last read, so
/// an edit made meanwhile surfaces as `conflict` (the banner then offers
/// Reload / Overwrite).
export async function save(document: EditorDocument, project: ProjectState, overwrite = false): Promise<void> {
  if (document.saving || document.truncated || document.kind !== 'text') return
  document.update((d) => (d.saving = true))
  const text = document.text
  try {
    const result = await backend.fileWrite(project.id, document.path, text, overwrite ? null : document.hash)
    document.hash = result.hash
    document.savedText = text
    document.isDirty = document.text !== text
    document.conflict = false
    useApp.getState().scheduleGitRefresh(project)
  } catch (error) {
    if (error instanceof BackendError && error.kind === 'conflict') {
      document.conflict = true
    } else {
      const message = errorText(error)
      toasts.push({ message: `Could not save ${document.name}: ${message}`, level: 'error', details: message })
    }
  } finally {
    document.update((d) => (d.saving = false))
  }
}

/** `saveSelectedDocument()`: ⌘S / Ctrl+S on the Editor tab. */
export function saveSelectedDocument(): void {
  const state = useApp.getState()
  const project = selectedProject(state)
  if (!project || state.sidebarItem !== 'editor') return
  const document = workspaces.get(project.id)?.selected
  if (!document || !document.isDirty) return
  void save(document, project)
}

/** `closeSelectedDocument()`: ⌘⇧W / Ctrl+Shift+W on the Editor tab. */
export function closeSelectedDocument(): void {
  const state = useApp.getState()
  const project = selectedProject(state)
  if (!project || state.sidebarItem !== 'editor') return
  const document = workspaces.get(project.id)?.selected
  if (!document) return
  void closeDocument(document, project)
}

/** `discard(_:in:)`: removes the tab and lets go of its language server document. */
function discard(document: EditorDocument, project: ProjectState): void {
  editors(project).close(document)
  document.cancelTimers()
  if (document.lspServer !== null) {
    void backend.lspClose(project.id, document.path).catch(() => undefined)
  }
}

const UNSAVED_INFORMATIVE_TEXT = "Your edits will be lost if you don't save them."

/** `closeDocument(_:in:)`: closes the tab; unsaved edits ask first (Save · Don't Save · Cancel). */
export async function closeDocument(document: EditorDocument, project: ProjectState): Promise<void> {
  const drop = () => discard(document, project)
  if (!document.isDirty) {
    drop()
    return
  }
  // Auto save on: save rather than ask; a failed or conflicting save keeps the tab.
  if (getPref('editorAutoSave') && !document.truncated) {
    await save(document, project)
    if (!document.isDirty && !document.conflict) drop()
    return
  }
  const choice = await askToSaveChanges({
    title: `Save changes to ${document.name}?`,
    message: UNSAVED_INFORMATIVE_TEXT,
    saveLabel: 'Save',
  })
  switch (choice) {
    case 'save':
      await save(document, project)
      if (!document.isDirty && !document.conflict) drop()
      return
    case 'dontSave':
      drop()
      return
    case 'cancel':
      return
  }
}

/// `closeDocuments(_:selecting:in:)`: Close Others · Close Tabs to the Right:
/// `documents` go, `survivor` keeps (or takes) the selection. Unsaved ones
/// ask once for all of them; one whose save fails or conflicts stays open.
export async function closeDocuments(documents: readonly EditorDocument[], survivor: EditorDocument, project: ProjectState): Promise<void> {
  if (documents.length === 0) return
  const workspace = editors(project)
  const drop = (list: readonly EditorDocument[]) => {
    if (list.some((document) => document.path === workspace.selectedPath)) workspace.select(survivor.path)
    for (const document of list) discard(document, project)
  }
  const dirty = documents.filter((document) => document.isDirty)
  if (dirty.length === 0) {
    drop(documents)
    return
  }
  const saveAll = async () => {
    for (const document of dirty) await save(document, project)
    drop(documents.filter((document) => !document.isDirty && !document.conflict))
  }
  if (getPref('editorAutoSave')) {
    await saveAll()
    return
  }
  const first = dirty[0]
  const single = dirty.length === 1 && first !== undefined
  const choice = await askToSaveChanges({
    title: single ? `Save changes to ${first.name}?` : `Save changes to ${dirty.length} files?`,
    message: single ? UNSAVED_INFORMATIVE_TEXT : `${dirty.map((document) => document.name).join(', ')}\n\n${UNSAVED_INFORMATIVE_TEXT}`,
    saveLabel: single ? 'Save' : 'Save All',
  })
  switch (choice) {
    case 'save':
      await saveAll()
      return
    case 'dontSave':
      drop(documents)
      return
    case 'cancel':
      return
  }
}

// MARK: Auto save

/// `setEditorAutoSave(_:)`: turns auto save on or off; turning it on saves
/// every edited document right away, as VS Code does. Settings → General
/// and the File → Auto Save toggle call it.
export function setEditorAutoSave(enabled: boolean): void {
  setPref('editorAutoSave', enabled)
  if (!enabled) {
    for (const workspace of workspaces.values()) {
      for (const document of workspace.documents) {
        if (document.autoSave !== null) clearTimeout(document.autoSave)
        document.autoSave = null
      }
    }
    return
  }
  const state = useApp.getState()
  for (const [projectId, workspace] of workspaces) {
    const project = projectById(state, projectId)
    if (!project) continue
    for (const document of workspace.documents) {
      if (document.isDirty) void autoSave(document, project)
    }
  }
}

/** `scheduleAutoSave(_:in:)`: saves a second after the last edit while auto save is on. */
export function scheduleAutoSave(document: EditorDocument, project: ProjectState): void {
  if (!getPref('editorAutoSave')) return
  if (document.autoSave !== null) clearTimeout(document.autoSave)
  document.autoSave = setTimeout(() => {
    document.autoSave = null
    void autoSave(document, project)
  }, AUTO_SAVE_DELAY_MS)
}

const AUTO_SAVE_DELAY_MS = 1000

/// Saves an edited document unless the user has to decide first (a conflict
/// waits for Reload / Overwrite); waits out a save in flight so edits made
/// during it are not left behind.
async function autoSave(document: EditorDocument, project: ProjectState): Promise<void> {
  while (document.saving) await sleep(100)
  if (!document.isDirty || document.conflict || document.truncated || document.kind !== 'text') return
  if (workspaces.get(project.id)?.document(document.path) !== document) return
  await save(document, project)
}

// MARK: Language server

/// `connectLanguageServer(_:in:)`: `lsp/open` with the buffer; the server
/// (if any for the language) then pushes diagnostics and answers completion
/// and hover for this document.
export function connectLanguageServer(document: EditorDocument, project: ProjectState): void {
  void (async () => {
    try {
      const status = await backend.lspOpen(project.id, document.path, document.text)
      document.lspDirty = false
      document.update((d) => {
        d.lspServer = status.server ?? null
        d.lspTriggers = status.trigger_characters ?? []
      })
    } catch (error) {
      document.update((d) => (d.lspServer = null))
      debug(() => `lsp/open ${document.path}: ${errorText(error)}`)
    }
  })()
}

const LSP_SYNC_DELAY_MS = 200

/** `scheduleLanguageServerSync(_:in:)`: debounced `lsp/change` after an edit. */
export function scheduleLanguageServerSync(document: EditorDocument, project: ProjectState): void {
  if (document.lspServer === null) return
  if (document.lspSync !== null) clearTimeout(document.lspSync)
  document.lspSync = setTimeout(() => {
    document.lspSync = null
    void flushLanguageServerSync(document, project)
  }, LSP_SYNC_DELAY_MS)
}

/** `flushLanguageServerSync(_:in:)`: sends the buffer now when the server has not seen it yet. */
export async function flushLanguageServerSync(document: EditorDocument, project: ProjectState): Promise<void> {
  if (document.lspServer === null || !document.lspDirty) return
  if (document.lspSync !== null) clearTimeout(document.lspSync)
  document.lspSync = null
  document.lspDirty = false
  try {
    await backend.lspChange(project.id, document.path, document.text)
  } catch (error) {
    document.lspDirty = true
    debug(() => `lsp/change ${document.path}: ${errorText(error)}`)
  }
}

/** `languageServerCompletions(for:line:character:in:)`: completion at a 0-based position, after the server has the buffer. */
export async function languageServerCompletions(document: EditorDocument, line: number, character: number, project: ProjectState): Promise<LspCompletionItem[]> {
  if (document.lspServer === null) return []
  await flushLanguageServerSync(document, project)
  try {
    return (await backend.lspCompletion(project.id, document.path, line, character)).items
  } catch {
    return []
  }
}

/** `languageServerHover(for:line:character:in:)`. */
export async function languageServerHover(document: EditorDocument, line: number, character: number, project: ProjectState): Promise<string | null> {
  if (document.lspServer === null) return null
  await flushLanguageServerSync(document, project)
  try {
    return (await backend.lspHover(project.id, document.path, line, character)).contents ?? null
  } catch {
    return null
  }
}

// MARK: Symbols

/** `findSymbols(_:mode:limit:in:preferring:)`: definitions of `query` from the index; the current file's first. */
export async function findSymbols(query: string, mode: SymbolsFindMode, limit: number, project: ProjectState, preferring: string | null = null): Promise<SymbolInfo[]> {
  let symbols: SymbolInfo[]
  try {
    symbols = await backend.symbolsFind(project.id, query, mode, limit)
  } catch {
    return []
  }
  if (preferring === null) return symbols
  return [...symbols.filter((symbol) => symbol.path === preferring), ...symbols.filter((symbol) => symbol.path !== preferring)]
}

/// `jumpToDefinition(of:line:character:from:in:)`: ⌘-click / Ctrl-click on
/// an identifier: the language server's definition when it has one, else the
/// index's (the one in the same file first).
export function jumpToDefinition(name: string, line: number, character: number, document: EditorDocument, project: ProjectState): void {
  void (async () => {
    if (document.lspServer !== null) {
      await flushLanguageServerSync(document, project)
      let location: { path: string; line: number } | undefined
      try {
        location = (await backend.lspDefinition(project.id, document.path, line, character))[0]
      } catch {
        location = undefined
      }
      if (location) {
        openInEditor(location.path, project, location.line)
        return
      }
    }
    const symbols = await findSymbols(name, 'exact', 10, project, document.path)
    const symbol = symbols[0]
    if (!symbol) {
      toasts.push({ message: `No definition of ${name} found`, level: 'info' })
      return
    }
    openInEditor(symbol.path, project, symbol.line)
  })()
}

/** `showFilesTab()`: opens the Inspector Files tab (the Editor's empty state). */
export function showFilesTab(): void {
  const app = useApp.getState()
  app.setInspectorVisible(true)
  app.setInspectorTab('files')
}

// MARK: - Notifications and project lifecycle

/** `case .lspDiagnostics(let d)`: the open document takes the server's diagnostics. */
function routeDiagnostics(diagnostics: LspDiagnostics): void {
  const document = workspaces.get(diagnostics.project_id)?.document(diagnostics.path)
  if (!document) return
  document.update((d) => (d.diagnostics = diagnostics.diagnostics))
}

extendApp({
  lspDiagnostics: routeDiagnostics,
  openInEditor: (path, project, line) => openInEditor(path, project, line),
})

onProjectClosed((projectId) => {
  workspaces.get(projectId)?.dispose()
  workspaces.delete(projectId)
})

// MARK: - Helpers

function pathExtension(path: string): string {
  const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  const name = slash < 0 ? path : path.slice(slash + 1)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}

/** Whether `path` is absolute on this platform's terms (POSIX, drive letter or UNC). */
function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path) || path.startsWith('\\\\')
}

/** `(root as NSString).appendingPathComponent(path)` with the root's separator. */
function joinPath(root: string, path: string): string {
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const base = root.replace(/[\\/]+$/, '')
  const rest = path.replace(/^[\\/]+/, '')
  return base + separator + (separator === '\\' ? rest.replace(/\//g, '\\') : rest)
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
