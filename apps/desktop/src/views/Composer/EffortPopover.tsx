// Port of apps/macos/ContextAgent/Views/Composer/EffortPopover.swift:
// `EffortLevel` (the reasoning-effort levels the composer offers,
// `model.reasoning_effort`) and `EffortPopover`, the effort chip's popover.

import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react'
import { Popover } from '../../components/Popover'
import { Icon, type IconName } from '../../theme/icons'
import { isPlainReturn, useNativeKeyDown } from './nativeKeyDown'
import styles from './EffortPopover.module.css'

// MARK: - EffortLevel

/// `EffortLevel`: one reasoning-effort level the composer offers
/// (`model.reasoning_effort`). `value` "" is Auto (no value sent, the model
/// decides).
export interface EffortLevel {
  /** The config value; "" is Auto. */
  value: string
  title: string
  /** `systemImage`, as a catalogue name. */
  icon: IconName
  description: string
}

/** `EffortLevel.auto`. */
export const EFFORT_AUTO: EffortLevel = { value: '', title: 'Auto', icon: 'effortAuto', description: 'The model decides' }
/** `EffortLevel.ultra`: the model's strongest effort plus sub-agents. */
export const EFFORT_ULTRA: EffortLevel = {
  value: 'ultra',
  title: 'Ultra',
  icon: 'agents',
  description: "The model's strongest effort, plus sub-agents in parallel",
}
/** `EffortLevel.standard`: offered for a model models.dev does not know. */
export const EFFORT_STANDARD: readonly string[] = ['low', 'medium', 'high']

/** `EffortLevel.known`: the values models.dev lists, with an icon and a few words each. */
const KNOWN: readonly EffortLevel[] = [
  { value: 'none', title: 'None', icon: 'effortNone', description: 'No reasoning' },
  { value: 'minimal', title: 'Minimal', icon: 'effortMinimal', description: 'Barely any reasoning' },
  { value: 'low', title: 'Low', icon: 'effortLow', description: 'Fastest replies' },
  { value: 'medium', title: 'Medium', icon: 'effortMedium', description: 'Balanced' },
  { value: 'high', title: 'High', icon: 'effortHigh', description: 'Deeper reasoning' },
  { value: 'xhigh', title: 'Extra High', icon: 'effortExtraHigh', description: 'Deeper still' },
  { value: 'max', title: 'Max', icon: 'effortMax', description: "The model's maximum" },
]

/** `EffortLevel.isUltra`. */
export function effortIsUltra(level: EffortLevel): boolean {
  return level.value === EFFORT_ULTRA.value
}

/** `EffortLevel.tint`: Ultra is `--ultra`, the rest primary text. */
export function effortTint(level: EffortLevel): string {
  return effortIsUltra(level) ? 'var(--ultra)' : 'var(--text-primary)'
}

/// `EffortLevel.level(for:)`: the level for a config value; an unknown
/// value gets a plain row of its own.
export function effortLevel(value: string | null | undefined): EffortLevel {
  const raw = value ?? ''
  const found = [EFFORT_AUTO, EFFORT_ULTRA, ...KNOWN].find((level) => level.value === raw)
  if (found) return found
  const chars = Array.from(raw)
  return { value: raw, title: (chars[0] ?? '').toUpperCase() + chars.slice(1).join(''), icon: 'effort', description: "The model's own setting" }
}

/// `EffortLevel.levels(for:)`: Auto, the effort values the model accepts as
/// models.dev lists them (`EFFORT_STANDARD` when it does not know the
/// model), then Ultra.
export function effortLevels(values: readonly string[] | null | undefined): EffortLevel[] {
  const listed = (values ?? EFFORT_STANDARD).filter((value) => value !== EFFORT_ULTRA.value && value.length > 0)
  return [EFFORT_AUTO, ...listed.map((value) => effortLevel(value)), EFFORT_ULTRA]
}

// MARK: - EffortPopover

export interface EffortPopoverProps {
  open: boolean
  /** Esc, an outside press, or a choice. */
  onClose: () => void
  /** The effort chip. */
  anchorRef: RefObject<HTMLElement | null>
  /** `model.reasoning_effort`; null / undefined is Auto. */
  current: string | null | undefined
  /** `effortLevels(modelInfo.effortLevels)`. */
  offered: readonly EffortLevel[]
  /** Called with the chosen value ("" for Auto) when it differs from `current`. */
  onSelect: (value: string) => void
}

/// `EffortPopover` (EffortPopover.swift): Auto, the model's own effort
/// values, Ultra, one row each with its icon and a few words, Ultra in
/// purple; a current value the model does not offer gets a row of its own.
/// A click or Return applies it and closes; ↑↓ move, Esc closes. 280 px.
export function EffortPopover({ open, onClose, anchorRef, current, offered, onSelect }: EffortPopoverProps) {
  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="below" align="center" width={282} returnFocus={false} aria-label="Reasoning effort" role="listbox">
      <EffortList current={current} offered={offered} onSelect={onSelect} onClose={onClose} />
    </Popover>
  )
}

function EffortList({ current, offered, onSelect, onClose }: Omit<EffortPopoverProps, 'open' | 'anchorRef'>) {
  const currentValue = current ?? ''
  const levels = offered.some((level) => level.value === currentValue) ? [...offered] : [...offered, effortLevel(current)]
  const [highlighted, setHighlighted] = useState<number | null>(() => {
    const index = levels.findIndex((level) => level.value === currentValue)
    return index >= 0 ? index : null
  })
  const containerRef = useRef<HTMLDivElement>(null)

  // Popover content is attached a beat after creation; focusing then sticks.
  useEffect(() => {
    const timer = window.setTimeout(() => containerRef.current?.focus({ preventScroll: true }), 60)
    return () => window.clearTimeout(timer)
  }, [])

  const choose = (level: EffortLevel) => {
    if (level.value !== currentValue) onSelect(level.value)
    onClose()
  }

  const move = (delta: number) => {
    if (levels.length === 0) return
    setHighlighted((index) => Math.min(Math.max((index ?? -1) + delta, 0), levels.length - 1))
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
    }
  }

  // Return applies the highlighted level (natively: see nativeKeyDown.ts).
  useNativeKeyDown(containerRef, (event) => {
    if (!isPlainReturn(event)) return
    event.preventDefault()
    event.stopPropagation()
    const level = highlighted !== null ? levels[highlighted] : undefined
    if (level) choose(level)
  })

  return (
    <div ref={containerRef} className={styles.list} tabIndex={0} onKeyDown={onKeyDown}>
      {levels.map((level, index) => {
        const isCurrent = level.value === currentValue
        const ultra = effortIsUltra(level)
        return (
          <button
            key={level.value}
            type="button"
            role="option"
            tabIndex={-1}
            aria-selected={isCurrent}
            aria-label={`${level.title}: ${level.description}`}
            className={[styles.row, highlighted === index ? styles.highlighted : ''].filter(Boolean).join(' ')}
            onClick={() => choose(level)}
            onMouseEnter={() => setHighlighted(index)}
          >
            <span className={styles.icon} style={{ color: ultra ? 'var(--ultra)' : 'var(--text-secondary)' }} aria-hidden="true">
              <Icon name={level.icon} size={13} strokeWidth={2} />
            </span>
            <span className={styles.text}>
              <span className={styles.title} style={{ color: effortTint(level), fontWeight: isCurrent || ultra ? 600 : 500 }}>
                {level.title}
              </span>
              <span className={styles.description}>{level.description}</span>
            </span>
            {isCurrent ? (
              <span className={styles.check} style={{ color: ultra ? 'var(--ultra)' : 'var(--accent)' }} aria-hidden="true">
                <Icon name="checkmark" size={11} strokeWidth={3} />
              </span>
            ) : null}
          </button>
        )
      })}
    </div>
  )
}
