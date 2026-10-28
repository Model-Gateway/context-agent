// Port of apps/macos/ContextAgent/Views/Remote/RemoteHostsSettingsSection.swift:
// Settings → Remote hosts: the SSH hosts Context can run `ctx serve` on,
// like Claude Code's and Codex's remote sessions. Each row shows the host's
// connection state and opens the editor; "Add host…" offers the `Host`
// aliases of ~/.ssh/config. The editor tests the connection the same way
// the app connects (`ssh … ctx serve`, then `initialize`).

import { useEffect, useRef, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { confirm } from '../../components/dialogs'
import { IconButton } from '../../components/IconButton'
import { InlineMessage, type InlineMessageData } from '../../components/InlineMessage'
import { Menu, type MenuItem } from '../../components/Menu'
import { Sheet } from '../../components/Sheet'
import { isMac, modLabel } from '../../components/platform'
import { hostDisplayName, hostIsValid, hostTarget, newSSHHost, normalizeHost, sshConfigAliases, useSSHHosts, type SSHHost } from '../../models/sshHost'
import { remoteConnection, useApp } from '../../state/app'
import { projectsOn } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { SettingsGroup, SettingsPage, SettingsRow } from './SettingsSections'
import shared from './SettingsSections.module.css'
import { TextField } from './SettingsControls'
import styles from './RemoteHostsSettingsSection.module.css'

const HOSTS_FOOTER =
  'Context runs ctx serve on the host over ssh and opens projects that live there: files, index, chats, API key and configuration are all the host’s, so ctx must be installed there (cargo install --path crates/cli, or copy a release binary to ~/.cargo/bin) and have a model configured. Authentication is ssh’s: keys and ssh-agent work, passwords cannot be typed; ~/.ssh/config aliases apply, so a ProxyJump or IdentityFile set there is used.'

const HOW_FOOTER =
  'Projects on a host appear in the sidebar’s Remote section under the host and are remembered across launches. The Terminal tab opens ssh shells in the project’s folder; images and PDFs of a remote project are not shown in the Editor; attachments you send are uploaded with the message.'

const SSH_LINE = 'ssh -o BatchMode=yes -T [-p port] [-i key] [-l user] <host> -- env PATH="$HOME/.cargo/bin:…:$PATH" ctx serve'

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
            Add host…
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

/**
 * `HostEditorSheet`: add / edit a host: name, address (with ~/.ssh/config
 * suggestions), user, port, identity file, the remote ctx path and extra
 * ssh options, with Test connection and Save.
 */
function HostEditorSheet({ host: initial, isNew, onClose }: { host: SSHHost; isNew: boolean; onClose: () => void }) {
  const [host, setHost] = useState<SSHHost>(initial)
  const [portText, setPortText] = useState(initial.port > 0 ? String(initial.port) : '')
  const [extraArguments, setExtraArguments] = useState(initial.extraArguments.join(' '))
  const [testing, setTesting] = useState(false)
  const [message, setMessage] = useState<InlineMessageData | null>(null)
  const aliases = useConfigAliases()
  const aliasButtonRef = useRef<HTMLButtonElement>(null)
  const [aliasMenuOpen, setAliasMenuOpen] = useState(false)
  const addressRef = useRef<HTMLInputElement>(null)

  const set = <K extends keyof SSHHost>(key: K, value: SSHHost[K]) => setHost((current) => ({ ...current, [key]: value }))

  /** The form's host with the option field split on whitespace. */
  const draft = (): SSHHost =>
    normalizeHost({
      ...host,
      port: Number.parseInt(portText.trim(), 10) || 0,
      extraArguments: extraArguments.split(/\s+/).filter((argument) => argument.length > 0),
    })
  const valid = hostIsValid(draft())

  const chooseIdentity = async () => {
    try {
      const picked = await openDialog({ directory: false, multiple: false, title: 'Choose the private key for this host' })
      const path = Array.isArray(picked) ? picked[0] : picked
      if (path) set('identityFile', path)
    } catch {
      // The dialog is unavailable (the browser preview): type the path.
    }
  }

  const test = async () => {
    const probe = draft()
    setTesting(true)
    setMessage({ kind: 'info', text: `Connecting to ${hostTarget(probe)} and starting ctx serve…` })
    try {
      const result = await useApp.getState().testHost(probe)
      setMessage(result.ok ? { kind: 'success', text: `Connected: ${result.summary}` } : { kind: 'error', text: result.message })
    } finally {
      setTesting(false)
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
    onSelect: () => setHost((current) => ({ ...current, address: alias, name: current.name.length === 0 ? alias : current.name })),
  }))

  return (
    <Sheet
      open
      onClose={onClose}
      title={isNew ? 'Add a remote host' : `Edit ${hostDisplayName(initial)}`}
      width={560}
      initialFocus={addressRef}
      footer={
        <>
          <Button variant="bordered" disabled={!valid || testing} loading={testing} onClick={() => void test()}>
            Test connection
          </Button>
          <span className={styles.footerSpacer} />
          <Button variant="bordered" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="prominent" disabled={!valid} onClick={save}>
            {isNew ? 'Add' : 'Save'}
          </Button>
        </>
      }
    >
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault()
          save()
        }}
      >
        <label className={styles.label} htmlFor="host-address">
          Address
        </label>
        <div className={styles.fieldRow}>
          <TextField value={host.address} onChange={(value) => set('address', value)} placeholder="devbox, 10.0.0.5 or a ~/.ssh/config alias" aria-label="Address" className={styles.grow} />
          {aliases.length > 0 ? (
            <>
              <IconButton ref={aliasButtonRef} icon="chevronDown" label="Hosts from ~/.ssh/config" size={11} aria-haspopup="menu" aria-expanded={aliasMenuOpen} onClick={() => setAliasMenuOpen((shown) => !shown)} />
              <Menu open={aliasMenuOpen} onClose={() => setAliasMenuOpen(false)} items={aliasItems} anchorRef={aliasButtonRef} align="end" aria-label="Hosts from ~/.ssh/config" />
            </>
          ) : null}
        </div>
        <label className={styles.label}>Name</label>
        <TextField value={host.name} onChange={(value) => set('name', value)} placeholder="Shown in the sidebar (defaults to the address)" aria-label="Name" />
        <label className={styles.label}>User</label>
        <TextField value={host.user} onChange={(value) => set('user', value)} placeholder="ssh’s default, or the config file’s" aria-label="User" />
        <label className={styles.label}>Port</label>
        <div className={styles.fieldRow}>
          <TextField value={portText} onChange={setPortText} placeholder="22" aria-label="Port" width={90} />
        </div>
        <label className={styles.label}>Identity file</label>
        <div className={styles.fieldRow}>
          <TextField value={host.identityFile} onChange={(value) => set('identityFile', value)} placeholder="~/.ssh/id_ed25519 (optional)" aria-label="Identity file" className={styles.grow} />
          <Button size="small" onClick={() => void chooseIdentity()}>
            Choose…
          </Button>
        </div>
        <label className={styles.label}>ctx path</label>
        <TextField value={host.ctxPath} onChange={(value) => set('ctxPath', value)} placeholder="ctx on the host’s PATH, or /path/to/ctx" aria-label="ctx path" />
        <label className={styles.label}>ssh options</label>
        <TextField value={extraArguments} onChange={setExtraArguments} placeholder="-o ProxyJump=bastion -o IdentitiesOnly=yes (optional)" aria-label="ssh options" className={styles.mono} />
        {/* Return in a field saves (`.keyboardShortcut(.defaultAction)`). */}
        <button type="submit" className="sr-only" tabIndex={-1} aria-hidden="true" disabled={!valid} />
      </form>
      {message ? (
        <div className={styles.message}>
          <InlineMessage message={message} />
        </div>
      ) : null}
    </Sheet>
  )
}
