// Port of apps/macos/ContextAgent/Views/Composer/ModelHoverCard.swift:
// `ModelHoverCard` (the card floated beside the model picker while a row is
// hovered), `AutoScrollingRow`, `ModelLogo` / `LabLogo`, and the web stand-in
// for `HoverCardPanel` (`FloatingCard`: a fixed, pointer-transparent layer
// placed beside the picker or under an anchor). `MiddleText` stands in for
// SwiftUI's `.truncationMode(.middle)` / `.head`, which CSS lacks.

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { modelStatCostLine, modelStatErrorRateLabel, modelStatSavingsLabel, type ModelStat } from '../../backend/types'
import { Chip } from '../../components/Chip'
import { MiddleTruncate, type TextRun } from '../../components/MiddleTruncate'
import { useReducedMotion } from '../../components/BrailleSpinner'
import { modelsDev, modelsDevCostLine, modelsDevTokens, useModelsDevVersion, type ModelsDevInfo } from '../../models/modelsDev'
import { useApp } from '../../state/app'
import { Icon } from '../../theme/icons'
import styles from './ModelHoverCard.module.css'

// MARK: - ModelHoverCard

/** The props of `ModelHoverCard` (its Swift stored properties; the catalogue is the app's one `modelsDev`). */
export interface ModelHoverCardProps {
  modelId: string
  /** Display name used when models.dev has no entry. */
  name: string
  info: ModelsDevInfo | null
  stat: ModelStat | null
}

/// `AppState.modelCard(_:)` (App/ProjectState.swift): the picker's hover
/// card for a model: models.dev's entry and/or the gateway's stats; null
/// with neither.
export function modelCard(id: string): ModelHoverCardProps | null {
  const app = useApp.getState()
  const info = app.modelInfo(id)
  const stat = app.modelStat(id)
  if (info === null && stat === null) return null
  return { modelId: id, name: app.modelDisplayName(id), info, stat }
}

/// `ModelHoverCard` (ModelHoverCard.swift): the lab's logo, models.dev's
/// name and one-line description, context / output limits and capability
/// chips, the price (what ModelGateway bills, else models.dev's list price),
/// then ModelGateway's 24 h error rate and savings. Either source
/// may be missing, not both. 280 px wide.
export function ModelHoverCard({ modelId, name, info, stat }: ModelHoverCardProps) {
  useModelsDevVersion()
  const savings = stat ? modelStatSavingsLabel(stat) : null
  const errorLabel = stat ? modelStatErrorRateLabel(stat) : null
  const cost = (stat ? modelStatCostLine(stat) : null) ?? (info ? modelsDevCostLine(info) : null)
  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <span className={styles.logoTile} aria-hidden="true">
          <LabLogo
            src={info ? modelsDev.logo(info.labId) : null}
            size={18}
            tint="var(--text-primary)"
            onError={info ? () => modelsDev.markLogoFailed(info.labId) : undefined}
          />
        </span>
        <div className={styles.headerText}>
          <div className={styles.name}>{info?.name ?? name}</div>
          <MiddleText className={styles.subtitle} text={info ? `${info.labName} · ${info.id}` : modelId} />
        </div>
      </div>
      {info ? (
        <>
          {info.description !== null ? <div className={styles.description}>{info.description}</div> : null}
          {info.contextWindow !== null || info.maxOutput !== null || info.knowledge !== null ? (
            <div className={styles.stats}>
              {info.contextWindow !== null ? <StatColumn label="Context" value={modelsDevTokens(info.contextWindow)} /> : null}
              {info.maxOutput !== null ? <StatColumn label="Output" value={modelsDevTokens(info.maxOutput)} /> : null}
              {info.knowledge !== null ? <StatColumn label="Knowledge" value={info.knowledge} /> : null}
            </div>
          ) : null}
          <AutoScrollingRow>
            <span className={styles.chips}>
              {info.toolCall ? (
                <Chip icon="toolUse">Tools</Chip>
              ) : (
                <Chip icon="warning" variant="tinted" tint="var(--warning)">
                  No tool calling
                </Chip>
              )}
              {info.reasoning ? <Chip icon="thinking">Reasoning</Chip> : null}
              {info.vision ? <Chip icon="vision">Vision</Chip> : null}
              {info.openWeights ? <Chip icon="openWeights">Open</Chip> : null}
            </span>
          </AutoScrollingRow>
        </>
      ) : null}
      {cost !== null ? <div className={styles.cost}>{cost}</div> : null}
      {stat && (stat.error_rate_24h != null || savings !== null) ? (
        <div className={styles.stats}>
          {stat.error_rate_24h != null && errorLabel !== null ? (
            <div className={styles.stat}>
              <span className={styles.statLabel}>Errors (24h)</span>
              <span className={styles.statValue}>
                <span
                  className={styles.rateDot}
                  style={{
                    background:
                      stat.error_rate_24h < 5 ? 'var(--success)' : stat.error_rate_24h < 20 ? 'var(--warning)' : 'var(--danger)',
                  }}
                  aria-hidden="true"
                />
                {errorLabel}
              </span>
            </div>
          ) : null}
          {savings !== null ? (
            <div className={styles.stat}>
              <span className={styles.statLabel}>Cost</span>
              <span className={`${styles.statValue} ${styles.savings}`}>{savings}</span>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function StatColumn({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.stat}>
      <span className={styles.statLabel}>{label}</span>
      <span className={styles.statValue}>{value}</span>
    </div>
  )
}

// MARK: - AutoScrollingRow

/** Points per second while sliding. */
const SLIDE_SPEED = 24
/** The faded edge, in px. */
const EDGE_FADE = 14
/** The rest before each slide, in seconds. */
const SLIDE_PAUSE = 1.2

/// `AutoScrollingRow` (ModelHoverCard.swift): a single line that may be
/// wider than its container. When it overflows it slides slowly to its end
/// and back (the hover card never takes the pointer, so it cannot be
/// scrolled by hand), with faded edges; with Reduce Motion it stays put and
/// fades out at the trailing edge.
export function AutoScrollingRow({ children }: { children: ReactNode }) {
  const reduceMotion = useReducedMotion()
  const containerRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [overflow, setOverflow] = useState(0)
  const [offset, setOffset] = useState(0)
  const [duration, setDuration] = useState(0)

  useLayoutEffect(() => {
    const container = containerRef.current
    const content = contentRef.current
    if (!container || !content) return
    const measure = () => setOverflow(Math.max(0, content.scrollWidth - container.clientWidth))
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(container)
    observer.observe(content)
    return () => observer.disconnect()
  }, [])

  // Pause, slide to the end, pause, slide back; repeats while visible.
  useEffect(() => {
    setDuration(0)
    setOffset(0)
    if (overflow <= 0 || reduceMotion) return
    const seconds = overflow / SLIDE_SPEED
    let cancelled = false
    const timers: number[] = []
    const wait = (s: number) => new Promise<void>((resolve) => timers.push(window.setTimeout(resolve, s * 1000)))
    void (async () => {
      while (!cancelled) {
        await wait(SLIDE_PAUSE)
        if (cancelled) return
        setDuration(seconds)
        setOffset(-overflow)
        await wait(seconds + SLIDE_PAUSE)
        if (cancelled) return
        setOffset(0)
        await wait(seconds)
      }
    })()
    return () => {
      cancelled = true
      timers.forEach((timer) => window.clearTimeout(timer))
    }
  }, [overflow, reduceMotion])

  const mask =
    overflow > 0
      ? `linear-gradient(to right, ${offset < 0 ? 'transparent' : 'black'} 0, black ${EDGE_FADE}px, black calc(100% - ${EDGE_FADE}px), ${offset <= -overflow ? 'black' : 'transparent'} 100%)`
      : undefined
  return (
    <div ref={containerRef} className={styles.autoScroll} style={mask ? { maskImage: mask, WebkitMaskImage: mask } : undefined}>
      <div
        ref={contentRef}
        className={styles.autoScrollContent}
        style={{ transform: `translateX(${offset}px)`, transition: duration > 0 ? `transform ${duration}s ease-in-out` : undefined }}
      >
        {children}
      </div>
    </div>
  )
}

// MARK: - Logos

export interface ModelLogoProps {
  modelId: string
  /** Glyph size in px; 12 by default. */
  size?: number
  /** Any CSS colour; `var(--text-secondary)` by default. */
  tint?: string
}

/// `ModelLogo` (ModelHoverCard.swift): the lab's logo for a model id
/// (models.dev), else the ✳ model glyph. Used by the composer's model chip,
/// the picker rows and the status line. Decorative.
export function ModelLogo({ modelId, size = 12, tint = 'var(--text-secondary)' }: ModelLogoProps) {
  useModelsDevVersion()
  useApp((state) => state.config?.model.base_url ?? '')
  const info = useApp.getState().modelInfo(modelId)
  const labId = info?.labId ?? null
  return (
    <LabLogo
      src={labId !== null ? modelsDev.logo(labId) : null}
      size={size}
      tint={tint}
      onError={labId !== null ? () => modelsDev.markLogoFailed(labId) : undefined}
    />
  )
}

export interface LabLogoProps {
  /** The logo's URL (`modelsDev.logo(labId)`), or null for the ✳ glyph. */
  src: string | null
  size: number
  /** Any CSS colour: the logo is drawn as a template (a mask filled with it). */
  tint: string
  /** The logo failed to load (`modelsDev.markLogoFailed`). */
  onError?: () => void
}

/// `LabLogo` (ModelHoverCard.swift): a template logo drawn at `size` in
/// `tint`, or the ✳ glyph (at 80 %) when there is none. Decorative.
export function LabLogo({ src, size, tint, onError }: LabLogoProps) {
  if (src === null) {
    return (
      <span className={styles.logoBox} style={{ width: size, height: size, color: tint }} aria-hidden="true">
        <Icon name="model" size={size * 0.8} strokeWidth={2.5} />
      </span>
    )
  }
  const mask = `url("${src}") center / contain no-repeat`
  return (
    <span className={styles.logoBox} style={{ width: size, height: size }} aria-hidden="true">
      <span className={styles.logoMask} style={{ background: tint, mask, WebkitMask: mask }} />
      {/* A CSS mask reports no load failure; this hidden probe does. */}
      <img className={styles.logoProbe} src={src} alt="" onError={onError} />
    </span>
  )
}

// MARK: - Floating card (HoverCardPanel)

/** `HoverCardPanel.Placement`. */
export type FloatingCardPlacement =
  /** Beside the host (the model picker), tops aligned with the anchor row; left of it when the window ends first. */
  | 'besideWindow'
  /** Under the anchor, leading edges aligned; above it when the window ends first. */
  | 'below'

export interface FloatingCardProps {
  /** The hovered row / chip. */
  anchor: HTMLElement
  placement: FloatingCardPlacement
  /** For `besideWindow`: the panel the card floats beside (the picker); the anchor by default. */
  host?: HTMLElement | null
  children: ReactNode
}

/** Gap between the card and what it floats beside, in px. */
const CARD_GAP = 6
/** The card stays this far inside the window, in px. */
const CARD_MARGIN = 8

/// `HoverCardPanel` (ModelHoverCard.swift) on the web: a fixed layer in
/// `document.body` that never takes the pointer or focus (the picker's
/// search field keeps it), placed beside the picker or under an anchor and
/// kept inside the window.
export function FloatingCard({ anchor, placement, host, children }: FloatingCardProps) {
  const ref = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null)

  useLayoutEffect(() => {
    const card = ref.current
    if (!card) return
    const size = card.getBoundingClientRect()
    const row = anchor.getBoundingClientRect()
    const width = window.innerWidth
    const height = window.innerHeight
    let left: number
    let top: number
    if (placement === 'besideWindow') {
      const frame = (host ?? anchor).getBoundingClientRect()
      left = frame.right + CARD_GAP
      if (left + size.width > width - CARD_MARGIN) left = frame.left - CARD_GAP - size.width
      top = row.top
    } else {
      left = Math.min(Math.max(row.left, CARD_MARGIN), width - CARD_MARGIN - size.width)
      top = row.bottom + CARD_GAP
      if (top + size.height > height - CARD_MARGIN) top = row.top - CARD_GAP - size.height
    }
    left = Math.max(CARD_MARGIN, left)
    top = Math.min(Math.max(top, CARD_MARGIN), Math.max(CARD_MARGIN, height - CARD_MARGIN - size.height))
    setPosition((previous) => (previous && previous.top === top && previous.left === left ? previous : { top, left }))
  })

  const style: CSSProperties = position ? { top: position.top, left: position.left } : { top: -9999, left: -9999, visibility: 'hidden' }
  return createPortal(
    <div ref={ref} className={styles.floating} style={style}>
      {children}
    </div>,
    document.body,
  )
}

// MARK: - Middle / head truncation

export interface MiddleTextProps {
  text: string
  /** `middle` keeps the end ("gpt-4.1-mi…-04-14"); `head` keeps only the end ("…src/app"). */
  mode?: 'middle' | 'head'
  className?: string
  style?: CSSProperties
}

/// One line truncated in the middle (`.truncationMode(.middle)`,
/// components/MiddleTruncate) or at the head (`.truncationMode(.head)`): the
/// end of the text always shows.
export function MiddleText({ text, mode = 'middle', className, style }: MiddleTextProps) {
  if (mode === 'head') {
    return (
      <span className={[styles.headText, className ?? ''].filter(Boolean).join(' ')} style={style}>
        <bdi dir="ltr">{text}</bdi>
      </span>
    )
  }
  return <MiddleTruncate natural text={text} className={className} style={style} />
}

/** A run of text with its own colour and weight (the picker's highlighted matches). */
export type { TextRun }

/** `MiddleText` for styled runs (the picker's highlighted matches), cut at the same characters. */
export function MiddleRuns({ runs, className }: { runs: TextRun[]; className?: string }) {
  return <MiddleTruncate natural text={runs.map((run) => run.text).join('')} runs={runs} className={className} />
}
