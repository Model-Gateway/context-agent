#!/usr/bin/env node
// Development-only bridge: lets the Vite frontend run in an ordinary browser
// (no Tauri shell) against a real `ctx serve`. It implements the Rust
// shell's commands over HTTP (POST /invoke {cmd, args}) and streams its
// events over Server-Sent Events (GET /events), so src/dev/webBridge.ts can
// install Tauri's IPC mocks and the app runs unchanged. Used for visual
// checks and screenshots; never shipped.
//
//   node scripts/dev-bridge.mjs            # mock backend, scratch data dir, demo repo
//   BRIDGE_REAL=1 node scripts/dev-bridge.mjs   # the configured model, real data dir
//
// Environment: BRIDGE_PORT (1422), CTX_BINARY (default: the repository's
// target/release/ctx, else target/debug/ctx), BRIDGE_PROJECT (the folder
// `fs_pick_project` returns and `launch_options.project`; default: a fresh
// demo repo from scripts/make-demo-repo.sh, never a real working tree), BRIDGE_DATA_DIR
// (default: a temporary directory unless BRIDGE_REAL is set), BRIDGE_AUTOTEST
// (`launch_options.autotest`: every page load runs the self-test and writes
// result.json into that directory), BRIDGE_UPDATE=<version> (plays an
// installed copy whose feed offers that version, for the Updates settings and
// the update sheet; the updater is off otherwise, as in every build that is
// not installed), CTX_SSH_BINARY (the ssh client the remote hosts use; the
// self-test points it at a shim that runs the remote command locally with
// `sh -c`, default `ssh`).
//
// Like the Rust shell it stands in for, the bridge keeps one `ctx serve`
// child per backend id: `local` is the ctx binary, any other id is
// `ssh … ctx serve` for the host `backend_start` was given, and every event
// carries its `backend`.

import { spawn, spawnSync } from 'node:child_process'
import { createServer } from 'node:http'
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir, platform as osPlatform } from 'node:os'
import { dirname, extname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..', '..', '..')
const port = Number(process.env.BRIDGE_PORT || 1422)
const real = process.env.BRIDGE_REAL === '1'
const exe = osPlatform() === 'win32' ? '.exe' : ''
const binary =
  process.env.CTX_BINARY ||
  [join(repo, 'target', 'release', `ctx${exe}`), join(repo, 'target', 'debug', `ctx${exe}`)].find((p) => existsSync(p)) ||
  `ctx${exe}`
const dataDir = process.env.BRIDGE_DATA_DIR || (real ? undefined : mkdtempSync(join(tmpdir(), 'ctx-bridge-')))
// Never this repository by default: the Changes tab stages, commits and
// discards for real. Without BRIDGE_PROJECT a throwaway demo repo is made.
const projectDir = process.env.BRIDGE_PROJECT || makeDemoRepo()

function makeDemoRepo() {
  const dir = join(mkdtempSync(join(tmpdir(), 'ctx-demo-')), 'polyglot')
  const made = spawnSync('bash', [join(here, 'make-demo-repo.sh'), dir], { stdio: 'ignore' })
  if (made.status === 0) return dir
  const empty = mkdtempSync(join(tmpdir(), 'ctx-empty-project-'))
  console.warn(`[bridge] could not create the demo repo (bash missing?); opening the empty folder ${empty}`)
  return empty
}

/** @type {Set<import('node:http').ServerResponse>} */
const listeners = new Set()
function emit(event, payload) {
  const frame = `data: ${JSON.stringify({ event, payload })}\n\n`
  for (const res of listeners) res.write(frame)
}

// --- ctx serve children (one per backend id) ----------------------------------

const sshBinary = process.env.CTX_SSH_BINARY || 'ssh'
/** The remote PATH prefix and ssh options of `src-tauri/src/backend/launch.rs`. */
const REMOTE_PATH = '$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/bin:/opt/homebrew/bin:$PATH'
const SSH_OPTIONS = ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=4', '-o', 'ClearAllForwardings=yes', '-T']

function shellQuote(word) {
  const plain = word.length > 0 && /^[\p{L}\p{N}\-_./~=:+@%]+$/u.test(word)
  if (plain && !word.startsWith('~')) return word
  if (plain && word.startsWith('~/')) return '~/' + shellQuote(word.slice(2))
  return `'${word.replace(/'/g, "'\\''")}'`
}

/** `BackendLaunch.sshArguments(host:)`: the fixed options, port, key, user, extras, then `--` and the address. */
function sshArguments(host) {
  const args = [...SSH_OPTIONS]
  if (host.port > 0) args.push('-p', String(host.port))
  if (host.identityFile) args.push('-i', host.identityFile.replace(/^~(?=\/|$)/, process.env.HOME ?? '~'))
  if (host.user) args.push('-l', host.user)
  args.push(...(host.extraArguments ?? []).filter(Boolean))
  args.push('--', host.address)
  return args
}

/** `BackendLaunch.remoteCommand(host:serve:)`. */
function remoteCommand(host, serveArgs) {
  const words = [host.ctxPath || 'ctx', ...serveArgs].map(shellQuote).join(' ')
  return `env PATH="${REMOTE_PATH}" ${words}`
}

// --- Install ctx on host (src-tauri/src/backend/install.rs) ---------------------

const INSTALL_SCRIPT = [
  'set -e',
  'dir="$HOME/.local/bin"',
  'mkdir -p "$dir"',
  'tmp=$(mktemp -d "$dir/.ctx-install.XXXXXX")',
  `trap 'rm -rf "$tmp"' EXIT`,
  'tar -xzf - -C "$tmp"',
  'chmod 755 "$tmp/ctx"',
  '"$tmp/ctx" --version',
  'mv -f "$tmp/ctx" "$dir/ctx"',
].join('\n')

/** `ssh <host> <command>` with `input` on stdin → `{ code, stdout, stderr }`. */
function sshRun(host, command, input) {
  return new Promise((done, fail) => {
    const child = spawn(sshBinary, [...sshArguments(host), command], { stdio: ['pipe', 'pipe', 'pipe'] })
    const out = []
    const err = []
    child.stdout.on('data', (chunk) => out.push(chunk))
    child.stderr.on('data', (chunk) => err.push(chunk))
    child.on('error', fail)
    child.on('close', (code) => done({ code: code ?? -1, stdout: Buffer.concat(out).toString(), stderr: Buffer.concat(err).toString() }))
    child.stdin.on('error', () => undefined)
    child.stdin.end(input ?? undefined)
  })
}

function installFailure(output, doing) {
  const tail = output.stderr.trim().split('\n').slice(-6).join('\n')
  return `Could not ${doing} (ssh exited with code ${output.code}).` + (tail ? `\n${tail}` : '')
}

/** `ssh_install_ctx({ host })`: probe, download (or read a file:// URL), unpack on the host. Throws a string, like the shell's command. */
async function installCtx(host) {
  const probe = await sshRun(host, 'uname -sm')
  if (probe.code !== 0) throw installFailure(probe, 'ask the host what it runs')
  const [system, machine] = probe.stdout.trim().toLowerCase().split(/\s+/)
  const target = system !== 'linux' ? null : { x86_64: 'x86_64-unknown-linux-musl', amd64: 'x86_64-unknown-linux-musl', aarch64: 'aarch64-unknown-linux-musl', arm64: 'aarch64-unknown-linux-musl' }[machine]
  if (!target) throw `There is no ctx build for ${probe.stdout.trim() || 'this host'}. Automatic install covers Linux on x86_64 and arm64.`
  const name = `ctx-${target}.tar.gz`
  const base = process.env.CTX_REMOTE_RELEASE_URL?.trim()
  const url = base ? `${base.replace(/\/+$/, '')}/${name}` : `https://github.com/Model-Gateway/context-agent/releases/download/v${updater.version}/${name}`
  let archive
  if (url.startsWith('file:')) {
    archive = readFileSync(fileURLToPath(url))
  } else {
    const response = await fetch(url)
    if (response.status === 404) throw `The v${updater.version} release has no Linux build of ctx (${name}). Update Context, or install ctx on the host yourself.`
    if (!response.ok) throw `Downloading ${url} failed (HTTP ${response.status}).`
    archive = Buffer.from(await response.arrayBuffer())
  }
  const result = await sshRun(host, `sh -c ${shellQuote(INSTALL_SCRIPT)}`, archive)
  if (result.code !== 0) throw installFailure(result, 'install ctx in ~/.local/bin')
  return result.stdout.trim().split('\n').pop() || `ctx ${updater.version}`
}

/** @type {Map<string, { child, generation, pending: Map, stderrRing: string[], stopping: boolean, binary: string }>} */
const backends = new Map()
let nextId = 1

function backendId(a) {
  const id = typeof a?.backend === 'string' ? a.backend.trim() : ''
  return id.length > 0 ? id : 'local'
}

function entryFor(id) {
  let entry = backends.get(id)
  if (!entry) {
    entry = { child: null, generation: 0, pending: new Map(), stderrRing: [], stopping: false, binary }
    backends.set(id, entry)
  }
  return entry
}

function startBackend(a) {
  const id = backendId(a)
  const entry = entryFor(id)
  stopBackendNow(entry)
  const serveArgs = ['serve']
  const useMock = a.mock ?? !real
  if (useMock) serveArgs.push('--mock')
  const host = a.host ?? null
  let program
  let args
  if (host) {
    // A remote host: ssh (or the shim) runs `ctx serve` there; the data dir is the host's.
    program = sshBinary
    args = [...sshArguments(host), remoteCommand(host, serveArgs)]
  } else {
    const dir = a.data_dir || dataDir
    if (dir) serveArgs.push('--data-dir', dir)
    program = binary
    args = serveArgs
  }
  const env = { ...process.env, CTX_DESKTOP: '1' }
  if (useMock) env.CTX_MOCK_PACE_MS = env.CTX_MOCK_PACE_MS || '20'
  const gen = ++entry.generation
  entry.stopping = false
  entry.binary = host ? `ssh ${host.user ? host.user + '@' : ''}${host.address}` : binary
  const child = spawn(program, args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
  entry.child = child
  entry.stderrRing = []
  let buffer = ''
  child.stdout.setEncoding('utf8')
  child.stdout.on('data', (chunk) => {
    buffer += chunk
    let nl
    while ((nl = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, nl)
      buffer = buffer.slice(nl + 1)
      if (!line.trim()) continue
      let frame
      try {
        frame = JSON.parse(line)
      } catch {
        continue
      }
      if (frame.id !== undefined && frame.id !== null && entry.pending.has(frame.id)) {
        const { resolve: ok, reject } = entry.pending.get(frame.id)
        entry.pending.delete(frame.id)
        if (frame.error) reject({ code: frame.error.code, message: frame.error.message, kind: frame.error.data?.kind })
        else ok(frame.result ?? null)
      } else if (frame.method) {
        emit('backend:notification', { backend: id, method: frame.method, params: frame.params ?? null })
      }
    }
  })
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => {
    const lines = chunk.split('\n').filter(Boolean)
    entry.stderrRing.push(...lines)
    if (entry.stderrRing.length > 500) entry.stderrRing = entry.stderrRing.slice(-500)
    emit('backend:stderr', { backend: id, lines })
  })
  child.on('error', (e) => {
    entry.stderrRing.push(String(e))
  })
  child.on('exit', (code) => {
    // Only this child's requests fail: a restart (a page reload) kills the
    // previous child after the new one already took requests.
    for (const [requestId, p] of entry.pending) {
      if (p.gen !== gen) continue
      entry.pending.delete(requestId)
      p.reject({ code: -1, message: `ctx exited (${code})`, kind: 'transport' })
    }
    if (gen === entry.generation && !entry.stopping) emit('backend:exited', { backend: id, code: code ?? -1, tail: entry.stderrRing.slice(-40).join('\n') })
    if (gen === entry.generation) entry.child = null
  })
  return { pid: child.pid ?? 0, binary: entry.binary, searched: [program] }
}

function stopBackendNow(entry) {
  if (!entry?.child) return
  entry.stopping = true
  try {
    entry.child.kill()
  } catch {
    /* already gone */
  }
  entry.child = null
}

function rpc(id, method, params) {
  const entry = backends.get(id)
  if (!entry?.child) return Promise.reject({ code: -1, message: 'the backend is not running', kind: 'not_running' })
  const requestId = nextId++
  return new Promise((ok, reject) => {
    entry.pending.set(requestId, { resolve: ok, reject, gen: entry.generation })
    const frame = { jsonrpc: '2.0', id: requestId, method }
    if (params !== undefined && params !== null) frame.params = params
    entry.child.stdin.write(JSON.stringify(frame) + '\n')
  })
}

/** The `Host` aliases of ~/.ssh/config (no Include handling here). */
function sshConfigHosts() {
  try {
    const text = readFileSync(join(process.env.HOME ?? '', '.ssh', 'config'), 'utf8')
    const names = []
    for (const raw of text.split('\n')) {
      const parts = raw.trim().replace(/=/g, ' ').split(/\s+/)
      if (parts[0]?.toLowerCase() !== 'host') continue
      for (const name of parts.slice(1)) if (name && !/[*?!]/.test(name) && !names.includes(name)) names.push(name)
    }
    return names
  } catch {
    return []
  }
}

// --- other shell commands -----------------------------------------------------

const secrets = new Map()
const terminals = new Map()
let nextTerminal = 1

function mimeFor(path) {
  const ext = extname(path).toLowerCase()
  return (
    {
      '.png': 'image/png',
      '.jpg': 'image/jpeg',
      '.jpeg': 'image/jpeg',
      '.gif': 'image/gif',
      '.webp': 'image/webp',
      '.svg': 'image/svg+xml',
      '.pdf': 'application/pdf',
    }[ext] ?? undefined
  )
}

// --- updater (src-tauri/src/updater.rs) ---------------------------------------

const offeredVersion = process.env.BRIDGE_UPDATE || null
const offeredRelease = offeredVersion && {
  version: offeredVersion,
  notes: `## Context ${offeredVersion}\n\n- Windows: automatic updates, like the macOS app\n- Faster startup on large repositories`,
  date: new Date().toISOString(),
}
const updater = {
  available: offeredVersion !== null && !process.env.BRIDGE_AUTOTEST,
  version: /^version = "(.*)"/m.exec(readFileSync(join(repo, 'Cargo.toml'), 'utf8'))?.[1] ?? '0.0.0',
  automatically_checks: true,
  automatically_installs: true,
  last_check: null,
  phase: { kind: 'idle' },
}
const sleep = (ms) => new Promise((done) => setTimeout(done, ms))

function updaterPhase(phase) {
  updater.phase = phase
  emit('updater:state', { ...updater })
}

const commands = {
  updater_status: () => ({ ...updater }),
  updater_check: async () => {
    if (!updater.available) return fail('This build does not update itself.')
    updaterPhase({ kind: 'checking' })
    await sleep(700)
    updater.last_check = new Date().toISOString()
    updaterPhase({ kind: 'available', release: offeredRelease })
    return { ...updater }
  },
  updater_download: async () => {
    if (updater.phase.kind !== 'available') return fail('There is no update to download. Check for updates first.')
    const total = 24_300_000
    for (let received = 0; received < total; received += 2_700_000) {
      updaterPhase({ kind: 'downloading', release: offeredRelease, received, total })
      await sleep(250)
    }
    updaterPhase({ kind: 'ready', release: offeredRelease })
    return { ...updater }
  },
  updater_install: () => fail('The browser preview cannot install updates.'),
  updater_dismiss: () => {
    if (updater.phase.kind === 'available') updaterPhase({ kind: 'idle' })
    return { ...updater }
  },
  updater_set_automatic: (a) => {
    if (typeof a.checks === 'boolean') updater.automatically_checks = a.checks
    if (typeof a.installs === 'boolean') updater.automatically_installs = a.installs
    emit('updater:state', { ...updater })
    return { ...updater }
  },
  launch_options: () => ({ mock: !real, project: process.env.BRIDGE_PROJECT ?? null, binary: null, autotest: process.env.BRIDGE_AUTOTEST || null }),
  // The end of the self-test (src/selftest.ts): writes <dir>/result.json like
  // the shell's `selftest_finish`, but keeps the preview running.
  selftest_finish: (a) => {
    mkdirSync(a.dir, { recursive: true })
    const path = join(a.dir, 'result.json')
    writeFileSync(path, String(a.result_json ?? ''))
    console.log(`[bridge] self-test wrote ${path} (exit code ${a.exit_code})`)
    return path
  },
  backend_start: (a) => startBackend(a ?? {}),
  backend_stop: async (a) => {
    const id = backendId(a)
    const entry = backends.get(id)
    if (entry?.child) {
      try {
        await Promise.race([rpc(id, 'shutdown'), new Promise((r) => setTimeout(r, 2000))])
      } catch {
        /* ignore */
      }
    }
    stopBackendNow(entry)
    return null
  },
  backend_remove: async (a) => {
    const id = backendId(a)
    await commands.backend_stop(a)
    if (id !== 'local') backends.delete(id)
    return null
  },
  backend_state: (a) => {
    const entry = backends.get(backendId(a))
    return entry?.child ? { state: 'running', pid: entry.child.pid } : { state: 'stopped' }
  },
  backend_stderr: (a) => backends.get(backendId(a))?.stderrRing ?? [],
  backend_candidates: () => [binary],
  ssh_config_hosts: () => sshConfigHosts(),
  ssh_install_ctx: (a) => installCtx(a.host),
  rpc_request: (a) => rpc(backendId(a), a.method, a.params),
  secret_get: (a) => secrets.get(a.account) ?? null,
  secret_set: (a) => (secrets.set(a.account, a.value), null),
  secret_remove: (a) => (secrets.delete(a.account), null),
  fs_list_dir: (a) =>
    readdirSync(a.path, { withFileTypes: true })
      .filter((d) => d.name !== '.git' && d.name !== '.DS_Store')
      .map((d) => {
        const path = join(a.path, d.name)
        let size = 0
        try {
          size = d.isDirectory() ? 0 : statSync(path).size
        } catch {
          /* ignore */
        }
        return { name: d.name, path, is_dir: d.isDirectory(), size }
      })
      .sort((x, y) => (x.is_dir === y.is_dir ? x.name.localeCompare(y.name, undefined, { numeric: true, sensitivity: 'base' }) : x.is_dir ? -1 : 1)),
  fs_read_text: (a) => {
    const max = a.max_bytes ?? 1024 * 1024
    const buf = readFileSync(a.path)
    return { content: buf.subarray(0, max).toString('utf8'), truncated: buf.length > max, size: buf.length }
  },
  fs_file_info: (a) => {
    const s = statSync(a.path)
    return { size: s.size, is_dir: s.isDirectory(), modified: s.mtime.toISOString(), mime: mimeFor(a.path) }
  },
  fs_write_temp_image: (a) => {
    const dir = join(dataDir || tmpdir(), 'Pasted Images')
    mkdirSync(dir, { recursive: true })
    const b64 = String(a.bytes_base64).replace(/^data:[^,]*,/, '')
    const path = join(dir, `${Date.now()}.${a.extension || 'png'}`)
    writeFileSync(path, Buffer.from(b64, 'base64'))
    return path
  },
  fs_save_text: (a) => {
    // No dialog in the preview: saves into <data dir>/Saved.
    const dir = join(dataDir || tmpdir(), 'Saved')
    mkdirSync(dir, { recursive: true })
    const path = join(dir, String(a.default_name || 'untitled.txt').replace(/[\\/]/g, '_'))
    writeFileSync(path, String(a.content ?? ''))
    return path
  },
  fs_pick_project: () => projectDir,
  reveal_in_file_manager: (a) => (console.log('[bridge] reveal', a.path), null),
  open_with_default_app: (a) => (console.log('[bridge] open', a.path), null),
  open_url: (a) => (console.log('[bridge] open url', a.url), null),
  terminal_open: (a) => {
    const id = String(nextTerminal++)
    const shell = osPlatform() === 'win32' ? 'powershell.exe' : process.env.SHELL || '/bin/sh'
    // A project on a host: `ssh -t <host> 'cd <root>; exec $SHELL -l'` (the shim runs it here).
    const proc = a.host
      ? spawn(sshBinary, [...sshArguments(a.host).filter((arg) => arg !== '-T'), `cd ${shellQuote(a.cwd)}; exec $SHELL -l`], { env: { ...process.env, TERM: 'dumb' } })
      : spawn(shell, osPlatform() === 'win32' ? [] : ['-i'], { cwd: a.cwd, env: { ...process.env, TERM: 'dumb' } })
    terminals.set(id, proc)
    const forward = (chunk) => emit('terminal:data', { id, data: chunk.toString('utf8').replace(/\n/g, '\r\n') })
    proc.stdout.on('data', forward)
    proc.stderr.on('data', forward)
    proc.on('exit', (code) => {
      terminals.delete(id)
      emit('terminal:exit', { id, code: code ?? 0 })
    })
    return { id }
  },
  terminal_write: (a) => (terminals.get(a.id)?.stdin.write(a.data.replace(/\r/g, '\n')), null),
  terminal_resize: () => null,
  terminal_close: (a) => (terminals.get(a.id)?.kill(), terminals.delete(a.id), null),
}

function fail(message) {
  return Promise.reject({ code: -1, message, kind: 'unsupported' })
}

async function invoke(cmd, args) {
  const handler = commands[cmd]
  if (handler) return handler(args ?? {})
  if (cmd.startsWith('browser_')) return fail('The embedded browser is not available in the browser preview.')
  return fail(`unknown command ${cmd}`)
}

// --- HTTP ---------------------------------------------------------------------

createServer((req, res) => {
  res.setHeader('Access-Control-Allow-Origin', '*')
  res.setHeader('Access-Control-Allow-Headers', 'content-type')
  if (req.method === 'OPTIONS') return res.end()
  if (req.url === '/events') {
    res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' })
    res.write(': connected\n\n')
    listeners.add(res)
    req.on('close', () => listeners.delete(res))
    return
  }
  if (req.url === '/invoke' && req.method === 'POST') {
    let body = ''
    req.on('data', (c) => (body += c))
    req.on('end', async () => {
      let parsed
      try {
        parsed = JSON.parse(body)
      } catch {
        res.writeHead(400).end()
        return
      }
      try {
        const result = await invoke(parsed.cmd, parsed.args)
        res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ ok: true, result: result ?? null }))
      } catch (error) {
        const value = typeof error === 'string' ? error : JSON.stringify(error && error.message && !error.code ? { code: -1, message: error.message } : error)
        res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ ok: false, error: value }))
      }
    })
    return
  }
  res.writeHead(404).end()
}).listen(port, () => {
  console.log(`[bridge] http://localhost:${port}  ctx: ${binary}  data: ${dataDir ?? '(default)'}  project: ${projectDir}  ${real ? 'real model' : 'mock model'}`)
})

process.on('SIGINT', () => {
  for (const entry of backends.values()) stopBackendNow(entry)
  process.exit(0)
})
