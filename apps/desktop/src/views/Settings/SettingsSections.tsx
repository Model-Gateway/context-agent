// Port of apps/macos/ContextAgent/Views/Settings/SettingsSections.swift:
// the individual Settings forms; ProfileView (SettingsView.tsx) arranges
// them in tabs. Every form reads `config` (the user-level configuration the
// backend reported with `initialize` / `config/get`) and writes through
// `patchConfig` (config/patch), the preferences or the credential-store
// intents of the store. RPC failures stay inline in the form.

import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { homeDir, localDataDir } from '@tauri-apps/api/path'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { backend } from '../../backend/client'
import { describe as describeFailure, errorText } from '../../backend/errors'
import type { DoctorReport, JSONObject, ModelSettings, PermissionSettings } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { CopyButton } from '../../components/CopyButton'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { IconButton } from '../../components/IconButton'
import { InlineMessage, type InlineMessageData } from '../../components/InlineMessage'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Skeleton } from '../../components/Skeleton'
import { isMac, modLabel, revealLabel } from '../../components/platform'
import { useModelsDevVersion } from '../../models/modelsDev'
import { permissionProfileOptions, type PermissionProfileOption } from '../../models/permissions'
import { APPEARANCES, appearanceLabel, usePrefState, type Appearance } from '../../models/preferences'
import { BUILD_COMMAND, launchOptions, useApp } from '../../state/app'
import { useAnyProjectRunning, useConnectionLabel } from '../../state/selectors'
import { setEditorAutoSave } from '../../state/editor'
import { useGatewayRoutingActive } from '../../state/gateway'
import { EFFORT_ULTRA, effortLevel, effortLevels } from '../Composer/EffortPopover'
import { FormButtons, Help, PopUpButton, SegmentedPicker, TextField, Toggle, type PopUpOption } from './SettingsControls'
import { isWindows, systemName } from './settingsPlatform'
import styles from './SettingsSections.module.css'

// MARK: - Shared helpers

/** `connection.isReady`. */
function useReady(): boolean {
  return useApp((s) => s.connection.kind === 'ready')
}

function restartBackend(): void {
  void useApp.getState().restartBackend()
}

/// `--mock` / `CTX_DESKTOP_MOCK=1` for this launch (Swift reads the process
/// arguments and environment; the shell parses them into the launch options).
function useForcedMock(): boolean {
  const [forced, setForced] = useState(false)
  useEffect(() => {
    let live = true
    void launchOptions().then((options) => {
      if (live) setForced(options.mock)
    })
    return () => {
      live = false
    }
  }, [])
  return forced
}

// MARK: - General

/** The Backend group's footer: the locator's search order on this platform. */
const BACKEND_FOOTER = isMac
  ? 'The ctx binary is auto-detected: the app bundle, $CTX_BINARY, ~/.cargo/bin, Homebrew, /usr/local/bin, then a repository checkout. Choose one to override the search.'
  : isWindows
    ? 'The ctx binary is auto-detected: next to the app, %CTX_BINARY%, %USERPROFILE%\\.cargo\\bin, %LOCALAPPDATA%\\Programs\\Context, PATH, then a repository checkout. Choose one to override the search.'
    : 'The ctx binary is auto-detected: next to the app, $CTX_BINARY, ~/.cargo/bin, /usr/local/bin, PATH, then a repository checkout. Choose one to override the search.'

const EDITOR_FOOTER =
  `File names in chat open in the Editor tab; ${modLabel}-click opens them in the editor chosen here instead. System default uses the app ${systemName === 'system' ? 'the system' : systemName} associates with each file type. ` +
  'Auto save writes an edited file a second after you stop typing, like VS Code, and closing an edited file then saves it without asking. A file changed on disk meanwhile is never overwritten: its banner offers Reload or Overwrite.'

const NOTIFICATIONS_FOOTER = isMac
  ? 'Posted when the app is in the background or another chat is selected. macOS asks for permission the first time one is sent.'
  : 'Posted when the app is in the background or another chat is selected.'

/**
 * `GeneralSettingsSection` (SettingsSections.swift): backend binary (detected
 * path, Choose…, Restart backend), status, mock toggle, data directory,
 * editor, Git, context engine, suggestions, developer mode and notification
 * toggles.
 */
export function GeneralSettingsSection() {
  const connection = useApp((s) => s.connection)
  const config = useApp((s) => s.config)
  const backendStarting = useApp((s) => s.backendStarting)
  const anyProjectRunning = useAnyProjectRunning()
  const connectionLabel = useConnectionLabel()
  const forcedMock = useForcedMock()
  const [override, setOverride] = usePrefState('backendPathOverride')
  const [useMockBackend, setUseMockBackend] = usePrefState('useMockBackend')
  const [editorAutoSave] = usePrefState('editorAutoSave')
  const [autoInitGit, setAutoInitGit] = usePrefState('autoInitGit')
  const [useContextEngine, setUseContextEngine] = usePrefState('useContextEngine')
  const [suggestNext, setSuggestNext] = usePrefState('suggestNext')
  const [developerMode, setDeveloperMode] = usePrefState('developerMode')
  const [notifyOnFinish, setNotifyOnFinish] = usePrefState('notifyOnFinish')
  const [notifyOnApproval, setNotifyOnApproval] = usePrefState('notifyOnApproval')
  const [mockHint, setMockHint] = useState<string | null>(null)

  const launched = backend.binaryPath
  const hasOverride = override !== null && override.length > 0
  const binaryPath =
    launched ?? (hasOverride ? override : connection.kind === 'failed' && connection.failure.kind === 'notFound' ? 'Not found' : 'Auto-detect')
  const binaryKnown = launched !== null || hasOverride

  const restartDisabled = backendStarting || anyProjectRunning
  const restartHelp = backendStarting
    ? 'The backend is starting'
    : anyProjectRunning
      ? 'Finishes after the running task'
      : 'Stop ctx and start it again with the current settings'

  const setMock = (value: boolean) => {
    setUseMockBackend(value)
    if (anyProjectRunning) {
      setMockHint('Applies when the backend restarts; finish or cancel the running task, then use Restart backend.')
    } else {
      setMockHint(null)
      restartBackend()
    }
  }

  const failure = connection.kind === 'failed' ? connection.failure : null
  const statusTint = connection.kind === 'ready' ? 'var(--success)' : connection.kind === 'connecting' ? 'var(--warning)' : 'var(--danger)'

  return (
    <SettingsPage>
      <SettingsGroup title="Backend" footer={BACKEND_FOOTER}>
        <SettingsRow label="Binary" alignment="top">
          <div className={styles.stack8}>
            <div className={styles.hstack6}>
              <MiddleTruncate lines={2} text={binaryPath} className={`selectable ${styles.binaryPath} ${binaryKnown ? styles.known : styles.notKnown}`} />
              {hasOverride ? (
                <Help text="Chosen in Settings; auto-detection is skipped">
                  <Chip>override</Chip>
                </Help>
              ) : null}
            </div>
            <div className={styles.hstack8}>
              <Button size="small" disabled={backendStarting} onClick={() => void useApp.getState().chooseBackendBinary()}>
                Choose…
              </Button>
              {hasOverride ? (
                <Help text={restartHelp}>
                  <Button
                    size="small"
                    disabled={restartDisabled}
                    onClick={() => {
                      setOverride(null)
                      restartBackend()
                    }}
                  >
                    Use auto-detect
                  </Button>
                </Help>
              ) : null}
              <Help text={restartHelp}>
                <Button size="small" disabled={restartDisabled} onClick={restartBackend}>
                  Restart backend
                </Button>
              </Help>
              {backendStarting ? <BrailleSpinner /> : null}
            </div>
          </div>
        </SettingsRow>
        <SettingsRow label="Status" alignment="top">
          <div className={styles.stack6}>
            <div className={styles.hstack6}>
              {connection.kind === 'connecting' ? (
                <BrailleSpinner size={11} />
              ) : (
                <span className={styles.statusDot} style={{ background: statusTint }} aria-hidden="true" />
              )}
              <span className={styles.statusLabel}>{connectionLabel}</span>
            </div>
            {failure ? (
              <>
                <span className={`selectable ${styles.failureDetail}`}>{describeFailure(failure).detail}</span>
                {failure.kind === 'notFound' ? (
                  <Help text={BUILD_COMMAND}>
                    <Button size="small" onClick={() => useApp.getState().copyBuildCommand()}>
                      Copy build command
                    </Button>
                  </Help>
                ) : null}
              </>
            ) : null}
          </div>
        </SettingsRow>
        <SettingsRow label="Mock mode" alignment="top">
          <div className={styles.stack4}>
            <Toggle label="Use mock backend (no API key)" checked={useMockBackend || forcedMock} onChange={setMock} disabled={forcedMock || backendStarting} />
            <span className={styles.caption}>
              {forcedMock
                ? 'Forced for this launch by --mock / CTX_DESKTOP_MOCK.'
                : 'Runs ctx serve --mock: a scripted model that reads files and never edits. Handy to try the app without a key.'}
            </span>
            {mockHint !== null ? <span className={styles.captionWarning}>{mockHint}</span> : null}
          </div>
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="Storage" footer="Sessions, tasks, the index and memories live in the data directory and are shared with the ctx command line.">
        <SettingsRow label="Data directory">
          <PathValue path={config?.data_dir ?? null} />
        </SettingsRow>
        <SettingsRow label="Configuration">
          <PathValue path={config?.config_file ?? null} />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="Editor" footer={EDITOR_FOOTER}>
        <SettingsRow label="Default editor">
          <DefaultEditorPicker />
        </SettingsRow>
        <SettingsRow label="Auto save" alignment="center">
          <Toggle label="Auto save" labelHidden checked={editorAutoSave} onChange={setEditorAutoSave} />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup
        title="Git"
        footer="A snapshot of the project files is taken before every run so a turn can be undone (Chat → Undo Last Turn) or a sent message edited and re-sent with the files reverted. Snapshots need a Git repository: with this on, a folder opened without one gets `git init` (never your home folder or a top-level directory)."
      >
        <Toggle label="Initialize a Git repository in projects that have none" checked={autoInitGit} onChange={setAutoInitGit} />
      </SettingsGroup>

      <SettingsGroup
        title="Context engine"
        footer="Retrieves relevant code, symbols and memories from the project index before the first model call of every run. Turn it off to start runs with only the prompt and conversation."
      >
        <Toggle label="Use the context engine" checked={useContextEngine} onChange={setUseContextEngine} />
      </SettingsGroup>

      <SettingsGroup
        title="Suggestions"
        footer="After a run completes, the project's model guesses your next message; it appears greyed out in the empty composer and → accepts it. Each suggestion is one short model call."
      >
        <Toggle label="Suggest a next message" checked={suggestNext} onChange={setSuggestNext} />
      </SettingsGroup>

      <SettingsGroup
        title="Developer"
        footer="Shows the full prompt sent to the model (policy, instructions, retrieved context, conversation and the tools offered) in a collapsed panel above the message that started each run. It is kept only while the app is open; reopened chats do not have it."
      >
        <Toggle label="Developer mode" checked={developerMode} onChange={setDeveloperMode} />
      </SettingsGroup>

      <SettingsGroup title="Notifications" footer={NOTIFICATIONS_FOOTER}>
        <Toggle label="Notify when a task finishes" checked={notifyOnFinish} onChange={setNotifyOnFinish} />
        <Toggle label="Notify when approval is needed" checked={notifyOnApproval} onChange={setNotifyOnApproval} />
      </SettingsGroup>
    </SettingsPage>
  )
}

/** `EditorApp`: an application that can open source files (Settings → General "Default editor"). */
interface EditorApp {
  /** The application's path (`EditorApp.id`). */
  id: string
  /** "Visual Studio Code", without the ".app" / ".exe". */
  name: string
}

/** The value of the pop-up's "Choose…" item (`DefaultEditorPicker.chooseTag`). */
const CHOOSE_TAG = '\u0000choose'

/** `EditorApp.name`: the file name without its extension. */
function editorName(path: string): string {
  const base = path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? path
  return base.replace(/\.(app|exe)$/i, '')
}

/// Well-known editors in menu order (`EditorApp.knownBundleIds`): the
/// install locations to look at on this platform, `~` and `%LOCALAPPDATA%`
/// expanded; only installed ones are listed.
async function knownEditorPaths(): Promise<Array<{ name: string; path: string }>> {
  let home = ''
  let local = ''
  try {
    home = (await homeDir()).replace(/[\\/]+$/, '')
  } catch {
    home = ''
  }
  if (isMac) {
    const apps = [
      ['Visual Studio Code', 'Visual Studio Code.app'],
      ['Visual Studio Code - Insiders', 'Visual Studio Code - Insiders.app'],
      ['Cursor', 'Cursor.app'],
      ['Windsurf', 'Windsurf.app'],
      ['Zed', 'Zed.app'],
      ['Xcode', 'Xcode.app'],
      ['Sublime Text', 'Sublime Text.app'],
      ['Nova', 'Nova.app'],
      ['BBEdit', 'BBEdit.app'],
      ['IntelliJ IDEA', 'IntelliJ IDEA.app'],
      ['Fleet', 'Fleet.app'],
      ['VSCodium', 'VSCodium.app'],
    ] as const
    const list: Array<{ name: string; path: string }> = []
    for (const [name, bundle] of apps) {
      list.push({ name, path: `/Applications/${bundle}` })
      if (home) list.push({ name, path: `${home}/Applications/${bundle}` })
    }
    list.push({ name: 'TextEdit', path: '/System/Applications/TextEdit.app' })
    return list
  }
  if (isWindows) {
    try {
      local = (await localDataDir()).replace(/[\\/]+$/, '')
    } catch {
      local = ''
    }
    const programs = local ? `${local}\\Programs` : ''
    const list: Array<{ name: string; path: string }> = []
    const add = (name: string, path: string) => list.push({ name, path })
    if (programs) add('Visual Studio Code', `${programs}\\Microsoft VS Code\\Code.exe`)
    add('Visual Studio Code', 'C:\\Program Files\\Microsoft VS Code\\Code.exe')
    if (programs) add('Visual Studio Code - Insiders', `${programs}\\Microsoft VS Code Insiders\\Code - Insiders.exe`)
    if (programs) add('Cursor', `${programs}\\cursor\\Cursor.exe`)
    if (programs) add('Windsurf', `${programs}\\Windsurf\\Windsurf.exe`)
    if (programs) add('Zed', `${programs}\\Zed\\Zed.exe`)
    add('Sublime Text', 'C:\\Program Files\\Sublime Text\\sublime_text.exe')
    if (programs) add('VSCodium', `${programs}\\VSCodium\\VSCodium.exe`)
    add('VSCodium', 'C:\\Program Files\\VSCodium\\VSCodium.exe')
    add('Notepad++', 'C:\\Program Files\\Notepad++\\notepad++.exe')
    add('Notepad', 'C:\\Windows\\System32\\notepad.exe')
    return list
  }
  return [
    { name: 'Visual Studio Code', path: '/usr/bin/code' },
    { name: 'VSCodium', path: '/usr/bin/codium' },
    { name: 'Cursor', path: '/usr/bin/cursor' },
    { name: 'Zed', path: home ? `${home}/.local/bin/zed` : '/usr/bin/zed' },
    { name: 'Sublime Text', path: '/usr/bin/subl' },
    { name: 'gedit', path: '/usr/bin/gedit' },
  ]
}

/** `EditorApp.installed()`: the known editors that exist on disk, one per name. */
async function installedEditors(): Promise<EditorApp[]> {
  const candidates = await knownEditorPaths()
  const found = await Promise.all(
    candidates.map(async (candidate) => {
      try {
        await invoke('fs_file_info', { path: candidate.path })
        return candidate
      } catch {
        return null
      }
    }),
  )
  const seen = new Set<string>()
  const list: EditorApp[] = []
  for (const entry of found) {
    if (!entry || seen.has(entry.name)) continue
    seen.add(entry.name)
    list.push({ id: entry.path, name: entry.name })
  }
  return list
}

/**
 * `DefaultEditorPicker`: System default, the installed well-known editors,
 * a previously chosen app, and Choose… (an application file dialog).
 */
function DefaultEditorPicker() {
  const [editorPath, setEditorPath] = usePrefState('defaultEditorPath')
  const [installed, setInstalled] = useState<EditorApp[]>([])

  useEffect(() => {
    let live = true
    void installedEditors().then((list) => {
      if (live) setInstalled(list)
    })
    return () => {
      live = false
    }
  }, [])

  // Installed editors plus the chosen one when it is not among them.
  const options = useMemo(() => {
    if (editorPath === null || installed.some((editor) => editor.id === editorPath)) return installed
    return [...installed, { id: editorPath, name: editorName(editorPath) }]
  }, [installed, editorPath])

  // One divider, not two, while no editor is listed (a browser preview, or a
  // computer with none of the known editors installed).
  const items: PopUpOption<string>[] = [
    { value: '', label: 'System default' },
    { divider: true },
    ...options.map((editor) => ({ value: editor.id, label: editor.name })),
    ...(options.length > 0 ? [{ divider: true } as const] : []),
    { value: CHOOSE_TAG, label: 'Choose…' },
  ]

  const chooseApp = async () => {
    let picked: string | string[] | null = null
    try {
      picked = await openDialog({
        title: 'Choose an Editor',
        directory: false,
        multiple: false,
        defaultPath: isMac ? '/Applications' : isWindows ? 'C:\\Program Files' : '/usr/bin',
        filters: isMac ? [{ name: 'Applications', extensions: ['app'] }] : isWindows ? [{ name: 'Applications', extensions: ['exe'] }] : undefined,
      })
    } catch {
      return
    }
    const path = Array.isArray(picked) ? picked[0] : picked
    if (path) setEditorPath(path)
  }

  return (
    <PopUpButton
      aria-label="Default editor"
      small
      value={editorPath ?? ''}
      options={items}
      onChange={(tag) => {
        if (tag === CHOOSE_TAG) void chooseApp()
        else setEditorPath(tag.length === 0 ? null : tag)
      }}
    />
  )
}

// MARK: - Model

/// `ModelDraft`: editable copy of `ModelSettings` as strings, so partial
/// input never clobbers the configuration before Save.
interface ModelDraft {
  name: string
  contextWindow: string
  maxOutputTokens: string
}

function modelDraft(model: ModelSettings): ModelDraft {
  return { name: model.name, contextWindow: String(model.context_window), maxOutputTokens: String(model.max_output_tokens) }
}

function modelDraftIsDirty(draft: ModelDraft, model: ModelSettings): boolean {
  const base = modelDraft(model)
  return draft.name !== base.name || draft.contextWindow !== base.contextWindow || draft.maxOutputTokens !== base.maxOutputTokens
}

/** `ModelDraft.integer(_:)`: "128000", "128,000" and "128_000" all parse. */
function parseInteger(text: string): number | null {
  const cleaned = text.trim().replace(/,/g, '').replace(/_/g, '')
  if (!/^[+-]?\d+$/.test(cleaned)) return null
  const value = Number(cleaned)
  return Number.isSafeInteger(value) ? value : null
}

/** `ModelDraft.patch(against:)`: the `config/patch` values for every field that differs, or a problem to show instead. */
function modelDraftPatch(draft: ModelDraft, model: ModelSettings): { values: JSONObject; problem: string | null } {
  const values: JSONObject = {}
  const name = draft.name.trim()
  if (name.length === 0) return { values: {}, problem: 'Model name cannot be empty.' }
  const window = parseInteger(draft.contextWindow)
  if (window === null || window <= 0) return { values: {}, problem: 'Context window must be a positive whole number of tokens.' }
  const output = parseInteger(draft.maxOutputTokens)
  if (output === null || output <= 0) return { values: {}, problem: 'Max output must be a positive whole number of tokens.' }
  if (!(output < window)) return { values: {}, problem: 'Max output must be smaller than the context window.' }
  if (name !== model.name) values['model.name'] = name
  if (window !== model.context_window) values['model.context_window'] = window
  if (output !== model.max_output_tokens) values['model.max_output_tokens'] = output
  return { values, problem: null }
}

/** A stable key for an Equatable `onChange` over a configuration table. */
function equatableKey(value: unknown): string {
  return value === null || value === undefined ? '' : JSON.stringify(value)
}

/**
 * `ModelSettingsSection` (SettingsSections.swift): the model (name, limits,
 * reasoning effort). Where requests go and the key they use are on the BYOK
 * tab (or ModelGateway's, when signed in).
 */
export function ModelSettingsSection() {
  const config = useApp((s) => s.config)
  const ready = useReady()
  const routingActive = useGatewayRoutingActive()
  useModelsDevVersion()
  const model = config?.model ?? null
  const [form, setForm] = useState<{ draft: ModelDraft; synced: ModelSettings | null }>({ draft: { name: '', contextWindow: '', maxOutputTokens: '' }, synced: null })
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  // `.onChange(of: appState.config?.model, initial: true)`: keeps edits in progress.
  const modelKey = equatableKey(model)
  useEffect(() => {
    if (!model) return
    setForm((previous) => {
      const keepEdits = previous.synced !== null && modelDraftIsDirty(previous.draft, previous.synced)
      return { synced: model, draft: keepEdits ? previous.draft : modelDraft(model) }
    })
    // `modelKey` is the Equatable value of `model`.
  }, [modelKey])

  const { draft, synced } = form
  const isDirty = synced !== null && modelDraftIsDirty(draft, synced)

  const apply = async (values: JSONObject, success: string) => {
    setSaving(true)
    setMessage(null)
    try {
      await useApp.getState().patchConfig(values)
      setMessage(SettingsMessage.success(success))
    } catch (error) {
      setMessage(SettingsMessage.error(errorText(error)))
    } finally {
      setSaving(false)
    }
  }

  const save = () => {
    if (synced === null || saving || !ready) return
    const prepared = modelDraftPatch(draft, synced)
    if (prepared.problem !== null) {
      setMessage(SettingsMessage.error(prepared.problem))
      return
    }
    if (Object.keys(prepared.values).length === 0) return
    void apply(prepared.values, 'Saved. Idle projects use the new model now; the context window and max output tokens apply when a project is reopened.')
  }

  const revert = () => {
    if (synced === null) return
    setForm({ synced, draft: modelDraft(synced) })
    setMessage(null)
  }

  const setDraft = (patch: Partial<ModelDraft>) => setForm((previous) => ({ ...previous, draft: { ...previous.draft, ...patch } }))

  if (!config) {
    return (
      <SettingsPage>
        <BackendUnavailable skeleton={[4, 2]} />
      </SettingsPage>
    )
  }

  const current = config.model.reasoning_effort ?? null
  const offered = effortLevels(useApp.getState().modelInfo(config.model.name)?.effortLevels)
  const effortOptions: PopUpOption<string>[] = offered.map((level) => ({
    value: level.value,
    label: level.value.length === 0 ? 'Default (model decides)' : level.value === EFFORT_ULTRA.value ? 'Ultra (deploys agents)' : level.title,
  }))
  if (current !== null && !offered.some((level) => level.value === current)) {
    effortOptions.push({ value: current, label: effortLevel(current).title })
  }
  const setEffort = (value: string) => {
    if (value === (current ?? '')) return
    void apply({ 'model.reasoning_effort': value.length === 0 ? null : value }, 'Reasoning effort saved.')
  }

  return (
    <SettingsPage>
      <SettingsGroup
        title="Model"
        footer={
          routingActive
            ? 'Requests go through ModelGateway, which serves the models of its catalogue under their own names. Saving rewrites ~/.context-agent/config.toml; idle projects use the new model at once, running ones after their task finishes.'
            : 'Saving rewrites ~/.context-agent/config.toml; idle projects use the new model at once, running ones after their task finishes. The endpoint and its key are on the BYOK tab.'
        }
      >
        <SettingsRow label="Model name">
          <TextField placeholder="gpt-4.1" aria-label="Model name" value={draft.name} onChange={(name) => setDraft({ name })} onSubmit={save} />
        </SettingsRow>
        <SettingsRow label="Context window">
          <span className={styles.fieldWithUnit}>
            <TextField
              placeholder="128000"
              aria-label="Context window"
              width={120}
              value={draft.contextWindow}
              onChange={(contextWindow) => setDraft({ contextWindow })}
              onSubmit={save}
            />
            <span className={styles.caption}>tokens</span>
          </span>
        </SettingsRow>
        <SettingsRow label="Max output">
          <span className={styles.fieldWithUnit}>
            <TextField
              placeholder="16000"
              aria-label="Max output"
              width={120}
              value={draft.maxOutputTokens}
              onChange={(maxOutputTokens) => setDraft({ maxOutputTokens })}
              onSubmit={save}
            />
            <span className={styles.caption}>tokens</span>
          </span>
        </SettingsRow>
        <SettingsRow label="Reasoning effort">
          <span className={styles.effort}>
            <PopUpButton aria-label="Reasoning effort" value={current ?? ''} options={effortOptions} onChange={setEffort} disabled={saving || !ready} maxWidth={220} />
            <span className={styles.caption}>
              {current === 'ultra'
                ? 'Ultra asks the model for its strongest reasoning and lets it deploy sub-agents that explore and implement in parallel.'
                : "The model's own effort values. Only sent to models that support it."}
            </span>
          </span>
        </SettingsRow>
        <FormButtons onSave={save} onRevert={revert} dirty={isDirty} saving={saving} ready={ready} defaultAction />
        {message ? <InlineMessage message={message} /> : null}
      </SettingsGroup>
    </SettingsPage>
  )
}

// MARK: - Permissions

/** `PermissionsSettingsSection.lines(_:)`: non-empty trimmed lines, in order. */
function commandLines(text: string): string[] {
  return text
    .split(/\r\n|\r|\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

function sameLines(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((line, index) => line === b[index])
}

/**
 * `PermissionsSettingsSection` (SettingsSections.swift): default permission
 * profile (with the profile descriptions), network commands, allowed /
 * denied command prefixes, all through config/patch.
 */
export function PermissionsSettingsSection() {
  const config = useApp((s) => s.config)
  const ready = useReady()
  const permissions = config?.permissions ?? null
  const [lists, setLists] = useState<{ allowed: string; denied: string; synced: PermissionSettings | null }>({ allowed: '', denied: '', synced: null })
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  const listsDirtyIn = (state: { allowed: string; denied: string; synced: PermissionSettings | null }) =>
    state.synced !== null &&
    (!sameLines(commandLines(state.allowed), state.synced.allowed_commands) || !sameLines(commandLines(state.denied), state.synced.denied_commands))

  const sync = (next: PermissionSettings | null, force: boolean) => {
    if (!next) return
    setLists((previous) => {
      const keepEdits = !force && listsDirtyIn(previous)
      return keepEdits
        ? { ...previous, synced: next }
        : { allowed: next.allowed_commands.join('\n'), denied: next.denied_commands.join('\n'), synced: next }
    })
    if (force) setMessage(null)
  }

  // `.onChange(of: appState.config?.permissions, initial: true)`.
  const permissionsKey = equatableKey(permissions)
  useEffect(() => {
    sync(permissions, false)
  }, [permissionsKey])

  const listsDirty = listsDirtyIn(lists)

  const apply = async (values: JSONObject, success: string) => {
    if (!ready) return
    setSaving(true)
    setMessage(null)
    try {
      await useApp.getState().patchConfig(values)
      setMessage(SettingsMessage.success(success))
    } catch (error) {
      setMessage(SettingsMessage.error(errorText(error)))
    } finally {
      setSaving(false)
    }
  }

  const saveLists = () => {
    const synced = lists.synced
    if (!synced || saving) return
    const values: JSONObject = {}
    const allowedLines = commandLines(lists.allowed)
    const deniedLines = commandLines(lists.denied)
    if (!sameLines(allowedLines, synced.allowed_commands)) values['permissions.allowed_commands'] = allowedLines
    if (!sameLines(deniedLines, synced.denied_commands)) values['permissions.denied_commands'] = deniedLines
    if (Object.keys(values).length === 0) return
    void apply(values, 'Command lists saved. They apply to projects opened from now on.')
  }

  if (!config) {
    return (
      <SettingsPage>
        <BackendUnavailable />
      </SettingsPage>
    )
  }

  const setNetwork = (value: boolean) => {
    if (value === config.permissions.allow_network_commands) return
    void apply({ 'permissions.allow_network_commands': value }, value ? 'Network commands run without asking.' : 'Network commands ask for approval.')
  }

  return (
    <SettingsPage>
      <SettingsGroup
        title="Default profile"
        footer="The profile every new run starts with. The permissions menu in the composer switches one project's mode for its next runs without changing this default."
      >
        {permissionProfileOptions(config.permissions.profile).map((option) => (
          <ProfileOptionRow
            key={option.id}
            option={option}
            isSelected={option.id === config.permissions.profile}
            disabled={saving || !ready}
            onSelect={() => void apply({ 'permissions.profile': option.id }, `Default profile set to ${option.title}.`)}
          />
        ))}
      </SettingsGroup>

      <SettingsGroup
        title="Commands"
        footer="One command prefix per line. Allowed prefixes run without asking under every profile (destructive commands still ask); denied prefixes never run, whatever the profile."
      >
        <Toggle
          label="Allow network commands without asking (Auto profile)"
          checked={config.permissions.allow_network_commands}
          onChange={setNetwork}
          disabled={saving || !ready}
        />
        <SettingsRow label="Allowed" alignment="top">
          <CommandListEditor label="Allowed" text={lists.allowed} onChange={(allowed) => setLists((previous) => ({ ...previous, allowed }))} placeholder={'cargo test\nnpm run lint'} />
        </SettingsRow>
        <SettingsRow label="Denied" alignment="top">
          <CommandListEditor label="Denied" text={lists.denied} onChange={(denied) => setLists((previous) => ({ ...previous, denied }))} placeholder={'git push --force\nrm -rf'} />
        </SettingsRow>
        <FormButtons onSave={saveLists} onRevert={() => sync(config.permissions, true)} dirty={listsDirty} saving={saving} ready={ready} />
      </SettingsGroup>

      {message ? <InlineMessage message={message} /> : null}
    </SettingsPage>
  )
}

/** `ProfileOptionRow`: a radio row with the profile's title, name and description. */
function ProfileOptionRow({ option, isSelected, disabled, onSelect }: { option: PermissionProfileOption; isSelected: boolean; disabled: boolean; onSelect: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={isSelected}
      aria-label={`${option.title}: ${option.description}`}
      disabled={disabled}
      className={[styles.profileRow, isSelected ? styles.profileSelected : ''].filter(Boolean).join(' ')}
      onClick={onSelect}
    >
      <RadioMark on={isSelected} />
      <span className={styles.profileText}>
        <span className={styles.profileTitleLine}>
          <span className={styles.profileTitle}>{option.title}</span>
          <span className={styles.profileId}>{option.id}</span>
        </span>
        <span className={styles.profileDescription}>{option.description}</span>
      </span>
    </button>
  )
}

/// `Image(systemName: isSelected ? Icons.radioOn : Icons.radioOff)` at 14 pt:
/// "circle", and "largecircle.fill.circle" (a ring around a large dot, which
/// lucide's CircleDot is not) drawn at the symbol's size.
function RadioMark({ on }: { on: boolean }) {
  return (
    <svg className={`${styles.radioIcon} ${on ? styles.radioOn : styles.radioOff}`} width={16} height={16} viewBox="0 0 16 16" aria-hidden="true">
      <circle cx={8} cy={8} r={6.6} fill="none" stroke="currentColor" strokeWidth={1.3} />
      {on ? <circle cx={8} cy={8} r={4} fill="currentColor" /> : null}
    </svg>
  )
}

/** `CommandListEditor`: multi-line, monospace list of command prefixes (one per line). */
function CommandListEditor({ label, text, onChange, placeholder }: { label: string; text: string; onChange: (text: string) => void; placeholder: string }) {
  return (
    <textarea
      className={styles.commandList}
      aria-label={`${label} commands`}
      value={text}
      placeholder={placeholder}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      onChange={(event) => onChange(event.target.value)}
    />
  )
}

// MARK: - Appearance

const darkQuery = typeof window !== 'undefined' && typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-color-scheme: dark)') : null

/** `prefers-color-scheme: dark`, followed live. */
function useSystemDark(): boolean {
  const [dark, setDark] = useState(darkQuery?.matches ?? false)
  useEffect(() => {
    if (!darkQuery) return
    const query = darkQuery
    const update = () => setDark(query.matches)
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  }, [])
  return dark
}

/** `AppearanceSettingsSection` (SettingsSections.swift): System / Light / Dark (`appearance`, applied live). */
export function AppearanceSettingsSection() {
  const [appearance, setAppearance] = usePrefState('appearance')
  const systemDark = useSystemDark()
  // The app's effective appearance (`NSApp.effectiveAppearance`) for the System swatch.
  const effectiveDark = appearance === 'dark' || (appearance === 'system' && systemDark)
  return (
    <SettingsPage>
      <SettingsGroup
        title="Appearance"
        footer={`System follows the ${systemName} appearance; Light and Dark pin the app. Transcript text, code blocks and diffs use matching light and dark palettes.`}
      >
        <SegmentedPicker<Appearance>
          aria-label="Appearance"
          maxWidth={320}
          value={appearance}
          options={APPEARANCES.map((value) => ({ value, label: appearanceLabel(value) }))}
          onChange={setAppearance}
        />
        <div className={styles.swatches}>
          {APPEARANCES.map((value) => (
            <AppearanceSwatch
              key={value}
              appearance={value}
              isSelected={appearance === value}
              isDark={value === 'dark' || (value === 'system' && effectiveDark)}
              onSelect={() => setAppearance(value)}
            />
          ))}
        </div>
      </SettingsGroup>
    </SettingsPage>
  )
}

/** `AppearanceSwatch`: a tiny window mock-up showing the palette of one appearance. */
function AppearanceSwatch({ appearance, isSelected, isDark, onSelect }: { appearance: Appearance; isSelected: boolean; isDark: boolean; onSelect: () => void }) {
  return (
    <button
      type="button"
      aria-label={`${appearanceLabel(appearance)} appearance`}
      aria-pressed={isSelected}
      className={[styles.swatch, isDark ? styles.swatchDark : '', isSelected ? styles.swatchSelected : ''].filter(Boolean).join(' ')}
      onClick={onSelect}
    >
      <span className={styles.swatchWindow} aria-hidden="true">
        <span className={styles.swatchSidebar} />
        <span className={styles.swatchBody}>
          <span className={styles.swatchLine} style={{ width: 36 }} />
          <span className={`${styles.swatchLine} ${styles.swatchLineFaint}`} style={{ width: 48 }} />
          <span className={styles.swatchSpacer} />
          <span className={styles.swatchComposer} />
        </span>
      </span>
      <span className={styles.swatchLabel}>{appearanceLabel(appearance)}</span>
    </button>
  )
}

// MARK: - AppState helpers

/// `AppState.probeEndpointFromSettings()`: `doctor/run` without a project, so
/// the result lands in `doctorReport` for Settings even while a project is
/// selected (`runDoctor(probe)` routes to the selected project's card instead).
export async function probeEndpointFromSettings(): Promise<void> {
  if (useApp.getState().doctorRunning) return
  useApp.setState((s) => ({ doctorRunning: true, version: s.version + 1 }))
  try {
    const report = await backend.doctorRun(null, true)
    useApp.setState((s) => ({ doctorReport: report, doctorError: null, version: s.version + 1 }))
  } catch (error) {
    useApp.setState((s) => ({ doctorError: errorText(error), version: s.version + 1 }))
  } finally {
    useApp.setState((s) => ({ doctorRunning: false, version: s.version + 1 }))
  }
}

// MARK: - Shared chrome

/** `SettingsPage`: scrollable page with the canvas background and 20 px padding, no wider than the readable width. */
export function SettingsPage({ children }: { children: ReactNode }) {
  return (
    <div className={styles.page}>
      <div className={styles.pageContent}>{children}</div>
    </div>
  )
}

export interface SettingsGroupProps {
  title: string
  footer?: string | null
  children?: ReactNode
}

/** `SettingsGroup`: a titled card with an optional footer line. */
export function SettingsGroup({ title, footer, children }: SettingsGroupProps) {
  return (
    <section className={`card ${styles.group}`} aria-label={title}>
      <h2 className={styles.groupTitle}>{title}</h2>
      <div className={styles.groupContent}>{children}</div>
      {footer ? <p className={styles.footer}>{footer}</p> : null}
    </section>
  )
}

export interface SettingsRowProps {
  label: string
  /** `VerticalAlignment`: `.firstTextBaseline` (the default), `.top` or `.center`. */
  alignment?: 'firstTextBaseline' | 'top' | 'center'
  children?: ReactNode
}

/** `SettingsRow`: label column (128 px, right-aligned) + control. */
export function SettingsRow({ label, alignment = 'firstTextBaseline', children }: SettingsRowProps) {
  const align = alignment === 'top' ? styles.alignTop : alignment === 'center' ? styles.alignCenter : styles.alignBaseline
  return (
    <div className={`${styles.row} ${align}`}>
      <span className={styles.rowLabel}>{label}</span>
      <div className={styles.rowContent}>{children}</div>
    </div>
  )
}

/** `SettingsMessage`: one line of feedback under a form (the shared `InlineMessage`'s data). */
export type SettingsMessage = InlineMessageData

/** `SettingsMessage.success(_:)` / `.error(_:)` / `.info(_:)`. */
export const SettingsMessage = {
  success: (text: string): SettingsMessage => ({ kind: 'success', text }),
  error: (text: string): SettingsMessage => ({ kind: 'error', text }),
  info: (text: string): SettingsMessage => ({ kind: 'info', text }),
} as const

/** `InlineMessage`: one-line feedback under a form (components/InlineMessage, shared with Apps). */
export { InlineMessage }

/** `PathValue`: monospace path with Copy and Reveal in Finder (Show in Explorer). */
function PathValue({ path }: { path: string | null }) {
  if (path === null || path.length === 0) {
    return (
      <span className={styles.unknown} aria-label="Unknown until the backend connects">
        —
      </span>
    )
  }
  // `.lineLimit(1).truncationMode(.middle)`: as wide as the path, and
  // dropping characters from its middle when the row is narrower. The hidden
  // sizer gives the measured text its natural width, so the buttons follow
  // the path instead of the row's trailing edge.
  return (
    <span className={styles.pathValue}>
      <span className={styles.pathFit}>
        <MiddleTruncate text={path} className={`selectable ${styles.pathText}`} />
        <span className={styles.pathSizer} aria-hidden="true">
          {path}
        </span>
      </span>
      <CopyButton text={path} label="Copy path" />
      <IconButton
        icon="folder"
        label={revealLabel}
        size={13}
        onClick={() => {
          void invoke('reveal_in_file_manager', { path }).catch(() => undefined)
        }}
      />
    </span>
  )
}

/**
 * `BackendUnavailable`: shown by the forms that need `config` while the
 * backend is down; while ctx is starting or its configuration has not
 * arrived, a skeleton of the page (`skeleton`: rows per card).
 */
export function BackendUnavailable({ skeleton = [3, 2] }: { skeleton?: number[] }) {
  const connection = useApp((s) => s.connection)
  const hasConfig = useApp((s) => s.config !== null)
  const backendStarting = useApp((s) => s.backendStarting)
  const loading = backendStarting || connection.kind === 'connecting' || (connection.kind === 'ready' && !hasConfig)
  if (loading) return <Skeleton.SettingsPage rows={skeleton} label="Loading settings" />
  const failure = connection.kind === 'failed' ? describeFailure(connection.failure) : null
  // Swift's "Starting the backend…" / "Settings load once ctx answers." apply
  // while connecting, which always shows the skeleton above.
  const title = failure?.title ?? 'Backend not connected'
  const subtitle = failure?.detail ?? 'The configuration is read from ctx once it is running.'
  return (
    <div className={styles.unavailable}>
      <EmptyPlaceholder icon="warning" title={title} subtitle={subtitle} actionTitle="Retry" onAction={restartBackend} />
    </div>
  )
}

/** `DoctorResultView`: `DoctorReport` as sections of label/value rows with ✓/✗ marks. */
export function DoctorResultView({ report }: { report: DoctorReport }) {
  const clean = report.problems === 0
  return (
    <div className={styles.doctor}>
      <div className={`${styles.doctorSummary} ${clean ? styles.ok : styles.problem}`}>
        <span className={styles.doctorSummaryMark} aria-hidden="true">
          {clean ? '✓' : '✗'}
        </span>
        <span>{clean ? 'No problems found' : `${report.problems} problem${report.problems === 1 ? '' : 's'} found`}</span>
      </div>
      {report.sections.map((section, sectionIndex) => (
        <div key={`${section.title}-${sectionIndex}`} className={styles.doctorSection}>
          <span className={styles.doctorSectionTitle}>{section.title.charAt(0).toUpperCase() + section.title.slice(1)}</span>
          {section.rows.map((row, index) => (
            <div key={index} className={styles.doctorRow} role="group" aria-label={`${row.ok ? 'OK' : 'Problem'}: ${row.label} ${row.value}`}>
              <span className={`${styles.doctorMark} ${row.ok ? styles.ok : styles.problem}`} aria-hidden="true">
                {row.ok ? '✓' : '✗'}
              </span>
              <span className={styles.doctorLabel}>{row.label}</span>
              <MiddleTruncate lines={3} text={row.value} className={`selectable ${styles.doctorValue} ${row.ok ? styles.primary : styles.problem}`} />
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}
