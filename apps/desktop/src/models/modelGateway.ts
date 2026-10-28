// Port of apps/macos/ContextAgent/Models/ModelGateway.swift (docs/desktop-protocol.md
// "ModelGateway"): the computed properties of the `gateway/*` wire shapes and the
// usage maths behind the Account page's charts. The wire shapes themselves are in
// backend/types.ts (`GatewayAccount`, `GatewaySubscription`, `GatewayUsageReport`,
// `GatewayBilling`, …, snake_case as on the wire); Swift's computed properties are
// functions here (`account.formatQuota(x)` → `formatQuota(account, x)`).
//
// Quotas are gateway units; dollars are `quota / quota_per_unit`.
//
// Dates: a usage bucket is the start of a local hour (24h) or a local day (7d,
// 30d, All), as a millisecond timestamp — the Swift `Date` keys of
// `UsageSeries.buckets`. Folding a year of hourly rows runs once per report in a
// memoised function (`usageBreakdownsByRange`) instead of on a detached task.

import {
  gatewayAccountName,
  type GatewayAccount,
  type GatewayBilling,
  type GatewayBillingPreference,
  type GatewayKeySettings,
  type GatewayPlan,
  type GatewayPriceRule,
  type GatewaySubscription,
  type GatewayTopUpOptions,
  type GatewayUsageReport,
  type GatewayUsageRow,
} from '../backend/types'

export { gatewayAccountName }

// MARK: - Number formatting (`UsageFormat`)

/** Swift's `Double.rounded()`: half away from zero (JS `Math.round` rounds half up). */
function roundHalfAway(value: number): number {
  return value < 0 ? -Math.round(-value) : Math.round(value)
}

/** `UsageFormat.trimmed`: one decimal below 10, none above, without a trailing ".0". */
function trimmed(value: number): string {
  const text = value < 10 ? value.toFixed(1) : value.toFixed(0)
  return text.endsWith('.0') ? text.slice(0, -2) : text
}

/** `Int.formatted()`: grouped digits for the user's locale ("1,284"). */
export function formatInteger(value: number): string {
  return Math.trunc(value).toLocaleString()
}

/** `UsageFormat` of ModelGateway.swift: number formatting shared by the Account page and the sidebar button. */
export const UsageFormat = {
  /// "$12.34"; tiny non-zero amounts show three decimals ("$0.004"), zero is
  /// "$0.00", thousands are compact ("$1.2k").
  usd(value: number): string {
    if (value === 0) return '$0.00'
    if (Math.abs(value) < 0.01) return `$${value.toFixed(3)}`
    if (Math.abs(value) >= 1000) return '$' + UsageFormat.compact(roundHalfAway(value))
    return `$${value.toFixed(2)}`
  },

  /** "12.3k", "1.2M", "845". */
  compact(value: number): string {
    const n = Math.trunc(value)
    const magnitude = Math.abs(n)
    if (magnitude < 1000) return `${n}`
    if (magnitude < 1_000_000) return trimmed(n / 1000) + 'k'
    if (magnitude < 1_000_000_000) return trimmed(n / 1_000_000) + 'M'
    return trimmed(n / 1_000_000_000) + 'B'
  },

  /** "1,284 requests", "1 request". */
  count(value: number, singular: string, plural: string): string {
    return `${formatInteger(value)} ${value === 1 ? singular : plural}`
  },
} as const

// MARK: - GatewayAccount

/** `GatewayAccount.name`: display name, else the username. */
export function accountName(account: GatewayAccount): string {
  return gatewayAccountName(account)
}

/** `GatewayAccount.initial`: one letter for the avatar. */
export function accountInitial(account: GatewayAccount): string {
  const first = Array.from(gatewayAccountName(account).trim())[0] ?? ''
  return first.toUpperCase()
}

/** `GatewayAccount.balanceUSD`. */
export function balanceUSD(account: GatewayAccount): number {
  return account.quota / Math.max(account.quota_per_unit, 1)
}

/** `GatewayAccount.usedUSD`. */
export function usedUSD(account: GatewayAccount): number {
  return account.used_quota / Math.max(account.quota_per_unit, 1)
}

/** `GatewayAccount.formatQuota(_:)`: a quota amount as the gateway shows it, dollars or raw units. */
export function formatQuota(account: GatewayAccount, amount: number): string {
  return account.display_in_currency ? UsageFormat.usd(amount / Math.max(account.quota_per_unit, 1)) : UsageFormat.compact(amount)
}

/** `GatewayAccount.formatCents`: cents of credit as the gateway shows quota. */
export function formatGatewayCents(account: GatewayAccount, cents: number): string {
  return account.display_in_currency ? UsageFormat.usd(cents / 100) : UsageFormat.compact(Math.round((cents / 100) * account.quota_per_unit))
}

/** The balance as the sidebar button and the header show it ("$24.69" or "12.3M"). */
export function balanceText(account: GatewayAccount): string {
  return account.display_in_currency ? UsageFormat.usd(balanceUSD(account)) : UsageFormat.compact(account.quota)
}

/** `GatewayAccount.billingPreferenceTitle`: `billing_preference` in words; null when unknown. */
export function billingPreferenceTitle(account: GatewayAccount): string | null {
  switch (account.billing_preference) {
    case 'subscription_first':
      return 'Plan first, then balance'
    case 'wallet_first':
      return 'Balance first, then plan'
    case 'subscription_only':
      return 'Plan only'
    case 'wallet_only':
      return 'Balance only'
    default:
      return null
  }
}

// MARK: - GatewaySubscription

const PRICE_UNITS = ['year', 'month', 'day', 'hour']

/** `Double.formatted(.currency(code:))`: "$20.00". */
function formatCurrency(amount: number, currency: string): string {
  const code = currency.length === 0 ? 'USD' : currency
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency: code }).format(amount)
  } catch {
    return `${code} ${amount.toFixed(2)}`
  }
}

/** "$20 / month", "$50 / 3 months", "Free" (shared by plans on sale and active subscriptions). */
function priceTitle(priceAmount: number, currency: string, durationUnit: string, durationValue: number): string {
  const price = priceAmount > 0 ? formatCurrency(priceAmount, currency) : 'Free'
  if (!(priceAmount > 0) || !PRICE_UNITS.includes(durationUnit) || durationValue <= 0) return price
  return durationValue === 1 ? `${price} / ${durationUnit}` : `${price} / ${durationValue} ${durationUnit}s`
}

/** `GatewaySubscription.name`: the plan title, else "Plan #id" (the plan was deleted). */
export function subscriptionName(subscription: GatewaySubscription): string {
  const title = subscription.title.trim()
  return title.length === 0 ? `Plan #${subscription.plan_id}` : title
}

/** `GatewaySubscription.isUnlimited`. */
export function subscriptionIsUnlimited(subscription: GatewaySubscription): boolean {
  return subscription.amount_total <= 0
}

/** `GatewaySubscription.remaining`. */
export function subscriptionRemaining(subscription: GatewaySubscription): number {
  return Math.max(0, subscription.amount_total - subscription.amount_used - subscription.amount_reserved)
}

/** `GatewaySubscription.usedFraction`: used share of the plan's quota, 0…1 (0 when unlimited). */
export function subscriptionUsedFraction(subscription: GatewaySubscription): number {
  if (subscriptionIsUnlimited(subscription)) return 0
  return Math.min(1, (subscription.amount_used + subscription.amount_reserved) / subscription.amount_total)
}

/** `GatewaySubscription.endDate` (ms), null when the plan does not end. */
export function subscriptionEndDate(subscription: GatewaySubscription): number | null {
  return subscription.end_time > 0 ? subscription.end_time * 1000 : null
}

/** `GatewaySubscription.nextResetDate` (ms), null when the quota does not reset. */
export function subscriptionNextResetDate(subscription: GatewaySubscription): number | null {
  return subscription.next_reset_time > 0 ? subscription.next_reset_time * 1000 : null
}

/** `GatewaySubscription.daysLeft(now:)`: whole days until the plan ends (0 on its last day). */
export function subscriptionDaysLeft(subscription: GatewaySubscription, now: number = Date.now()): number | null {
  const end = subscriptionEndDate(subscription)
  if (end === null) return null
  return Math.max(0, Math.trunc((end - now) / 86_400_000))
}

/** `GatewaySubscription.priceTitle`: "$20 / month", "Free"; null when the plan is unknown. */
export function subscriptionPriceTitle(subscription: GatewaySubscription): string | null {
  if (subscription.title.length === 0) return null
  return priceTitle(subscription.price_amount, subscription.currency, subscription.duration_unit, subscription.duration_value)
}

/** `GatewaySubscription.resetTitle`: "Resets monthly", null when the quota never resets. */
export function subscriptionResetTitle(subscription: GatewaySubscription): string | null {
  switch (subscription.quota_reset_period) {
    case 'daily':
    case 'weekly':
    case 'monthly':
      return `Resets ${subscription.quota_reset_period}`
    case 'custom':
      return 'Resets periodically'
    default:
      return null
  }
}

// MARK: - Usage maths

/** `GatewayUsageTotals`: spend, tokens and requests of one bucket (or of a whole series). */
export interface GatewayUsageTotals {
  quota: number
  tokens: number
  requests: number
}

/** `GatewayUsageTotals()`. */
export function emptyTotals(): GatewayUsageTotals {
  return { quota: 0, tokens: 0, requests: 0 }
}

/** `GatewayUsageTotals.isEmpty`. */
export function totalsIsEmpty(totals: GatewayUsageTotals): boolean {
  return totals.quota === 0 && totals.tokens === 0 && totals.requests === 0
}

/** `GatewayUsageTotals.add(_: GatewayUsageRow)`. */
function addRow(totals: GatewayUsageTotals, row: GatewayUsageRow): void {
  totals.quota += row.quota
  totals.tokens += row.token_used
  totals.requests += row.count
}

/** `UsageMetric`: what a bar or heatmap cell measures. */
export type UsageMetric = 'cost' | 'tokens' | 'requests'

/** `UsageMetric.allCases`. */
export const USAGE_METRICS: readonly UsageMetric[] = ['cost', 'tokens', 'requests']

/** `UsageMetric.title`. */
export function usageMetricTitle(metric: UsageMetric): string {
  switch (metric) {
    case 'cost':
      return 'Cost'
    case 'tokens':
      return 'Tokens'
    case 'requests':
      return 'Requests'
  }
}

/** `UsageMetric.value(_:quotaPerUnit:)`. */
export function usageMetricValue(metric: UsageMetric, totals: GatewayUsageTotals, quotaPerUnit: number): number {
  switch (metric) {
    case 'cost':
      return totals.quota / Math.max(quotaPerUnit, 1)
    case 'tokens':
      return totals.tokens
    case 'requests':
      return totals.requests
  }
}

/** `UsageMetric.format(_:quotaPerUnit:)`: "$1.24", "12.3k tokens", "84 requests". */
export function usageMetricFormat(metric: UsageMetric, totals: GatewayUsageTotals, quotaPerUnit: number): string {
  switch (metric) {
    case 'cost':
      return UsageFormat.usd(usageMetricValue(metric, totals, quotaPerUnit))
    case 'tokens':
      return UsageFormat.compact(totals.tokens) + ' tokens'
    case 'requests':
      return UsageFormat.count(totals.requests, 'request', 'requests')
  }
}

/** `UsageScope`: Total · By model · By project. */
export type UsageScope = 'total' | 'model' | 'project'

/** `UsageScope.allCases`. */
export const USAGE_SCOPES: readonly UsageScope[] = ['total', 'model', 'project']

/** `UsageScope.title`. */
export function usageScopeTitle(scope: UsageScope): string {
  switch (scope) {
    case 'total':
      return 'Total'
    case 'model':
      return 'By model'
    case 'project':
      return 'By project'
  }
}

/// `UsageRange`: the Usage card's period, 24h · 7d · 30d · All. "All" is the
/// whole report (`GATEWAY_USAGE_DAYS`) as a heatmap of days; the others are bar
/// charts of their last `usageRangeBucketCount` hours or days, the current one included.
export type UsageRange = 'day' | 'week' | 'month' | 'all'

/** `UsageRange.allCases`. */
export const USAGE_RANGES: readonly UsageRange[] = ['day', 'week', 'month', 'all']

/** `UsageRange.title`. */
export function usageRangeTitle(range: UsageRange): string {
  switch (range) {
    case 'day':
      return '24h'
    case 'week':
      return '7d'
    case 'month':
      return '30d'
    case 'all':
      return 'All'
  }
}

/** `UsageRange.caption`: "Last 24 hours", for captions. */
export function usageRangeCaption(range: UsageRange): string {
  switch (range) {
    case 'day':
      return 'Last 24 hours'
    case 'week':
      return 'Last 7 days'
    case 'month':
      return 'Last 30 days'
    case 'all':
      return 'Last 12 months'
  }
}

/** `UsageRange.bucketCount`: bars in the chart; null for the heatmap. */
export function usageRangeBucketCount(range: UsageRange): number | null {
  switch (range) {
    case 'day':
      return 24
    case 'week':
      return 7
    case 'month':
      return 30
    case 'all':
      return null
  }
}

/** The start of the local hour `time` falls in (`calendar.dateInterval(of: .hour, for:)`). */
export function startOfHour(time: number): number {
  const date = new Date(time)
  date.setMinutes(0, 0, 0)
  return date.getTime()
}

/** `calendar.startOfDay(for:)`: local midnight of `time`'s day. */
export function startOfDay(time: number): number {
  const date = new Date(time)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()
}

/** `calendar.date(byAdding: .day, value: days, to: day)` for a local day start. */
export function addDays(day: number, days: number): number {
  const date = new Date(day)
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days).getTime()
}

/** `UsageRange.bucketStart(of:calendar:)`: the bucket a moment falls in (its local hour or day). */
export function usageRangeBucketStart(range: UsageRange, time: number): number {
  return range === 'day' ? startOfHour(time) : startOfDay(time)
}

/// `UsageRange.buckets(now:calendar:)`: the chart's buckets, oldest first,
/// ending with the one `now` is in; empty for `all`.
export function usageRangeBuckets(range: UsageRange, now: number = Date.now()): number[] {
  const count = usageRangeBucketCount(range)
  if (count === null) return []
  const last = usageRangeBucketStart(range, now)
  const buckets: number[] = []
  for (let back = count - 1; back >= 0; back -= 1) {
    buckets.push(range === 'day' ? last - back * 3_600_000 : addDays(last, -back))
  }
  return buckets
}

/// `UsageSeries`: one chart's data, the totals per bucket (a local day, or an
/// hour for the 24-hour range) plus the sum. Keys are bucket starts in ms.
export interface UsageSeries {
  id: string
  title: string
  buckets: Map<number, GatewayUsageTotals>
  total: GatewayUsageTotals
}

/** `UsageSeries(id:title:)`. */
export function makeUsageSeries(id: string, title: string): UsageSeries {
  return { id, title, buckets: new Map(), total: emptyTotals() }
}

/** `UsageSeries.add(_:bucket:)`. */
export function addToSeries(series: UsageSeries, row: GatewayUsageRow, bucket: number): void {
  let totals = series.buckets.get(bucket)
  if (!totals) {
    totals = emptyTotals()
    series.buckets.set(bucket, totals)
  }
  addRow(totals, row)
  addRow(series.total, row)
}

/** `UsageBreakdown.unattributed`: the series of rows without a project. */
export const UNATTRIBUTED = 'Unattributed'

/// `UsageBreakdown`: the Account page's three views of one usage report over
/// one range. Hours are folded into local buckets, so a day on the chart is a
/// local day.
export interface UsageBreakdown {
  range: UsageRange
  /** The bar chart's buckets, oldest first (empty for the heatmap). */
  bucketStarts: number[]
  total: UsageSeries
  models: UsageSeries[]
  projects: UsageSeries[]
  quotaPerUnit: number
  displayInCurrency: boolean
  /** Rows in the whole report, whatever the range. */
  rowCount: number
}

/** One breakdown per range, as the Usage card's picker switches between them. */
export type UsageByRange = Record<UsageRange, UsageBreakdown>

/** `UsageBreakdown.sorted`: biggest spender first, the unattributed bucket last among equals. */
function sortedSeries(series: Map<string, UsageSeries>): UsageSeries[] {
  return [...series.values()].sort((a, b) => {
    if (a.total.quota !== b.total.quota) return b.total.quota - a.total.quota
    if (a.total.requests !== b.total.requests) return b.total.requests - a.total.requests
    const aUnattributed = a.title === UNATTRIBUTED
    const bUnattributed = b.title === UNATTRIBUTED
    if (aUnattributed !== bUnattributed) return aUnattributed ? 1 : -1
    return a.title.localeCompare(b.title, undefined, { sensitivity: 'accent' })
  })
}

/** `UsageBreakdown(report:range:now:calendar:)`. */
export function makeUsageBreakdown(report: GatewayUsageReport, range: UsageRange = 'all', now: number = Date.now()): UsageBreakdown {
  const bucketStarts = usageRangeBuckets(range, now)
  const first = bucketStarts[0]
  const since = first === undefined ? Number.NEGATIVE_INFINITY : Math.trunc(first / 1000)
  const total = makeUsageSeries('total', 'All usage')
  const models = new Map<string, UsageSeries>()
  const projects = new Map<string, UsageSeries>()
  // Rows are hourly; every hour of one bucket maps to the same bucket, so the
  // lookup is cached per hour stamp.
  const bucketCache = new Map<number, number>()
  for (const row of report.rows) {
    if (row.created_at < since) continue
    let bucket = bucketCache.get(row.created_at)
    if (bucket === undefined) {
      bucket = usageRangeBucketStart(range, row.created_at * 1000)
      bucketCache.set(row.created_at, bucket)
    }
    addToSeries(total, row, bucket)
    const model = row.model_name.length === 0 ? '(unknown model)' : row.model_name
    let modelSeries = models.get(model)
    if (!modelSeries) {
      modelSeries = makeUsageSeries('model:' + model, model)
      models.set(model, modelSeries)
    }
    addToSeries(modelSeries, row, bucket)
    const project = row.project.length === 0 ? UNATTRIBUTED : row.project
    let projectSeries = projects.get(project)
    if (!projectSeries) {
      projectSeries = makeUsageSeries('project:' + project, project)
      projects.set(project, projectSeries)
    }
    addToSeries(projectSeries, row, bucket)
  }
  return {
    range,
    bucketStarts,
    total,
    models: sortedSeries(models),
    projects: sortedSeries(projects),
    quotaPerUnit: report.quota_per_unit,
    displayInCurrency: report.display_in_currency,
    rowCount: report.rows.length,
  }
}

const breakdownCache = new WeakMap<GatewayUsageReport, { hour: number; value: UsageByRange }>()

/// `UsageBreakdown.byRange(report:now:calendar:)`: one breakdown per range.
/// Memoised per report (and per local hour of `now`, so the bar charts still
/// end with the current hour or day), which keeps switching the period instant.
export function usageBreakdownsByRange(report: GatewayUsageReport, now: number = Date.now()): UsageByRange {
  const hour = startOfHour(now)
  const cached = breakdownCache.get(report)
  if (cached && cached.hour === hour) return cached.value
  const value: UsageByRange = {
    day: makeUsageBreakdown(report, 'day', now),
    week: makeUsageBreakdown(report, 'week', now),
    month: makeUsageBreakdown(report, 'month', now),
    all: makeUsageBreakdown(report, 'all', now),
  }
  breakdownCache.set(report, { hour, value })
  return value
}

/** `UsageBreakdown.series(for:)`. */
export function breakdownSeries(usage: UsageBreakdown, scope: UsageScope): UsageSeries[] {
  switch (scope) {
    case 'total':
      return [usage.total]
    case 'model':
      return usage.models
    case 'project':
      return usage.projects
  }
}

/** `UsageBarChart.figures`: "$1.24 · 12.3k tokens · 84 requests" (the charts' captions). */
export function usageFigures(totals: GatewayUsageTotals, quotaPerUnit: number): string {
  return `${UsageFormat.usd(totals.quota / Math.max(quotaPerUnit, 1))} · ${UsageFormat.compact(totals.tokens)} tokens · ${UsageFormat.count(totals.requests, 'request', 'requests')}`
}

// MARK: - Billing (`gateway/billing*`)

/** `GatewayBillingPreference.title`. */
export function billingPreferenceOptionTitle(preference: GatewayBillingPreference): string {
  switch (preference) {
    case 'subscription_first':
      return 'Subscription first, then credits'
    case 'wallet_first':
      return 'Credits first, then subscription'
    case 'subscription_only':
      return 'Subscription only'
    case 'wallet_only':
      return 'Credits only'
  }
}

/** `GatewayBillingPreference.detail`. */
export function billingPreferenceOptionDetail(preference: GatewayBillingPreference): string {
  switch (preference) {
    case 'subscription_first':
      return "Requests use your plan's allowance; once it runs out they are paid from your credit balance."
    case 'wallet_first':
      return "Requests are paid from your credit balance; once it is empty they use your plan's allowance."
    case 'subscription_only':
      return "Requests only use your plan's allowance and fail once it runs out."
    case 'wallet_only':
      return 'Requests are only paid from your credit balance; your plan is not used.'
  }
}

/** `GatewayBilling.preference`. */
export function billingPreference(billing: GatewayBilling): GatewayBillingPreference | null {
  switch (billing.billing_preference) {
    case 'subscription_first':
    case 'wallet_first':
    case 'subscription_only':
    case 'wallet_only':
      return billing.billing_preference
    default:
      return null
  }
}

/** `GatewayTopUpOptions.isUSD`. */
export function topUpIsUSD(topUp: GatewayTopUpOptions): boolean {
  return topUp.amount_unit !== 'quota'
}

/** `GatewayTopUpOptions.label(_:)`: "$20" or "20,000 units". */
export function topUpLabel(topUp: GatewayTopUpOptions, amount: number): string {
  return topUpIsUSD(topUp) ? UsageFormat.usd(amount) : `${formatInteger(amount)} units`
}

/** `GatewayTopUpOptions.discountLabel(_:)`: "5% off"; null when none. */
export function topUpDiscountLabel(topUp: GatewayTopUpOptions, amount: number): string | null {
  const multiplier = topUp.discounts[String(amount)]
  if (multiplier === undefined || !(multiplier > 0) || !(multiplier < 1)) return null
  return `${roundHalfAway((1 - multiplier) * 100)}% off`
}

/** `GatewayPlan.name`. */
export function planName(plan: GatewayPlan): string {
  return plan.title.length === 0 ? `Plan #${plan.id}` : plan.title
}

/** `GatewayPlan.priceTitle`: "$20 / month", "Free". */
export function planPriceTitle(plan: GatewayPlan): string {
  return priceTitle(plan.price_amount, plan.currency, plan.duration_unit, plan.duration_value)
}

/** `GatewayPlan.allowanceTitle(quotaPerUnit:)`: "$10 of usage, resets monthly" / "Unlimited usage". */
export function planAllowanceTitle(plan: GatewayPlan, quotaPerUnit: number): string {
  let reset = ''
  switch (plan.quota_reset_period) {
    case 'daily':
    case 'weekly':
    case 'monthly':
      reset = `, resets ${plan.quota_reset_period}`
      break
    default:
      break
  }
  if (!(plan.total_amount > 0)) return 'Unlimited usage' + reset
  return UsageFormat.usd(plan.total_amount / Math.max(quotaPerUnit, 1)) + ' of usage' + reset
}

// MARK: - Device key settings

/** `GatewayPriceRule.paidPercent`: the share of the standard price the key pays at most, 0–100. */
export function priceRulePaidPercent(rule: GatewayPriceRule): number | null {
  return rule.discount_percent == null ? null : 100 - rule.discount_percent
}

/** `GatewayKeySettings.formatQuota(_:)`: a quota amount as the gateway shows it. */
export function formatKeyQuota(settings: GatewayKeySettings, amount: number): string {
  return settings.display_in_currency ? UsageFormat.usd(amount / Math.max(settings.quota_per_unit, 1)) : UsageFormat.compact(amount)
}
