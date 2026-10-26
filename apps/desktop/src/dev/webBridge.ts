/**
 * Development only: when the frontend runs in a plain browser (Vite without
 * the Tauri shell), route every Tauri command through `scripts/dev-bridge.mjs`
 * and replay its events, so the whole app works against a real `ctx serve`.
 * Plugin commands the bridge cannot do (dialogs, notifications, window
 * controls) get browser stand-ins.
 *
 * `?platform=windows|macos|linux` picks the platform the UI renders for
 * (default: windows), so the Windows chrome can be previewed on a Mac.
 */
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { emit } from '@tauri-apps/api/event'

const BRIDGE = '/__bridge'

type Args = Record<string, unknown> | undefined

async function bridgeInvoke(cmd: string, args: Args): Promise<unknown> {
  const response = await fetch(`${BRIDGE}/invoke`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cmd, args: args ?? {} }),
  })
  const body = (await response.json()) as { ok: boolean; result?: unknown; error?: string }
  if (!body.ok) throw body.error ?? 'bridge error'
  return body.result ?? null
}

function pluginStandIn(cmd: string, args: Args): { handled: boolean; value?: unknown } {
  const a = (args ?? {}) as Record<string, unknown>
  switch (cmd) {
    case 'plugin:dialog|open': {
      // Directory picks open the bridge's project folder (BRIDGE_PROJECT, else
      // the repository); file picks take `?files=a;b` from the URL, so
      // automated runs never block on a prompt.
      const options = (a.options ?? {}) as { directory?: boolean; multiple?: boolean }
      if (options.directory) return { handled: false }
      const files = new URLSearchParams(window.location.search).get('files')
      const paths = (files ?? '').split(';').map((p) => p.trim()).filter(Boolean)
      if (paths.length === 0) return { handled: true, value: null }
      return { handled: true, value: options.multiple ? paths : paths[0] ?? null }
    }
    case 'plugin:dialog|save':
      return { handled: true, value: window.prompt('Save as') }
    case 'plugin:dialog|message':
      window.alert(String(a.message ?? ''))
      return { handled: true, value: true }
    case 'plugin:dialog|ask':
    case 'plugin:dialog|confirm':
      return { handled: true, value: window.confirm(String(a.message ?? '')) }
    case 'plugin:clipboard-manager|write_text':
      void navigator.clipboard?.writeText(String(a.text ?? ''))
      return { handled: true, value: null }
    case 'plugin:clipboard-manager|read_text':
      return { handled: true, value: navigator.clipboard?.readText() ?? '' }
    case 'plugin:notification|is_permission_granted':
      return { handled: true, value: false }
    case 'plugin:notification|request_permission':
      return { handled: true, value: 'denied' }
    case 'plugin:notification|notify':
      return { handled: true, value: null }
    case 'plugin:opener|open_url':
      window.open(String(a.url ?? ''), '_blank', 'noopener')
      return { handled: true, value: null }
    case 'plugin:opener|open_path':
    case 'plugin:opener|reveal_item_in_dir':
      return { handled: true, value: null }
    case 'plugin:window-state|save_window_state':
    case 'plugin:window-state|restore_state':
    case 'plugin:process|exit':
    case 'plugin:process|restart':
      return { handled: true, value: null }
  }
  if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:webview|')) {
    if (cmd.endsWith('is_maximized') || cmd.endsWith('is_fullscreen') || cmd.endsWith('is_minimized')) return { handled: true, value: false }
    if (cmd.endsWith('is_focused') || cmd.endsWith('is_visible')) return { handled: true, value: document.hasFocus() }
    if (cmd.endsWith('scale_factor')) return { handled: true, value: window.devicePixelRatio }
    if (cmd.endsWith('inner_size') || cmd.endsWith('outer_size')) return { handled: true, value: { width: window.innerWidth, height: window.innerHeight } }
    if (cmd.endsWith('inner_position') || cmd.endsWith('outer_position')) return { handled: true, value: { x: 0, y: 0 } }
    return { handled: true, value: null }
  }
  if (cmd.startsWith('plugin:app|')) {
    if (cmd.endsWith('version')) return { handled: true, value: '0.1.0' }
    if (cmd.endsWith('name')) return { handled: true, value: 'Context' }
    return { handled: true, value: null }
  }
  if (cmd.startsWith('plugin:path|')) {
    return { handled: true, value: '/tmp/context-desktop' }
  }
  return { handled: false }
}

/** Installs the mocks; resolves once the event stream is connected. */
export async function installWebBridge(): Promise<void> {
  const params = new URLSearchParams(window.location.search)
  const platform = params.get('platform') ?? 'windows'
  const w = window as unknown as Record<string, unknown>
  w.__TAURI_OS_PLUGIN_INTERNALS__ = {
    platform,
    os_type: platform,
    family: platform === 'windows' ? 'windows' : 'unix',
    arch: 'x86_64',
    version: '10.0.26100',
    eol: platform === 'windows' ? '\r\n' : '\n',
    exe_extension: platform === 'windows' ? 'exe' : '',
  }
  mockWindows('main')
  mockIPC(
    async (cmd, args) => {
      const standIn = pluginStandIn(cmd, args as Args)
      if (standIn.handled) return standIn.value
      if (cmd === 'plugin:dialog|open') return bridgeInvoke('fs_pick_project', {})
      return bridgeInvoke(cmd, args as Args)
    },
    { shouldMockEvents: true },
  )
  w.__CTX_WEB_BRIDGE__ = true
  // `convertFileSrc` (images, PDFs, attachment previews): the shell's asset
  // protocol does not exist here, so files go through Vite's `/@fs/` route
  // (served when Vite allows the folder; otherwise the views' own "could not
  // read" states show instead of a thrown TypeError).
  const internals = w.__TAURI_INTERNALS__ as Record<string, unknown> | undefined
  if (internals && typeof internals.convertFileSrc !== 'function') {
    internals.convertFileSrc = (path: string) => `/@fs${path.startsWith('/') ? '' : '/'}${encodeURI(path.replace(/\\/g, '/'))}`
  }
  // For visual checks of states the mock backend never produces (approvals,
  // questions, failures): `window.__ctxEmit('backend:notification', { method:
  // 'event', params: { project_id, session_id, event: {...} } })`.
  w.__ctxEmit = (event: string, payload: unknown) => emit(event, payload)
  await new Promise<void>((resolve) => {
    const source = new EventSource(`${BRIDGE}/events`)
    source.onopen = () => resolve()
    source.onerror = () => resolve()
    source.onmessage = (message) => {
      const { event, payload } = JSON.parse(message.data) as { event: string; payload: unknown }
      void emit(event, payload)
    }
  })
}
