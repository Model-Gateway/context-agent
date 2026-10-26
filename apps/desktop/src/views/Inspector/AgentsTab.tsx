import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Chip } from '../../components/Chip'
import { DisclosureRow } from '../../components/DisclosureRow'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { Skeleton } from '../../components/Skeleton'
import { TailWindow } from '../../components/TailWindow'
import { Tooltip } from '../../components/Tooltip'
import { agentModeLabel, tokenUsageTotal } from '../../backend/types'
import { MarkdownView } from '../../markdown/MarkdownView'
import { TranscriptFormat, agentRunToolGroup, agentStatusIsRunning, type AgentRun, type ToolCallItem, type ToolGroup, type ToolGroupTerminal } from '../../models/transcript'
import { useSelectedChat } from '../../state/app'
import { Icon } from '../../theme/icons'
import {
  AgentDetailLine,
  AgentElapsed,
  AgentStatusIcon,
  agentDetailLine,
  agentModeIcon,
  agentStatusIcon,
  agentStatusTint,
  agentWaitingForApproval,
} from '../Chat/AgentTimelineView'
import { ToolTimelineView } from '../Chat/ToolTimelineView'
import { MiddleText } from './MiddleText'
import { cx, revealInStrip } from './inspectorShared'
import styles from './AgentsTab.module.css'

/**
 * Port of `AgentsTab` (Views/Inspector/AgentsTab.swift): a horizontally
 * scrolling strip with one tab per sub-agent of the selected chat, over the
 * selected agent's view — its header, then (scrolling, opened at the
 * bottom) its brief, tool timeline, the line it is writing while it runs
 * and its report. With no agent picked it follows the latest running, else
 * latest, agent; clicking an agent row in the transcript selects it here
 * (`showAgent`).
 */
export function AgentsTab() {
  const chat = useSelectedChat()
  if (!chat) return <EmptyPlaceholder icon="agents" title="No chat selected" subtitle="Sub-agents of the selected chat show up here." />
  const agents = chat.allAgents
  if (agents.length === 0) {
    return <EmptyPlaceholder icon="agents" title="No agents" subtitle="Sub-agents deployed by the Ultra effort tier show up here, one tab each." />
  }
  const inspected = chat.inspectedAgent
  return (
    <div className={styles.tab}>
      <AgentTabStrip agents={agents} selectedId={inspected?.id ?? null} onSelect={(id) => chat.setInspectedAgent(id)} />
      {inspected ? <AgentPane key={inspected.id} agent={inspected} /> : null}
    </div>
  )
}

// MARK: - Tab strip

/** `AgentTabStrip`: 36 px, chips 2 px apart, the selection scrolled into view. */
function AgentTabStrip({ agents, selectedId, onSelect }: { agents: AgentRun[]; selectedId: string | null; onSelect: (id: string) => void }) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const appeared = useRef(false)

  useEffect(() => {
    const strip = scrollRef.current
    if (selectedId === null || !strip) return
    const chip = strip.querySelector<HTMLElement>(`[data-agent-id="${CSS.escape(selectedId)}"]`)
    // On appear the strip jumps to the selection; later changes scroll there.
    if (chip) revealInStrip(strip, chip, appeared.current ? 'smooth' : 'auto')
    appeared.current = true
  }, [selectedId])

  return (
    <div className={styles.strip}>
      <div ref={scrollRef} className={styles.stripScroll} role="tablist" aria-label="Agent tabs">
        {agents.map((agent) => (
          <AgentTabChip key={agent.id} agent={agent} selected={agent.id === selectedId} onSelect={onSelect} />
        ))}
      </div>
    </div>
  )
}

/** `AgentTabChip`: spinner or verdict icon, then the agent's name; the objective as help. */
const AgentTabChip = memo(function AgentTabChip({ agent, selected, onSelect }: { agent: AgentRun; selected: boolean; onSelect: (id: string) => void }) {
  const running = agentStatusIsRunning(agent.status)
  return (
    <Tooltip content={agent.objective}>
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        aria-label={`Agent ${agent.name}, ${agentDetailLine(agent)}`}
        data-agent-id={agent.id}
        className={cx(styles.chip, selected && styles.selected)}
        onClick={() => onSelect(agent.id)}
      >
        {running ? (
          <BrailleSpinner size={10} tint={agentWaitingForApproval(agent) ? 'var(--warning)' : 'var(--accent)'} />
        ) : (
          <Icon name={agentStatusIcon(agent)} size={10} strokeWidth={2} style={{ color: agentStatusTint(agent), flex: '0 0 auto' }} />
        )}
        <MiddleText text={agent.name} maxWidth={140} className={styles.chipName} />
      </button>
    </Tooltip>
  )
})

// MARK: - Agent view

/** Label widths of the tool-call skeleton rows standing for unloaded calls. */
const SKELETON_WIDTHS = [0.55, 0.4, 0.7, 0.48, 0.62, 0.35]

/**
 * `AgentPane`: a pinned header with the agent's status over a scroll view
 * that opens at the bottom — its brief, tool timeline, the line it is
 * writing while it runs, and its report once finished. Only the newest
 * tool calls are loaded (`TailWindow`, pages of 30, at most 90); the others
 * stand as empty space at their measured height, with tool-call skeletons
 * next to the loaded rows.
 */
function AgentPane({ agent }: { agent: AgentRun }) {
  const [briefExpanded, setBriefExpanded] = useState(true)
  const [atNewest, setAtNewest] = useState(true)
  const group = useMemo(() => agentRunToolGroup(agent), [agent])
  const lastIndex = agent.calls.length - 1
  // The ending row (stopped, failed, blocked) shows under the newest call: the connector stops at its top.
  const endsWithTerminal = atNewest && group.terminal !== null && group.terminal !== 'done'

  return (
    <div className={styles.pane}>
      <AgentHeader agent={agent} />
      <TailWindow
        items={agent.calls}
        itemKey={(call) => call.id}
        renderItem={(call, index) => <AgentCallRow call={call} groupId={group.id} terminal={index === lastIndex ? group.terminal : null} />}
        page={30}
        limit={90}
        rowGap={0}
        placeholderRowHeight={36}
        renderPlaceholder={(index) => <Skeleton.ToolCall label={SKELETON_WIDTHS[index % SKELETON_WIDTHS.length] ?? 0.5} />}
        header={
          <div className={styles.brief}>
            <DisclosureRow expanded={briefExpanded} onToggle={setBriefExpanded} label={<span className={styles.briefLabel}>Brief</span>}>
              <p className={`${styles.objective} selectable`}>{agent.objective}</p>
            </DisclosureRow>
          </div>
        }
        footer={atNewest ? <AgentNewest agent={agent} /> : null}
        onFollowsTailChange={setAtNewest}
        className={styles.timeline}
        contentClassName={styles.timelineContent}
        rowsClassName={cx(styles.timelineRows, endsWithTerminal && styles.timelineRowsToEnd)}
        aria-label={`${agent.name} timeline`}
      />
    </div>
  )
}

/// One loaded call as a one-call slice of the agent's timeline (no header);
/// the group's ending row goes with the newest call. The rows' shared
/// connector is drawn behind them by the rows container.
const AgentCallRow = memo(function AgentCallRow({ call, groupId, terminal }: { call: ToolCallItem; groupId: string; terminal: ToolGroupTerminal | null }) {
  const slice = useMemo<ToolGroup>(
    () => ({
      kind: 'tools',
      id: `${groupId}.${call.id}`,
      calls: [call],
      filesChanged: new Set(),
      commandsRun: 0,
      startedAt: call.startedAt,
      endedAt: null,
      terminal,
    }),
    [call, groupId, terminal],
  )
  return <ToolTimelineView group={slice} showsHeader={false} />
})

/** What follows the newest call: the line being written, the report. */
function AgentNewest({ agent }: { agent: AgentRun }) {
  const report = agent.report
  const writing = agentStatusIsRunning(agent.status) && agent.liveText.length > 0
  if (!writing && !report) return null
  return (
    <div className={styles.newest}>
      {writing ? (
        <div className={styles.liveLine}>
          <BrailleSpinner size={11} tint="var(--text-tertiary)" style={{ marginTop: 3 }} />
          <p className={styles.liveText}>{agent.liveText}</p>
        </div>
      ) : null}
      {report ? (
        <>
          {report.summary.length > 0 ? <MarkdownView markdown={report.summary} /> : null}
          {report.unresolved.length > 0 ? (
            <ul className={styles.unresolved}>
              {report.unresolved.map((line, index) => (
                <li key={index} className={styles.unresolvedLine}>
                  <span aria-hidden="true">•</span>
                  <span className="selectable">{line}</span>
                </li>
              ))}
            </ul>
          ) : null}
          <p className={styles.usage}>
            {`${report.steps} step${report.steps === 1 ? '' : 's'} · ${report.tool_calls} tool call${report.tool_calls === 1 ? '' : 's'} · ${TranscriptFormat.tokens(tokenUsageTotal(report.usage))} tokens · ${TranscriptFormat.durationMs(report.duration_ms)}`}
          </p>
        </>
      ) : null}
    </div>
  )
}

/** The pane's header: status square, name and mode chip over the detail line, the clock. */
function AgentHeader({ agent }: { agent: AgentRun }) {
  return (
    <div className={styles.header} role="heading" aria-level={3} aria-label={`${agent.name}, ${agentModeLabel(agent.mode)}, ${agentDetailLine(agent)}`}>
      <AgentStatusIcon agent={agent} size={28} />
      <div className={styles.headerText}>
        <div className={styles.headerTitle}>
          <MiddleText text={agent.name} className={styles.agentName} />
          <Chip icon={agentModeIcon(agent)}>{agentModeLabel(agent.mode)}</Chip>
        </div>
        <AgentDetailLine agent={agent} />
      </div>
      <span className={styles.headerSpacer} />
      <AgentElapsed agent={agent} />
    </div>
  )
}
