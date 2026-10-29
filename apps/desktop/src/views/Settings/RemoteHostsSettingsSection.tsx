// Port of apps/macos/ContextAgent/Views/Remote/RemoteHostsSettingsSection.swift:
// Settings → Remote hosts: the SSH hosts Context can run `ctx serve` on,
// like Claude Code's and Codex's remote sessions. Each row shows the host's
// connection state and opens the editor; "Add host…" offers the `Host`
// aliases of ~/.ssh/config. The editor tests the connection the same way
// the app connects (`ssh … ctx serve`, then `initialize`).

import { useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { confirm } from '../../components/dialogs'
import { IconButton } from '../../components/IconButton'
import { InlineMessage } from '../../components/InlineMessage'
import { Menu, type MenuItem } from '../../components/Menu'
import { Sheet } from '../../components/Sheet'
import { toasts } from '../../components/toasts'
import { isMac, modLabel } from '../../components/platform'
import { hostDisplayName, hostIsValid, hostLogin, hostTarget, newSSHHost, normalizeHost, splitLogin, sshConfigAliases, useSSHHosts, type SSHHost } from '../../models/sshHost'
import { remoteConnection, useApp } from '../../state/app'
import { INSTALL_DIRECTORY, isOlderVersion } from '../../state/remote'
import { describe as describeFailure } from '../../backend/errors'
import { projectsOn } from '../../state/selectors'
import { useUpdater } from '../../state/updater'
import { Icon } from '../../theme/icons'
import { SettingsGroup, SettingsPage, SettingsRow } from './SettingsSections'
import shared from './SettingsSections.module.css'
import { TextField } from './SettingsControls'
import styles from './RemoteHostsSettingsSection.module.css'

const HOSTS_FOOTER =
  'Context runs ctx serve on the host over ssh and opens projects that live there: files, index, chats and git are all the host’s. Adding a host installs ctx on it when it is missing or older than the app (the Linux build of this version, x86_64 or arm64, copied to ~/.local/bin over ssh: no root, Rust or internet access needed there). The host uses this computer’s sign-in and model, kept in its memory only, unless it has a model key of its own. Authentication is ssh’s: keys and ssh-agent work, passwords cannot be typed; ~/.ssh/config aliases apply, so a ProxyJump or IdentityFile set there is used.'

const HOW_FOOTER =
  'Projects on a host appear in the sidebar’s Remote section under the host and are remembered across launches. The Terminal tab opens ssh shells in the project’s folder; images and PDFs of a remote project are not shown in the Editor; attachments you send are uploaded with the message.'

const SSH_LINE = 'ssh -o BatchMode=yes -T [-p port] [-i key] [-l user] <host> -- env PATH="$HOME/.local/bin:$HOME/.cargo/bin:…:$PATH" ctx serve'

/** The `Host` aliases of ~/.ssh/config, read once per mount. */
function useConfigAliases(): string[] {
  const [aliases, setAliases] = useState<string[]>([])
  useEffect(() => {
    let live = true
    void sshConfigAliases().then((list) => {
      if (live) setAliases(list)
    })
    return () => {
      live = false
    }
  }, [])
  return aliases
}

/** `RemoteHostsSettingsSection`: the Hosts group (rows, Add host…, From ~/.ssh/config) and the How-it-works group. */
export function RemoteHostsSettingsSection() {
  const hosts = useSSHHosts()
  const [editing, setEditing] = useState<{ host: SSHHost; isNew: boolean } | null>(null)
  const aliases = useConfigAliases().filter((alias) => !hosts.some((host) => host.address === alias))
  const aliasButtonRef = useRef<HTMLButtonElement>(null)
  const [aliasMenuOpen, setAliasMenuOpen] = useState(false)
  const aliasItems: MenuItem[] = aliases.map((alias) => ({
    id: `alias-${alias}`,
    label: alias,
    onSelect: () => setEditing({ host: newSSHHost({ name: alias, address: alias }), isNew: true }),
  }))

  return (
    <SettingsPage>
      <SettingsGroup title="Hosts" footer={HOSTS_FOOTER}>
        {hosts.length === 0 ? (
          <p className={styles.empty}>No remote hosts yet.</p>
        ) : (
          <div className={styles.list} role="list" aria-label="Remote hosts">
            {hosts.map((host, index) => (
              <HostSettingsRow key={host.id} host={host} divider={index < hosts.length - 1} onEdit={() => setEditing({ host, isNew: false })} />
            ))}
          </div>
        )}
        <div className={shared.hstack8}>
          <Button size="small" onClick={() => setEditing({ host: newSSHHost(), isNew: true })}>
            Add SSH connection…
          </Button>
          {aliases.length > 0 ? (
            <>
              <Button ref={aliasButtonRef} size="small" aria-haspopup="menu" aria-expanded={aliasMenuOpen} onClick={() => setAliasMenuOpen((shown) => !shown)}>
                From ~/.ssh/config
                <Icon name="chevronDown" size={9} strokeWidth={2.5} />
              </Button>
              <Menu open={aliasMenuOpen} onClose={() => setAliasMenuOpen(false)} items={aliasItems} anchorRef={aliasButtonRef} aria-label="Hosts from ~/.ssh/config" />
            </>
          ) : null}
        </div>
      </SettingsGroup>

      <SettingsGroup title="How it works" footer={HOW_FOOTER}>
        <SettingsRow label="Open">
          <span className={styles.howText}>Welcome → Open on a remote host…, the host’s row in the sidebar, or {isMac ? '⌘⇧O' : `${modLabel}+Shift+O`}.</span>
        </SettingsRow>
        <SettingsRow label="ssh">
          <span className={`selectable ${styles.sshLine}`}>{SSH_LINE}</span>
        </SettingsRow>
      </SettingsGroup>

      {editing ? <HostEditorSheet key={editing.host.id} host={editing.host} isNew={editing.isNew} onClose={() => setEditing(null)} /> : null}
    </SettingsPage>
  )
}

// MARK: - Row

/** `HostSettingsRow`: the host's name and target, its state line with the open count and hint, and Connect / Disconnect · Edit… · Remove…. */
function HostSettingsRow({ host, divider, onEdit }: { host: SSHHost; divider: boolean; onEdit: () => void }) {
  // The connection's version is the change signal of its state and hint.
  const connection = useApp(() => remoteConnection(host.id))
  useApp(() => connection?.version ?? -1)
  const openCount = useApp((state) => projectsOn(state, host.id).length)
  const anyRunning = useApp((state) => projectsOn(state, host.id).some((project) => project.isRunning))
  const name = hostDisplayName(host)
  const target = hostTarget(host)
  const kind = connection?.state.kind ?? 'none'
  const tint = kind === 'ready' ? 'var(--success)' : kind === 'failed' ? 'var(--danger)' : kind === 'connecting' ? 'var(--warning)' : 'var(--text-tertiary)'
  const appVersion = useUpdater((state) => state.status?.version ?? null)
  const outdated = connection?.isOutdated ?? false
  const installing = connection?.installing ?? false

  const install = async () => {
    const result = await useApp.getState().installCtx(host)
    if (!result.ok) toasts.push({ message: `Could not install ctx on ${name}.`, level: 'error', details: result.message })
  }

  const remove = async () => {
    const confirmed = await confirm({
      title: `Remove “${name}”?`,
      message: 'Its open projects close and its recents are forgotten. Nothing on the host is touched.',
      confirmLabel: 'Remove',
      destructive: true,
    })
    if (confirmed) await useApp.getState().removeHost(host)
  }

  return (
    <div role="listitem" className={[styles.row, divider ? styles.rowDivider : ''].filter(Boolean).join(' ')}>
      <span className={styles.rowIcon} aria-hidden="true">
        <Icon name="remoteHost" size={14} strokeWidth={2} />
      </span>
      <div className={styles.rowText}>
        <div className={styles.rowTitleLine}>
          <span className={styles.rowTitle}>{name}</span>
          {name !== target ? <span className={styles.rowTarget}>{target}</span> : null}
        </div>
        <div className={styles.rowStatusLine}>
          {kind === 'connecting' ? <BrailleSpinner size={10} tint="var(--text-tertiary)" /> : <span className={styles.rowDot} style={{ background: tint }} aria-hidden="true" />}
          <span className={styles.rowStatus}>{connection ? connection.statusText : 'Not connected'}</span>
          {openCount > 0 ? <Chip>{`${openCount} open`}</Chip> : null}
        </div>
        {connection && kind === 'failed' && connection.hint !== null ? <p className={`selectable ${styles.rowHint}`}>{connection.hint}</p> : null}
      </div>
      <div className={styles.rowButtons}>
        {connection && (connection.needsCtx || outdated) ? (
          <Button
            size="small"
            loading={installing}
            disabled={anyRunning}
            title={outdated ? `Replace ctx on the host with the build of this version${appVersion ? ` (${appVersion})` : ''}` : `Copy ctx${appVersion ? ` ${appVersion}` : ''} for Linux to ${INSTALL_DIRECTORY} on the host`}
            onClick={() => void install()}
          >
            {outdated ? 'Update ctx' : 'Install ctx'}
          </Button>
        ) : null}
        {kind === 'ready' ? (
          <Button size="small" disabled={anyRunning} title={anyRunning ? 'Finish or cancel the running tasks first' : undefined} onClick={() => void useApp.getState().disconnectHost(host)}>
            Disconnect
          </Button>
        ) : (
          <Button size="small" disabled={kind === 'connecting'} onClick={() => void useApp.getState().reconnectHost(host)}>
            {connection?.failure ? 'Reconnect' : 'Connect'}
          </Button>
        )}
        <Button size="small" onClick={onEdit}>
          Edit…
        </Button>
        <Button size="small" variant="destructive" onClick={() => void remove()}>
          Remove…
        </Button>
      </div>
    </div>
  )
}

// MARK: - Editor

/** One line of the editor's progress list. */
interface Step {
  id: 'connect' | 'install' | 'signIn'
  state: 'running' | 'done' | 'failed' | 'warning'
  text: string
  /** Bytes of the ctx archive, while it is downloaded or uploaded. */
  bytes?: { done: number; total: number | null }
}

/** `ssh:install-progress` (src-tauri/src/backend/install.rs). */
interface InstallProgress {
  host: string
  phase: 'probe' | 'download' | 'upload' | 'verify'
  done: number
  total: number | null
}

function megabytes(bytes: number): string {
  return (bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)
}

/** "Uploading ctx to the host (3.1/9.8 MB)…" for an install phase. */
function installText(progress: InstallProgress, version: string | null): string {
  const of = (done: number, total: number | null) => (total === null ? `${megabytes(done)} MB` : `${megabytes(done)}/${megabytes(total)} MB`)
  switch (progress.phase) {
    case 'probe':
      return 'Checking what the host runs…'
    case 'download':
      return `Downloading ctx${version ? ` ${version}` : ''} for Linux (${of(progress.done, progress.total)})…`
    case 'upload':
      return `Uploading ctx to the host (${of(progress.done, progress.total)})…`
    case 'verify':
      return `Unpacking ctx in ${INSTALL_DIRECTORY}…`
  }
}

/**
 * `HostEditorSheet`: Add SSH connection / edit a host, laid out like Claude
 * Code's: name, SSH host (user@host or a ~/.ssh/config alias), port and key,
 * with the ctx path and extra ssh options under Advanced. Adding connects
 * the way the app will: when ctx is missing or older than the app it is
 * installed on the spot (with the download and upload shown), the host is
 * saved and connected, and this computer's sign-in is forwarded to it.
 */
function HostEditorSheet({ host: initial, isNew, onClose }: { host: SSHHost; isNew: boolean; onClose: () => void }) {
  const [host, setHost] = useState<SSHHost>(initial)
  const [login, setLogin] = useState(hostLogin(initial))
  const [portText, setPortText] = useState(initial.port > 0 ? String(initial.port) : '')
  const [extraArguments, setExtraArguments] = useState(initial.extraArguments.join(' '))
  const [advanced, setAdvanced] = useState(initial.ctxPath.length > 0 || initial.extraArguments.length > 0)
  const [busy, setBusy] = useState(false)
  const [steps, setSteps] = useState<Step[]>([])
  const [error, setError] = useState<string | null>(null)
  /** The last attempt failed after the fields were fine: Add anyway saves without a connection. */
  const [failedOnce, setFailedOnce] = useState(false)
  const appVersion = useUpdater((state) => state.status?.version ?? null)
  const aliases = useConfigAliases()
  const aliasButtonRef = useRef<HTMLButtonElement>(null)
  const [aliasMenuOpen, setAliasMenuOpen] = useState(false)
  const live = useRef(true)
  useEffect(() => {
    live.current = true
    return () => {
      live.current = false
    }
  }, [])

  const set = <K extends keyof SSHHost>(key: K, value: SSHHost[K]) => setHost((current) => ({ ...current, [key]: value }))

  /** The form's host: the SSH host field split into user and address, the options on whitespace. */
  const draft = (): SSHHost =>
    normalizeHost({
      ...host,
      ...splitLogin(login),
      port: Number.parseInt(portText.trim(), 10) || 0,
      extraArguments: extraArguments.split(/\s+/).filter((argument) => argument.length > 0),
    })
  const valid = hostIsValid(draft())

  // Install progress for this host, from the shell.
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let disposed = false
    void listen<InstallProgress>('ssh:install-progress', (event) => {
      if (event.payload.host !== initial.id) return
      const progress = event.payload
      setSteps((current) =>
        current.map((step) =>
          step.id === 'install' && step.state === 'running'
            ? { ...step, text: installText(progress, appVersion), bytes: progress.phase === 'download' || progress.phase === 'upload' ? { done: progress.done, total: progress.total } : undefined }
            : step,
        ),
      )
    })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [initial.id, appVersion])

  const chooseIdentity = async () => {
    try {
      const picked = await openDialog({ directory: false, multiple: false, title: 'Choose the private key for this host' })
      const path = Array.isArray(picked) ? picked[0] : picked
      if (path) set('identityFile', path)
    } catch {
      // The dialog is unavailable (the browser preview): type the path.
    }
  }

  const step = (id: Step['id'], state: Step['state'], text: string) => {
    if (!live.current) return
    setSteps((current) => [...current.filter((existing) => existing.id !== id), { id, state, text }].sort((a, b) => order(a.id) - order(b.id)))
  }

  const fail = (id: Step['id'], text: string, message: string) => {
    step(id, 'failed', text)
    if (live.current) {
      setError(message)
      setFailedOnce(true)
    }
  }

  /**
   * Connects like the app will (Test connection for an existing host; the
   * whole of Add for a new one): `ssh … ctx serve`, an install when ctx is
   * missing or older than the app, then — when `keep` — save, connect for
   * real and forward the sign-in. True when it got through.
   */
  const run = async (keep: boolean): Promise<boolean> => {
    let probe = draft()
    const target = hostTarget(probe)
    setBusy(true)
    setError(null)
    setSteps([])
    try {
      step('connect', 'running', `Connecting to ${target}…`)
      let tested = await useApp.getState().testHost(probe)
      const outdated = tested.ok && !tested.mock && appVersion !== null && isOlderVersion(tested.ctxVersion, appVersion)
      if ((!tested.ok && tested.needsCtx) || outdated) {
        step('connect', 'done', tested.ok ? `Connected to ${target}: ctx ${tested.ctxVersion} is older than this app` : `Connected to ${target}: ctx is not installed`)
        step('install', 'running', `Installing ctx${appVersion ? ` ${appVersion}` : ''}…`)
        const installed = await useApp.getState().installCtx(probe)
        if (!installed.ok) {
          fail('install', 'Could not install ctx', installed.message)
          return false
        }
        probe = { ...probe, ctxPath: '' }
        if (live.current) set('ctxPath', '')
        step('install', 'done', `Installed ${installed.installed} in ${INSTALL_DIRECTORY}`)
        step('connect', 'running', `Starting ctx on ${target}…`)
        tested = await useApp.getState().testHost(probe)
      }
      if (!tested.ok) {
        fail('connect', `Could not connect to ${target}`, tested.message)
        return false
      }
      step('connect', 'done', `Connected: ${tested.summary}`)
      if (!keep) return true
      const saved = useApp.getState().saveHost(probe)
      step('signIn', 'running', 'Signing the host in with this computer’s account…')
      const connection = await useApp.getState().connectHost(saved, true)
      const model = connection?.result?.model
      if (!connection || !model) {
        const failure = remoteConnection(saved.id)
        fail('signIn', 'Saved, but the connection did not come up', [failure?.failure ? describeFailure(failure.failure).summary : null, failure?.hint ?? null].filter(Boolean).join('\n'))
        return false
      }
      if (model.key_forwarded) step('signIn', 'done', 'Using this computer’s sign-in (kept in memory on the host)')
      else if (model.api_key_set) step('signIn', 'done', 'The host uses its own model key')
      else step('signIn', 'warning', 'No model key: sign in on this computer (Settings → Account) or configure a key on the host')
      return true
    } finally {
      if (live.current) setBusy(false)
    }
  }

  const add = async () => {
    if (!valid || busy) return
    if (await run(true)) {
      toasts.push({ message: `Connected to ${hostDisplayName(draft())}.`, level: 'success' })
      onClose()
    }
  }

  const save = () => {
    if (!valid) return
    useApp.getState().saveHost(draft())
    onClose()
  }

  const aliasItems: MenuItem[] = aliases.map((alias) => ({
    id: `alias-${alias}`,
    label: alias,
    onSelect: () => {
      setLogin(alias)
      setHost((current) => ({ ...current, name: current.name.length === 0 ? alias : current.name }))
    },
  }))

  return (
    <Sheet
      open
      onClose={busy ? () => undefined : onClose}
      title={isNew ? 'Add SSH connection' : `Edit ${hostDisplayName(initial)}`}
      width={520}
      dismissOnBackdrop={!busy}
      footer={
        <>
          {!isNew ? (
            <Button variant="bordered" disabled={!valid || busy} loading={busy} onClick={() => void run(false)}>
              Test connection
            </Button>
          ) : null}
          <span className={styles.footerSpacer} />
          <Button variant="bordered" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          {isNew && failedOnce && !busy ? (
            <Button variant="bordered" disabled={!valid} title="Save the host without a working connection; connect from the sidebar later" onClick={save}>
              Add anyway
            </Button>
          ) : null}
          <Button variant="prominent" disabled={!valid || busy} loading={isNew && busy} onClick={isNew ? () => void add() : save}>
            {isNew ? 'Add SSH connection' : 'Save'}
          </Button>
        </>
      }
    >
      <p className={styles.intro}>Connect to a remote machine to run Context there. It uses this computer’s sign-in.</p>
      <form
        className={styles.stack}
        onSubmit={(event) => {
          event.preventDefault()
          if (isNew) void add()
          else save()
        }}
      >
        <div className={styles.field}>
          <label className={styles.fieldLabel}>Name</label>
          <TextField value={host.name} onChange={(value) => set('name', value)} placeholder="devbox" aria-label="Name" disabled={busy} />
          <p className={styles.help}>A friendly name for this SSH connection.</p>
        </div>
        <div className={styles.field}>
          <label className={styles.fieldLabel}>SSH host</label>
          <div className={styles.fieldRow}>
            <TextField value={login} onChange={setLogin} placeholder="root@192.168.1.10" aria-label="SSH host" className={styles.grow} disabled={busy} />
            {aliases.length > 0 ? (
              <>
                <IconButton ref={aliasButtonRef} icon="chevronDown" label="Hosts from ~/.ssh/config" size={11} aria-haspopup="menu" aria-expanded={aliasMenuOpen} onClick={() => setAliasMenuOpen((shown) => !shown)} />
                <Menu open={aliasMenuOpen} onClose={() => setAliasMenuOpen(false)} items={aliasItems} anchorRef={aliasButtonRef} align="end" aria-label="Hosts from ~/.ssh/config" />
              </>
            ) : null}
          </div>
          <p className={styles.help}>user@myserver.com, or a host from ~/.ssh/config.</p>
        </div>
        <div className={styles.field}>
          <label className={styles.fieldLabel}>SSH port</label>
          <TextField value={portText} onChange={setPortText} placeholder="22" aria-label="SSH port" width={110} disabled={busy} />
          <p className={styles.help}>Leave empty to use the default (22) or your SSH configuration.</p>
        </div>
        <div className={styles.field}>
          <label className={styles.fieldLabel}>SSH key (optional)</label>
          <div className={styles.fieldRow}>
            <TextField value={host.identityFile} onChange={(value) => set('identityFile', value)} placeholder="~/.ssh/id_ed25519" aria-label="SSH key" className={styles.grow} disabled={busy} />
            <Button size="small" disabled={busy} onClick={() => void chooseIdentity()}>
              Browse…
            </Button>
          </div>
          <p className={styles.help}>Path to a private key. Leave empty to use your SSH configuration or agent. Passwords cannot be typed here.</p>
        </div>
        <button type="button" className={styles.advancedToggle} aria-expanded={advanced} onClick={() => setAdvanced((shown) => !shown)}>
          <Icon name={advanced ? 'chevronDown' : 'chevronRight'} size={9} strokeWidth={2.5} />
          Advanced
        </button>
        {advanced ? (
          <>
            <div className={styles.field}>
              <label className={styles.fieldLabel}>ctx path</label>
              <TextField value={host.ctxPath} onChange={(value) => set('ctxPath', value)} placeholder={`${INSTALL_DIRECTORY}/ctx, installed for you when missing`} aria-label="ctx path" disabled={busy} />
            </div>
            <div className={styles.field}>
              <label className={styles.fieldLabel}>ssh options</label>
              <TextField value={extraArguments} onChange={setExtraArguments} placeholder="-o ProxyJump=bastion" aria-label="ssh options" className={styles.mono} disabled={busy} />
            </div>
          </>
        ) : null}
        {/* Return in a field adds or saves (`.keyboardShortcut(.defaultAction)`). */}
        <button type="submit" className="sr-only" tabIndex={-1} aria-hidden="true" disabled={!valid || busy} />
      </form>
      {steps.length > 0 ? (
        <ul className={styles.steps} aria-live="polite">
          {steps.map((item) => (
            <li key={item.id} className={styles.step} data-state={item.state}>
              <span className={styles.stepMark} aria-hidden="true">
                {item.state === 'running' ? <BrailleSpinner size={10} tint="var(--accent)" /> : <span className={styles.stepDot} />}
              </span>
              <span className={styles.stepBody}>
                <span className="selectable">{item.text}</span>
                {item.bytes ? (
                  <span className={styles.progress} role="progressbar" aria-valuemin={0} aria-valuemax={item.bytes.total ?? undefined} aria-valuenow={item.bytes.done}>
                    <span className={styles.progressFill} data-indeterminate={item.bytes.total === null} style={item.bytes.total ? { width: `${Math.min(100, (item.bytes.done / item.bytes.total) * 100)}%` } : undefined} />
                  </span>
                ) : null}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
      {error ? (
        <div className={styles.message}>
          <InlineMessage message={{ kind: 'error', text: error }} />
        </div>
      ) : null}
    </Sheet>
  )
}

function order(id: Step['id']): number {
  return id === 'connect' ? 0 : id === 'install' ? 1 : 2
}
