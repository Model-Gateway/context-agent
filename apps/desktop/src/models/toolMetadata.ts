// Port of apps/macos/ContextAgent/Models/ToolMetadata.swift: presentation
// metadata for the backend's tools (docs/desktop-protocol.md "Tool names"):
// icon, human label, category chip and the verbs used in timeline rows
// ("Reading src/app.rs") and summaries ("Read 3 files"). SF Symbol names
// become keys of the `Icons` catalogue in theme/icons.tsx.

import { DISCOUNT_DROP_TOOL, jsonArray, jsonBool, jsonInt, jsonObject, jsonString, type JSONValue } from '../backend/types'
import type { IconName } from '../theme/icons'
import { toolCallTarget, type ToolCallItem } from './transcript'

/** `ToolCategory`: chip label for a tool's category in the timeline. */
export type ToolCategory = 'file' | 'search' | 'shell' | 'git' | 'memory' | 'validation' | 'context' | 'agents' | 'browser' | 'mcp'

/** `ToolMetadata`. */
export interface ToolMetadata {
  icon: IconName
  label: string
  category: ToolCategory
  /** Present participle: "Reading". */
  verb: string
  /** Past tense: "Read". */
  pastVerb: string
}

/// `ToolMetadata.knownToolNames`: the 15 built-in tools, the sub-agent
/// tools under Ultra effort, and the desktop app's `browser`.
export const KNOWN_TOOL_NAMES: readonly string[] = [
  'read_file',
  'search_code',
  'find_symbol',
  'find_references',
  'list_files',
  'apply_patch',
  'write_file',
  'execute_command',
  'run_validation',
  'git_status',
  'git_diff',
  'retrieve_memory',
  'inspect_context',
  'web_search',
  'web_fetch',
  'spawn_agents',
  'send_input',
  'wait_agents',
  'read_agent',
  'close_agent',
  'browser',
  'ask_user',
]

function meta(icon: IconName, label: string, category: ToolCategory, verb: string, pastVerb: string): ToolMetadata {
  return { icon, label, category, verb, pastVerb }
}

/** `ToolMetadata.info(for:)`. */
export function toolInfo(toolName: string): ToolMetadata {
  switch (toolName) {
    case 'read_file':
      return meta('file', 'Read file', 'file', 'Reading', 'Read') // doc.text
    case 'search_code':
      return meta('search', 'Search code', 'search', 'Searching', 'Searched') // magnifyingglass
    case 'find_symbol':
      return meta('code', 'Find symbol', 'search', 'Finding', 'Found') // function
    case 'find_references':
      return meta('branch', 'Find references', 'search', 'Finding', 'Found') // arrow.triangle.branch
    case 'list_files':
      return meta('folder', 'List files', 'file', 'Listing', 'Listed') // folder
    case 'apply_patch':
      return meta('edit', 'Apply patch', 'file', 'Editing', 'Edited') // pencil.line
    case 'write_file':
      return meta('editor', 'Write file', 'file', 'Writing', 'Wrote') // square.and.pencil
    case 'execute_command':
      return meta('terminal', 'Run command', 'shell', 'Running', 'Ran') // terminal
    case 'run_validation':
      return meta('validationPass', 'Run validation', 'validation', 'Validating', 'Validated') // checkmark.seal
    case 'git_status':
      return meta('pull', 'Git status', 'git', 'Inspecting', 'Inspected') // arrow.triangle.pull
    case 'git_diff':
      return meta('pull', 'Git diff', 'git', 'Inspecting', 'Inspected') // arrow.triangle.pull
    case 'retrieve_memory':
      return meta('memory', 'Retrieve memory', 'memory', 'Recalling', 'Recalled') // brain.head.profile
    case 'inspect_context':
      return meta('context', 'Inspect context', 'context', 'Inspecting', 'Inspected') // text.magnifyingglass
    case 'web_search':
      return meta('browser', 'Search the web', 'search', 'Searching', 'Searched') // globe
    case 'web_fetch':
      return meta('externalLink', 'Fetch page', 'search', 'Fetching', 'Fetched') // safari
    case 'spawn_agents':
      return meta('agents', 'Deploy agents', 'agents', 'Deploying', 'Deployed')
    case 'send_input':
      return meta('agents', 'Message agent', 'agents', 'Messaging', 'Messaged')
    case 'wait_agents':
      return meta('agents', 'Wait for agents', 'agents', 'Waiting for', 'Waited for')
    case 'read_agent':
      return meta('agents', 'Check on agent', 'agents', 'Checking on', 'Checked on')
    case 'close_agent':
      return meta('agents', 'Stop agent', 'agents', 'Stopping', 'Stopped')
    case 'browser':
      return meta('browser', 'Browser', 'browser', 'Browsing', 'Browsed')
    case 'ask_user':
      return meta('chat', 'Ask you', 'context', 'Asking', 'Asked') // questionmark.bubble
    case DISCOUNT_DROP_TOOL:
      return meta('spending', 'Lower discount', 'shell', 'Continuing', 'Continued')
    default: {
      const parts = mcpParts(toolName)
      if (parts) return meta('mcp', `${parts.server} › ${parts.tool}`, 'mcp', 'Calling', 'Called')
      return meta('fix', humanize(toolName), 'shell', 'Calling', 'Called') // wrench
    }
  }
}

/** The `(server: String, tool: String)` of `ToolMetadata.mcpParts(_:)`. */
export interface McpToolParts {
  server: string
  tool: string
}

/// `ToolMetadata.mcpParts(_:)`: `mcp__<server>__<tool>` → the server and
/// tool names (docs/desktop-protocol.md "Tool names"); null for any other name.
export function mcpParts(toolName: string): McpToolParts | null {
  if (!toolName.startsWith('mcp__')) return null
  const rest = toolName.slice(5)
  const cut = rest.indexOf('__')
  if (cut < 0) return null
  const server = rest.slice(0, cut)
  const tool = rest.slice(cut + 2)
  if (server.length === 0 || tool.length === 0) return null
  return { server, tool }
}

/// `ToolMetadata.pathArgument(for:)`: the call's path-like argument
/// (`path`, then `directory`, then `cwd`), for context menus (Show in
/// Changes, Reveal in Finder, …).
export function pathArgument(call: ToolCallItem): string | null {
  for (const key of ['path', 'directory', 'cwd']) {
    const value = argumentString(call.arguments, key)
    if (value !== null) return value
  }
  return null
}

/// `ToolMetadata.rowLabel(for:)`: timeline row label: verb + target.
/// Commands are wrapped in backticks: "Reading src/app.rs", "Running
/// `cargo test`", "Searching “needle”".
export function rowLabel(call: ToolCallItem): string {
  const info = toolInfo(call.name)
  const args = call.arguments

  switch (call.name) {
    case 'read_file':
    case 'write_file':
    case 'apply_patch': {
      const path = pathArgument(call)
      if (path !== null) return `${info.verb} ${path}`
      break
    }
    case 'list_files': {
      const pattern = argumentString(args, 'pattern')
      const directory = argumentString(args, 'directory')
      if (pattern !== null && directory !== null) return `Listing ${pattern} in ${directory}`
      if (pattern !== null) return `Listing ${pattern}`
      if (directory !== null) return `Listing ${directory}`
      return 'Listing files'
    }
    case 'search_code': {
      const query = argumentString(args, 'query')
      if (query !== null) return `Searching “${abbreviate(query)}”`
      break
    }
    case 'find_symbol': {
      const name = argumentString(args, 'name')
      if (name !== null) return `Finding ${abbreviate(name)}`
      break
    }
    case 'find_references': {
      const name = argumentString(args, 'name')
      if (name !== null) return `Finding references to ${abbreviate(name)}`
      break
    }
    case 'execute_command': {
      const command = argumentString(args, 'command')
      if (command !== null) return `Running \`${abbreviate(command)}\``
      break
    }
    case 'run_validation': {
      const command = argumentString(args, 'command')
      if (command !== null) return `Validating \`${abbreviate(command)}\``
      const kind = argumentString(args, 'kind')
      if (kind !== null && kind !== 'all') return `Validating ${kind}`
      return 'Validating'
    }
    case 'git_status':
      return 'Inspecting git status'
    case 'git_diff': {
      const path = pathArgument(call)
      if (path !== null) return `Inspecting diff of ${path}`
      return 'Inspecting git diff'
    }
    case 'retrieve_memory': {
      const query = argumentString(args, 'query')
      if (query !== null) return `Recalling “${abbreviate(query)}”`
      if (jsonBool(jsonObject(args)?.recent_tasks) === true) return 'Recalling recent tasks'
      const task = argumentString(args, 'task_id')
      if (task !== null) return `Recalling task ${abbreviate(task)}`
      const category = argumentString(args, 'category')
      if (category !== null) return `Recalling ${category.replace(/_/g, ' ')}`
      return 'Recalling memory'
    }
    case 'inspect_context': {
      const what = argumentString(args, 'what')
      if (what !== null) return `Inspecting context ${what}`
      return 'Inspecting context'
    }
    case 'web_search': {
      const query = argumentString(args, 'query')
      if (query !== null) return `Searching the web for “${abbreviate(query)}”`
      return 'Searching the web'
    }
    case 'web_fetch': {
      const url = argumentString(args, 'url')
      if (url !== null) return `Fetching ${abbreviate(url)}`
      return 'Fetching a page'
    }
    case 'browser':
      return browserRowLabel(args)
    case 'ask_user': {
      const questions = (jsonArray(jsonObject(args)?.questions) ?? []).flatMap((entry) => {
        const question = jsonString(jsonObject(entry)?.question)
        return question === null ? [] : [question]
      })
      if (questions.length === 1) return `Asking “${abbreviate(questions[0] ?? '')}”`
      return questions.length === 0 ? 'Asking you' : `Asking you ${questions.length} questions`
    }
    case 'spawn_agents': {
      const agents = jsonArray(jsonObject(args)?.agents) ?? []
      const names = agents.flatMap((entry) => {
        const name = jsonString(jsonObject(entry)?.name)
        return name === null ? [] : [name]
      })
      if (agents.length === 0) return 'Deploying agents'
      return `Deploying ${agents.length} agent${agents.length === 1 ? '' : 's'}` + (names.length === 0 ? '' : ': ' + abbreviate(names.join(', ')))
    }
    case 'send_input': {
      const id = argumentString(args, 'id')
      if (id !== null) return `Messaging agent ${id}`
      return 'Messaging an agent'
    }
    case 'wait_agents': {
      const ids = (jsonArray(jsonObject(args)?.ids) ?? []).flatMap((entry) => (typeof entry === 'string' ? [entry] : []))
      return ids.length === 0 ? 'Waiting for agents' : 'Waiting for ' + abbreviate(ids.join(', '))
    }
    case 'read_agent': {
      const id = argumentString(args, 'id')
      if (id !== null) return `Checking on agent ${id}`
      return 'Listing agents'
    }
    case 'close_agent': {
      const id = argumentString(args, 'id')
      if (id !== null) return `Stopping agent ${id}`
      return 'Stopping an agent'
    }
    default:
      break
  }

  const parts = mcpParts(call.name)
  if (parts) return `Calling ${parts.tool} on ${parts.server}`
  const target = toolCallTarget(call)
  if (target !== null) return `${info.verb} ${abbreviate(target)}`
  return KNOWN_TOOL_NAMES.includes(call.name) ? info.label : `${info.verb} ${info.label.toLowerCase()}`
}

/** `(added: Int, removed: Int)` of `lineChanges(for:)`. */
export interface LineChanges {
  added: number
  removed: number
}

/// `ToolMetadata.lineChanges(for:)`: lines added / removed by a
/// `write_file` or `apply_patch` call, from its arguments, for the "+12 −3"
/// at the right of its row. `apply_patch` edits drop the lines `old_text`
/// and `new_text` share at either end, so the context the model quoted is
/// not counted; `write_file` counts every line as added (an overwrite's old
/// content is not in the call).
export function lineChanges(call: ToolCallItem): LineChanges | null {
  const object = jsonObject(call.arguments)
  switch (call.name) {
    case 'write_file': {
      const content = jsonString(object?.content)
      if (content === null) return null
      return { added: linesOf(content).length, removed: 0 }
    }
    case 'apply_patch': {
      const edits = jsonArray(object?.edits)
      if (!edits || edits.length === 0) return null
      let added = 0
      let removed = 0
      for (const entry of edits) {
        const edit = jsonObject(entry)
        let old = linesOf(jsonString(edit?.old_text) ?? jsonString(edit?.old) ?? '')
        let fresh = linesOf(jsonString(edit?.new_text) ?? jsonString(edit?.new) ?? '')
        while (old.length > 0 && fresh.length > 0 && old[0] === fresh[0]) {
          old = old.slice(1)
          fresh = fresh.slice(1)
        }
        while (old.length > 0 && fresh.length > 0 && old[old.length - 1] === fresh[fresh.length - 1]) {
          old = old.slice(0, -1)
          fresh = fresh.slice(0, -1)
        }
        added += fresh.length
        removed += old.length
      }
      return { added, removed }
    }
    default:
      return null
  }
}

/** "Opening example.com", "Clicking [3]", "Typing into [2]" … */
function browserRowLabel(args: JSONValue): string {
  const object = jsonObject(args)
  const refNumber = jsonInt(object?.ref)
  const ref = refNumber === null ? 'an element' : `[${refNumber}]`
  switch (argumentString(args, 'action')) {
    case 'navigate': {
      const url = argumentString(args, 'url')
      if (url !== null) {
        const shown = url.replace(/^https?:\/\//, '')
        return `Opening ${abbreviate(shown, 60)}`
      }
      return 'Opening a page'
    }
    case 'back':
      return 'Going back'
    case 'forward':
      return 'Going forward'
    case 'reload':
      return 'Reloading the page'
    case 'read':
      return 'Reading the page'
    case 'snapshot': {
      const query = argumentString(args, 'query')
      if (query !== null) return `Finding “${abbreviate(query)}” on the page`
      return "Listing the page's elements"
    }
    case 'click':
      return `Clicking ${ref}`
    case 'type': {
      const text = argumentString(args, 'text')
      if (text !== null) return `Typing “${abbreviate(text, 40)}” into ${ref}`
      return `Typing into ${ref}`
    }
    case 'select':
      return `Choosing an option in ${ref}`
    case 'scroll':
      if (refNumber !== null) return `Scrolling to ${ref}`
      return `Scrolling ${argumentString(args, 'direction') ?? 'down'}`
    case 'evaluate':
      return 'Running script in the page'
    case 'screenshot':
      return 'Taking a screenshot'
    default:
      return 'Browsing'
  }
}

// MARK: - Helpers

/** "a\nb\n" → ["a", "b"]; "" → []. */
function linesOf(text: string): string[] {
  if (text.length === 0) return []
  const parts = text.split('\n')
  if (parts.length > 1 && parts[parts.length - 1] === '') parts.pop()
  return parts
}

/** `ToolMetadata.string(_:_:)`: a trimmed, non-empty string argument. */
function argumentString(args: JSONValue, key: string): string | null {
  const raw = jsonString(jsonObject(args)?.[key])
  if (raw === null) return null
  const trimmed = raw.trim()
  return trimmed.length === 0 ? null : trimmed
}

/** `ToolMetadata.abbreviate(_:limit:)`: one line, whitespace collapsed, capped at `limit` characters with an ellipsis. */
export function abbreviate(text: string, limit = 80): string {
  const collapsed = text.split(/\s+/).filter((part) => part.length > 0).join(' ')
  const chars = Array.from(collapsed)
  if (chars.length <= limit) return collapsed
  return chars.slice(0, Math.max(0, limit - 1)).join('').replace(/[ \t]+$/, '') + '…'
}

/** `ToolMetadata.humanize(_:)`: "my_custom_tool" → "My custom tool". */
function humanize(name: string): string {
  const words = name.split(/[_.-]/).filter((word) => word.length > 0)
  const first = words[0]
  if (first === undefined) return name
  const rest = words.slice(1).map((word) => word.toLowerCase())
  return [first.slice(0, 1).toUpperCase() + first.slice(1).toLowerCase(), ...rest].join(' ')
}

/// `ToolMetadata`'s static API under its Swift name, for call sites ported
/// verbatim (`ToolMetadata.info(for:)` → `ToolMetadata.info(name)`).
export const ToolMetadata = {
  knownToolNames: KNOWN_TOOL_NAMES,
  info: toolInfo,
  mcpParts,
  pathArgument,
  rowLabel,
  lineChanges,
  abbreviate,
} as const
