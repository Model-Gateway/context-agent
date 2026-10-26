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
// result.json into that directory).

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

// --- ctx serve child --------------------------------------------------------

let child = null
let generation = 0
let nextId = 1
const pending = new Map()
let stderrRing = []
let stopping = false

function startBackend({ mock, data_dir }) {
  stopBackendNow()
  const args = ['serve']
  const useMock = mock ?? !real
  if (useMock) args.push('--mock')
  const dir = data_dir || dataDir
  if (dir) args.push('--data-dir', dir)
  const env = { ...process.env, CTX_DESKTOP: '1' }
  if (useMock) env.CTX_MOCK_PACE_MS = env.CTX_MOCK_PACE_MS || '20'
  const gen = ++generation
  stopping = false
  child = spawn(binary, args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
  stderrRing = []
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
      if (frame.id !== undefined && frame.id !== null && pending.has(frame.id)) {
        const { resolve: ok, reject } = pending.get(frame.id)
        pending.delete(frame.id)
        if (frame.error) reject({ code: frame.error.code, message: frame.error.message, kind: frame.error.data?.kind })
        else ok(frame.result ?? null)
      } else if (frame.method) {
        emit('backend:notification', { method: frame.method, params: frame.params ?? null })
      }
    }
  })
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => {
    const lines = chunk.split('\n').filter(Boolean)
    stderrRing.push(...lines)
    if (stderrRing.length > 500) stderrRing = stderrRing.slice(-500)
    emit('backend:stderr', { lines })
  })
  child.on('error', (e) => {
    stderrRing.push(String(e))
  })
  child.on('exit', (code) => {
    // Only this child's requests fail: a restart (a page reload) kills the
    // previous child after the new one already took requests.
    for (const [id, p] of pending) {
      if (p.gen !== gen) continue
      pending.delete(id)
      p.reject({ code: -1, message: `ctx exited (${code})`, kind: 'transport' })
    }
    if (gen === generation && !stopping) emit('backend:exited', { code: code ?? -1, tail: stderrRing.slice(-40).join('\n') })
    if (gen === generation) child = null
  })
  return { pid: child.pid ?? 0, binary, searched: [binary] }
}

function stopBackendNow() {
  if (!child) return
  stopping = true
  try {
    child.kill()
  } catch {
    /* already gone */
  }
  child = null
}

function rpc(method, params) {
  if (!child) return Promise.reject({ code: -1, message: 'the backend is not running', kind: 'not_running' })
  const id = nextId++
  return new Promise((ok, reject) => {
    pending.set(id, { resolve: ok, reject, gen: generation })
    const frame = { jsonrpc: '2.0', id, method }
    if (params !== undefined && params !== null) frame.params = params
    child.stdin.write(JSON.stringify(frame) + '\n')
  })
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

const commands = {
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
  backend_stop: async () => {
    if (child) {
      try {
        await Promise.race([rpc('shutdown'), new Promise((r) => setTimeout(r, 2000))])
      } catch {
        /* ignore */
      }
    }
    stopBackendNow()
    return null
  },
  backend_state: () => (child ? { state: 'running', pid: child.pid } : { state: 'stopped' }),
  backend_stderr: () => stderrRing,
  backend_candidates: () => [binary],
  rpc_request: (a) => rpc(a.method, a.params),
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
    const proc = spawn(shell, osPlatform() === 'win32' ? [] : ['-i'], { cwd: a.cwd, env: { ...process.env, TERM: 'dumb' } })
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
  stopBackendNow()
  process.exit(0)
})
