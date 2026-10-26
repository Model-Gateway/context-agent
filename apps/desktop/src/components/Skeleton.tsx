import { useState, type CSSProperties, type ReactNode } from 'react'
import { useReducedMotion } from './BrailleSpinner'
import styles from './Skeleton.module.css'

/*
 * Skeleton.swift: placeholders for content that is still loading (the
 * chat list, a project reopening at launch, a transcript's history, a
 * settings page): grey bars in the shape of what will replace them,
 * pulsing together. Small inline waits keep using `BrailleSpinner`.
 *
 * One exported binding: `Skeleton` is the pulse wrapper (`.skeletonPulse`)
 * and carries the shapes as members (`Skeleton.Bar`, `Skeleton.Transcript`,
 * `Skeleton.ToolCall`, `Skeleton.ChatLoading`, `Skeleton.EmptyChat`,
 * `Skeleton.SettingsRows`, `Skeleton.SettingsPage`).
 */

/** The pulse period (ms): every skeleton on screen pulses in step. */
export const SKELETON_PERIOD = 1400

export interface SkeletonPulseProps {
  /** What VoiceOver reads instead of the bars; "Loading" by default. */
  label?: string
  /** Inactive bars hold still (a skeleton scrolled out of view). */
  active?: boolean
  children: ReactNode
  className?: string
  style?: CSSProperties
}

/**
 * `.skeletonPulse(label, active)`: fades its content between 45 % and
 * 100 % opacity on a 1.4 s cycle taken from the absolute time, so every
 * skeleton on screen pulses in step however late it appeared. Static with
 * Reduce Motion or while not `active`. Hidden from assistive technology
 * behind `label`.
 */
function SkeletonPulse({ label = 'Loading', active = true, children, className, style }: SkeletonPulseProps) {
  const reduced = useReducedMotion()
  // The phase at mount, so the CSS animation starts where every other one is.
  const [delay] = useState(() => -(Date.now() % SKELETON_PERIOD))
  const pulsing = active && !reduced
  return (
    <div
      role="status"
      aria-label={label}
      aria-busy="true"
      className={[styles.pulse, pulsing ? styles.animated : styles.still, className ?? ''].filter(Boolean).join(' ')}
      style={pulsing ? { ...style, animationDelay: `${delay}ms` } : style}
    >
      <div aria-hidden="true" className={styles.inner}>
        {children}
      </div>
    </div>
  )
}

export interface SkeletonBarProps {
  /** Pixel width, a CSS width ("42%"), or nothing for the full width. */
  width?: number | string
  /** 10 by default. */
  height?: number
  /** 4 by default. */
  radius?: number
  className?: string
  style?: CSSProperties
}

/** `SkeletonBar`: one rounded `border`-coloured placeholder bar. */
function Bar({ width, height = 10, radius = 4, className, style }: SkeletonBarProps) {
  return (
    <div
      className={[styles.bar, width === undefined ? styles.fill : '', className ?? ''].filter(Boolean).join(' ')}
      style={{ width, height, borderRadius: radius, ...style }}
    />
  )
}

export interface SkeletonExchangeProps {
  /** The user bubble's width as a fraction of the column. */
  question: number
  /** The reply lines' widths as fractions of the column. */
  reply: number[]
  className?: string
}

/**
 * `TranscriptSkeleton.Exchange`: a user bubble at the right over a reply of
 * `reply.length` lines; 132 px with four lines.
 */
function Exchange({ question, reply, className }: SkeletonExchangeProps) {
  return (
    <div className={[styles.exchange, className ?? ''].filter(Boolean).join(' ')}>
      <div className={styles.bubbleRow}>
        <Bar width={`${question * 100}%`} height={38} radius={16} />
      </div>
      <div className={styles.replyLines}>
        {reply.map((fraction, index) => (
          <Bar key={index} width={`${fraction * 100}%`} height={11} />
        ))}
      </div>
    </div>
  )
}

/**
 * `TranscriptSkeleton`: a conversation's shape while its history loads: a
 * user bubble at the right, then an assistant reply of a few lines, twice.
 */
function Transcript({ className }: { className?: string }) {
  return (
    <SkeletonPulse label="Loading history" className={className}>
      <div className={styles.transcript}>
        <Exchange question={0.42} reply={[1, 0.94, 0.62]} />
        <Exchange question={0.3} reply={[0.97, 1, 0.88, 0.4]} />
      </div>
    </SkeletonPulse>
  )
}

export interface SkeletonToolCallProps {
  /** The label bar's width as a fraction of the remaining row. */
  label: number
  className?: string
}

/** `ToolCallSkeleton`: a tool-call row's shape, the 24 px icon square and a label bar; 36 px. */
function ToolCall({ label, className }: SkeletonToolCallProps) {
  return (
    <div className={[styles.toolCall, className ?? ''].filter(Boolean).join(' ')}>
      <Bar width={24} height={24} radius={6} />
      <div className={styles.toolLabel}>
        <Bar width={`${label * 100}%`} height={10} />
      </div>
    </div>
  )
}

/**
 * `ChatLoadingView`: the Chat tab while the chat it will show is still
 * being looked up: the transcript skeleton in the transcript's column.
 */
function ChatLoading({ className }: { className?: string }) {
  return (
    <div className={[styles.chatLoading, className ?? ''].filter(Boolean).join(' ')}>
      <div className={styles.chatColumn}>
        <Transcript />
      </div>
    </div>
  )
}

/**
 * `EmptyChatSkeleton`: the empty chat's shape (logo, title, composer, three
 * starter cards) while the project it belongs to is still opening.
 */
function EmptyChat({ className }: { className?: string }) {
  return (
    <div className={[styles.emptyChat, className ?? ''].filter(Boolean).join(' ')}>
      <SkeletonPulse label="Opening project" className={styles.emptyChatColumn}>
        <div className={styles.emptyChatStack}>
          <div className={styles.emptyChatHero}>
            <Bar width={30} height={30} radius={15} />
            <Bar width={240} height={20} radius={6} />
          </div>
          <Bar height={112} radius={16} />
          <div className={styles.starterRow}>
            <Bar height={64} radius={12} />
            <Bar height={64} radius={12} />
            <Bar height={64} radius={12} />
          </div>
        </div>
      </SkeletonPulse>
    </div>
  )
}

const VALUE_WIDTHS = [0.55, 0.35, 0.7, 0.45]
const LABEL_WIDTHS = [70, 96, 58, 84]

/** Label and value bars laid out like `SettingsRow`s (a 128 px label column). */
function SettingsRowBars({ count }: { count: number }) {
  return (
    <div className={styles.settingsRows}>
      {Array.from({ length: count }, (_, index) => (
        <div key={index} className={styles.settingsRow}>
          <div className={styles.settingsLabel}>
            <Bar width={LABEL_WIDTHS[index % LABEL_WIDTHS.length]} height={10} />
          </div>
          <div className={styles.settingsValue}>
            <Bar width={`${(VALUE_WIDTHS[index % VALUE_WIDTHS.length] ?? 0.5) * 100}%`} height={10} />
          </div>
        </div>
      ))}
    </div>
  )
}

export interface SkeletonSettingsRowsProps {
  /** 2 by default. */
  count?: number
  /** "Loading" by default. */
  label?: string
  className?: string
}

/** `SettingsRowsSkeleton`: rows inside a settings card whose content is still loading. */
function SettingsRows({ count = 2, label = 'Loading', className }: SkeletonSettingsRowsProps) {
  return (
    <SkeletonPulse label={label} className={className}>
      <SettingsRowBars count={count} />
    </SkeletonPulse>
  )
}

export interface SkeletonSettingsPageProps {
  /** One card per entry, with that many rows; [3, 2] by default. */
  rows?: number[]
  /** "Loading settings" by default. */
  label?: string
  className?: string
}

/**
 * `SettingsPageSkeleton`: a whole settings page while its data loads: one
 * card per entry of `rows`, each with a title, that many rows and a footer
 * line, like `SettingsGroup`.
 */
function SettingsPage({ rows = [3, 2], label = 'Loading settings', className }: SkeletonSettingsPageProps) {
  return (
    <SkeletonPulse label={label} className={className}>
      <div className={styles.settingsPage}>
        {rows.map((count, index) => (
          <div key={index} className={`card ${styles.settingsCard}`}>
            <Bar width={index % 2 === 0 ? 120 : 92} height={13} />
            <SettingsRowBars count={count} />
            <Bar width="80%" height={8} />
          </div>
        ))}
      </div>
    </SkeletonPulse>
  )
}

/**
 * The pulse wrapper with the shapes of Skeleton.swift as members:
 * `Skeleton.Bar`, `Skeleton.Exchange`, `Skeleton.Transcript`,
 * `Skeleton.ToolCall`, `Skeleton.ChatLoading`, `Skeleton.EmptyChat`,
 * `Skeleton.SettingsRows`, `Skeleton.SettingsPage`.
 */
export const Skeleton: typeof SkeletonPulse & {
  Bar: typeof Bar
  Exchange: typeof Exchange
  Transcript: typeof Transcript
  ToolCall: typeof ToolCall
  ChatLoading: typeof ChatLoading
  EmptyChat: typeof EmptyChat
  SettingsRows: typeof SettingsRows
  SettingsPage: typeof SettingsPage
} = Object.assign(SkeletonPulse, { Bar, Exchange, Transcript, ToolCall, ChatLoading, EmptyChat, SettingsRows, SettingsPage })
