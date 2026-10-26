import { memo, useMemo, useRef, useState, type PointerEvent } from 'react'
import {
  addDays,
  startOfDay,
  totalsIsEmpty,
  usageFigures,
  usageMetricFormat,
  usageMetricValue,
  type UsageMetric,
  type UsageSeries,
} from '../../models/modelGateway'
import { useTrailingScroll } from './useTrailingScroll'
import styles from './UsageHeatmap.module.css'

// MARK: - Grid geometry (`HeatmapGrid`)

/// `HeatmapGrid`'s metrics: 53 week columns × 7 weekday rows of 10 px squares
/// 3 px apart, 30 px of weekday labels at the left, 16 px of month labels on top.
export const HEATMAP = {
  weeks: 53,
  cell: 10,
  gap: 3,
  pitch: 13,
  labelWidth: 30,
  labelHeight: 16,
} as const

/** `HeatmapGrid.width` (also `UsageBarChart.width`): 716 px. */
export const HEATMAP_WIDTH = HEATMAP.labelWidth + HEATMAP.weeks * HEATMAP.pitch - HEATMAP.gap
/** `HeatmapGrid.height` (also `UsageBarChart.height`): 104 px. */
export const HEATMAP_HEIGHT = HEATMAP.labelHeight + 7 * HEATMAP.pitch - HEATMAP.gap

/// `Calendar.current.firstWeekday` (1 = Sunday … 7 = Saturday) from the
/// locale's week info; Sunday when the runtime does not say.
function localeFirstWeekday(): number {
  try {
    const locale = new Intl.Locale(navigator.language) as Intl.Locale & {
      getWeekInfo?: () => { firstDay?: number }
      weekInfo?: { firstDay?: number }
    }
    const info = typeof locale.getWeekInfo === 'function' ? locale.getWeekInfo() : locale.weekInfo
    const firstDay = info?.firstDay
    // Intl: 1 = Monday … 7 = Sunday.
    if (typeof firstDay === 'number' && firstDay >= 1 && firstDay <= 7) return (firstDay % 7) + 1
  } catch {
    // Older runtimes: no week info.
  }
  return 1
}

const FIRST_WEEKDAY = localeFirstWeekday()

/** `Calendar.shortWeekdaySymbols`, Sunday first. */
const SHORT_WEEKDAYS: readonly string[] = (() => {
  const format = new Intl.DateTimeFormat(undefined, { weekday: 'short' })
  // 1 January 2023 was a Sunday.
  return Array.from({ length: 7 }, (_, index) => format.format(new Date(2023, 0, 1 + index)))
})()

/** `Calendar.shortMonthSymbols`. */
const SHORT_MONTHS: readonly string[] = (() => {
  const format = new Intl.DateTimeFormat(undefined, { month: 'short' })
  return Array.from({ length: 12 }, (_, index) => format.format(new Date(2023, index, 1)))
})()

/** `.dateTime.weekday(.abbreviated).month(.abbreviated).day().year()`: "Mon, Oct 24, 2026". */
const DAY_FORMAT = new Intl.DateTimeFormat(undefined, { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' })

/// `HeatmapGrid`: column 0 is the week 52 weeks before this one, row 0 the
/// calendar's first weekday; days after today are not drawn.
export interface HeatmapGrid {
  /** Start of today (ms). */
  today: number
  /** The day drawn at column 0, row 0 (ms). */
  start: number
}

/** `HeatmapGrid(calendar:today:)`. */
export function makeHeatmapGrid(now: number = Date.now()): HeatmapGrid {
  const today = startOfDay(now)
  const weekday = new Date(today).getDay() + 1
  const offset = (weekday - FIRST_WEEKDAY + 7) % 7
  const weekStart = addDays(today, -offset)
  return { today, start: addDays(weekStart, -7 * (HEATMAP.weeks - 1)) }
}

/** `HeatmapGrid.date(column:row:)`: the day of a cell, or null for a day still to come. */
export function heatmapDate(grid: HeatmapGrid, column: number, row: number): number | null {
  const date = addDays(grid.start, column * 7 + row)
  return date <= grid.today ? date : null
}

/** `HeatmapGrid.rect(column:row:)`'s origin. */
function cellOrigin(column: number, row: number): { x: number; y: number } {
  return { x: HEATMAP.labelWidth + column * HEATMAP.pitch, y: HEATMAP.labelHeight + row * HEATMAP.pitch }
}

/** `HeatmapGrid.date(at:)`: the cell under a point (gaps count as the cell before them). */
function cellAt(grid: HeatmapGrid, x: number, y: number): { column: number; row: number; date: number } | null {
  if (x < HEATMAP.labelWidth || y < HEATMAP.labelHeight) return null
  const column = Math.floor((x - HEATMAP.labelWidth) / HEATMAP.pitch)
  const row = Math.floor((y - HEATMAP.labelHeight) / HEATMAP.pitch)
  if (column < 0 || column >= HEATMAP.weeks || row < 0 || row >= 7) return null
  const date = heatmapDate(grid, column, row)
  return date === null ? null : { column, row, date }
}

/// `HeatmapGrid.weekdayLabel(row:)`: Mon · Wed · Fri (rows 1, 3, 5 of a
/// Sunday-first week; whichever weekdays land on those rows for other calendars).
function weekdayLabel(row: number): string | null {
  if (row % 2 !== 1) return null
  const symbol = SHORT_WEEKDAYS[(FIRST_WEEKDAY - 1 + row) % 7]
  return symbol === undefined ? null : Array.from(symbol).slice(0, 3).join('')
}

/// `HeatmapGrid.monthLabels()`: a label on the first column of each month;
/// the partial month at the left edge is labelled only when it keeps at least
/// two columns; of two labels closer than three columns the later is dropped.
function monthLabels(grid: HeatmapGrid): Array<{ column: number; text: string }> {
  const labels: Array<{ column: number; text: string }> = []
  let previousMonth: number | null = null
  for (let column = 0; column < HEATMAP.weeks; column += 1) {
    const month = new Date(addDays(grid.start, column * 7)).getMonth()
    if (month !== previousMonth) {
      const spansTwo = new Date(addDays(grid.start, (column + 1) * 7)).getMonth() === month
      if (previousMonth !== null || spansTwo) {
        const text = SHORT_MONTHS[month]
        if (text !== undefined) labels.push({ column, text })
      }
      previousMonth = month
    }
  }
  const result: Array<{ column: number; text: string }> = []
  for (const label of labels) {
    const last = result[result.length - 1]
    if (last && label.column - last.column < 3) continue
    result.push(label)
  }
  return result
}

/** `UsageHeatmap.color(for:maxValue:)`: empty days step 0, the rest one of four steps by their share of the busiest day. */
function step(value: number, maxValue: number): number {
  if (!(value > 0) || !(maxValue > 0)) return 0
  return Math.min(4, Math.max(1, Math.ceil((value / maxValue) * 4)))
}

const LEVEL_CLASSES = [styles.level0, styles.level1, styles.level2, styles.level3, styles.level4]

// MARK: - View

/** Props of `UsageHeatmap` (Views/Account/UsageHeatmap.swift). */
export interface UsageHeatmapProps {
  series: UsageSeries
  metric: UsageMetric
  quotaPerUnit: number
  /** The caption's resting text shows the series total. */
  showsTotal?: boolean
}

/**
 * `UsageHeatmap` (Views/Account/UsageHeatmap.swift): a GitHub-style
 * contribution chart of one series, 53 week columns × 7 weekday rows of 10 px
 * squares ending today, shaded in four steps of `accent` by the chosen
 * metric; month labels above, Mon/Wed/Fri at the left. Hovering a day shows
 * its figures in the caption under the grid; the legend sits at its right.
 * Narrower than the grid, it scrolls sideways, kept on today.
 */
export const UsageHeatmap = memo(function UsageHeatmap({ series, metric, quotaPerUnit, showsTotal = true }: UsageHeatmapProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  useTrailingScroll(scrollerRef)
  const [hovered, setHovered] = useState<{ column: number; row: number; date: number } | null>(null)
  const today = startOfDay(Date.now())
  const grid = useMemo(() => makeHeatmapGrid(today), [today])

  // The grid itself only changes with the data; hover draws an outline on top.
  const cells = useMemo(() => {
    let maxValue = 0
    for (const totals of series.buckets.values()) maxValue = Math.max(maxValue, usageMetricValue(metric, totals, quotaPerUnit))
    const rects = []
    for (let column = 0; column < HEATMAP.weeks; column += 1) {
      for (let row = 0; row < 7; row += 1) {
        const date = heatmapDate(grid, column, row)
        if (date === null) continue
        const totals = series.buckets.get(date)
        const value = totals ? usageMetricValue(metric, totals, quotaPerUnit) : 0
        const { x, y } = cellOrigin(column, row)
        rects.push(
          <rect key={column * 7 + row} x={x} y={y} width={HEATMAP.cell} height={HEATMAP.cell} rx={2} ry={2} className={LEVEL_CLASSES[step(value, maxValue)]} />,
        )
      }
    }
    return rects
  }, [grid, series, metric, quotaPerUnit])

  const labels = useMemo(
    () => (
      <>
        {monthLabels(grid).map(({ column, text }) => (
          <text key={`m${column}`} x={cellOrigin(column, 0).x} y={0} className={styles.axisLabel} dominantBaseline="text-before-edge">
            {text}
          </text>
        ))}
        {Array.from({ length: 7 }, (_, row) => {
          const label = weekdayLabel(row)
          if (label === null) return null
          return (
            <text key={`w${row}`} x={0} y={cellOrigin(0, row).y + HEATMAP.cell / 2} className={styles.axisLabel} dominantBaseline="central">
              {label}
            </text>
          )
        })}
      </>
    ),
    [grid],
  )

  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    const cell = cellAt(grid, event.clientX - box.left, event.clientY - box.top)
    setHovered((current) => (current?.date === cell?.date ? current : cell))
  }

  let caption: string
  if (hovered) {
    const day = DAY_FORMAT.format(new Date(hovered.date))
    const totals = series.buckets.get(hovered.date)
    caption = !totals || totalsIsEmpty(totals) ? `${day} · no usage` : `${day} · ${usageFigures(totals, quotaPerUnit)}`
  } else if (!showsTotal) {
    caption = ''
  } else if (totalsIsEmpty(series.total)) {
    caption = 'No usage in the last year'
  } else {
    caption = `Last 12 months · ${usageFigures(series.total, quotaPerUnit)}`
  }

  const outline = hovered ? cellOrigin(hovered.column, hovered.row) : null

  return (
    <div className={styles.chart}>
      <div ref={scrollerRef} className={styles.scroller}>
        <svg
          width={HEATMAP_WIDTH}
          height={HEATMAP_HEIGHT}
          viewBox={`0 0 ${HEATMAP_WIDTH} ${HEATMAP_HEIGHT}`}
          className={styles.canvas}
          role="img"
          aria-label={`Usage chart, ${series.title}: ${usageMetricFormat(metric, series.total, quotaPerUnit)} over the last year`}
          onPointerMove={onPointerMove}
          onPointerLeave={() => setHovered(null)}
        >
          {labels}
          {cells}
          {outline ? (
            <rect x={outline.x} y={outline.y} width={HEATMAP.cell} height={HEATMAP.cell} rx={2} ry={2} className={styles.hoverOutline} />
          ) : null}
        </svg>
      </div>
      <div className={styles.captionRow}>
        <span className={[styles.caption, hovered ? styles.captionHovered : ''].filter(Boolean).join(' ')}>{caption}</span>
        <span className={styles.spacer} />
        <span className={styles.legend} aria-hidden="true">
          <span className={`${styles.legendText} ${styles.legendLess}`}>Less</span>
          {LEVEL_CLASSES.map((className, index) => (
            <svg key={index} width={10} height={10} className={styles.legendSquare}>
              <rect width={10} height={10} rx={2} ry={2} className={className} />
            </svg>
          ))}
          <span className={`${styles.legendText} ${styles.legendMore}`}>More</span>
        </span>
      </div>
    </div>
  )
})
