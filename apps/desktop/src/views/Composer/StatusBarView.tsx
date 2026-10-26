// Port of apps/macos/ContextAgent/Views/Composer/StatusBarView.swift: the
// one-line status bar above the composer (apps/macos/ARCHITECTURE.md
// "Status bar").

import { useEffect, useState } from 'react'
import { executionStateLabel, tokenUsageTotal } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { Tooltip } from '../../components/Tooltip'
import { chatStatusProgress } from '../../models/reducer'
import { TranscriptFormat } from '../../models/transcript'
import { useModelsDevVersion } from '../../models/modelsDev'
import { approvalModeLabel, selectedProject, useApp, useChatValue, useProjectValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import type { ProjectState } from '../../state/project'
import { projectById } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { ModelLogo } from './ModelHoverCard'
import styles from './StatusBarView.module.css'

export interface StatusBarViewProps {
  /** The chat whose run, tokens and context it shows; null shows the selected project only. */
  chat: ChatState | null
}

/// `StatusBarView` (StatusBarView.swift): running → spinner + state + step
/// + elapsed (the composer's stop button cancels); always "ctx 12.4k/128k"
/// (click → Inspector Run tab, "compacted" badge), "42.1k tok", "$0.12"
/// with pricing, "index gen 12" / "indexing 120/310". Idle it shows model ·
/// approval mode instead of the run segment. 22 px tall.
export function StatusBarView({ chat }: StatusBarViewProps) {
  const project = useApp((state) => projectById(state, chat?.projectId ?? '') ?? selectedProject(state))
  return (
    <div className={styles.bar} role="group" aria-label="Status">
      <Leading chat={chat} project={project} />
      <span className={styles.spacer} />
      <Trailing chat={chat} project={project} />
    </div>
  )
}

// MARK: - Left: run state or model · mode

function Dot() {
  return (
    <span className={styles.dot} aria-hidden="true">
      ·
    </span>
  )
}

function Leading({ chat, project }: { chat: ChatState | null; project: ProjectState | null }) {
  const status = useChatValue(chat, (c) => c.status, null)
  const cancelRequested = useChatValue(chat, (c) => c.cancelRequested, false)
  const info = useProjectValue(project, (p) => p.info, null)
  useModelsDevVersion()
  useApp((state) => state.config?.model.base_url ?? '')

  const progress = status ? chatStatusProgress(status) : null
  if (chat && progress) {
    return (
      <span className={styles.group}>
        <BrailleSpinner size={11} tint="var(--text-secondary)" />
        <span className={styles.primary}>{executionStateLabel(progress.state)}</span>
        {progress.step > 0 ? (
          <>
            <Dot />
            <span>step {progress.step}</span>
          </>
        ) : null}
        <Dot />
        <ElapsedText since={progress.startedAt} />
        {cancelRequested ? (
          <>
            <Dot />
            <span className={styles.tertiary}>Cancelling…</span>
          </>
        ) : null}
      </span>
    )
  }
  if (chat && status?.kind === 'loadingHistory') {
    return (
      <span className={styles.group}>
        <BrailleSpinner size={11} tint="var(--text-secondary)" />
        <span>Loading history…</span>
      </span>
    )
  }
  if (info) {
    return (
      <Tooltip content="Project model · approval mode">
        <span className={styles.group}>
          <ModelLogo modelId={info.model} size={11} tint="var(--text-tertiary)" />
          <span className={styles.primary}>{useApp.getState().modelDisplayName(info.model)}</span>
          <Dot />
          <span>{approvalModeLabel(info.permission_profile)}</span>
        </span>
      </Tooltip>
    )
  }
  return null
}

/** "6m 12s", updated every second. */
function ElapsedText({ since }: { since: Date }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [since])
  return (
    <span className={styles.digits} aria-label="Elapsed">
      {TranscriptFormat.duration(Math.max(0, (now - since.getTime()) / 1000))}
    </span>
  )
}

// MARK: - Right: context, tokens, cost, index

function Trailing({ chat, project }: { chat: ChatState | null; project: ProjectState | null }) {
  const stats = useChatValue(chat, (c) => c.contextStats, null)
  const usage = useChatValue(chat, (c) => c.usage, null)
  const cost = useChatValue(chat, (c) => c.costUsd, null)
  const pricing = useApp((state) => state.config?.pricing != null)

  const openInspector = () => {
    const app = useApp.getState()
    app.setInspectorVisible(true)
    app.setInspectorTab('run')
  }

  return (
    <span className={styles.trailing}>
      {stats ? (
        <Tooltip content="Context window usage · click to open the Inspector">
          <button
            type="button"
            className={styles.contextButton}
            onClick={openInspector}
            aria-label={`Context ${TranscriptFormat.tokens(stats.estimated_prompt_tokens)} of ${TranscriptFormat.tokens(stats.context_window)} tokens${stats.compacted ? ', compacted' : ''}`}
          >
            <span className={styles.digits}>
              ctx {TranscriptFormat.tokens(stats.estimated_prompt_tokens)}/{TranscriptFormat.tokens(stats.context_window)}
            </span>
            {stats.compacted ? (
              <Chip variant="tinted" tint="var(--warning)">
                compacted
              </Chip>
            ) : null}
          </button>
        </Tooltip>
      ) : null}

      {usage ? (
        <>
          <Tooltip content={pricing ? 'Tokens used in this chat' : 'Tokens used in this chat · set pricing in Settings → Pricing to see costs'}>
            <span className={styles.digits}>{TranscriptFormat.tokens(tokenUsageTotal(usage))} tok</span>
          </Tooltip>
          {pricing && cost !== null ? (
            <Tooltip content="Estimated cost from Settings → Pricing">
              <span className={styles.digits}>{TranscriptFormat.cost(cost)}</span>
            </Tooltip>
          ) : null}
        </>
      ) : null}

      {project ? <IndexSegment project={project} /> : null}
    </span>
  )
}

function IndexSegment({ project }: { project: ProjectState }) {
  const progress = useProjectValue(project, (p) => p.indexProgress, null)
  const ready = useProjectValue(project, (p) => p.lastIndexReady, null)
  const info = useProjectValue(project, (p) => p.info, project.info)

  if (progress && progress.total > 0) {
    return (
      <Tooltip content="Indexing the repository">
        <span className={styles.indexGroup}>
          <BrailleSpinner size={10} tint="var(--text-tertiary)" />
          <span className={styles.digits}>
            indexing {progress.done}/{progress.total}
          </span>
        </span>
      </Tooltip>
    )
  }
  if (info.indexing && ready === null) {
    return (
      <Tooltip content="Indexing the repository">
        <span className={styles.indexGroup}>
          <BrailleSpinner size={10} tint="var(--text-tertiary)" />
          <span>indexing</span>
        </span>
      </Tooltip>
    )
  }
  const error = ready?.error
  if (error != null) {
    return (
      <Tooltip content={`Indexing failed: ${error} · retrieval falls back to text tools`}>
        <span className={styles.indexFailed}>
          <Icon name="warning" size={10} strokeWidth={2} />
          <span>index failed</span>
        </span>
      </Tooltip>
    )
  }
  return (
    <Tooltip content={`Index ready · ${info.index.files} files · generation ${info.index.generation}`}>
      <span className={styles.digits}>index gen {info.index.generation}</span>
    </Tooltip>
  )
}
