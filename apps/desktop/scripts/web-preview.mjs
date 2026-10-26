#!/usr/bin/env node
// Runs the dev bridge and Vite together so the app can be opened in a plain
// browser at http://localhost:1420 (development only; see dev-bridge.mjs).
import { spawn } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const children = [
  spawn(process.execPath, [join(root, 'scripts', 'dev-bridge.mjs')], { cwd: root, stdio: 'inherit', env: process.env }),
  spawn(process.execPath, [join(root, 'node_modules', 'vite', 'bin', 'vite.js')], { cwd: root, stdio: 'inherit', env: process.env }),
]
const stop = () => {
  for (const c of children) c.kill()
  process.exit(0)
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
for (const c of children) c.on('exit', stop)
