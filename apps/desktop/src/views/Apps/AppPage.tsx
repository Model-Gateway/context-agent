import { memo, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { McpCatalogApp, McpServerStatus, McpToolInfo, McpToolPermission } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { isMac } from '../../components/platform'
import { useApp } from '../../state/app'
import { useIsReady } from '../../state/selectors'
import { Icon, type IconName } from '../../theme/icons'
import { AppIcon } from './AppIcon'
import { ComputerUsePermissions } from './ComputerUsePermissions'
import { InlineMessage, type InlineMessageData as AppsMessage } from '../../components/InlineMessage'
import { PromptBanner } from './PromptBanner'
import { StandingChip } from './StandingChip'
import { Switch } from '../../components/Switch'
import { ToolPermissionButton } from './ToolPermissionButton'
import { usePageKeys } from './pageKeys'
import { COMPUTER_USE_ID, standingIsInstalled, standingOf, type AppStanding } from './standing'
import styles from './AppPage.module.css'

export interface AppPageProps {
  app: McpCatalogApp
  busy: boolean
  message: AppsMessage | null
  onBack: () => void
  onInstall: () => void
  onUninstall: () => void
  onReconnect: () => void
  onSetEnabled: (enabled: boolean) => void
  onSetToolEnabled: (tool: string, enabled: boolean) => void
  onSetBypass: (on: boolean) => void
  onSetToolPermission: (tool: string, permission: McpToolPermission | null) => void
  /** Update…: the connect page for the installed app. */
  onConfigure: () => void
}

/** `NSWorkspace.shared.open(url)` through the shell's `open_url` command. */
function openURL(url: string): void {
  invoke('open_url', { url }).catch(() => {
    /* Outside Tauri there is nothing to open with. */
  })
}

/** `String.capitalizedFirst`. */
function capitalizedFirst(text: string): string {
  return text === '' ? text : text.charAt(0).toUpperCase() + text.slice(1)
}

/** `statusLine(_:_:)`. */
function statusLine(standing: AppStanding, status: McpServerStatus | null): string {
  switch (standing.kind) {
    case 'connected': {
      const server = [status?.server_name, status?.server_version].filter((part): part is string => part != null).join(' ')
      return server === '' ? 'Connected' : `Connected • ${server}`
    }
    case 'connecting':
      return 'Connecting…'
    case 'failed':
      return 'Failed'
    case 'off':
      return 'Paused • turn it on to connect'
    case 'installed':
      return 'Not reported by the backend yet'
    case 'notInstalled':
      return 'Not connected'
  }
}

/** `Spacer(minLength:)`. */
function Spacer({ min }: { min: number }) {
  return <span className={styles.spacer} style={{ minWidth: min }} />
}

/**
 * `AppPage` (Views/Apps/AppsView.swift): one app's page — the "Apps / name"
 * breadcrumb, the header (tile, name, status, Homepage, Connect or
 * Disconnect), its banner, feedback, then for an installed app the Tools
 * card (counts, status with the Enabled switch and Reconnect, keys with
 * Update…) and, once connected, the tool permissions; Computer Use adds
 * its macOS permissions; About last. Esc goes back to the marketplace.
 */
export function AppPage(props: AppPageProps) {
  const { app, message, onBack } = props
  const config = useApp((state) => state.config)
  const servers = useApp((state) => state.mcpServers)
  const rootRef = useRef<HTMLDivElement>(null)
  const standing = standingOf(config, servers, app.id)
  const status = servers.find((server) => server.name === app.id) ?? null
  const installed = standingIsInstalled(standing)

  // Esc goes back to the marketplace (a keyboard shortcut on the
  // breadcrumb button itself would stop it taking clicks).
  usePageKeys(rootRef, { onEscape: onBack })

  return (
    <div ref={rootRef} className={styles.scroll}>
      <div className={styles.column}>
        <nav className={styles.breadcrumb} aria-label="Breadcrumb">
          <BreadcrumbButton title="Apps" icon="apps" onClick={onBack} />
          <span className={styles.slash}>/</span>
          <span className={styles.crumbName} aria-current="page">
            {app.name}
          </span>
        </nav>
        <Header {...props} standing={standing} />
        <PromptBanner apps={[app]} variant="app" />
        {message ? <InlineMessage message={message} /> : null}
        {standing.kind === 'failed' ? <InlineMessage message={{ kind: 'error', text: standing.error }} /> : null}
        {installed ? (
          <>
            <Overview {...props} standing={standing} status={status} />
            {status && status.state === 'connected' ? <Permissions {...props} status={status} /> : null}
          </>
        ) : (
          <About app={app} />
        )}
        {isMac && app.builtin && app.id === COMPUTER_USE_ID ? <ComputerUsePermissions /> : null}
        {installed ? <About app={app} /> : null}
      </div>
    </div>
  )
}

/// `BreadcrumbButton`: "⊞ Apps" — a plain button with a full hit area and
/// a hover tint, so the whole label takes the click.
function BreadcrumbButton({ title, icon, onClick }: { title: string; icon: IconName; onClick: () => void }) {
  return (
    <button type="button" className={styles.crumbButton} aria-label={`Back to ${title}`} onClick={onClick}>
      <Icon name={icon} size={12} />
      <span>{title}</span>
    </button>
  )
}

/** `header(_:)`. */
function Header({ app, busy, standing, onInstall, onUninstall }: AppPageProps & { standing: AppStanding }) {
  const isReady = useIsReady()
  const homepage = app.homepage ?? null
  return (
    <div className={styles.header}>
      <AppIcon app={app} size={44} />
      <div className={styles.headerRow}>
        <div className={styles.headerText}>
          <div className={styles.nameRow}>
            <h1 className={styles.name}>{app.name}</h1>
            <StandingChip standing={standing} />
          </div>
          <p className={styles.tagline}>{app.tagline + '.'}</p>
        </div>
        <Spacer min={12} />
        {homepage && isURL(homepage) ? (
          <Button onClick={() => openURL(homepage)}>
            <span className={styles.homepage}>
              Homepage
              <Icon name="externalLink" size={10} strokeWidth={2.25} />
            </span>
          </Button>
        ) : null}
        {busy ? (
          <BrailleSpinner />
        ) : standingIsInstalled(standing) ? (
          <Button variant="destructive" onClick={onUninstall}>
            Disconnect
          </Button>
        ) : (
          <Button variant="prominent" disabled={!isReady} onClick={onInstall}>
            Connect
          </Button>
        )}
      </div>
    </div>
  )
}

/** `URL(string:)` succeeds. */
function isURL(text: string): boolean {
  try {
    new URL(text)
    return true
  } catch {
    return false
  }
}

/** `overview(_:_:)`: like the sync card — counts, then the status line with Reconnect. */
function Overview({ app, busy, standing, status, onSetEnabled, onReconnect, onConfigure }: AppPageProps & { standing: AppStanding; status: McpServerStatus | null }) {
  const enabledInConfig = useApp((state) => state.config?.mcp.servers[app.id]?.enabled ?? false)
  const tools = status?.tools ?? []
  const enabled = tools.filter((tool) => tool.enabled)
  const readOnly = enabled.filter((tool) => tool.read_only).length
  return (
    <section className={styles.block} aria-label="Tools">
      <h2 className={styles.heading}>Tools</h2>
      <div className={styles.card}>
        <CountRow icon="mcp" label="Tools available" value={tools.length === 0 ? '—' : String(enabled.length)} />
        <div className={styles.divider} />
        <CountRow icon="check" label="Run without asking" value={tools.length === 0 ? '—' : String(readOnly)} />
        <div className={styles.divider} />
        <div className={[styles.lineRow, styles.tinted].join(' ')}>
          <span className={styles.lineLabel}>Status</span>
          <span className={styles.lineValue}>{statusLine(standing, status)}</span>
          <Spacer min={8} />
          <Switch label="Enabled" checked={enabledInConfig} disabled={busy} onChange={onSetEnabled} />
          <Button size="small" disabled={busy || standing.kind === 'off'} onClick={onReconnect}>
            Reconnect
          </Button>
        </div>
        {app.inputs.length > 0 ? (
          <>
            <div className={styles.divider} />
            <div className={[styles.lineRow, styles.tinted].join(' ')}>
              <span className={styles.lineLabel}>Keys</span>
              <span className={styles.keysValue}>{app.inputs.map((input) => input.label).join(', ')}</span>
              <Spacer min={8} />
              <Button size="small" disabled={busy} onClick={onConfigure}>
                Update…
              </Button>
            </div>
          </>
        ) : null}
      </div>
    </section>
  )
}

/** `countRow(icon:label:value:)`. */
function CountRow({ icon, label, value }: { icon: IconName; label: string; value: string }) {
  return (
    <div className={styles.countRow}>
      <div className={styles.countLabel}>
        <span className={styles.countIcon}>
          <Icon name={icon} size={12} />
        </span>
        <span>{label}</span>
      </div>
      <div className={styles.verticalDivider} />
      <span className={styles.countValue}>{value}</span>
    </div>
  )
}

/** `permissions(_:)`: one row per tool with its permission toggle (the entry's allowlist). */
function Permissions({ app, busy, status, onSetBypass, onSetToolEnabled, onSetToolPermission }: AppPageProps & { status: McpServerStatus }) {
  const server = useApp((state) => state.config?.mcp.servers[app.id] ?? null)
  const bypass = server?.bypass_permissions ?? false
  return (
    <section className={styles.block} aria-label="Tool permissions">
      <h2 className={styles.heading}>Tool permissions</h2>
      <div className={styles.card}>
        <div className={styles.bypassRow}>
          <span
            className={styles.bypassIcon}
            style={{
              color: bypass ? 'var(--warning)' : 'var(--text-secondary)',
              background: bypass ? 'color-mix(in srgb, var(--warning) 12%, transparent)' : 'var(--surface-raised)',
            }}
            aria-hidden="true"
          >
            <Icon name="boltShield" size={13} />
          </span>
          <div className={styles.bypassText}>
            <span className={styles.lineLabel}>Bypass permissions</span>
            <span className={styles.caption}>
              {bypass
                ? `Every ${app.name} tool runs without asking, except tools set to Always ask.`
                : `Run every ${app.name} tool without asking, destructive ones included.`}
            </span>
          </div>
          <Spacer min={8} />
          <Switch label="Bypass permissions" checked={bypass} disabled={busy || server === null} onChange={onSetBypass} />
        </div>
        {status.tools.length > 0 ? <div className={styles.divider} /> : null}
        {status.tools.length === 0 ? (
          <p className={styles.noTools}>The server offers no tools.</p>
        ) : (
          status.tools.map((tool, index) => (
            <div key={tool.qualified_name}>
              <ToolRow
                tool={tool}
                override={server?.permissions[tool.name] ?? null}
                bypass={bypass}
                busy={busy}
                hasServer={server !== null}
                onSetToolEnabled={onSetToolEnabled}
                onSetToolPermission={onSetToolPermission}
              />
              {index < status.tools.length - 1 ? <div className={[styles.divider, styles.insetDivider].join(' ')} /> : null}
            </div>
          ))
        )}
      </div>
      <p className={styles.footnote}>
        Off takes the tool out of the model's tool list for this app. Click a tool's permission icon to change when it asks. By default read-only tools
        run without asking under every profile, other tools ask under Ask, and destructive ones ask under Ask and Auto. The chat's Bypass profile never
        asks.
      </p>
    </section>
  )
}

interface ToolRowProps {
  tool: McpToolInfo
  override: McpToolPermission | null
  bypass: boolean
  busy: boolean
  hasServer: boolean
  onSetToolEnabled: (tool: string, enabled: boolean) => void
  onSetToolPermission: (tool: string, permission: McpToolPermission | null) => void
}

/** One tool: its name and permission icon, its description, the on/off switch. */
const ToolRow = memo(function ToolRow({ tool, override, bypass, busy, hasServer, onSetToolEnabled, onSetToolPermission }: ToolRowProps) {
  return (
    <div className={styles.toolRow}>
      <div className={styles.toolText}>
        <div className={styles.toolNameRow}>
          <span className={styles.lineLabel}>{capitalizedFirst(tool.name.replaceAll('_', ' '))}</span>
          <ToolPermissionButton
            tool={tool}
            override={override}
            bypass={bypass}
            enabled={!busy && hasServer}
            onSelect={(permission) => onSetToolPermission(tool.name, permission)}
          />
        </div>
        <span className={[styles.caption, styles.toolDescription].join(' ')}>{tool.description === '' ? tool.qualified_name : tool.description}</span>
      </div>
      <Spacer min={8} />
      <span className={styles.toolSwitch}>
        <Switch label={tool.name} checked={tool.enabled} disabled={busy} onChange={(on) => onSetToolEnabled(tool.name, on)} />
      </span>
    </div>
  )
})

/** `about`: the description, then By / Category / Requires / Needs. */
function About({ app }: { app: McpCatalogApp }) {
  return (
    <section className={styles.block} aria-label="About">
      <h2 className={styles.heading}>About</h2>
      <div className={[styles.card, styles.aboutCard].join(' ')}>
        <p className={styles.description}>{app.description}</p>
        <div className={styles.facts}>
          <Labeled label="By" value={app.vendor} />
          <Labeled label="Category" value={app.category} />
          {app.requirements.length > 0 ? <Labeled label="Requires" value={app.requirements.join(', ')} /> : null}
          {app.inputs.length > 0 ? (
            <Labeled label="Needs" value={app.inputs.map((input) => input.label + (input.required ? '' : ' (optional)')).join(', ')} />
          ) : null}
        </div>
      </div>
    </section>
  )
}

/** `labeled(_:_:)`. */
function Labeled({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.labeled}>
      <span className={styles.labeledLabel}>{label}</span>
      <span className={styles.labeledValue}>{value}</span>
    </div>
  )
}
