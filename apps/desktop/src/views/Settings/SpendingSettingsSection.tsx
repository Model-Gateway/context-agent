// Port of apps/macos/ContextAgent/Views/Settings/SpendingSettingsSection.swift.

import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { backend } from '../../backend/client'
import { errorText } from '../../backend/errors'
import {
  gatewayPriceLimitsJSON,
  gatewayPriceRuleIsEmpty,
  spendPeriodPhrase,
  spendPeriodTitle,
  SPEND_PERIODS,
  type GatewayKeySettings,
  type GatewayPriceLimits,
  type GatewayPriceRule,
  type JSONObject,
  type SpendPeriod,
  type SpendSettings,
  type SpendSummary,
} from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { IconButton } from '../../components/IconButton'
import { Skeleton } from '../../components/Skeleton'
import { ModelCatalog } from '../../models/modelCatalog'
import { formatKeyQuota, priceRulePaidPercent, UsageFormat } from '../../models/modelGateway'
import { useApp } from '../../state/app'
import { SettingsNumber } from './BYOKSettingsSection'
import { gatewayMessage, refreshGatewayKeySettings, refreshGatewayModels, updateGatewayKeySettings, useGateway, useGatewayRoutingActive } from '../../state/gateway'
import { FormButtons, SegmentedPicker, TextField, Toggle } from './SettingsControls'
import { BackendUnavailable, InlineMessage, SettingsGroup, SettingsMessage, SettingsPage, SettingsRow } from './SettingsSections'
import { thisDevice, thisDevices } from './settingsPlatform'
import styles from './SpendingSettingsSection.module.css'

/** `localizedStandardCompare`: Finder order (case-insensitive, numbers by value). */
const standardCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

/** A stable key for an Equatable `onChange`. */
function equatableKey(value: unknown): string {
  return value === null || value === undefined ? '' : JSON.stringify(value)
}

function useReady(): boolean {
  return useApp((s) => s.connection.kind === 'ready')
}

let nextDraftId = 0
function draftId(): number {
  nextDraftId += 1
  return nextDraftId
}

// MARK: - Spending

/** `LimitDraft`: one editable per-model limit. */
interface LimitDraft {
  id: number
  model: string
  amount: string
}

function limitNormalized(limit: LimitDraft): string {
  return `${limit.model.trim()}\u0000${limit.amount.trim()}`
}

/** `SpendingSettingsSection.drafts(_:)`: the form values of `[spend]`. */
function spendDrafts(spend: SpendSettings): { total: string; limits: LimitDraft[] } {
  const limits = Object.keys(spend.models)
    .sort((a, b) => standardCollator.compare(a, b))
    .map((model) => ({ id: draftId(), model, amount: SettingsNumber.format(spend.models[model] ?? 0) }))
  return { total: spend.max_usd == null ? '' : SettingsNumber.format(spend.max_usd), limits }
}

interface SpendForm {
  period: SpendPeriod
  total: string
  limits: LimitDraft[]
  synced: SpendSettings | null
}

function spendIsDirty(form: SpendForm): boolean {
  const synced = form.synced
  if (!synced) return false
  const base = spendDrafts(synced)
  return (
    form.period !== synced.period ||
    form.total.trim() !== base.total ||
    form.limits.map(limitNormalized).join('\u0001') !== base.limits.map(limitNormalized).join('\u0001')
  )
}

/**
 * `SpendingSettingsSection` (SpendingSettingsSection.swift): while signed in
 * to ModelGateway, the device key's price limits (the most it pays, as a
 * share of each model's standard price) and spending cap. Below them, this
 * device's spend (`spend/summary`); while requests do not go through
 * ModelGateway, also the local `[spend]` caps per day, week or month, on all
 * models together and per model.
 */
export function SpendingSettingsSection() {
  const config = useApp((s) => s.config)
  const ready = useReady()
  const signedIn = useGateway((s) => s.isSignedIn)
  const routingActive = useGatewayRoutingActive()
  const spend = config?.spend ?? null
  const [form, setForm] = useState<SpendForm>({ period: 'month', total: '', limits: [], synced: null })
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)
  const [summary, setSummary] = useState<SpendSummary | null>(null)
  const [summaryError, setSummaryError] = useState<string | null>(null)

  const sync = useCallback((next: SpendSettings | null, force: boolean) => {
    if (!next) return
    setForm((previous) => {
      const keepEdits = !force && spendIsDirty(previous)
      if (keepEdits) return { ...previous, synced: next }
      const drafts = spendDrafts(next)
      return { period: next.period, total: drafts.total, limits: drafts.limits, synced: next }
    })
    if (force) setMessage(null)
  }, [])

  const loadSummary = useCallback(async () => {
    if (useApp.getState().connection.kind !== 'ready') return
    try {
      setSummary(await backend.spendSummary())
      setSummaryError(null)
    } catch (error) {
      setSummaryError(errorText(error))
    }
  }, [])

  // `.onChange(of: config?.spend, initial: true)` and `.task(id: config?.spend)`.
  const spendKey = equatableKey(spend)
  useEffect(() => {
    sync(spend, false)
    void loadSummary()
  }, [spendKey]) // `spendKey` is the Equatable value of `spend`.

  // `.task(id: appState.gateway.isSignedIn)`.
  useEffect(() => {
    void refreshGatewayKeySettings()
  }, [signedIn])

  const isDirty = spendIsDirty(form)
  const { period } = form

  const save = () => {
    if (saving || !ready) return
    const values: JSONObject = { 'spend.period': form.period }
    const totalText = form.total.trim()
    if (totalText.length === 0) {
      values['spend.max_usd'] = null
    } else {
      const amount = SettingsNumber.usd(totalText)
      if (amount === null) {
        setMessage(SettingsMessage.error('The limit on all models must be a dollar amount of zero or more.'))
        return
      }
      values['spend.max_usd'] = amount
    }
    const models: JSONObject = {}
    for (const limit of form.limits) {
      const model = limit.model.trim()
      const amount = limit.amount.trim()
      if (model.length === 0 && amount.length === 0) continue
      if (model.length === 0) {
        setMessage(SettingsMessage.error('Every model limit needs a model id.'))
        return
      }
      const value = SettingsNumber.usd(amount)
      if (value === null) {
        setMessage(SettingsMessage.error(`The limit on ${model} must be a dollar amount of zero or more.`))
        return
      }
      models[model] = value
    }
    values['spend.models'] = Object.keys(models).length === 0 ? null : models
    setSaving(true)
    setMessage(null)
    void (async () => {
      try {
        await useApp.getState().patchConfig(values)
        setMessage(SettingsMessage.success('Limits saved; they apply from the next request.'))
      } catch (error) {
        setMessage(SettingsMessage.error(errorText(error)))
      } finally {
        setSaving(false)
      }
    })()
  }

  if (!config) {
    return (
      <SettingsPage>
        <BackendUnavailable skeleton={[2, 2, 3]} />
      </SettingsPage>
    )
  }

  /** The configured model, unless it already has a limit. */
  const suggestedModel = form.limits.some((limit) => limit.model.trim() === config.model.name) ? '' : config.model.name
  const periodUnit = spendPeriodPhrase(period).replace('this ', 'per ').replace('today', 'per day')

  const dollarField = (placeholder: string, value: string, onChange: (value: string) => void, label: string) => (
    <span className={styles.dollarField}>
      <span className={styles.symbol}>$</span>
      <TextField placeholder={placeholder} aria-label={label} width={110} value={value} onChange={onChange} onSubmit={save} />
      <span className={styles.caption}>{periodUnit}</span>
    </span>
  )

  const setLimit = (id: number, patch: Partial<LimitDraft>) =>
    setForm((previous) => ({ ...previous, limits: previous.limits.map((limit) => (limit.id === id ? { ...limit, ...patch } : limit)) }))

  return (
    <SettingsPage>
      {signedIn ? (
        <>
          <GatewayPriceLimitGroup />
          <GatewayKeyCapGroup />
          <DiscountDropGroup />
        </>
      ) : (
        <SettingsGroup title="Price limit">
          <InlineMessage
            message={SettingsMessage.info('Sign in to ModelGateway on the Account page to set the most you pay for a model, as a share of its standard price.')}
          />
        </SettingsGroup>
      )}

      <SettingsGroup
        title={`Spent ${spendPeriodPhrase(summary?.period ?? period)}`}
        footer={`Counts every request Context sent from ${thisDevice} (the app, the terminal UI and ctx run), priced at models.dev's list prices; requests through ModelGateway count what the gateway billed. Spend from other devices is on the Account page.`}
      >
        {summary ? (
          <>
            <SpendBar title="All models" spent={summary.total_usd} limit={summary.max_usd ?? null} />
            {summary.models.map((model) => (
              <SpendBar
                key={model.model}
                title={model.model}
                spent={model.cost_usd}
                limit={model.limit_usd ?? null}
                note={model.unpriced_requests > 0 ? `${model.unpriced_requests} request${model.unpriced_requests === 1 ? '' : 's'} without a known price` : null}
                mono
              />
            ))}
            {summary.models.length === 0 ? <span className={styles.nothing}>{`Nothing spent ${spendPeriodPhrase(summary.period)} yet.`}</span> : null}
          </>
        ) : summaryError !== null ? (
          <InlineMessage message={SettingsMessage.error(summaryError)} />
        ) : (
          <Skeleton.SettingsRows count={2} label="Loading spending" />
        )}
        <div className={styles.trailing}>
          <Button size="small" onClick={() => void loadSummary()}>
            Refresh
          </Button>
        </div>
      </SettingsGroup>

      {!routingActive ? (
        <SettingsGroup
          title="Limits"
          footer={`Before each model request the agent checks what was spent ${spendPeriodPhrase(period)} against the limit on its model and the limit on all models; at a limit the task stops and says which one. Leave a field empty for no limit.`}
        >
          <SettingsRow label="Period">
            <SegmentedPicker<SpendPeriod>
              aria-label="Period"
              maxWidth={280}
              value={period}
              options={SPEND_PERIODS.map((value) => ({ value, label: spendPeriodTitle(value) }))}
              onChange={(value) => setForm((previous) => ({ ...previous, period: value }))}
            />
          </SettingsRow>
          <SettingsRow label="All models">
            {dollarField('No limit', form.total, (total) => setForm((previous) => ({ ...previous, total })), 'Limit on all models')}
          </SettingsRow>
          {form.limits.map((limit) => (
            <div key={limit.id} className={styles.limitRow}>
              <TextField placeholder="model id" aria-label="Model id" width={128} value={limit.model} onChange={(model) => setLimit(limit.id, { model })} />
              {dollarField('No limit', limit.amount, (amount) => setLimit(limit.id, { amount }), `Limit on ${limit.model.trim() || 'this model'}`)}
              <IconButton
                icon="close"
                label="Remove this limit"
                size={11}
                onClick={() => setForm((previous) => ({ ...previous, limits: previous.limits.filter((entry) => entry.id !== limit.id) }))}
              />
            </div>
          ))}
          <FormButtons
            onSave={save}
            onRevert={() => sync(useApp.getState().config?.spend ?? null, true)}
            dirty={isDirty}
            saving={saving}
            ready={ready}
            defaultAction
            leading={
              <Button
                size="small"
                icon="addCustom"
                onClick={() => setForm((previous) => ({ ...previous, limits: [...previous.limits, { id: draftId(), model: suggestedModel, amount: '' }] }))}
              >
                Add model limit
              </Button>
            }
          />
          {message ? <InlineMessage message={message} /> : null}
        </SettingsGroup>
      ) : null}
    </SettingsPage>
  )
}

// MARK: - ModelGateway key

/// `RuleDraft`: one editable price rule: the share of the standard price
/// paid at most (shown as paid, stored as `discount_percent` = 100 − paid)
/// and the max dollar rate per million output tokens.
interface RuleDraft {
  id: number
  model: string
  paid: string
  rate: string
}

/** `RuleDraft(model:rule:)`. */
function ruleDraft(model: string, rule?: GatewayPriceRule | null): RuleDraft {
  const paid = rule ? priceRulePaidPercent(rule) : null
  return {
    id: draftId(),
    model,
    paid: paid === null ? '' : SettingsNumber.format(Math.round(paid * 1_000_000) / 1_000_000),
    rate: rule?.max_price_per_m == null ? '' : SettingsNumber.format(rule.max_price_per_m),
  }
}

/** `RuleDraft.isBlank`. */
function ruleIsBlank(row: RuleDraft): boolean {
  return row.paid.trim().length === 0 && row.rate.trim().length === 0
}

/** `RuleDraft.normalized`: trimmed fields, without the row id, for dirty checks. */
function ruleNormalized(row: RuleDraft): string {
  return `${row.model.trim()}\u0000${row.paid.trim()}\u0000${row.rate.trim()}`
}

/** `RuleDraft.rule(for:)`: the price rule, or the problem to show. */
function ruleFor(row: RuleDraft, name: string): { rule: GatewayPriceRule } | { problem: string } {
  const rule: GatewayPriceRule = {}
  let share = row.paid.trim()
  if (share.endsWith('%')) share = share.slice(0, -1)
  share = share.trim()
  if (share.length > 0) {
    const value = /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(share) ? Number(share) : Number.NaN
    if (!Number.isFinite(value) || !(value > 0) || !(value <= 100)) return { problem: `The share ${name} pays must be a percentage above 0 and at most 100.` }
    rule.discount_percent = Math.round((100 - value) * 1_000_000) / 1_000_000
  }
  if (row.rate.trim().length > 0) {
    const value = SettingsNumber.usd(row.rate)
    if (value === null || !(value > 0)) return { problem: `The max rate of ${name} must be a dollar amount above zero.` }
    rule.max_price_per_m = value
  }
  return { rule }
}

function sortedRules(rows: RuleDraft[]): RuleDraft[] {
  return [...rows].sort((a, b) => standardCollator.compare(a.model, b.model))
}

/** `GatewayPriceLimitGroup.drafts(_:models:)`: one row per model with a saved rule or served by the gateway. */
function priceDrafts(limits: GatewayPriceLimits, models: string[]): { all: RuleDraft; rows: RuleDraft[] } {
  const names = new Set([...Object.keys(limits.models), ...models])
  const rows = sortedRules([...names].map((name) => ruleDraft(name, limits.models[name])))
  return { all: ruleDraft('', limits.default), rows }
}

/** `GatewayPriceLimitGroup.limited(_:)`: the rows that carry a limit, for dirty checks. */
function limitedKey(rows: RuleDraft[]): string {
  return rows
    .filter((row) => !ruleIsBlank(row))
    .map(ruleNormalized)
    .join('\u0001')
}

interface PriceForm {
  all: RuleDraft
  rows: RuleDraft[]
  synced: GatewayPriceLimits | null
}

function priceIsDirty(form: PriceForm): boolean {
  if (!form.synced) return false
  const base = priceDrafts(form.synced, [])
  return ruleNormalized(form.all) !== ruleNormalized(base.all) || limitedKey(form.rows) !== limitedKey(base.rows)
}

/** `RuleDraftRow.percentWidth` / `rateWidth`. */
const PERCENT_WIDTH = 96
const RATE_WIDTH = 120

/**
 * `GatewayPriceLimitGroup`: `price_limits` of the device key, for all models
 * and per model (a model's rule replaces the default). Every model
 * ModelGateway serves has a row already; an empty one uses the All models
 * row. Collapsed, only the models with a limit show.
 */
function GatewayPriceLimitGroup() {
  const ready = useReady()
  const routingActive = useGatewayRoutingActive()
  const keySettings = useGateway((s) => s.keySettings)
  const gatewayModels = useGateway((s) => s.models)
  const priceLimits = keySettings?.price_limits ?? null
  const [form, setForm] = useState<PriceForm>(() => ({ all: ruleDraft(''), rows: [], synced: null }))
  const [showAll, setShowAll] = useState(false)
  const [filter, setFilter] = useState('')
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  const sync = useCallback((limits: GatewayPriceLimits | null, force: boolean) => {
    if (!limits) return
    setForm((previous) => {
      const keepEdits = !force && priceIsDirty(previous)
      if (keepEdits) return { ...previous, synced: limits }
      const drafts = priceDrafts(limits, useGateway.getState().models)
      return { all: drafts.all, rows: drafts.rows, synced: limits }
    })
    if (force) setMessage(null)
  }, [])

  // `.onChange(of: keySettings?.priceLimits, initial: true)`.
  const limitsKey = equatableKey(priceLimits)
  useEffect(() => {
    sync(priceLimits, false)
  }, [limitsKey])

  // `.onChange(of: appState.gateway.models)`: a row for every model the gateway serves.
  useEffect(() => {
    setForm((previous) => {
      const listed = new Set(previous.rows.map((row) => row.model))
      const added = gatewayModels.filter((model) => !listed.has(model)).map((model) => ruleDraft(model))
      if (added.length === 0) return previous
      return { ...previous, rows: sortedRules([...previous.rows, ...added]) }
    })
  }, [gatewayModels])

  // `.task(id: appState.gatewayRoutingActive)`.
  useEffect(() => {
    void refreshGatewayModels()
  }, [routingActive])

  const editRow = useCallback((id: number, field: 'paid' | 'rate', value: string) => {
    setForm((previous) =>
      previous.all.id === id
        ? { ...previous, all: { ...previous.all, [field]: value } }
        : { ...previous, rows: previous.rows.map((row) => (row.id === id ? { ...row, [field]: value } : row)) },
    )
  }, [])

  const clearRow = useCallback((id: number) => {
    setForm((previous) => ({ ...previous, rows: previous.rows.map((row) => (row.id === id ? { ...row, paid: '', rate: '' } : row)) }))
  }, [])

  const isDirty = priceIsDirty(form)
  const query = filter.trim().toLocaleLowerCase()
  const synced = form.synced
  /// Expanded: every model matching the filter. Collapsed: the models with a
  /// saved or edited limit.
  const shown = useMemo(
    () =>
      form.rows.filter((row) => {
        if (showAll) return query.length === 0 || row.model.toLocaleLowerCase().includes(query)
        return !ruleIsBlank(row) || (synced !== null && synced.models[row.model] !== undefined)
      }),
    [form.rows, showAll, query, synced],
  )

  const save = () => {
    if (saving || !ready) return
    const limits: GatewayPriceLimits = { default: null, models: {} }
    const all = ruleFor(form.all, 'All models')
    if ('problem' in all) {
      setMessage(SettingsMessage.error(all.problem))
      return
    }
    limits.default = gatewayPriceRuleIsEmpty(all.rule) ? null : all.rule
    for (const row of form.rows) {
      if (ruleIsBlank(row)) continue
      const result = ruleFor(row, row.model)
      if ('problem' in result) {
        setMessage(SettingsMessage.error(result.problem))
        return
      }
      limits.models[row.model] = result.rule
    }
    setSaving(true)
    setMessage(null)
    void (async () => {
      try {
        await updateGatewayKeySettings({ price_limits: gatewayPriceLimitsJSON(limits) })
        setMessage(SettingsMessage.success('Saved on your ModelGateway key; it applies from the next request.'))
      } catch (error) {
        setMessage(SettingsMessage.error(gatewayMessage(error)))
      } finally {
        setSaving(false)
      }
    })()
  }

  return (
    <SettingsGroup
      title="Price limit"
      footer={`Requests through ModelGateway only go to providers charging at most this share of the model's standard output price, and at most the max rate when one is set. A model's own limit replaces the All models row; a model left empty uses it. When no provider is under the limit, the request fails instead of costing more. Saved on ${thisDevices} ModelGateway key; leave All models empty for no limit.`}
    >
      {keySettings === null ? (
        <KeySettingsStatus />
      ) : (
        <>
          {!routingActive ? (
            <InlineMessage message={SettingsMessage.info('Routing through ModelGateway is off on the BYOK tab; these limits apply once it is on.')} />
          ) : null}
          <div className={styles.rules}>
            <div className={styles.ruleHeader} aria-hidden="true">
              <span className={styles.ruleModel}>Model</span>
              <span style={{ width: PERCENT_WIDTH }}>Pay at most</span>
              <span style={{ width: RATE_WIDTH }}>Max per 1M output</span>
              <span className={styles.ruleRemove} />
            </div>
            <RuleDraftRow row={form.all} label="All models" inherited={null} onEdit={editRow} onClear={null} />
            {showAll && form.rows.length > ModelCatalog.groupingThreshold ? (
              <div className={styles.filter}>
                <TextField placeholder="Filter models" value={filter} onChange={setFilter} />
              </div>
            ) : null}
            {shown.map((row) => (
              <RuleDraftRow key={row.id} row={row} label={row.model} inherited={form.all} onEdit={editRow} onClear={ruleIsBlank(row) ? null : clearRow} />
            ))}
          </div>
          <FormButtons
            onSave={save}
            onRevert={() => sync(useGateway.getState().keySettings?.price_limits ?? null, true)}
            dirty={isDirty}
            saving={saving}
            ready={ready}
            defaultAction
            leading={
              gatewayModels.length > 0 ? (
                <Button
                  size="small"
                  icon={showAll ? 'chevronUp' : 'chevronDown'}
                  onClick={() => {
                    setShowAll((value) => !value)
                    setFilter('')
                  }}
                >
                  {showAll ? 'Show models with a limit' : `Show all ${form.rows.length} models`}
                </Button>
              ) : null
            }
          />
          {message ? <InlineMessage message={message} /> : null}
        </>
      )}
    </SettingsGroup>
  )
}

interface RuleDraftRowProps {
  row: RuleDraft
  /** `fixedLabel`: "All models" or the model id. */
  label: string
  /** The All models row a model row falls back to, shown as placeholders. */
  inherited: RuleDraft | null
  onEdit: (id: number, field: 'paid' | 'rate', value: string) => void
  /** Clears a model's limit; null shows an empty slot. */
  onClear: ((id: number) => void) | null
}

/** `RuleDraftRow`: the model, "Pay at most" %, "Max per 1M output" $ and the clear button. */
const RuleDraftRow = memo(function RuleDraftRow({ row, label, inherited, onEdit, onClear }: RuleDraftRowProps) {
  const placeholder = (value: string | undefined, fallback: string) => {
    const trimmed = value?.trim() ?? ''
    return trimmed.length === 0 ? fallback : trimmed
  }
  return (
    <div className={styles.ruleRow}>
      <span className={`${styles.ruleModel} ${styles.ruleLabel} ${inherited === null ? styles.ruleLabelAll : styles.ruleLabelModel}`}>{label}</span>
      <span className={styles.ruleCell} style={{ width: PERCENT_WIDTH }}>
        <TextField
          placeholder={placeholder(inherited?.paid, '100')}
          aria-label={`${label}: pay at most, percent`}
          width={64}
          value={row.paid}
          onChange={(value) => onEdit(row.id, 'paid', value)}
        />
        <span className={styles.symbol}>%</span>
      </span>
      <span className={styles.ruleCell} style={{ width: RATE_WIDTH }}>
        <span className={styles.symbol}>$</span>
        <TextField
          placeholder={placeholder(inherited?.rate, 'No max')}
          aria-label={`${label}: max per 1M output`}
          width={80}
          value={row.rate}
          onChange={(value) => onEdit(row.id, 'rate', value)}
        />
      </span>
      <span className={styles.ruleRemove}>
        {onClear ? <IconButton icon="close" label="Clear this model's limit" size={11} onClick={() => onClear(row.id)} /> : null}
      </span>
    </div>
  )
})

/// `DiscountDropGroup`: `spend.confirm_discount_drop`, whether a run asks
/// before ModelGateway falls back to a provider discounted at least 10
/// points below the model's best offer; saved on change.
function DiscountDropGroup() {
  const ready = useReady()
  const confirm = useApp((s) => s.config?.spend.confirm_discount_drop ?? true)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  const save = async (value: boolean) => {
    setSaving(true)
    setMessage(null)
    try {
      await useApp.getState().patchConfig({ 'spend.confirm_discount_drop': value })
    } catch (error) {
      setMessage(SettingsMessage.error(errorText(error)))
    } finally {
      setSaving(false)
    }
  }

  return (
    <SettingsGroup
      title="Lower discounts"
      footer="When the providers offering a model's best discount are unavailable, ModelGateway can fall back to providers discounted at least 10 points less (98% off becoming 88% off or less). With this on, the run pauses and asks whether to cancel, continue, or always continue."
    >
      <SettingsRow label="Ask before a much lower discount" alignment="center">
        <span className={styles.hstack8}>
          {saving ? <BrailleSpinner /> : null}
          <Toggle label="Ask before a much lower discount" labelHidden checked={confirm} onChange={(value) => void save(value)} disabled={saving || !ready} />
        </span>
      </SettingsRow>
      {message ? <InlineMessage message={message} /> : null}
    </SettingsGroup>
  )
}

/** `GatewayKeyCapGroup.remainingText(_:)`. */
function remainingText(key: GatewayKeySettings): string {
  return key.display_in_currency
    ? SettingsNumber.format(Math.round((key.remain_quota / Math.max(key.quota_per_unit, 1)) * 100) / 100)
    : String(key.remain_quota)
}

interface CapForm {
  capped: boolean
  remaining: string
  synced: GatewayKeySettings | null
}

function capIsDirty(form: CapForm): boolean {
  const synced = form.synced
  if (!synced) return false
  if (form.capped === synced.unlimited_quota) return true
  return form.capped && form.remaining.trim() !== remainingText(synced)
}

/// `GatewayKeyCapGroup`: `unlimited_quota` / `remain_quota` of the device
/// key: what it may still spend before requests through ModelGateway stop.
function GatewayKeyCapGroup() {
  const ready = useReady()
  const key = useGateway((s) => s.keySettings)
  const [form, setForm] = useState<CapForm>({ capped: false, remaining: '', synced: null })
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  const sync = useCallback((next: GatewayKeySettings | null, force: boolean) => {
    if (!next) return
    setForm((previous) => {
      const keepEdits = !force && capIsDirty(previous)
      return keepEdits ? { ...previous, synced: next } : { capped: !next.unlimited_quota, remaining: remainingText(next), synced: next }
    })
    if (force) setMessage(null)
  }, [])

  // `.onChange(of: appState.gateway.keySettings, initial: true)`.
  const keyKey = equatableKey(key)
  useEffect(() => {
    sync(key, false)
  }, [keyKey])

  const isDirty = capIsDirty(form)

  const save = () => {
    const synced = form.synced
    if (!synced || saving || !ready) return
    const values: JSONObject = { unlimited_quota: !form.capped }
    if (form.capped) {
      const amount = SettingsNumber.usd(form.remaining)
      if (amount === null) {
        setMessage(
          SettingsMessage.error(synced.display_in_currency ? 'The remaining amount must be a dollar amount of zero or more.' : 'The remaining amount must be a number of zero or more.'),
        )
        return
      }
      values.remain_quota = synced.display_in_currency ? Math.round(amount * synced.quota_per_unit) : Math.round(amount)
    }
    const capped = form.capped
    setSaving(true)
    setMessage(null)
    void (async () => {
      try {
        await updateGatewayKeySettings(values)
        setMessage(SettingsMessage.success(capped ? 'Cap saved on your ModelGateway key.' : 'This key is no longer capped.'))
      } catch (error) {
        setMessage(SettingsMessage.error(gatewayMessage(error)))
      } finally {
        setSaving(false)
      }
    })()
  }

  return (
    <SettingsGroup
      title="Spending cap"
      footer={`What ${thisDevices} ModelGateway key may still spend. Each request through ModelGateway lowers it, and at zero they stop until you raise it. Your credit balance and plan apply as well.`}
    >
      {key ? (
        <>
          {key.exhausted ? <InlineMessage message={SettingsMessage.error('This key reached its cap: requests through ModelGateway fail until you raise it.')} /> : null}
          <SettingsRow label="Cap this key" alignment="center">
            <Toggle label="Cap this key" labelHidden checked={form.capped} onChange={(capped) => setForm((previous) => ({ ...previous, capped }))} />
          </SettingsRow>
          {form.capped ? (
            <SettingsRow label="Remaining">
              <span className={styles.dollarField}>
                {key.display_in_currency ? <span className={styles.symbol}>$</span> : null}
                <TextField
                  placeholder={key.display_in_currency ? '10.00' : '5000000'}
                  aria-label="Remaining"
                  width={110}
                  value={form.remaining}
                  onChange={(remaining) => setForm((previous) => ({ ...previous, remaining }))}
                  onSubmit={save}
                />
                {!key.display_in_currency ? <span className={styles.caption}>quota units</span> : null}
              </span>
            </SettingsRow>
          ) : null}
          <SettingsRow label="Spent with this key">
            <span className={styles.spent}>{formatKeyQuota(key, key.used_quota)}</span>
          </SettingsRow>
          <FormButtons onSave={save} onRevert={() => sync(useGateway.getState().keySettings, true)} dirty={isDirty} saving={saving} ready={ready} />
          {message ? <InlineMessage message={message} /> : null}
        </>
      ) : (
        <KeySettingsStatus />
      )}
    </SettingsGroup>
  )
}

/** `KeySettingsStatus`: loading, or why the key settings could not be loaded. */
function KeySettingsStatus() {
  const error = useGateway((s) => s.keySettingsError)
  if (error !== null) {
    return (
      <div className={styles.keyStatus}>
        <InlineMessage message={SettingsMessage.error(error)} />
        <Button size="small" onClick={() => void refreshGatewayKeySettings()}>
          Retry
        </Button>
      </div>
    )
  }
  return <Skeleton.SettingsRows count={2} label="Loading your ModelGateway key" />
}

interface SpendBarProps {
  title: string
  spent: number
  limit: number | null
  note?: string | null
  mono?: boolean
}

/** `SpendBar`: "gpt-4.1   $4.20 of $10.00" with a bar filled to the share used. */
function SpendBar({ title, spent, limit, note = null, mono = false }: SpendBarProps) {
  const fraction = limit !== null && limit > 0 ? Math.min(1, spent / limit) : 0
  const reached = limit !== null && spent >= limit
  const amount = limit === null ? UsageFormat.usd(spent) : `${UsageFormat.usd(spent)} of ${UsageFormat.usd(limit)}`
  const fill = reached ? 'var(--danger)' : fraction > 0.8 ? 'var(--warning)' : 'var(--accent)'
  return (
    <div className={styles.spendBar} role="group" aria-label={`${title}, ${amount}${note ? `, ${note}` : ''}`}>
      <div className={styles.spendLine}>
        <span className={`${styles.spendTitle} ${mono ? styles.mono : ''}`}>{title}</span>
        <span className={styles.spendSpacer} />
        <span className={`${styles.spendAmount} ${reached ? styles.reached : ''}`}>{amount}</span>
      </div>
      {limit !== null && limit > 0 ? (
        <div className={styles.track} aria-hidden="true">
          <div className={styles.fill} style={{ width: `${fraction * 100}%`, background: fill }} />
        </div>
      ) : null}
      {note ? <span className={styles.caption}>{note}</span> : null}
    </div>
  )
}
