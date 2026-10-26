// Port of apps/macos/ContextAgent/Views/Chat/AgentTimelineView.swift: the
// Ultra tier's sub-agents as one card per `spawn_agents` call, plus the
// agent visuals the Inspector's Agents tab shares (`AgentStatusIcon`,
// `AgentDetailLine`, `AgentElapsed`, `SteadyText` and the `AgentRun`
// presentation helpers).

import { memo, useEffect, useRef, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { Tooltip } from '../../components/Tooltip'
import { agentModeLabel, taskStatusLabel } from '../../backend/types'
import { rowLabel } from '../../models/toolMetadata'
import {
  agentGroupFailedCount,
  agentGroupHeadline,
  agentGroupIsOpen,
  agentStatusIsRunning,
  type AgentGroup,
  type AgentRun,
} from '../../models/transcript'
import { useApp } from '../../state/app'
import type { ChatState } from '../../state/chat'
import { selectedChat } from '../../state/selectors'
import { Icon, type IconName } from '../../theme/icons'
import { Fade, LiveDuration } from './ToolTimelineView'
import styles from './AgentTimelineView.module.css'

/** Groups that ended within this window start expanded (seconds → ms). */
const RECENT_WINDOW_MS = 120_000

function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(' ')
}

function startsExpanded(group: AgentGroup): boolean {
  if (agentGroupIsOpen(group) || agentGroupFailedCount(group) > 0) return true
  if (group.endedAt === null) return true
  return Date.now() - group.endedAt.getTime() < RECENT_WINDOW_MS
}

export interface AgentTimelineViewProps {
  group: AgentGroup
  chat: ChatState
}

/**
 * `AgentTimelineView` of the macOS app: a header "3 agents, 2 running ⌄"
 * with the elapsed time at the right, then one row per agent (24 px status
 * square with a spinner while it runs, the agent's name, a mode chip, its
 * current step or verdict and its time) joined by the tool timeline's
 * connector. Clicking a row shows the agent in the Inspector Agents tab
 * (`showAgent`); the inspected row stays highlighted.
 */
export function AgentTimelineView({ group, chat }: AgentTimelineViewProps) {
  const [isExpanded, setExpanded] = useState(() => startsExpanded(group))
  const headline = agentGroupHeadline(group)
  const isOpen = agentGroupIsOpen(group)
  // The agent the inspector currently shows, when it is on screen. Only the
  // id is read: a committed group must not redraw on every event of the
  // agents still running.
  const inspectedAgentId = useApp((state) => {
    if (!state.inspectorVisible || state.inspectorTab !== 'agents') return null
    return selectedChat(state)?.shownAgentId ?? null
  })

  return (
    <section className={styles.timeline} aria-label={`Agents, ${headline}`}>
      <button
        type="button"
        className={cx(styles.header, !isExpanded && styles.collapsed)}
        aria-expanded={isExpanded}
        aria-label={`${headline}, ${isExpanded ? 'Expanded' : 'Collapsed'}. Shows or hides the agents`}
        onClick={() => setExpanded((value) => !value)}
      >
        <span className={styles.headerIcon} style={{ color: isOpen ? 'var(--accent)' : 'var(--text-primary)' }} aria-hidden="true">
          <Icon name="agents" size={14} strokeWidth={2} />
        </span>
        <span className={styles.headline}>{headline}</span>
        <Icon name="chevronDown" size={13.5} strokeWidth={1.75} className={styles.chevron} />
        <span className={styles.spacer} />
        <span className={styles.elapsed} aria-label="Elapsed">
          <LiveDuration startedAt={group.startedAt} endedAt={group.endedAt} running={isOpen} />
        </span>
      </button>
      <Fade show={isExpanded} className={styles.underHeader}>
        <div className={styles.rows}>
          <div className={styles.connector} aria-hidden="true" />
          {group.agents.map((agent) => (
            <AgentRow key={agent.id + ':' + agent.agentTaskId} agent={agent} isSelected={agent.id === inspectedAgentId} chat={chat} />
          ))}
        </div>
      </Fade>
    </section>
  )
}

// MARK: - Agent row

const AgentRow = memo(function AgentRow({ agent, isSelected, chat }: { agent: AgentRun; isSelected: boolean; chat: ChatState }) {
  const detail = agentDetailLine(agent)
  return (
    <Tooltip content={agent.objective}>
      <button
        type="button"
        className={cx(styles.row, isSelected && styles.selected)}
        aria-label={`Agent ${agent.name}, ${agent.mode}, ${detail}. Shows the agent in the inspector`}
        aria-pressed={isSelected}
        onClick={() => useApp.getState().showAgent(agent.id, chat)}
      >
        <AgentStatusIcon agent={agent} />
        <span className={styles.labels}>
          <span className={styles.nameLine}>
            <span className={styles.name}>{agent.name}</span>
            <Chip icon={agentModeIcon(agent)}>{agentModeLabel(agent.mode)}</Chip>
          </span>
          <AgentDetailLine agent={agent} />
        </span>
        <span className={styles.spacer} />
        <AgentElapsed agent={agent} />
        <Icon name="chevronRight" size={12.5} strokeWidth={1.75} className={styles.disclosure} />
      </button>
    </Tooltip>
  )
})

// MARK: - Shared agent visuals

export interface AgentStatusIconProps {
  agent: AgentRun
  /** Square size in px; 24 in the transcript, 28 in the Inspector's agent header. */
  size?: number
}

/** `AgentStatusIcon`: the status square shown next to an agent: a spinner while it runs, else its verdict icon. */
export function AgentStatusIcon({ agent, size = 24 }: AgentStatusIconProps) {
  const running = agentStatusIsRunning(agent.status)
  return (
    <span
      className={styles.statusIcon}
      style={{ width: size, height: size, borderRadius: size / 4, background: agentStatusBackground(agent) }}
      aria-hidden="true"
    >
      {running ? (
        <BrailleSpinner size={size * 0.46} tint={agentWaitingForApproval(agent) ? 'var(--warning)' : 'var(--accent)'} />
      ) : (
        <Icon name={agentStatusIcon(agent)} size={Math.round(size * 0.58)} strokeWidth={2} style={{ color: agentStatusTint(agent) }} />
      )}
    </span>
  )
}

/**
 * `AgentDetailLine`: "Reading src/auth.rs · 4 tool calls": what the agent
 * is doing, then its tool count. A running agent changes activity many
 * times a second, so the activity is a `SteadyText` that holds each value
 * briefly and cross-fades, and the count ticks on its own.
 */
export function AgentDetailLine({ agent, className }: { agent: AgentRun; className?: string }) {
  const edited = agentEditedLabel(agent)
  return (
    <span className={cx(styles.detailLine, className)} style={{ color: agentDetailTint(agent) }}>
      {/* A verdict lands at once; each agent gets its own text state. */}
      <SteadyText key={agent.id} text={agentActivityLabel(agent)} minimumDwell={agentStatusIsRunning(agent.status) ? 0.8 : 0} className={styles.activity} />
      <span className={styles.fixed}>
        {' · '}
        <NumericText value={agent.calls.length} />
        {` tool call${agent.calls.length === 1 ? '' : 's'}`}
      </span>
      {edited !== null ? <span className={styles.fixed}>{` · ${edited}`}</span> : null}
    </span>
  )
}

export interface SteadyTextProps {
  text: string
  /** Seconds each value stays on screen at least; 0.8 by default. */
  minimumDwell?: number
  className?: string
}

/** `.contentTransition(.opacity)` under `.easeInOut(duration: 0.25)`. */
const CROSS_FADE_MS = 250

/**
 * `SteadyText`: text that holds each value on screen for at least
 * `minimumDwell` and cross-fades to the next (the old value fades out over
 * the new one fading in). Values that come and go within the dwell are
 * skipped; the latest one always lands.
 */
export function SteadyText({ text, minimumDwell = 0.8, className }: SteadyTextProps) {
  const [shown, setShown] = useState({ text, previous: null as string | null, serial: 0 })
  const shownAt = useRef(0)

  useEffect(() => {
    if (text === shown.text) return
    const show = () => {
      const animate = !window.matchMedia('(prefers-reduced-motion: reduce)').matches
      setShown((current) => ({ text, previous: animate ? current.text : null, serial: current.serial + 1 }))
      shownAt.current = Date.now()
    }
    const wait = minimumDwell * 1000 - (Date.now() - shownAt.current)
    if (wait <= 0) {
      show()
      return
    }
    const timer = window.setTimeout(show, wait)
    return () => window.clearTimeout(timer)
  }, [text, shown.text, minimumDwell])

  // The outgoing value leaves once the cross-fade is over.
  useEffect(() => {
    if (shown.previous === null) return
    const timer = window.setTimeout(() => setShown((current) => (current.serial === shown.serial ? { ...current, previous: null } : current)), CROSS_FADE_MS)
    return () => window.clearTimeout(timer)
  }, [shown.previous, shown.serial])

  return (
    <span className={cx(styles.steady, className)}>
      {shown.previous !== null ? (
        <span key={`out-${shown.serial}`} className={cx(styles.steadyValue, styles.crossFadeOut)} aria-hidden="true">
          {shown.previous}
        </span>
      ) : null}
      <span key={`in-${shown.serial}`} className={cx(styles.steadyValue, shown.previous !== null && styles.crossFade)}>
        {shown.text}
      </span>
    </span>
  )
}

/**
 * A count with `.contentTransition(.numericText())` under
 * `.easeOut(duration: 0.2)`: a new value rolls in from below (an increase)
 * or above (a decrease) instead of replacing the old one in place.
 */
function NumericText({ value }: { value: number }) {
  const [state, setState] = useState({ value, direction: 0 })
  if (state.value !== value) setState({ value, direction: value > state.value ? 1 : -1 })
  return (
    <span
      key={state.value}
      className={cx(styles.numeric, state.direction > 0 && styles.rollUp, state.direction < 0 && styles.rollDown)}
    >
      {state.value}
    </span>
  )
}

/** `AgentElapsed`: the agent's running clock, or its final duration. */
export function AgentElapsed({ agent, className }: { agent: AgentRun; className?: string }) {
  return (
    <span className={cx(styles.agentElapsed, className)}>
      <LiveDuration startedAt={agent.startedAt} endedAt={agent.endedAt} running={agentStatusIsRunning(agent.status)} />
    </span>
  )
}

// MARK: - AgentRun presentation (`extension AgentRun`)

/** `AgentRun.modeIcon`. */
export function agentModeIcon(agent: AgentRun): IconName {
  return agent.mode === 'explore' ? 'agentExplore' : 'agentImplement'
}

/** `AgentRun.waitingForApproval`. */
export function agentWaitingForApproval(agent: AgentRun): boolean {
  return agentStatusIsRunning(agent.status) && agent.calls.some((call) => call.status === 'running' && call.approvalRequestId !== null)
}

/** `AgentRun.activityLabel`: the running agent's current step, or its verdict. */
export function agentActivityLabel(agent: AgentRun): string {
  switch (agent.status.kind) {
    case 'running': {
      if (agentWaitingForApproval(agent)) return 'Waiting for approval'
      for (let index = agent.calls.length - 1; index >= 0; index -= 1) {
        const call = agent.calls[index]
        if (call && call.status === 'running') return rowLabel(call)
      }
      return agent.liveText.length === 0 ? 'Thinking' : 'Writing'
    }
    case 'finished':
      return taskStatusLabel(agent.status.status)
    case 'stopped':
      return 'Stopped'
  }
}

/** `AgentRun.toolCountLabel`: "4 tool calls". */
export function agentToolCountLabel(agent: AgentRun): string {
  return `${agent.calls.length} tool call${agent.calls.length === 1 ? '' : 's'}`
}

/** `AgentRun.editedLabel`: "edited 2 files" once a finished agent reports edits. */
export function agentEditedLabel(agent: AgentRun): string | null {
  if (agent.status.kind !== 'finished' || !agent.report || agent.report.modified_files.length === 0) return null
  const count = agent.report.modified_files.length
  return `edited ${count} file${count === 1 ? '' : 's'}`
}

/** `AgentRun.detailLine`: activity, tool count and edits joined with " · ". */
export function agentDetailLine(agent: AgentRun): string {
  const edited = agentEditedLabel(agent)
  return [agentActivityLabel(agent), agentToolCountLabel(agent), ...(edited !== null ? [edited] : [])].join(' · ')
}

/** `AgentRun.detailTint` (a CSS colour). */
export function agentDetailTint(agent: AgentRun): string {
  switch (agent.status.kind) {
    case 'running':
      return agentWaitingForApproval(agent) ? 'var(--warning)' : 'var(--text-secondary)'
    case 'finished':
      switch (agent.status.status) {
        case 'completed':
          return 'var(--text-secondary)'
        case 'blocked':
          return 'var(--warning)'
        case 'failed':
          return 'var(--danger)'
        case 'cancelled':
          return 'var(--text-secondary)'
      }
      break
    case 'stopped':
      return 'var(--text-secondary)'
  }
  return 'var(--text-secondary)'
}

/** `AgentRun.statusIcon`. */
export function agentStatusIcon(agent: AgentRun): IconName {
  switch (agent.status.kind) {
    case 'running':
      return 'agents'
    case 'finished':
      switch (agent.status.status) {
        case 'completed':
          return 'check'
        case 'blocked':
          return 'approval'
        case 'failed':
          return 'failed'
        case 'cancelled':
          return 'stopped'
      }
      break
    case 'stopped':
      return 'stopped'
  }
  return 'stopped'
}

/** `AgentRun.statusTint` (a CSS colour). */
export function agentStatusTint(agent: AgentRun): string {
  switch (agent.status.kind) {
    case 'running':
      return 'var(--accent)'
    case 'finished':
      switch (agent.status.status) {
        case 'completed':
          return 'var(--success)'
        case 'blocked':
          return 'var(--warning)'
        case 'failed':
          return 'var(--danger)'
        case 'cancelled':
          return 'var(--text-secondary)'
      }
      break
    case 'stopped':
      return 'var(--text-secondary)'
  }
  return 'var(--text-secondary)'
}

/** `AgentRun.statusBackground` (a CSS colour). */
export function agentStatusBackground(agent: AgentRun): string {
  if (agent.status.kind === 'running') return 'var(--accent-soft)'
  if (agent.status.kind === 'finished' && agent.status.status === 'failed') return 'color-mix(in srgb, var(--danger) 12%, transparent)'
  return 'var(--surface-raised)'
}
