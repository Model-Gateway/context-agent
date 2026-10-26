// The scripted self-test (`--autotest <dir>`, ARCHITECTURE.md "Self-test
// mode"), the port of apps/macos/ContextAgent/App/UITestDriver.swift without
// the screenshots: it drives the store exactly like a user would — wait for
// the backend, open the `--project` folder, start a new chat, send
// "Summarise this repository", wait for `task/finished`, check the
// transcript (two tool rows and the final reply, in the store and on
// screen), visit the Changes and Project tabs and come back to the chat —
// then hands `result.json` (`{ ok, steps: [{ name, ok, detail }], … }`) to
// the shell's `selftest_finish`, which writes it into the directory and
// exits with 0 (passed) or 1 (failed). The browser preview's bridge writes
// the file without exiting.

import { invoke } from '@tauri-apps/api/core'
import { describe } from './backend/errors'
import { chatStatusIsRunning, type ChatStatus } from './models/reducer'
import { rowLabel } from './models/toolMetadata'
import type { TranscriptItem } from './models/transcript'
import { launchOptions, useApp, type SidebarItem } from './state/app'
import type { ChatState } from './state/chat'
import { selectedChat, selectedProject } from './state/selectors'

/** The prompt the test sends. */
export const SELFTEST_PROMPT = 'Summarise this repository'

/** Poll interval of every wait. */
const POLL_MS = 100
const BACKEND_TIMEOUT_MS = 90_000
const PROJECT_TIMEOUT_MS = 60_000
const TASK_TIMEOUT_MS = 180_000
/** How long a screen gets to render after a switch. */
const SETTLE_MS = 400

/** One scripted step, as written to `result.json`. */
export interface SelfTestStep {
  name: string
  ok: boolean
  detail: string | null
  elapsedMs: number
}

export interface SelfTestResult {
  ok: boolean
  steps: SelfTestStep[]
  /** Committed transcript items of the test chat. */
  transcriptItems: number
  /** One short label per transcript item ("user", "assistant", "tools(2)", "outcome(completed)"). */
  transcript: string[]
  error: string | null
  directory: string
  project: string | null
  finishedAt: string
}

let started = false

/// Runs the self-test once when `--autotest <dir>` was given (a no-op
/// otherwise and when it already runs). Called from `App` after the launch
/// `startBackend()` returned.
export async function startSelfTestIfRequested(): Promise<void> {
  const options = await launchOptions()
  if (options.autotest === null || started) return
  started = true
  await new Session(options.autotest, options.project).run()
}

// MARK: - Session

/** A failed step's reason. */
class StepFailure extends Error {}

class Session {
  private steps: SelfTestStep[] = []
  private error: string | null = null
  private chat: ChatState | null = null
  private projectRoot: string | null = null

  constructor(
    private readonly directory: string,
    private readonly projectPath: string | null,
  ) {}

  async run(): Promise<void> {
    log(`self-test started; output directory ${this.directory}`)
    const began = Date.now()
    const script: [string, () => Promise<string | null>][] = [
      ['wait-for-backend', () => this.waitForBackend()],
      ['open-project', () => this.openProject()],
      ['new-chat', () => this.newChat()],
      ['send-prompt', () => this.sendPrompt()],
      ['wait-for-finish', () => this.waitForFinish()],
      ['check-transcript', () => this.checkTranscript()],
      ['changes-tab', () => this.visitTab('changes')],
      ['project-tab', () => this.visitTab('project')],
      ['back-to-chat', () => this.backToChat()],
    ]
    let ok = true
    for (const [name, action] of script) {
      const stepBegan = Date.now()
      let stepOk = true
      let detail: string | null = null
      try {
        detail = await action()
      } catch (error) {
        stepOk = false
        detail = error instanceof Error ? error.message : String(error)
      }
      const elapsedMs = Date.now() - stepBegan
      this.steps.push({ name, ok: stepOk, detail, elapsedMs })
      if (stepOk) {
        log(`ok   ${name} (${elapsedMs} ms)${detail ? ` — ${detail}` : ''}`)
      } else {
        this.error = `${name}: ${detail ?? 'failed'}`
        log(`FAIL ${name}: ${detail ?? 'failed'}`)
        ok = false
        break
      }
    }
    const seconds = Math.round((Date.now() - began) / 1000)
    log(ok ? `self-test passed in ${seconds} s` : `self-test FAILED in ${seconds} s: ${this.error ?? 'unknown error'}`)
    await this.finish(ok)
  }

  // MARK: Steps

  private async waitForBackend(): Promise<string> {
    const deadline = Date.now() + BACKEND_TIMEOUT_MS
    while (Date.now() < deadline) {
      const state = useApp.getState()
      const connection = state.connection
      if (connection.kind === 'ready') {
        const result = connection.result
        return `ctx ${result.ctx_version} · protocol ${result.protocol_version}${result.mock ? ' · mock' : ''} · model ${result.model.name}`
      }
      if (connection.kind === 'failed' && !state.backendStarting) {
        const failure = describe(connection.failure)
        throw new StepFailure(`backend failure — ${failure.title}: ${clip(failure.detail, 600)}`)
      }
      await sleep(POLL_MS)
    }
    throw new StepFailure(`backend not ready after ${BACKEND_TIMEOUT_MS / 1000} s`)
  }

  private async openProject(): Promise<string> {
    if (this.projectPath === null) throw new StepFailure('no --project folder was given')
    // Launch already opens `--project`; this selects it (and opens it when the launch open lost a race).
    const project = (await useApp.getState().openProject(this.projectPath)) ?? (await this.waitFor(() => selectedProject(useApp.getState()), PROJECT_TIMEOUT_MS, 'the project to open'))
    this.projectRoot = project.root
    return `${project.name} at ${project.root} · ${project.info.file_count} files · model ${project.info.model}`
  }

  private async newChat(): Promise<string> {
    const state = useApp.getState()
    const project = selectedProject(state)
    if (!project) throw new StepFailure('no selected project')
    state.selectProject(project.id, false)
    state.newChat()
    await sleep(SETTLE_MS)
    const after = useApp.getState()
    if (after.selectedSessionId !== null || after.sidebarItem !== 'chat') {
      throw new StepFailure(`new chat did not reach the empty state (session ${after.selectedSessionId ?? 'null'}, tab ${after.sidebarItem})`)
    }
    return `empty chat in ${project.name}`
  }

  private async sendPrompt(): Promise<string> {
    await useApp.getState().send(SELFTEST_PROMPT)
    const chat = selectedChat(useApp.getState())
    if (!chat) throw new StepFailure('send did not create a session')
    this.chat = chat
    if (chat.lastError !== null && !chatStatusIsRunning(chat.status)) throw new StepFailure(`task/run failed: ${clip(chat.lastError, 400)}`)
    return `session ${chat.sessionId} · status ${describeStatus(chat.status)}`
  }

  private async waitForFinish(): Promise<string> {
    const chat = this.requireChat()
    const deadline = Date.now() + TASK_TIMEOUT_MS
    while (Date.now() < deadline) {
      const connection = useApp.getState().connection
      if (connection.kind === 'failed') {
        const failure = describe(connection.failure)
        throw new StepFailure(`backend failed while the task ran — ${failure.title}: ${clip(failure.detail, 400)}`)
      }
      const status = chat.status
      if (status.kind === 'finished') {
        if (status.status === 'failed' && chat.lastError !== null) throw new StepFailure(`task failed: ${clip(chat.lastError, 400)}`)
        return `status ${status.status} · ${chat.transcript.count} transcript items · ${chat.transcript.allToolCalls.length} tool calls`
      }
      if (status.kind === 'idle' && chat.lastError !== null) throw new StepFailure(`task/run failed: ${clip(chat.lastError, 400)}`)
      await sleep(POLL_MS)
    }
    throw new StepFailure(`task did not finish within ${TASK_TIMEOUT_MS / 1000} s (status ${describeStatus(chat.status)})`)
  }

  /// Two tool rows and the final reply: in the transcript, and drawn in the chat.
  private async checkTranscript(): Promise<string> {
    const chat = this.requireChat()
    // Let the last deltas flush (33 ms) and the chat render.
    await sleep(SETTLE_MS)
    const calls = chat.transcript.allToolCalls
    if (calls.length !== 2) throw new StepFailure(`expected 2 tool rows, found ${calls.length} (${calls.map((call) => call.name).join(', ')})`)
    const reply = finalReply(chat.transcript.items)
    if (reply === null) throw new StepFailure('no final assistant reply in the transcript')
    const labels = calls.map((call) => rowLabel(call))
    await this.expectOnScreen(labels, 'tool rows')
    await this.expectOnScreen([textFragment(reply)], 'the final reply')
    return `${labels.join(' · ')} · reply “${clip(reply, 80)}”`
  }

  private async visitTab(item: SidebarItem): Promise<string> {
    useApp.getState().setSidebarItem(item)
    await sleep(SETTLE_MS)
    const now = useApp.getState().sidebarItem
    if (now !== item) throw new StepFailure(`tab did not switch to ${item} (now ${now})`)
    // The top bar's tab pill follows (its accessible name carries the badge: "Changes, 3").
    const selectedTab = document.querySelector('[role="tablist"][aria-label="Tabs"] [role="tab"][aria-selected="true"]')
    const title = selectedTab?.getAttribute('aria-label') ?? ''
    if (title.length === 0) throw new StepFailure(`${item} is shown but no top-bar tab is selected`)
    return `${item} tab shown (“${title}” selected)`
  }

  private async backToChat(): Promise<string> {
    const chat = this.requireChat()
    const state = useApp.getState()
    state.setSidebarItem('chat')
    state.focusComposer()
    await sleep(SETTLE_MS)
    const after = useApp.getState()
    if (after.sidebarItem !== 'chat' || after.selectedSessionId !== chat.sessionId) {
      throw new StepFailure(`did not return to the chat (tab ${after.sidebarItem}, session ${after.selectedSessionId ?? 'null'})`)
    }
    const reply = finalReply(chat.transcript.items)
    if (reply !== null) await this.expectOnScreen([textFragment(reply)], 'the final reply after switching back')
    return `chat ${chat.sessionId} shown again`
  }

  // MARK: Result

  private async finish(ok: boolean): Promise<void> {
    const items = this.chat?.transcript.items ?? []
    const result: SelfTestResult = {
      ok,
      steps: this.steps,
      transcriptItems: items.length,
      transcript: items.map(itemLabel),
      error: this.error,
      directory: this.directory,
      project: this.projectRoot,
      finishedAt: new Date().toISOString(),
    }
    try {
      const path = await invoke<string>('selftest_finish', { dir: this.directory, result_json: JSON.stringify(result, null, 2), exit_code: ok ? 0 : 1 })
      log(`wrote ${path}`)
    } catch (error) {
      log(`could not write result.json: ${error instanceof Error ? error.message : String(error)}`)
    }
  }

  // MARK: Helpers

  private requireChat(): ChatState {
    if (!this.chat) throw new StepFailure('no chat')
    return this.chat
  }

  private async waitFor<T>(probe: () => T | null | undefined, timeoutMs: number, what: string): Promise<T> {
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      const value = probe()
      if (value !== null && value !== undefined) return value
      await sleep(POLL_MS)
    }
    throw new StepFailure(`timed out waiting for ${what}`)
  }

  /** Waits (up to 3 s) until every fragment is in the window's visible text. */
  private async expectOnScreen(fragments: string[], what: string): Promise<void> {
    const deadline = Date.now() + 3000
    let missing = fragments
    while (Date.now() < deadline) {
      const text = normalize(document.body.innerText)
      missing = fragments.filter((fragment) => !text.includes(normalize(fragment)))
      if (missing.length === 0) return
      await sleep(POLL_MS)
    }
    throw new StepFailure(`${what} not on screen: ${missing.map((fragment) => `“${clip(fragment, 60)}”`).join(', ')}`)
  }
}

// MARK: - Helpers

/** The last assistant turn's text, once it is final. */
function finalReply(items: readonly TranscriptItem[]): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index]
    if (item?.kind !== 'assistant') continue
    const text = item.text.trim()
    if (text.length > 0 && !item.isStreaming) return text
  }
  return null
}

/// The start of a Markdown reply as it reads on screen: the first line
/// without Markdown punctuation, at most 40 characters.
function textFragment(markdown: string): string {
  const firstLine = markdown.split('\n').find((line) => line.trim().length > 0) ?? ''
  const plain = firstLine
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_`#>]/g, '')
    .replace(/^\s*(?:[-+]|\d+\.)\s+/, '')
    .trim()
  return [...plain].slice(0, 40).join('').trim()
}

function normalize(text: string): string {
  return text.replace(/\s+/g, ' ').trim()
}

/** `UITestDriver.label(for:)`. */
function itemLabel(item: TranscriptItem): string {
  switch (item.kind) {
    case 'user':
      return item.isRuntime ? 'user(runtime)' : 'user'
    case 'assistant':
      return 'assistant'
    case 'tools':
      return `tools(${item.calls.length})`
    case 'agents':
      return `agents(${item.agents.length})`
    case 'approval':
      return `approval(${item.decision ?? 'pending'})`
    case 'outcome':
      return `outcome(${item.outcome.status}${item.summaryRepeatsReply ? ', summary repeats reply' : ''})`
    case 'notice':
      return item.isError ? 'notice(error)' : 'notice'
  }
}

function describeStatus(status: ChatStatus): string {
  switch (status.kind) {
    case 'idle':
      return 'idle'
    case 'loadingHistory':
      return 'loadingHistory'
    case 'running':
      return `running(${status.progress.state.toLowerCase()}, step ${status.progress.step})`
    case 'finished':
      return `finished(${status.status})`
  }
}

function clip(text: string, limit: number): string {
  const flat = text.replace(/\n/g, ' ')
  return [...flat].length <= limit ? flat : [...flat].slice(0, limit).join('') + '…'
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

/// Each step goes to the developer console (the shell's stderr shows the
/// webview's console only in development builds; `result.json` has it all).
function log(message: string): void {
  // eslint-disable-next-line no-console -- the self-test's step log, like UITestDriver's stderr lines
  console.info(`[autotest] ${message}`)
}
