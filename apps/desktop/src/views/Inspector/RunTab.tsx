import { useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { StatusPill } from '../../components/StatusPill'
import { Tooltip } from '../../components/Tooltip'
import { executionStateLabel, tokenUsageTotal, type ContextStats, type TaskOutcome } from '../../backend/types'
import { chatStatusIsRunning, chatStatusProgress } from '../../models/reducer'
import { TranscriptFormat } from '../../models/transcript'
import { approvalModeLabel, useApp, useSelectedChat, useSelectedProject } from '../../state/app'
import { ChatState } from '../../state/chat'
import { connectionFailure, useConnectionLabel } from '../../state/selectors'
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

/** `ContextStatsCard`: the window usage bar and the prompt's composition, or why there is none yet. */
function ContextStatsCard({ chat }: { chat: ChatState }) {
  const stats = chat.contextStats
  return (
    <InspectorCard title="Context">
      {stats ? (
        <>
          <UsageBar stats={stats} />
          <StatRow
            label="Prompt estimate"
            value={`${TranscriptFormat.tokens(stats.estimated_prompt_tokens)} / ${TranscriptFormat.tokens(stats.context_window)}`}
          />
          <StatRow label="Stable prefix" value={TranscriptFormat.tokens(stats.stable_prefix_tokens)} />
          <StatRow
            label="Repository"
            value={`${stats.repository_snippets} snippet${stats.repository_snippets === 1 ? '' : 's'} · ${TranscriptFormat.tokens(stats.repository_tokens)}`}
          />
          <StatRow
            label="Memory"
            value={`${stats.memory_records} record${stats.memory_records === 1 ? '' : 's'} · ${TranscriptFormat.tokens(stats.memory_tokens)}`}
          />
          <StatRow label="Conversation" value={`${stats.conversation_messages} msg · ${TranscriptFormat.tokens(stats.conversation_tokens)}`} />
          <StatRow label="Retrieval budget" value={TranscriptFormat.tokens(stats.retrieval_budget_tokens)} />
          {stats.compacted ? (
            <div className={styles.chipRow}>
              <Tooltip content="The conversation was compacted to fit the context window">
                <Chip icon="collapseAll" variant="tinted" tint="var(--warning)">
                  compacted
                </Chip>
              </Tooltip>
            </div>
          ) : null}
        </>
      ) : (
        <p className={styles.note}>
          {chatStatusIsRunning(chat.status)
            ? 'Waiting for the first context compilation…'
            : 'No context statistics yet. They appear once a run compiles its prompt.'}
        </p>
      )}
    </InspectorCard>
  )
}

/** The 6 px capsule: accent, `warning` past 70 %, `danger` past 90 %. */
function UsageBar({ stats }: { stats: ContextStats }) {
  const fraction = stats.context_window > 0 ? Math.min(1, stats.estimated_prompt_tokens / stats.context_window) : 0
  const tint = fraction > 0.9 ? 'var(--danger)' : fraction > 0.7 ? 'var(--warning)' : 'var(--accent)'
  return (
    <div className={styles.usageBar} role="img" aria-label={`Context window usage ${Math.trunc(fraction * 100)} percent`}>
      <div className={styles.usageFill} style={{ width: `max(4px, ${fraction * 100}%)`, background: tint }} />
    </div>
  )
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
 * `BackendFailurePanel`: its summary, Retry and Choose binary…).
 */
function BackendCard() {
  const connection = useApp((state) => state.connection)
  const isMock = useApp((state) => state.config?.mock === true)
  const modelName = useApp((state) => state.config?.model.name ?? '')
  const modelLabel = useApp((state) => (modelName.length > 0 ? state.modelDisplayName(modelName) : ''))
  const connectionLabel = useConnectionLabel()
  const project = useSelectedProject()
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
