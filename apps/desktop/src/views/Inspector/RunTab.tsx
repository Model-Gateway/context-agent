import { useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { StatusPill } from '../../components/StatusPill'
import { Tooltip } from '../../components/Tooltip'
import { executionStateLabel, tokenUsageTotal, type ContextStats, type TaskOutcome } from '../../backend/types'
import { chatStatusIsRunning, chatStatusProgress } from '../../models/reducer'
import { TranscriptFormat } from '../../models/transcript'
import { approvalModeLabel, remoteConnection, useApp, useSelectedChat, useSelectedProject } from '../../state/app'
import { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { connectionFailure, useConnectionLabel } from '../../state/selectors'
import { SETTINGS_TAB_REMOTE_HOSTS } from '../../state/app'
import { Button } from '../../components/Button'
import { hostTarget } from '../../models/sshHost'
import { Icon } from '../../theme/icons'
import { BackendFailurePanel } from '../Welcome/BackendFailurePanel'
import { ChangesCard } from './ChangesCard'
import { InspectorCard, StatRow } from './InspectorCard'
import { MiddleText } from './MiddleText'
import { useNow } from './inspectorShared'
import styles from './RunTab.module.css'

/**
 * Port of `RunTab` (Views/Inspector/InspectorView.swift): with a chat, the
 * Backend card, the chat's task state, its context statistics, the
 * project's uncommitted changes (git projects) and the counters, 12 px
 * apart in a scrolling column; a git project without a chat (a fresh
 * worktree) still shows the Backend and Changes cards; otherwise the
 * Backend card over "No chat selected".
 */
export function RunTab() {
  const chat = useSelectedChat()
  const project = useSelectedProject()
  if (chat) {
    return (
      <div className={styles.scroll}>
        <div className={styles.stack}>
          <BackendCard />
          <TaskStateCard chat={chat} />
          <ContextStatsCard chat={chat} />
          {project && project.isGitRepository ? <ChangesCard project={project} /> : null}
          <CountersCard chat={chat} />
        </div>
      </div>
    )
  }
  if (project && project.isGitRepository) {
    // A fresh worktree has no chat yet but can still be committed, pushed or merged.
    return (
      <div className={styles.scroll}>
        <div className={styles.stack}>
          <BackendCard />
          <ChangesCard project={project} />
        </div>
      </div>
    )
  }
  return (
    <div className={styles.emptyColumn}>
      <div className={styles.backendPadding}>
        <BackendCard />
      </div>
      <EmptyPlaceholder icon="info" title="No chat selected" subtitle="Context statistics and counters of the selected chat show up here." />
    </div>
  )
}

// MARK: - Task

/** `TaskStateCard`: status pill and clock, state and step, cancel / approval / error, the session's model. */
function TaskStateCard({ chat }: { chat: ChatState }) {
  const progress = chatStatusProgress(chat.status)
  const now = useNow(progress !== null)
  const sessionModel = useApp((state) => state.modelDisplayName(chat.summary.model))
  return (
    <InspectorCard title="Task">
      <div className={styles.taskHeader}>
        <TaskStatusPill chat={chat} />
        <span className={styles.flexSpacer} />
        {progress ? <span className={styles.elapsed}>{TranscriptFormat.duration((now - progress.startedAt.getTime()) / 1000)}</span> : null}
      </div>
      {progress ? (
        <>
          <StatRow label="State" value={executionStateLabel(progress.state)} />
          <StatRow label="Step" value={`${progress.step}`} />
          {chat.cancelRequested ? (
            <div className={styles.cancelling}>
              <BrailleSpinner size={10} tint="var(--text-tertiary)" />
              <span>Cancelling…</span>
            </div>
          ) : null}
        </>
      ) : null}
      {chat.pendingApproval ? <StatRow label="Approval" value={ChatState.clip(chat.pendingApproval.summary, 36)} tint="var(--warning)" /> : null}
      {chat.lastError !== null ? <p className={`${styles.error} ${styles.clamp4} selectable`}>{chat.lastError}</p> : null}
      <StatRow label="Session model" value={sessionModel} />
    </InspectorCard>
  )
}

function TaskStatusPill({ chat }: { chat: ChatState }) {
  const status = chat.status
  switch (status.kind) {
    case 'idle':
      return <StatusPill running={false} />
    case 'loadingHistory':
      return <StatusPill text="Loading history" tint="var(--text-tertiary)" />
    case 'running':
      return <StatusPill running />
    case 'finished':
      return <StatusPill status={status.status} />
  }
}

// MARK: - Context

/**
 * `ContextStatsCard`: the context window as one stacked bar — the last
 * compiled prompt's slices, the autocompact buffer pinned to the end and the
 * free space between — with a legend row per slice (tokens and share of the
 * window) and the MCP tools broken down by server; or why there is none yet.
 */
function ContextStatsCard({ chat }: { chat: ChatState }) {
  const stats = chat.contextStats
  const [hovered, setHovered] = useState<string | null>(null)
  const [serversExpanded, setServersExpanded] = useState(false)
  if (!stats) {
    return (
      <InspectorCard title="Context">
        <p className={styles.note}>
          {chatStatusIsRunning(chat.status)
            ? 'Waiting for the first context compilation…'
            : 'No context statistics yet. They appear once a run compiles its prompt.'}
        </p>
      </InspectorCard>
    )
  }
  const layout = contextLayout(stats)
  const usedPercent = layout.window > 0 ? Math.round((stats.estimated_prompt_tokens / layout.window) * 100) : 0
  const nearCompaction = stats.estimated_prompt_tokens >= layout.window - contextBufferTokens(stats)
  const dim = (id: string) => (hovered !== null && hovered !== id ? styles.dimmed : '')
  const legend: ContextSlice[] = [
    ...layout.slices,
    ...(layout.buffer > 0 ? [{ id: 'buffer', label: 'Autocompact buffer', tokens: layout.buffer, color: CONTEXT_BUFFER_COLOR }] : []),
    { id: 'free', label: 'Free space', tokens: layout.free, color: 'var(--surface-raised)' },
  ]
  return (
    <InspectorCard title="Context">
      <div
        className={styles.contextHeader}
        role="text"
        aria-label={`Context ${usedPercent} percent used, ${TranscriptFormat.tokens(stats.estimated_prompt_tokens)} of ${TranscriptFormat.tokens(stats.context_window)} tokens`}
      >
        <span className={styles.contextUsed} style={nearCompaction ? { color: 'var(--warning)' } : undefined}>
          {usedPercent}% used
        </span>
        <span className={`${styles.contextTotal} selectable`}>
          {TranscriptFormat.tokens(stats.estimated_prompt_tokens)} / {TranscriptFormat.tokens(stats.context_window)}
        </span>
      </div>
      <div className={styles.contextBar} aria-hidden>
        <div className={styles.contextSlices}>
          {layout.slices.map((slice) => (
            <div
              key={slice.id}
              className={`${styles.contextSlice} ${dim(slice.id)}`}
              style={{ width: `max(2px, calc(${(slice.tokens / layout.window) * 100}% - 1px))`, background: slice.color }}
              title={`${slice.label} · ${TranscriptFormat.tokens(slice.tokens)}`}
            />
          ))}
        </div>
        {layout.buffer > 0 ? (
          <div
            className={`${styles.contextBuffer} ${dim('buffer')}`}
            style={{ width: `${(layout.buffer / layout.window) * 100}%`, background: CONTEXT_BUFFER_COLOR }}
          />
        ) : null}
      </div>
      <div className={styles.contextLegend}>
        {legend.map((slice) => {
          const quiet = slice.id === 'free' || slice.id === 'buffer'
          const row = (
            <div
              key={slice.id}
              className={styles.legendRow}
              onMouseEnter={() => setHovered(slice.id)}
              onMouseLeave={() => setHovered((current) => (current === slice.id ? null : current))}
              role="text"
              aria-label={`${slice.label}${slice.detail ? `, ${slice.detail}` : ''}: ${TranscriptFormat.tokens(slice.tokens)} tokens, ${contextPercent(slice.tokens, layout.window)}`}
            >
              <span className={`${styles.legendSwatch} ${slice.id === 'free' ? styles.legendSwatchFree : ''}`} style={{ background: slice.color }} />
              <span className={quiet ? styles.legendLabelQuiet : styles.legendLabel}>{slice.label}</span>
              {slice.detail ? <span className={styles.legendDetail}>{slice.detail}</span> : null}
              <span className={styles.legendSpacer} />
              <span className={styles.legendTokens}>{TranscriptFormat.tokens(slice.tokens)}</span>
              <span className={quiet ? styles.legendPercentQuiet : styles.legendPercent}>{contextPercent(slice.tokens, layout.window)}</span>
            </div>
          )
          return slice.id === 'buffer' ? (
            <Tooltip key={slice.id} content={`The conversation is compacted once the prompt reaches ${TranscriptFormat.tokens(layout.window - contextBufferTokens(stats))} tokens`}>
              {row}
            </Tooltip>
          ) : (
            row
          )
        })}
      </div>
      {layout.servers.length > 0 ? (
        <div className={styles.serverList}>
          <button
            type="button"
            className={`${styles.serverToggle} ${serversExpanded ? styles.serverToggleOpen : ''}`}
            onClick={() => setServersExpanded((open) => !open)}
            aria-expanded={serversExpanded}
            aria-label={`MCP servers, ${layout.servers.length}`}
          >
            <Icon name="chevronRight" size={9} strokeWidth={2.5} className={`${styles.serverChevron} ${serversExpanded ? styles.serverChevronOpen : ''}`} />
            <span className={styles.legendLabel}>MCP servers</span>
            <span className={styles.legendSpacer} />
            <span className={styles.legendTokens}>{layout.servers.reduce((sum, group) => sum + group.tools, 0)} tools</span>
          </button>
          {serversExpanded
            ? layout.servers.map((group) => (
                <div key={group.server} className={styles.serverRow}>
                  <MiddleText text={group.server ?? ''} className={styles.serverName} />
                  <span className={styles.legendDetail}>
                    · {group.tools} tool{group.tools === 1 ? '' : 's'}
                  </span>
                  <span className={styles.legendSpacer} />
                  <span className={styles.legendTokens}>{TranscriptFormat.tokens(group.tokens)}</span>
                </div>
              ))
            : null}
        </div>
      ) : null}
      <div className={styles.contextFooter}>
        <Tooltip content="The most repository source this prompt could include">
          <span className={styles.note}>Retrieval budget {TranscriptFormat.tokens(stats.retrieval_budget_tokens)}</span>
        </Tooltip>
        <span className={styles.legendSpacer} />
        {stats.compacted ? (
          <Tooltip content="The conversation was compacted to fit the context window">
            <Chip icon="collapseAll" variant="tinted" tint="var(--warning)">
              compacted
            </Chip>
          </Tooltip>
        ) : null}
      </div>
    </InspectorCard>
  )
}

/** One slice of the window in `ContextStatsCard` (`ContextLayout.Slice`). */
interface ContextSlice {
  id: string
  label: string
  detail?: string
  tokens: number
  color: string
}

const CONTEXT_BUFFER_COLOR = 'light-dark(#b9bcc3, #4a4c52)'

/** The window past the compaction threshold; the default threshold (75 %) when ctx predates reporting it. */
function contextBufferTokens(stats: ContextStats): number {
  const threshold = stats.compaction_threshold_tokens > 0 ? stats.compaction_threshold_tokens : Math.floor((stats.context_window * 3) / 4)
  return Math.max(0, stats.context_window - threshold)
}

/** `ContextLayout`: the prompt's slices in legend order, then the buffer and the free space, which add up to the window. */
function contextLayout(stats: ContextStats) {
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
  const slices: ContextSlice[] = [
    { id: 'conversation', label: 'Conversation', detail: plural(stats.conversation_messages, 'message'), tokens: stats.conversation_tokens, color: 'light-dark(#6e76ee, #8c92ff)' },
    { id: 'repository', label: 'Repository', detail: plural(stats.repository_snippets, 'snippet'), tokens: stats.repository_tokens, color: 'light-dark(#2e95d3, #5db6ee)' },
  ]
  const servers = stats.tool_groups.filter((group) => group.server !== null)
  const memory: ContextSlice = { id: 'memory', label: 'Memory', detail: plural(stats.memory_records, 'record'), tokens: stats.memory_tokens, color: 'light-dark(#1c8c86, #3db8ae)' }
  let accounted = stats.conversation_tokens + stats.repository_tokens + stats.memory_tokens
  if (stats.system_prompt_tokens > 0 || stats.tool_tokens > 0) {
    const mcpTokens = servers.reduce((sum, group) => sum + group.tokens, 0)
    const builtin = stats.tool_groups.find((group) => group.server === null)
    if (mcpTokens > 0) {
      slices.push({ id: 'mcp', label: 'MCP tools', detail: plural(servers.length, 'server'), tokens: mcpTokens, color: 'light-dark(#e08a2e, #f2ae62)' })
    }
    slices.push({ id: 'tools', label: 'Built-in tools', detail: builtin ? plural(builtin.tools, 'tool') : undefined, tokens: stats.tool_tokens - mcpTokens, color: 'light-dark(#23a266, #5bd497)' })
    slices.push(memory)
    slices.push({ id: 'instructions', label: 'Instructions', tokens: stats.instructions_tokens, color: 'light-dark(#9b5de5, #b98cff)' })
    slices.push({ id: 'system', label: 'System prompt', tokens: stats.system_prompt_tokens, color: 'light-dark(#3f6f91, #6f9dc0)' })
    accounted += stats.stable_prefix_tokens + stats.instructions_tokens
  } else {
    // Events from before ctx split the stable prefix.
    slices.push(memory)
    slices.push({ id: 'system', label: 'System prompt & tools', tokens: stats.stable_prefix_tokens, color: 'light-dark(#3f6f91, #6f9dc0)' })
    accounted += stats.stable_prefix_tokens
  }
  // Retrieval summary, checkpoint and runtime notes around the snippets.
  const other = stats.estimated_prompt_tokens - accounted
  if (other > 0) {
    slices.push({ id: 'other', label: 'Summaries & notes', tokens: other, color: 'light-dark(#9a9ea6, #7a7e86)' })
  }
  const window = Math.max(stats.context_window, stats.estimated_prompt_tokens)
  const used = stats.estimated_prompt_tokens
  const buffer = Math.max(0, Math.min(contextBufferTokens(stats), window - used))
  return { window, slices: slices.filter((slice) => slice.tokens > 0), buffer, free: Math.max(0, window - used - buffer), servers }
}

/** 19.4%, <0.1% for a sliver, 0% for nothing. */
function contextPercent(tokens: number, window: number): string {
  if (window <= 0 || tokens <= 0) return '0%'
  const value = (tokens / window) * 100
  return value < 0.1 ? '<0.1%' : `${value.toFixed(1)}%`
}

// MARK: - Counters

/** `CountersCard`: the last task's authoritative counts once finished, the session's running counts otherwise. */
function CountersCard({ chat }: { chat: ChatState }) {
  const outcome = finishedOutcome(chat)
  const toolCalls = chat.transcript.allToolCalls.length + (chat.liveTools?.calls.length ?? 0)
  const modelCalls = chat.transcript.items.reduce((count, item) => (item.kind === 'assistant' ? count + 1 : count), 0) + (chat.live === null ? 0 : 1)
  const usage = chat.usage
  const cached = usage?.cached_prompt_tokens ?? null
  return (
    <InspectorCard title={outcome === null ? 'Counters · session' : 'Counters · last task'}>
      {outcome !== null ? (
        <>
          <StatRow label="Steps" value={`${outcome.steps}`} />
          <StatRow label="Model calls" value={`${outcome.model_calls}`} />
          <StatRow label="Tool calls" value={`${outcome.tool_calls}`} />
          <StatRow label="Duration" value={TranscriptFormat.durationMs(outcome.duration_ms)} />
          <StatRow label="Tokens" value={TranscriptFormat.tokens(tokenUsageTotal(outcome.usage))} />
          {outcome.cost_usd != null ? <StatRow label="Cost" value={TranscriptFormat.cost(outcome.cost_usd)} /> : null}
        </>
      ) : (
        <>
          <StatRow label="Steps" value={`${chatStatusProgress(chat.status)?.step ?? 0}`} />
          <StatRow label="Model calls" value={`${modelCalls}`} />
          <StatRow label="Tool calls" value={`${toolCalls}`} />
          {usage ? (
            <>
              <StatRow
                label="Tokens"
                value={`${TranscriptFormat.tokens(tokenUsageTotal(usage))} (${TranscriptFormat.tokens(usage.prompt_tokens)} in · ${TranscriptFormat.tokens(usage.completion_tokens)} out)`}
              />
              {cached !== null && cached > 0 ? <StatRow label="Cached" value={TranscriptFormat.tokens(cached)} /> : null}
            </>
          ) : null}
          {chat.costUsd !== null ? <StatRow label="Cost" value={TranscriptFormat.cost(chat.costUsd)} /> : null}
        </>
      )}
      {chat.changedPaths.size > 0 ? <StatRow label="Files changed" value={`${chat.changedPaths.size}`} /> : null}
    </InspectorCard>
  )
}

/** The last outcome when the chat finished, for authoritative counts. */
function finishedOutcome(chat: ChatState): TaskOutcome | null {
  if (chat.status.kind !== 'finished') return null
  const last = chat.transcript.last
  return last && last.kind === 'outcome' ? last.outcome : null
}

// MARK: - Backend card

/**
 * `BackendCard`: the connection dot and label, the user-level default model
 * and the selected project's approval mode; a click opens Settings. While
 * the connection has failed it also shows the failure (the Shell's compact
 * `BackendFailurePanel`: its summary, Retry and Choose binary…). For a
 * project on a remote host the card describes the host's connection
 * instead (its state, the host, Reconnect after a failure).
 */
function BackendCard() {
  const project = useSelectedProject()
  if (project && project.host !== null) return <RemoteBackendCard project={project} />
  return <LocalBackendCard project={project} />
}

/** The Backend card of a project on a remote host: the host's connection, its model and the project's approval mode. */
function RemoteBackendCard({ project }: { project: ProjectState }) {
  const host = project.host
  const connection = useApp(() => (host ? remoteConnection(host.id) : null))
  useApp(() => connection?.version ?? -1)
  const modelName = connection?.result?.model.name ?? ''
  const modelLabel = useApp((state) => (modelName.length > 0 ? state.modelDisplayName(modelName) : ''))
  const [overChip, setOverChip] = useState(false)
  const kind = connection?.state.kind ?? 'disconnected'
  const label = connection ? connection.label.replace(' · mock', '') : 'Not connected'
  const chipHover = {
    onPointerEnter: () => setOverChip(true),
    onPointerLeave: () => setOverChip(false),
  }
  if (!host) return null
  return (
    <div className={styles.backendCard}>
      <Tooltip content={`${hostTarget(host)} · Open Settings → Remote hosts`} disabled={overChip}>
        <button
          type="button"
          className={styles.backendSummary}
          aria-label={`Host ${project.hostLabel ?? ''}: ${label}. Open Settings`}
          onClick={() => useApp.getState().openSettings(SETTINGS_TAB_REMOTE_HOSTS)}
        >
          <span className={styles.backendStatusLine}>
            <StatusDot kind={kind === 'ready' ? 'ready' : kind === 'connecting' ? 'connecting' : 'failed'} />
            <span className={styles.backendLabel}>{label}</span>
            <span className={styles.flexSpacer} />
            <Icon name="remoteHost" size={11} strokeWidth={1.75} className={styles.tertiaryIcon} />
          </span>
          <span className={styles.backendModelLine}>
            <MiddleText text={connection?.hostSummary ?? hostTarget(host)} className={styles.backendModel} />
            <span className={styles.chipHover} {...chipHover}>
              <Tooltip content="The project lives on this SSH host">
                <Chip icon="remoteHost">{project.hostLabel ?? host.address}</Chip>
              </Tooltip>
            </span>
            {connection?.result?.mock ? (
              <span className={styles.chipHover} {...chipHover}>
                <Tooltip content="Mock backend on the host: no API key, scripted replies">
                  <Chip>mock</Chip>
                </Tooltip>
              </span>
            ) : null}
          </span>
          {modelName.length > 0 ? (
            <span className={styles.backendModelLine}>
              <MiddleText text={modelLabel} className={styles.backendModel} />
              <span className={styles.chipHover} {...chipHover}>
                <Tooltip content="The host's default model">
                  <Chip>default</Chip>
                </Tooltip>
              </span>
            </span>
          ) : null}
          <span className={styles.backendApprovalLine}>
            <Icon name="approval" size={10} strokeWidth={2} className={styles.tertiaryIcon} />
            <MiddleText text={`${approvalModeLabel(project.info.permission_profile)} · ${project.name}`} grow className={styles.backendApproval} />
          </span>
        </button>
      </Tooltip>
      {kind === 'failed' || kind === 'disconnected' ? (
        <div className={styles.remoteFailure}>
          {connection?.hint ? <p className={`${styles.error} ${styles.clamp4} selectable`}>{connection.hint}</p> : null}
          <Button size="small" variant="prominent" icon="refresh" onClick={() => void useApp.getState().reconnectHost(host)}>
            Reconnect
          </Button>
        </div>
      ) : null}
    </div>
  )
}

/** The Backend card of the local backend (and of no project). */
function LocalBackendCard({ project }: { project: ProjectState | null }) {
  const connection = useApp((state) => state.connection)
  const isMock = useApp((state) => state.config?.mock === true)
  const modelName = useApp((state) => state.config?.model.name ?? '')
  const modelLabel = useApp((state) => (modelName.length > 0 ? state.modelDisplayName(modelName) : ''))
  const connectionLabel = useConnectionLabel()
  // The chips carry their own help; the card's "Open Settings" stays out of their way.
  const [overChip, setOverChip] = useState(false)
  const failure = connectionFailure(connection)
  // "· mock" moves to a chip on the model line to keep the status line short.
  const label = isMock ? connectionLabel.replace(' · mock', '') : connectionLabel

  const chipHover = {
    onPointerEnter: () => setOverChip(true),
    onPointerLeave: () => setOverChip(false),
  }

  return (
    <div className={styles.backendCard}>
      <Tooltip content="Open Settings" disabled={overChip}>
        <button
          type="button"
          className={styles.backendSummary}
          aria-label={`Backend: ${connectionLabel}. Open Settings`}
          onClick={() => useApp.getState().openSettings()}
        >
          <span className={styles.backendStatusLine}>
            <StatusDot kind={connection.kind} />
            <span className={styles.backendLabel}>{label}</span>
            <span className={styles.flexSpacer} />
            <Icon name="settings" size={11} strokeWidth={1.75} className={styles.tertiaryIcon} />
          </span>
          {modelName.length > 0 ? (
            <span className={styles.backendModelLine}>
              <MiddleText text={modelLabel} className={styles.backendModel} />
              <span className={styles.chipHover} {...chipHover}>
                <Tooltip content="User-level default model">
                  <Chip>default</Chip>
                </Tooltip>
              </span>
              {isMock ? (
                <span className={styles.chipHover} {...chipHover}>
                  <Tooltip content="Mock backend: no API key, scripted replies">
                    <Chip>mock</Chip>
                  </Tooltip>
                </span>
              ) : null}
            </span>
          ) : null}
          {project ? (
            <span className={styles.backendApprovalLine}>
              <Icon name="approval" size={10} strokeWidth={2} className={styles.tertiaryIcon} />
              <MiddleText text={`${approvalModeLabel(project.info.permission_profile)} · ${project.name}`} grow className={styles.backendApproval} />
            </span>
          ) : null}
        </button>
      </Tooltip>
      {failure ? <BackendFailurePanel failure={failure} compact /> : null}
    </div>
  )
}

/** The connection's dot: a spinner while starting, green when ready, red when failed. */
function StatusDot({ kind }: { kind: 'connecting' | 'ready' | 'failed' }) {
  if (kind === 'connecting') return <BrailleSpinner size={10} tint="var(--text-secondary)" />
  return <span className={kind === 'ready' ? styles.dotReady : styles.dotFailed} aria-hidden="true" />
}
