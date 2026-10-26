#!/usr/bin/env node
// Development only: drive the browser preview (npm run web) in headless
// Chrome and take screenshots, for comparing screens with the macOS app.
//
//   node scripts/snap.mjs steps.json out-dir [--url http://localhost:1420/?platform=windows] [--size 1180x780] [--dark]
//
// steps.json is an array of actions, run in order:
//   { "wait": 500 }                         sleep (ms)
//   { "waitFor": "text=Where should we begin?" , "timeout": 15000 }
//   { "click": "role=button[name=\"Open a project…\"]" }   any Playwright selector
//   { "dblclick": "…" } { "hover": "…" } { "rightClick": "…" }
//   { "fill": "textarea", "text": "Summarise this repository" }
//   { "type": "hello" } { "press": "Enter" }   (keyboard, focused element)
//   { "eval": "window.localStorage.clear()" }
//   { "shot": "01-welcome" }                 full-window PNG into out-dir
//   { "shotOf": ".selector", "name": "02-card" }
// It prints console errors and page errors, and exits non-zero when a step fails.
import { chromium } from 'playwright-core'
import { existsSync, mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const [stepsFile, outDir, ...rest] = process.argv.slice(2)
if (!stepsFile || !outDir) {
  console.error('usage: node scripts/snap.mjs steps.json out-dir [--url URL] [--size WxH] [--dark]')
  process.exit(2)
}
const flag = (name, fallback) => {
  const i = rest.indexOf(name)
  return i >= 0 ? rest[i + 1] : fallback
}
const url = flag('--url', 'http://localhost:1420/?platform=windows')
const [width, height] = flag('--size', '1180x780').split('x').map(Number)
const dark = rest.includes('--dark')
const chrome = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  '/usr/bin/google-chrome',
].find((p) => p && existsSync(p))

mkdirSync(outDir, { recursive: true })
const steps = JSON.parse(readFileSync(stepsFile, 'utf8'))
const browser = await chromium.launch({ executablePath: chrome, headless: true })
const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2, colorScheme: dark ? 'dark' : 'light' })
const page = await context.newPage()
const problems = []
page.on('console', (m) => {
  if (m.type() === 'error') problems.push(`console: ${m.text()}`)
})
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`))
let failed = false
try {
  await page.goto(url, { waitUntil: 'domcontentloaded' })
  for (const [i, step] of steps.entries()) {
    const label = JSON.stringify(step)
    try {
      if (step.wait) await page.waitForTimeout(step.wait)
      else if (step.waitFor) await page.locator(step.waitFor).first().waitFor({ timeout: step.timeout ?? 15000 })
      else if (step.click) await page.locator(step.click).first().click({ timeout: step.timeout ?? 10000 })
      else if (step.dblclick) await page.locator(step.dblclick).first().dblclick({ timeout: step.timeout ?? 10000 })
      else if (step.rightClick) await page.locator(step.rightClick).first().click({ button: 'right', timeout: step.timeout ?? 10000 })
      else if (step.hover) await page.locator(step.hover).first().hover({ timeout: step.timeout ?? 10000 })
      else if (step.fill) await page.locator(step.fill).first().fill(step.text ?? '', { timeout: step.timeout ?? 10000 })
      else if (step.type !== undefined) await page.keyboard.type(step.type)
      else if (step.press) await page.keyboard.press(step.press)
      else if (step.eval) await page.evaluate(step.eval)
      else if (step.shot) await page.screenshot({ path: join(outDir, `${step.shot}.png`) })
      else if (step.shotOf) await page.locator(step.shotOf).first().screenshot({ path: join(outDir, `${step.name ?? `step-${i}`}.png`) })
      console.log(`ok   ${i} ${label}`)
    } catch (error) {
      failed = true
      console.log(`FAIL ${i} ${label}: ${String(error).split('\n')[0]}`)
      await page.screenshot({ path: join(outDir, `fail-step-${i}.png`) }).catch(() => {})
      if (!step.optional) break
    }
  }
} finally {
  for (const p of problems) console.log(p)
  await browser.close()
}
process.exit(failed ? 1 : 0)
