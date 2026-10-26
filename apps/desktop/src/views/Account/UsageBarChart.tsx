import { memo, useMemo, useRef, useState, type PointerEvent } from 'react'
import {
  totalsIsEmpty,
  usageFigures,
  usageMetricFormat,
  usageMetricValue,
  usageRangeCaption,
  type UsageMetric,
  type UsageRange,
  type UsageSeries,
} from '../../models/modelGateway'
import { HEATMAP_HEIGHT, HEATMAP_WIDTH } from './UsageHeatmap'
import { useTrailingScroll } from './useTrailingScroll'
import styles from './UsageBarChart.module.css'

/** `UsageBarChart.width`: as wide as `UsageHeatmap`. */
const WIDTH = HEATMAP_WIDTH
/** `UsageBarChart.height`: as tall as `UsageHeatmap`. */
const HEIGHT = HEATMAP_HEIGHT
/** Room under the bars for the axis labels. */
const AXIS_HEIGHT = 16
const PLOT_HEIGHT = HEIGHT - AXIS_HEIGHT

/** `.dateTime.hour()`: "12 AM", "6 PM". */
const HOUR_FORMAT = new Intl.DateTimeFormat(undefined, { hour: 'numeric' })
/** `.dateTime.weekday(.abbreviated)`: "Mon". */
const WEEKDAY_FORMAT = new Intl.DateTimeFormat(undefined, { weekday: 'short' })
/** `.dateTime.month(.abbreviated).day()`: "Oct 24". */
const MONTH_DAY_FORMAT = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' })
/** `.dateTime.weekday(.abbreviated).hour().minute()`: "Mon 2:00 PM". */
const HOUR_CAPTION_FORMAT = new Intl.DateTimeFormat(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' })
/** `.dateTime.weekday(.abbreviated).month(.abbreviated).day()`: "Mon, Oct 24". */
const DAY_CAPTION_FORMAT = new Intl.DateTimeFormat(undefined, { weekday: 'short', month: 'short', day: 'numeric' })

/// `UsageBarChart.axisLabels()`: every sixth hour ("12 AM", "6 AM"…), every
/// day ("Mon"), or every seventh day counting back from today ("Oct 24").
function axisLabels(range: UsageRange, buckets: readonly number[]): Array<{ index: number; text: string }> {
  const labels: Array<{ index: number; text: string }> = []
  buckets.forEach((bucket, index) => {
    const date = new Date(bucket)
    switch (range) {
      case 'day':
        if (date.getHours() % 6 === 0) labels.push({ index, text: HOUR_FORMAT.format(date) })
        break
      case 'week':
        labels.push({ index, text: WEEKDAY_FORMAT.format(date) })
        break
      case 'month':
      case 'all':
        if ((buckets.length - 1 - index) % 7 === 0) labels.push({ index, text: MONTH_DAY_FORMAT.format(date) })
        break
    }
  })
  return labels
}

/** Props of `UsageBarChart` (Views/Account/UsageBarChart.swift). */
export interface UsageBarChartProps {
  series: UsageSeries
  range: UsageRange
  /** Bucket starts (ms), oldest first: `UsageBreakdown.bucketStarts`. */
  buckets: readonly number[]
  metric: UsageMetric
  quotaPerUnit: number
  /** The caption's resting text shows the series total. */
  showsTotal?: boolean
}

/**
 * `UsageBarChart` (Views/Account/UsageBarChart.swift): the Usage card's chart
 * for the 24h, 7d and 30d ranges, one bar per hour or day of `buckets`, as
 * tall as its share of the busiest one by the chosen metric, with hour or
 * date labels underneath. As wide and tall as `UsageHeatmap`, with the same
 * caption: hovering a bar shows its figures, otherwise the series total.
 * Narrower than the chart, it scrolls sideways, kept on today.
 */
export const UsageBarChart = memo(function UsageBarChart({ series, range, buckets, metric, quotaPerUnit, showsTotal = true }: UsageBarChartProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  useTrailingScroll(scrollerRef)
  const [hoveredIndex, setHovered] = useState<number | null>(null)
  // A bar hovered before the period changed may no longer exist.
  const hovered = hoveredIndex !== null && hoveredIndex < buckets.length ? hoveredIndex : null

  const values = useMemo(
    () =>
      buckets.map((bucket) => {
        const totals = series.buckets.get(bucket)
        return totals ? usageMetricValue(metric, totals, quotaPerUnit) : 0
      }),
    [buckets, series, metric, quotaPerUnit],
  )
  const maxValue = values.reduce((max, value) => Math.max(max, value), 0)
  const labels = useMemo(() => axisLabels(range, buckets), [range, buckets])

  // Geometry: a quarter of the pitch is gap; seven-day bars stop at 40 px.
  const pitch = buckets.length === 0 ? WIDTH : WIDTH / buckets.length
  const barWidth = Math.min(40, Math.max(2, pitch - Math.max(3, Math.round(pitch * 0.25))))
  const radius = Math.min(2, barWidth / 2)

  const barRect = (index: number, value: number) => {
    const x = index * pitch + (pitch - barWidth) / 2
    // Empty buckets keep a 2 px stub so the row of buckets stays visible.
    const height = value > 0 && maxValue > 0 ? Math.max(3, PLOT_HEIGHT * (value / maxValue)) : 2
    return { x, y: PLOT_HEIGHT - height, width: barWidth, height }
  }

  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const box = event.currentTarget.getBoundingClientRect()
    const x = event.clientX - box.left
    const y = event.clientY - box.top
    let index: number | null = null
    if (buckets.length > 0 && x >= 0 && y >= 0 && y <= HEIGHT) {
      const candidate = Math.floor(x / pitch)
      index = candidate >= 0 && candidate < buckets.length ? candidate : null
    }
    setHovered(index)
  }

  let caption: string
  const hoveredBucket = hovered === null ? undefined : buckets[hovered]
  if (hoveredBucket !== undefined) {
    const date = new Date(hoveredBucket)
    const when = range === 'day' ? HOUR_CAPTION_FORMAT.format(date) : DAY_CAPTION_FORMAT.format(date)
    const totals = series.buckets.get(hoveredBucket)
    caption = !totals || totalsIsEmpty(totals) ? `${when} · no usage` : `${when} · ${usageFigures(totals, quotaPerUnit)}`
  } else if (!showsTotal) {
    caption = ''
  } else if (totalsIsEmpty(series.total)) {
    caption = `No usage in the ${usageRangeCaption(range).toLowerCase()}`
  } else {
    caption = `${usageRangeCaption(range)} · ${usageFigures(series.total, quotaPerUnit)}`
  }

  return (
    <div className={styles.chart}>
      <div ref={scrollerRef} className={styles.scroller}>
        <svg
          width={WIDTH}
          height={HEIGHT}
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          className={styles.canvas}
          role="img"
          aria-label={`Usage chart, ${series.title}: ${usageMetricFormat(metric, series.total, quotaPerUnit)} over the ${usageRangeCaption(range).toLowerCase()}`}
          onPointerMove={onPointerMove}
          onPointerLeave={() => setHovered(null)}
        >
          {hovered !== null ? <rect x={hovered * pitch} y={0} width={pitch} height={PLOT_HEIGHT} rx={3} ry={3} className={styles.hoverColumn} /> : null}
          {values.map((value, index) => {
            const rect = barRect(index, value)
            const className = value > 0 ? (hovered === null || hovered === index ? styles.bar : styles.barDimmed) : styles.barEmpty
            return <rect key={index} x={rect.x} y={rect.y} width={rect.width} height={rect.height} rx={radius} ry={radius} className={className} />
          })}
          {labels.map(({ index, text }) => {
            const midX = index * pitch + pitch / 2
            const leading = midX < 20
            const trailing = !leading && midX > WIDTH - 20
            const x = leading ? index * pitch + (pitch - barWidth) / 2 : trailing ? index * pitch + (pitch + barWidth) / 2 : midX
            return (
              <text
                key={index}
                x={x}
                y={PLOT_HEIGHT + 4}
                textAnchor={leading ? 'start' : trailing ? 'end' : 'middle'}
                dominantBaseline="text-before-edge"
                className={styles.axisLabel}
              >
                {text}
              </text>
            )
          })}
        </svg>
      </div>
      <span className={[styles.caption, hovered !== null ? styles.captionHovered : ''].filter(Boolean).join(' ')}>{caption}</span>
    </div>
  )
})
