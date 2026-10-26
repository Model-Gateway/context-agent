// Port of apps/macos/ContextAgent/Models/Preferences.swift: small per-user
// settings, persisted to `localStorage` under `ctx.<key>` (the Swift app's
// UserDefaults). One zustand store holds every value; `usePref(key)`
// subscribes a view to one of them and `setPref(key, value)` writes through
// to storage. API keys live in the OS credential store (`secret_*`), never
// here. The key names are the ones apps/desktop/ARCHITECTURE.md lists
// ("Preferences"); where they differ from the Swift property, the doc
// comment names the Swift one.

import { create } from 'zustand'
import type { GatewayAccount } from '../backend/types'
import type { RecentProject } from './recentProject'

// MARK: - Appearance

/** `Appearance` (Settings → Appearance). */
export type Appearance = 'system' | 'light' | 'dark'

/** `Appearance.allCases`. */
export const APPEARANCES: readonly Appearance[] = ['system', 'light', 'dark']

/** `Appearance.label`. */
export function appearanceLabel(appearance: Appearance): string {
  switch (appearance) {
    case 'system':
      return 'System'
    case 'light':
      return 'Light'
    case 'dark':
      return 'Dark'
  }
}

/// `Preferences.applyAppearance()`: sets `data-theme` on the root element,
/// which theme/tokens.css reads (`system` follows `prefers-color-scheme`).
export function applyAppearance(appearance: Appearance = getPref('appearance')): void {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.theme = appearance
}

// MARK: - Values

/** Every preference with its type (`Preferences`' stored properties). */
export interface PreferenceValues {
  /** Settings → General "Choose…" (also the backend failure panel). Null means "auto-detect". */
  backendPathOverride: string | null
  appearance: Appearance
  /// Settings → General: retrieve repository context before the first model
  /// call (proactive retrieval). On unless turned off there. Swift
  /// `contextEngineEnabled`.
  useContextEngine: boolean
  lastSelectedProjectId: string | null
  /** Project id → last selected session id. */
  lastSelectedSession: Record<string, string>
  sidebarVisible: boolean
  inspectorVisible: boolean
  /** Last width the inspector was dragged to (px). */
  inspectorWidth: number
  /** Project id → `SidebarItem` raw value. */
  lastTab: Record<string, string>
  /** Project id → last selected file in the Changes tab. */
  lastChangesPath: Record<string, string>
  /// Repository root → the permission profile last chosen for it in the
  /// composer; re-applied whenever the project is opened.
  permissionProfiles: Record<string, string>
  /// Settings → General: after a completed run, ask the model for the user's
  /// likely next message (composer ghost text). On by default. Swift
  /// `suggestNextMessage`.
  suggestNext: boolean
  /// Settings → General: ask every run for the full prompt of its first model
  /// request (`task/run.options.debug_prompt`) and show it above the message
  /// that started the run. Off unless turned on there.
  developerMode: boolean
  /** Base URL → recently used model names (most recent first, max 8). Swift `recentModels`. */
  modelRecents: Record<string, string[]>
  notifyOnFinish: boolean
  notifyOnApproval: boolean
  /** Settings → General "Use mock backend (no API key)". */
  useMockBackend: boolean
  /** Repository roots open at quit; reopened at launch. */
  openProjectPaths: string[]
  /** Repository roots whose worktree rows are folded away in the sidebar (Swift `Set<String>`, kept sorted). */
  collapsedProjects: string[]
  /// Settings → General "Initialize a Git repository in projects that have
  /// none": `project/open` runs `git init` for a folder outside any
  /// repository, so the undo snapshots taken before every run exist. On
  /// unless turned off.
  autoInitGit: boolean
  /** Settings → General "Default editor": the app Open with Default Editor uses. Null is the system's choice. */
  defaultEditorPath: string | null
  /// Settings → General "Auto save": an edited file is saved a second after
  /// the last edit, and closing one saves instead of asking. Off unless
  /// turned on.
  editorAutoSave: boolean
  /** The profile page's last tab (`SettingsTab` raw value). */
  settingsTab: string
  /** The signed-in ModelGateway account; null when signed out. The key itself is in the credential store. */
  gatewayAccount: GatewayAccount | null
  /** Account → Settings "Route model requests through ModelGateway". On by default. Swift `gatewayRouteRequests`. */
  routeThroughGateway: boolean
  /// Account → Settings "Attribute usage to projects" (sends the project
  /// name as a request header). On by default. Swift `gatewayAttributeProjects`.
  attributeProjects: boolean
  /// `base_url` / `api_key_env` from before sign-in switched the endpoint to
  /// the gateway; restored on sign-out.
  gatewayPreviousEndpoint: Record<string, string>
  /// Remembered repository roots (Welcome grid, sidebar "pinned", Open
  /// Recent): newest first, at most 30. Swift keeps them in
  /// `recent-projects.json` (`RecentProjectStore`); see models/recentProject.ts.
  recents: RecentProject[]
}

/** A preference's name (the storage key without the `ctx.` prefix). */
export type PrefKey = keyof PreferenceValues

/** The storage key prefix. */
export const PREFERENCE_KEY_PREFIX = 'ctx.'

/** `Preferences.maxRecentModels`. */
export const MAX_RECENT_MODELS = 8

/** `RecentProjectStore.maxCount`. Declared before the store loads the
 * stored values (`trimRecentProjects` reads it while decoding `recents`). */
export const MAX_RECENT_PROJECTS = 30

/** `Theme.Metrics.inspectorWidth`. */
const DEFAULT_INSPECTOR_WIDTH = 320

/** The values a fresh install starts with (`Preferences.init`'s fallbacks). */
export const PREFERENCE_DEFAULTS: Readonly<PreferenceValues> = Object.freeze({
  backendPathOverride: null,
  appearance: 'system',
  useContextEngine: true,
  lastSelectedProjectId: null,
  lastSelectedSession: {},
  sidebarVisible: true,
  inspectorVisible: false,
  inspectorWidth: DEFAULT_INSPECTOR_WIDTH,
  lastTab: {},
  lastChangesPath: {},
  permissionProfiles: {},
  suggestNext: true,
  developerMode: false,
  modelRecents: {},
  notifyOnFinish: true,
  notifyOnApproval: true,
  useMockBackend: false,
  openProjectPaths: [],
  collapsedProjects: [],
  autoInitGit: true,
  defaultEditorPath: null,
  editorAutoSave: false,
  settingsTab: 'account',
  gatewayAccount: null,
  routeThroughGateway: true,
  attributeProjects: true,
  gatewayPreviousEndpoint: {},
  recents: [],
})

const PREF_KEYS = Object.keys(PREFERENCE_DEFAULTS) as PrefKey[]

// MARK: - Codecs

type Decoder<T> = (raw: unknown) => T | undefined

const bool: Decoder<boolean> = (raw) => (typeof raw === 'boolean' ? raw : undefined)
const finiteNumber: Decoder<number> = (raw) => (typeof raw === 'number' && Number.isFinite(raw) ? raw : undefined)
const string: Decoder<string> = (raw) => (typeof raw === 'string' ? raw : undefined)
const optionalString: Decoder<string | null> = (raw) => (typeof raw === 'string' && raw.length > 0 ? raw : undefined)
const stringArray: Decoder<string[]> = (raw) => (Array.isArray(raw) ? raw.filter((item): item is string => typeof item === 'string') : undefined)
const stringSet: Decoder<string[]> = (raw) => {
  const list = stringArray(raw)
  return list ? [...new Set(list)].sort() : undefined
}
const stringMap: Decoder<Record<string, string>> = (raw) => {
  if (!isPlainObject(raw)) return undefined
  const out: Record<string, string> = {}
  for (const [key, value] of Object.entries(raw)) if (typeof value === 'string') out[key] = value
  return out
}
const stringListMap: Decoder<Record<string, string[]>> = (raw) => {
  if (!isPlainObject(raw)) return undefined
  const out: Record<string, string[]> = {}
  for (const [key, value] of Object.entries(raw)) {
    const list = stringArray(value)
    if (list) out[key] = list
  }
  return out
}
const appearance: Decoder<Appearance> = (raw) => (raw === 'system' || raw === 'light' || raw === 'dark' ? raw : undefined)
const gatewayAccount: Decoder<GatewayAccount | null> = (raw) => (isPlainObject(raw) ? (raw as unknown as GatewayAccount) : undefined)
const recents: Decoder<RecentProject[]> = (raw) => {
  if (!Array.isArray(raw)) return undefined
  const seen = new Set<string>()
  const out: RecentProject[] = []
  for (const entry of raw) {
    if (!isPlainObject(entry)) continue
    const path = entry.path
    const name = entry.name
    if (typeof path !== 'string' || typeof name !== 'string' || seen.has(path)) continue
    const opened = typeof entry.lastOpened === 'string' ? new Date(entry.lastOpened) : null
    seen.add(path)
    out.push({
      path,
      name,
      lastOpened: opened && !Number.isNaN(opened.getTime()) ? opened : new Date(0),
      pinned: entry.pinned === true,
    })
  }
  return trimRecentProjects(out)
}

const DECODERS: { [K in PrefKey]: Decoder<PreferenceValues[K]> } = {
  backendPathOverride: optionalString,
  appearance,
  useContextEngine: bool,
  lastSelectedProjectId: optionalString,
  lastSelectedSession: stringMap,
  sidebarVisible: bool,
  inspectorVisible: bool,
  inspectorWidth: finiteNumber,
  lastTab: stringMap,
  lastChangesPath: stringMap,
  permissionProfiles: stringMap,
  suggestNext: bool,
  developerMode: bool,
  modelRecents: stringListMap,
  notifyOnFinish: bool,
  notifyOnApproval: bool,
  useMockBackend: bool,
  openProjectPaths: stringArray,
  collapsedProjects: stringSet,
  autoInitGit: bool,
  defaultEditorPath: optionalString,
  editorAutoSave: bool,
  settingsTab: string,
  gatewayAccount,
  routeThroughGateway: bool,
  attributeProjects: bool,
  gatewayPreviousEndpoint: stringMap,
  recents,
}

function isPlainObject(raw: unknown): raw is Record<string, unknown> {
  return raw !== null && typeof raw === 'object' && !Array.isArray(raw)
}

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}

function readValue<K extends PrefKey>(key: K): PreferenceValues[K] {
  const fallback = PREFERENCE_DEFAULTS[key]
  const store = storage()
  if (!store) return fallback
  try {
    const text = store.getItem(PREFERENCE_KEY_PREFIX + key)
    if (text === null) return fallback
    const decoded = DECODERS[key](JSON.parse(text))
    return decoded === undefined ? fallback : decoded
  } catch {
    return fallback
  }
}

/// `Preferences.write(_:_:)` and the `didSet` writers: an empty optional
/// string or a nil account removes the key, everything else is JSON.
function writeValue<K extends PrefKey>(key: K, value: PreferenceValues[K]): void {
  const store = storage()
  if (!store) return
  const storageKey = PREFERENCE_KEY_PREFIX + key
  try {
    if (value === null || (typeof value === 'string' && value.length === 0 && (key === 'backendPathOverride' || key === 'lastSelectedProjectId' || key === 'defaultEditorPath'))) {
      store.removeItem(storageKey)
      return
    }
    store.setItem(storageKey, JSON.stringify(value))
  } catch {
    // Quota or a blocked storage: the value stays in memory for this run.
  }
}

function loadAll(): PreferenceValues {
  const values = {} as Record<PrefKey, unknown>
  for (const key of PREF_KEYS) values[key] = readValue(key)
  return values as unknown as PreferenceValues
}

// MARK: - Store

/** The preferences store: every value plus its writer. */
export interface PreferencesStore extends PreferenceValues {
  /** Sets one preference and persists it. */
  setPref: <K extends PrefKey>(key: K, value: PreferenceValues[K]) => void
}

/** The zustand store behind `usePref` (one per window, shared by every view). */
export const usePreferences = create<PreferencesStore>()((set) => ({
  ...loadAll(),
  setPref: (key, value) => {
    const normalized = normalize(key, value)
    writeValue(key, normalized)
    set({ [key]: normalized } as Partial<PreferencesStore>)
  },
}))

/** Applies the per-key normalisation the Swift writers do (sorted set, empty → nil). */
function normalize<K extends PrefKey>(key: K, value: PreferenceValues[K]): PreferenceValues[K] {
  if (key === 'collapsedProjects' && Array.isArray(value)) {
    return [...new Set(value as string[])].sort() as PreferenceValues[K]
  }
  if ((key === 'backendPathOverride' || key === 'lastSelectedProjectId' || key === 'defaultEditorPath') && value === '') {
    return null as PreferenceValues[K]
  }
  return value
}

/** Subscribes a view to one preference. */
export function usePref<K extends PrefKey>(key: K): PreferenceValues[K] {
  return usePreferences((state) => state[key])
}

/** `usePref` plus its setter, for controls bound to a preference. */
export function usePrefState<K extends PrefKey>(key: K): [PreferenceValues[K], (value: PreferenceValues[K]) => void] {
  const value = usePref(key)
  const setter = usePreferences((state) => state.setPref)
  return [value, (next: PreferenceValues[K]) => setter(key, next)]
}

/** One preference's current value (outside React). */
export function getPref<K extends PrefKey>(key: K): PreferenceValues[K] {
  return usePreferences.getState()[key]
}

/** Sets and persists one preference (outside React). */
export function setPref<K extends PrefKey>(key: K, value: PreferenceValues[K]): void {
  usePreferences.getState().setPref(key, value)
}

// Another window (or devtools) changed storage: follow it.
if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.storageArea !== storage() || event.key === null || !event.key.startsWith(PREFERENCE_KEY_PREFIX)) return
    const key = event.key.slice(PREFERENCE_KEY_PREFIX.length) as PrefKey
    if (!(PREF_KEYS as string[]).includes(key)) return
    usePreferences.setState({ [key]: readValue(key) } as Partial<PreferencesStore>)
  })
}

// `appearance`'s `didSet`: applied at launch and whenever it changes.
applyAppearance(getPref('appearance'))
usePreferences.subscribe((state, previous) => {
  if (state.appearance !== previous.appearance) applyAppearance(state.appearance)
})

// MARK: - Helpers (Preferences' methods)

/** `Preferences.normalized(_:)`: trimmed, one trailing slash dropped. */
function normalizedBaseURL(baseURL: string): string {
  const trimmed = baseURL.trim()
  return trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed
}

/** `Preferences.recentModels(for:)`. */
export function recentModels(baseURL: string): string[] {
  return getPref('modelRecents')[normalizedBaseURL(baseURL)] ?? []
}

/** `Preferences.promoteRecentModel(_:baseURL:)`: moves `name` to the front of the base URL's recents (max 8). */
export function promoteRecentModel(name: string, baseURL: string): void {
  const key = normalizedBaseURL(baseURL)
  const all = getPref('modelRecents')
  const list = [name, ...(all[key] ?? []).filter((entry) => entry !== name)].slice(0, MAX_RECENT_MODELS)
  setPref('modelRecents', { ...all, [key]: list })
}

/** `Preferences.removeRecentModel(_:baseURL:)`. */
export function removeRecentModel(name: string, baseURL: string): void {
  const key = normalizedBaseURL(baseURL)
  const all = getPref('modelRecents')
  const list = (all[key] ?? []).filter((entry) => entry !== name)
  const next = { ...all }
  if (list.length === 0) delete next[key]
  else next[key] = list
  setPref('modelRecents', next)
}

/** `Preferences.lastTab(for:)`. */
export function lastTabFor(projectId: string): string | null {
  return getPref('lastTab')[projectId] ?? null
}

/** `preferences.lastTab[projectId] = tab`. */
export function setLastTab(projectId: string, tab: string): void {
  setPref('lastTab', { ...getPref('lastTab'), [projectId]: tab })
}

/** `preferences.lastSelectedSession[projectId] = sessionId` (null removes it). */
export function setLastSelectedSession(projectId: string, sessionId: string | null): void {
  const next = { ...getPref('lastSelectedSession') }
  if (sessionId === null) delete next[projectId]
  else next[projectId] = sessionId
  setPref('lastSelectedSession', next)
}

/** `preferences.lastChangesPath[projectId] = path` (null removes it). */
export function setLastChangesPath(projectId: string, path: string | null): void {
  const next = { ...getPref('lastChangesPath') }
  if (path === null) delete next[projectId]
  else next[projectId] = path
  setPref('lastChangesPath', next)
}

/** `preferences.permissionProfiles[root] = profile`. */
export function setPermissionProfile(root: string, profile: string): void {
  setPref('permissionProfiles', { ...getPref('permissionProfiles'), [root]: profile })
}

/// `Preferences.rememberOpenProject(path:)`: appends a newly opened folder;
/// one already listed keeps its place, so launch's parallel reopen
/// (finishing in any order) keeps the sidebar order.
export function rememberOpenProject(path: string): void {
  const paths = getPref('openProjectPaths')
  if (paths.includes(path)) return
  setPref('openProjectPaths', [...paths, path])
}

/** `Preferences.forgetOpenProject(path:)`. */
export function forgetOpenProject(path: string): void {
  setPref(
    'openProjectPaths',
    getPref('openProjectPaths').filter((entry) => entry !== path),
  )
}

/** `preferences.collapsedProjects.contains(root)`. */
export function isProjectCollapsed(root: string): boolean {
  return getPref('collapsedProjects').includes(root)
}

/** `preferences.collapsedProjects.insert/remove(root)`. */
export function setProjectCollapsed(root: string, collapsed: boolean): void {
  const current = getPref('collapsedProjects')
  if (current.includes(root) === collapsed) return
  setPref('collapsedProjects', collapsed ? [...current, root] : current.filter((entry) => entry !== root))
}

/** Flips `isProjectCollapsed(root)`. */
export function toggleProjectCollapsed(root: string): void {
  setProjectCollapsed(root, !isProjectCollapsed(root))
}

/// `RecentProjectStore.trim()`: over 30 entries, the oldest unpinned ones are
/// dropped (pinned entries are always kept). `items` is newest first.
export function trimRecentProjects(items: RecentProject[]): RecentProject[] {
  if (items.length <= MAX_RECENT_PROJECTS) return items
  const kept: RecentProject[] = []
  let overflow = items.length - MAX_RECENT_PROJECTS
  for (let i = items.length - 1; i >= 0; i -= 1) {
    const entry = items[i]
    if (!entry) continue
    if (overflow > 0 && !entry.pinned) {
      overflow -= 1
      continue
    }
    kept.unshift(entry)
  }
  return kept
}
