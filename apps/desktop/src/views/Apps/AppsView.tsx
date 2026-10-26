import { Component, createRef, useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { errorText } from '../../backend/errors'
import { mcpServerPatchValue, type JSONObject, type McpCatalogApp, type McpServerConfig, type McpToolPermission } from '../../backend/types'
import { confirm } from '../../components/dialogs'
import { useApp } from '../../state/app'
import { isReady, useIsReady } from '../../state/selectors'
import { AppPage } from './AppPage'
import { ConnectView } from './ConnectView'
import { CustomConnectView } from './CustomConnectView'
import type { InlineMessageData as AppsMessage } from '../../components/InlineMessage'
import { Marketplace } from './Marketplace'
import { useCatalog } from './standing'
import styles from './AppsView.module.css'

/** The configured entry of server `id` right now (`appState.config?.mcp.servers[id]`). */
function serverConfig(id: string): McpServerConfig | null {
  return useApp.getState().config?.mcp.servers[id] ?? null
}

/**
 * `AppsView` (Views/Apps/AppsView.swift), the sidebar's Apps page: a
 * marketplace of the MCP servers Context can install (`mcp/catalog`) —
 * title, search, a banner of example prompts, then the catalog two columns
 * wide with a "+" to install and a check for what is installed — and, for
 * one app, its own page (status, Connect / Disconnect, the tools it adds
 * with a permission toggle each, and for Computer Use the macOS
 * permissions). Every connection goes through the connect page; a server
 * of the user's own through the custom connect page. Installing writes the
 * server through `mcp/install`; the backend connects it and pushes
 * `mcp/status` as it comes up. No props.
 */
export function AppsView() {
  const ready = useIsReady()
  const catalog = useCatalog()
  const config = useApp((state) => state.config)
  const [selectedAppId, setSelectedAppId] = useState<string | null>(null)
  /** The app whose connect page is showing (Connect, "+", or Update…). */
  const [connectingAppId, setConnectingAppId] = useState<string | null>(null)
  /** The connect page for a server of the user's own, by URL. */
  const [addingCustom, setAddingCustom] = useState(false)
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set())
  const [message, setMessage] = useState<AppsMessage | null>(null)

  // `.task(id: appState.connection.isReady)`: the catalog, then the servers' state.
  useEffect(() => {
    if (!ready) return
    void (async () => {
      const state = useApp.getState()
      await state.loadMcpCatalog()
      await state.refreshMcpStatus()
    })()
  }, [ready])

  // MARK: Actions

  const run = useCallback((busyId: string | null, success: string | null, work: () => Promise<void>) => {
    if (!isReady(useApp.getState())) return
    if (busyId !== null) setBusy((current) => new Set(current).add(busyId))
    setMessage(null)
    void (async () => {
      try {
        await work()
        if (success !== null) setMessage({ kind: 'success', text: success })
      } catch (error) {
        setMessage({ kind: 'error', text: errorText(error) })
      } finally {
        if (busyId !== null) {
          setBusy((current) => {
            const next = new Set(current)
            next.delete(busyId)
            return next
          })
        }
      }
    })()
  }, [])

  const patch = useCallback(
    (values: JSONObject, busyId: string | null, success: string | null) => {
      run(busyId, success, async () => {
        const state = useApp.getState()
        await state.patchConfig(values)
        await state.refreshMcpStatus()
      })
    },
    [run],
  )

  /** Every connection goes through the connect page: what the app can access, its credentials, then Approve. */
  const beginInstall = useCallback((app: McpCatalogApp) => {
    setMessage(null)
    setConnectingAppId(app.id)
  }, [])

  const install = useCallback(
    (app: McpCatalogApp, inputs: { [key: string]: string }, excludeTools: string[]) => {
      setSelectedAppId(app.id)
      run(app.id, `${app.name} connected; its tools are available in every chat.`, () => useApp.getState().installMcpApp(app.id, inputs, excludeTools))
    },
    [run],
  )

  /** A server of the user's own: written as `[mcp.servers.<name>]`, which the backend connects like any other entry. */
  const addCustom = useCallback(
    (name: string, server: McpServerConfig) => {
      patch({ [`mcp.servers.${name}`]: mcpServerPatchValue(server) }, name, `${name} added; it shows under Your servers once it connects.`)
    },
    [patch],
  )

  const uninstall = useCallback(
    async (app: McpCatalogApp) => {
      const confirmed = await confirm({
        title: `Disconnect ${app.name}?`,
        message: app.builtin
          ? 'The agent loses its tools. You can connect it again from Apps.'
          : 'Its server entry (and any key you entered for it) is deleted from ~/.context-agent/config.toml. You can connect it again from Apps.',
        confirmLabel: 'Disconnect',
        cancelLabel: 'Cancel',
      })
      if (!confirmed) return
      run(app.id, `${app.name} disconnected.`, () => useApp.getState().uninstallMcpApp(app.id))
    },
    [run],
  )

  const setEnabled = useCallback(
    (id: string, enabled: boolean) => {
      const server = serverConfig(id)
      if (!server) return
      patch({ [`mcp.servers.${id}`]: mcpServerPatchValue({ ...server, enabled }) }, id, enabled ? `${id} is connecting…` : `${id} is paused.`)
    },
    [patch],
  )

  /// Per-tool permission. The toggles work on exact names: the tools on now
  /// (as the backend reports them, groups switched off on the connect page
  /// included) with this one flipped become the entry's `tools` allowlist
  /// (empty = every tool), and the pattern exclusions are folded in and cleared.
  const setToolEnabled = useCallback(
    (id: string, tool: string, enabled: boolean) => {
      const state = useApp.getState()
      const server = serverConfig(id)
      const status = state.mcpServers.find((candidate) => candidate.name === id)
      if (!server || !status) return
      const all = status.tools.map((info) => info.name)
      const allowed = new Set(status.tools.filter((info) => info.enabled).map((info) => info.name))
      if (enabled) allowed.add(tool)
      else allowed.delete(tool)
      const tools = all.every((name) => allowed.has(name)) ? [] : all.filter((name) => allowed.has(name))
      patch({ [`mcp.servers.${id}`]: mcpServerPatchValue({ ...server, tools, exclude_tools: [] }) }, id, null)
    },
    [patch],
  )

  /** The app's "Bypass permissions" switch: every tool runs without asking, except one set to Always ask. */
  const setBypass = useCallback(
    (id: string, on: boolean) => {
      const server = serverConfig(id)
      if (!server) return
      patch({ [`mcp.servers.${id}`]: mcpServerPatchValue({ ...server, bypass_permissions: on }) }, id, null)
    },
    [patch],
  )

  /** One tool's permission from its dropdown; null is Default (the server's annotations and the chat's profile decide). */
  const setToolPermission = useCallback(
    (id: string, tool: string, permission: McpToolPermission | null) => {
      const server = serverConfig(id)
      if (!server) return
      const permissions = { ...server.permissions }
      if (permission === null) delete permissions[tool]
      else permissions[tool] = permission
      patch({ [`mcp.servers.${id}`]: mcpServerPatchValue({ ...server, permissions }) }, id, null)
    },
    [patch],
  )

  const reconnect = useCallback(
    (id: string) => {
      run(id, null, async () => {
        const state = useApp.getState()
        await state.reconnectMcp(id)
        const status = useApp.getState().mcpServers.find((candidate) => candidate.name === id)
        if (status && status.state === 'failed') throw new Error(`${id} failed: ${status.error ?? 'unknown error'}`)
      })
    },
    [run],
  )

  // The selected app's callbacks, stable per app so the page's rows stay memoised.
  const pageActions = useMemo(() => {
    const id = selectedAppId ?? ''
    return {
      onBack: () => setSelectedAppId(null),
      onReconnect: () => reconnect(id),
      onSetEnabled: (enabled: boolean) => setEnabled(id, enabled),
      onSetToolEnabled: (tool: string, enabled: boolean) => setToolEnabled(id, tool, enabled),
      onSetBypass: (on: boolean) => setBypass(id, on),
      onSetToolPermission: (tool: string, permission: McpToolPermission | null) => setToolPermission(id, tool, permission),
      onConfigure: () => setConnectingAppId(id),
    }
  }, [selectedAppId, reconnect, setEnabled, setToolEnabled, setBypass, setToolPermission])

  const openApp = useCallback((app: McpCatalogApp) => setSelectedAppId(app.id), [])
  const connectCustom = useCallback(() => {
    setMessage(null)
    setAddingCustom(true)
  }, [])

  // MARK: Body

  const connectingApp = connectingAppId !== null ? (catalog.find((app) => app.id === connectingAppId) ?? null) : null
  const selectedApp = selectedAppId !== null ? (catalog.find((app) => app.id === selectedAppId) ?? null) : null

  let page: { key: string; transition: PageTransition; element: ReactNode }
  if (addingCustom) {
    const taken = new Set([...Object.keys(config?.mcp.servers ?? {}), ...catalog.map((app) => app.id)])
    page = {
      key: 'custom',
      transition: 'fade',
      element: (
        <CustomConnectView
          taken={taken}
          onCancel={() => setAddingCustom(false)}
          onConnect={(name, server) => {
            setAddingCustom(false)
            addCustom(name, server)
          }}
        />
      ),
    }
  } else if (connectingApp) {
    page = {
      key: `connect-${connectingApp.id}`,
      transition: 'fade',
      element: (
        <ConnectView
          app={connectingApp}
          existing={config?.mcp.servers[connectingApp.id] ?? null}
          onCancel={() => setConnectingAppId(null)}
          onApprove={(inputs, excluded) => {
            setConnectingAppId(null)
            install(connectingApp, inputs, excluded)
          }}
        />
      ),
    }
  } else if (selectedApp) {
    page = {
      key: `app-${selectedApp.id}`,
      transition: 'slide',
      element: (
        <AppPage
          app={selectedApp}
          busy={busy.has(selectedApp.id)}
          message={message}
          onInstall={() => beginInstall(selectedApp)}
          onUninstall={() => void uninstall(selectedApp)}
          {...pageActions}
        />
      ),
    }
  } else {
    page = {
      key: 'marketplace',
      transition: 'fade',
      element: <Marketplace busy={busy} message={message} onOpen={openApp} onInstall={beginInstall} onConnectCustom={connectCustom} />,
    }
  }

  return (
    <div className={styles.apps} role="region" aria-label="Apps">
      <PageStack pageKey={page.key} transition={page.transition}>
        {page.element}
      </PageStack>
    </div>
  )
}

type PageTransition = 'fade' | 'slide'

interface PageStackProps {
  pageKey: string
  transition: PageTransition
  children: ReactNode
}

/** What leaves when the page changes: a static copy of its DOM and how it goes. */
interface LeavingPage {
  copy: HTMLElement
  /** Indices (in `querySelectorAll('*')` order) of scrolled elements, with their offsets. */
  scrolls: [number, number][]
  transition: PageTransition
}

/** `.easeInOut(duration: 0.18)`. */
const TRANSITION_MS = 180

/**
 * The page `Group` of AppsView.swift with its transitions: the new page
 * comes in (`.opacity`, or `.move(edge: .trailing)` + opacity for an app's
 * page) while the old one goes out the same way, over 0.18 s. The outgoing
 * page is a static, inert copy of its DOM taken just before React replaces
 * it (`getSnapshotBeforeUpdate`), so nothing of it (key handlers, effects,
 * focus) stays alive while it fades.
 */
class PageStack extends Component<PageStackProps> {
  private readonly current = createRef<HTMLDivElement>()
  private readonly layer = createRef<HTMLDivElement>()
  private readonly timers = new Set<number>()

  getSnapshotBeforeUpdate(previous: PageStackProps): LeavingPage | null {
    if (previous.pageKey === this.props.pageKey) return null
    const node = this.current.current
    if (!node || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return null
    const scrolls: [number, number][] = []
    node.querySelectorAll('*').forEach((element, index) => {
      if (element.scrollTop > 0) scrolls.push([index, element.scrollTop])
    })
    const copy = node.cloneNode(true) as HTMLElement
    copy.removeAttribute('id')
    copy.querySelectorAll('[id]').forEach((element) => element.removeAttribute('id'))
    return { copy, scrolls, transition: previous.transition }
  }

  componentDidUpdate(_previous: PageStackProps, _state: unknown, leaving: LeavingPage | null): void {
    const layer = this.layer.current
    if (!leaving || !layer) return
    const { copy, scrolls, transition } = leaving
    copy.className = [styles.page, transition === 'slide' ? styles.slideOut : styles.fadeOut].join(' ')
    copy.setAttribute('inert', '')
    copy.setAttribute('aria-hidden', 'true')
    layer.replaceChildren(copy)
    const elements = copy.querySelectorAll('*')
    for (const [index, top] of scrolls) {
      const element = elements[index]
      if (element) element.scrollTop = top
    }
    const timer = window.setTimeout(() => {
      this.timers.delete(timer)
      copy.remove()
    }, TRANSITION_MS)
    this.timers.add(timer)
  }

  componentWillUnmount(): void {
    for (const timer of this.timers) window.clearTimeout(timer)
    this.timers.clear()
  }

  render(): ReactNode {
    const { pageKey, transition, children } = this.props
    return (
      <>
        <div key={pageKey} ref={this.current} className={[styles.page, transition === 'slide' ? styles.slide : styles.fade].join(' ')}>
          {children}
        </div>
        <div ref={this.layer} className={styles.leaving} aria-hidden="true" />
      </>
    )
  }
}
