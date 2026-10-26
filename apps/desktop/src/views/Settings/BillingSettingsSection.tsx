// Port of apps/macos/ContextAgent/Views/Settings/BillingSettingsSection.swift.

import { useEffect, useState } from 'react'
import {
  GATEWAY_BILLING_PREFERENCES,
  parseGatewayBillingPreference,
  type GatewayAccount,
  type GatewayPlan,
  type GatewaySubscription,
  type GatewayTopUpOptions,
  type GatewayTopUpQuote,
} from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { confirm } from '../../components/dialogs'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { Skeleton } from '../../components/Skeleton'
import {
  billingPreference,
  billingPreferenceOptionDetail,
  billingPreferenceOptionTitle,
  formatQuota,
  planAllowanceTitle,
  planName,
  planPriceTitle,
  subscriptionEndDate,
  subscriptionIsUnlimited,
  subscriptionName,
  subscriptionNextResetDate,
  subscriptionPriceTitle,
  subscriptionRemaining,
  topUpDiscountLabel,
  topUpIsUSD,
  topUpLabel,
} from '../../models/modelGateway'
import { useApp } from '../../state/app'
import { onAppBecameActive } from '../../state/notifications'
import {
  addGatewayFunds,
  gatewayMessage,
  openGatewayWallet,
  quoteGatewayTopUp,
  refreshGateway,
  refreshGatewayBilling,
  setGatewayBillingPreference,
  subscribeToGatewayPlan,
  useGateway,
} from '../../state/gateway'
import { RadioGroup, TextField } from './SettingsControls'
import { BackendUnavailable, InlineMessage, SettingsGroup, SettingsMessage, SettingsPage, SettingsRow } from './SettingsSections'
import styles from './BillingSettingsSection.module.css'

/// `refresh()`: the account summary, then the billing options (both skip
/// themselves while signed out).
async function refresh(): Promise<void> {
  await refreshGateway()
  await refreshGatewayBilling()
}

/** `Date.formatted(date: .abbreviated, time: .omitted)`: "Oct 25, 2026". */
const abbreviatedDate = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

/** `Double.formatted(.currency(code:))`. */
function formatCurrency(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(amount)
  } catch {
    return `${currency} ${amount.toFixed(2)}`
  }
}

/**
 * `BillingSettingsSection` (BillingSettingsSection.swift): the ModelGateway
 * account's money (shown only while signed in): the credit balance and plan,
 * adding funds, buying a plan, and whether requests use the plan or the
 * credits first. Payments happen on the gateway's checkout page in the
 * browser; the balance here refreshes when the app comes back to the front.
 */
export function BillingSettingsSection() {
  const signedIn = useGateway((s) => s.isSignedIn)
  const ready = useApp((s) => s.connection.kind === 'ready')
  const billing = useGateway((s) => s.billing)
  const billingError = useGateway((s) => s.billingError)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  // `.task(id: gateway.isSignedIn)`.
  useEffect(() => {
    void refresh()
  }, [signedIn])

  // `NSApplication.didBecomeActiveNotification`: back from the checkout page.
  useEffect(() => onAppBecameActive(() => void refresh()), [])

  /** `subscribe(_:fromBalance:)`. */
  const subscribe = async (plan: GatewayPlan, fromBalance: boolean) => {
    setMessage(null)
    try {
      const active = await subscribeToGatewayPlan(plan, fromBalance)
      setMessage(
        active
          ? SettingsMessage.success(`${planName(plan)} is active.`)
          : SettingsMessage.info(`Finish paying for ${planName(plan)} in your browser; the plan shows here once the payment completes.`),
      )
    } catch (error) {
      setMessage(SettingsMessage.error(gatewayMessage(error)))
    }
  }

  /** The `.confirmationDialog` of "Pay from balance". */
  const confirmBalancePlan = async (plan: GatewayPlan) => {
    const price = planPriceTitle(plan)
    const confirmed = await confirm({
      title: `Buy ${planName(plan)} from your balance?`,
      message: `${price} is taken from your credit balance now and the plan starts at once.`,
      confirmLabel: `Pay ${price} from balance`,
      cancelLabel: 'Cancel',
    })
    if (confirmed) await subscribe(plan, true)
  }

  if (!signedIn) {
    return (
      <SettingsPage>
        <div className={styles.placeholder}>
          <EmptyPlaceholder icon="gateway" title="Not signed in to ModelGateway" subtitle="Sign in on the Account page to manage credits and plans." />
        </div>
      </SettingsPage>
    )
  }
  if (!ready) {
    return (
      <SettingsPage>
        <BackendUnavailable skeleton={[2, 3, 2]} />
      </SettingsPage>
    )
  }
  return (
    <SettingsPage>
      <BalanceGroup />
      {message ? <InlineMessage message={message} /> : null}
      {billing === null && billingError === null ? (
        // Add funds, plans and the billing preference all come with `gateway/billing`.
        <Skeleton.SettingsPage rows={[2, 3]} label="Loading billing" />
      ) : (
        <>
          <AddFundsGroup setMessage={setMessage} />
          <PlansGroup onSubscribe={(plan) => void subscribe(plan, false)} onPayFromBalance={(plan) => void confirmBalancePlan(plan)} />
          <PreferenceGroup setMessage={setMessage} />
        </>
      )}
    </SettingsPage>
  )
}

// MARK: Balance

/** `planDetail(_:account:)`: "$4.20 of $10.00 left · resets Nov 1, 2026 · ends Dec 1, 2026". */
function planDetail(plan: GatewaySubscription, account: GatewayAccount): string {
  const parts: string[] = []
  parts.push(
    subscriptionIsUnlimited(plan)
      ? 'Unlimited usage'
      : `${formatQuota(account, subscriptionRemaining(plan))} of ${formatQuota(account, plan.amount_total)} left`,
  )
  const reset = subscriptionNextResetDate(plan)
  if (reset !== null) parts.push(`resets ${abbreviatedDate.format(reset)}`)
  const end = subscriptionEndDate(plan)
  if (end !== null) parts.push(`ends ${abbreviatedDate.format(end)}`)
  return parts.join(' · ')
}

function BalanceGroup() {
  const account = useGateway((s) => s.account)
  const accountError = useGateway((s) => s.accountError)
  const refreshing = useGateway((s) => s.refreshing)
  const billingLoading = useGateway((s) => s.billingLoading)
  const billingError = useGateway((s) => s.billingError)
  const plan = account?.subscriptions?.[0] ?? null
  const price = plan ? subscriptionPriceTitle(plan) : null
  const error = billingError ?? accountError
  return (
    <SettingsGroup title="ModelGateway">
      {account === null && accountError === null ? <Skeleton.SettingsRows count={2} label="Loading your balance" /> : null}
      {account !== null ? (
        <>
          <SettingsRow label="Credits">
            <span className={styles.credits}>
              <span className={`${styles.balance} ${account.quota > 0 ? '' : styles.low}`}>{formatQuota(account, account.quota)}</span>
              <span className={styles.caption}>{`${formatQuota(account, account.used_quota)} used in total`}</span>
            </span>
          </SettingsRow>
          <SettingsRow label="Plan" alignment="top">
            {plan ? (
              <span className={styles.planStack}>
                <span className={styles.planLine}>
                  <span className={styles.planName}>{subscriptionName(plan)}</span>
                  {price !== null ? <Chip>{price}</Chip> : null}
                </span>
                <span className={styles.caption}>{planDetail(plan, account)}</span>
              </span>
            ) : (
              <span className={styles.payAsYouGo}>Pay as you go from your credits</span>
            )}
          </SettingsRow>
        </>
      ) : null}
      <div className={styles.actions}>
        {refreshing || billingLoading ? <BrailleSpinner size={11} /> : null}
        {error !== null ? <span className={styles.error}>{error}</span> : null}
        <span className={styles.spacer} />
        <Button size="small" onClick={() => openGatewayWallet()}>
          Open wallet
        </Button>
        <Button size="small" disabled={refreshing || billingLoading} onClick={() => void refresh()}>
          Refresh
        </Button>
      </div>
    </SettingsGroup>
  )
}

// MARK: Plans

function PlansGroup({ onSubscribe, onPayFromBalance }: { onSubscribe: (plan: GatewayPlan) => void; onPayFromBalance: (plan: GatewayPlan) => void }) {
  const billing = useGateway((s) => s.billing)
  const billingBusy = useGateway((s) => s.billingBusy)
  if (!billing || billing.plans.length === 0) return null
  const disabled = billing.has_active_subscription || billingBusy
  return (
    <SettingsGroup
      title="Plans"
      footer={
        billing.has_active_subscription
          ? 'You already have an active plan; another can be bought once it ends.'
          : "Card payments open ModelGateway's checkout in your browser; the plan starts when the payment completes."
      }
    >
      {billing.plans.map((plan) => (
        <div key={plan.id} className={styles.plan}>
          <div className={styles.planText}>
            <span className={styles.planLine}>
              <span className={styles.planTitle}>{planName(plan)}</span>
              <Chip>{planPriceTitle(plan)}</Chip>
            </span>
            <span className={styles.captionSecondary}>{planAllowanceTitle(plan, billing.quota_per_unit)}</span>
            {plan.subtitle.length > 0 ? <span className={styles.caption}>{plan.subtitle}</span> : null}
          </div>
          <span className={styles.planSpacer} />
          <div className={styles.planButtons}>
            {plan.stripe ? (
              <Button size="small" disabled={disabled} onClick={() => onSubscribe(plan)}>
                Subscribe…
              </Button>
            ) : null}
            {plan.balance ? (
              <Button size="small" disabled={disabled} onClick={() => onPayFromBalance(plan)}>
                Pay from balance
              </Button>
            ) : null}
            {!plan.stripe && !plan.balance ? (
              <Button size="small" disabled={disabled} onClick={() => openGatewayWallet()}>
                On the website…
              </Button>
            ) : null}
          </div>
        </div>
      ))}
    </SettingsGroup>
  )
}

// MARK: Preference

function PreferenceGroup({ setMessage }: { setMessage: (message: SettingsMessage | null) => void }) {
  const billing = useGateway((s) => s.billing)
  const billingBusy = useGateway((s) => s.billingBusy)
  if (!billing) return null
  const current = billingPreference(billing)
  const select = (value: string) => {
    const preference = parseGatewayBillingPreference(value)
    if (preference === null || value === billing.billing_preference) return
    setMessage(null)
    void (async () => {
      try {
        await setGatewayBillingPreference(preference)
        setMessage(SettingsMessage.success(`Requests now use: ${billingPreferenceOptionTitle(preference).toLowerCase()}.`))
      } catch (error) {
        setMessage(SettingsMessage.error(gatewayMessage(error)))
      }
    })()
  }
  return (
    <SettingsGroup title="Use credits or subscription first" footer="Saved to your ModelGateway account, so it applies to every device and API key.">
      <RadioGroup
        aria-label="Use credits or subscription first"
        value={billing.billing_preference}
        options={GATEWAY_BILLING_PREFERENCES.map((preference) => ({ value: preference, label: billingPreferenceOptionTitle(preference) }))}
        onChange={select}
        disabled={billingBusy}
      />
      {current !== null ? <span className={styles.captionSecondary}>{billingPreferenceOptionDetail(current)}</span> : null}
    </SettingsGroup>
  )
}

// MARK: - Add funds

/** `AddFundsGroup.options(_:)`: the gateway's amounts (at most six), else 10 · 25 · 50 · 100 raised to the minimum. */
function topUpOptions(topUp: GatewayTopUpOptions): number[] {
  const listed = topUp.amount_options.filter((amount) => amount >= topUp.min_amount && amount <= topUp.max_amount)
  if (listed.length > 0) return listed.slice(0, 6)
  const out: number[] = []
  for (const amount of [10, 25, 50, 100].map((value) => Math.max(value, topUp.min_amount))) {
    if (!out.includes(amount)) out.push(amount)
  }
  return out
}

/** `Int(_:)` of the custom amount: whole numbers only. */
function parseWhole(text: string): number | null {
  const cleaned = text.trim().replace(/\$/g, '')
  if (!/^[+-]?\d+$/.test(cleaned)) return null
  const value = Number(cleaned)
  return Number.isSafeInteger(value) ? value : null
}

function AddFundsGroup({ setMessage }: { setMessage: (message: SettingsMessage | null) => void }) {
  const topUp = useGateway((s) => s.billing?.topup ?? null)
  const billingBusy = useGateway((s) => s.billingBusy)
  const [amount, setAmount] = useState<number | null>(null)
  const [custom, setCustom] = useState('')
  const [quote, setQuote] = useState<GatewayTopUpQuote | null>(null)

  /** `validAmount(_:)`: the chosen or typed amount when it is within the limits. */
  const valid = (() => {
    if (!topUp) return null
    const value = custom.length === 0 ? amount : parseWhole(custom)
    if (value === null || value < topUp.min_amount || value > topUp.max_amount) return null
    return value
  })()

  // `.task(id: validAmount(topUp))`: the price, 300 ms after the amount settles.
  useEffect(() => {
    if (valid === null) {
      setQuote(null)
      return
    }
    let live = true
    const timer = window.setTimeout(() => {
      void quoteGatewayTopUp(valid).then((next) => {
        if (live) setQuote(next)
      })
    }, 300)
    return () => {
      live = false
      window.clearTimeout(timer)
    }
  }, [valid])

  if (!topUp) return null
  const selected = custom.length === 0 ? amount : null

  const addFunds = async () => {
    if (valid === null) return
    setMessage(null)
    try {
      await addGatewayFunds(valid)
      setMessage(SettingsMessage.info('Finish paying in your browser; your credits update here once the payment completes.'))
    } catch (error) {
      setMessage(SettingsMessage.error(`ModelGateway could not start the checkout: ${gatewayMessage(error)} You can still add funds with Open wallet.`))
    }
  }

  return (
    <SettingsGroup
      title="Add funds"
      footer={topUp.in_app ? "Opens ModelGateway's checkout in your browser; the credits arrive when the payment completes." : "Adding funds happens on ModelGateway's wallet page."}
    >
      {!topUp.enabled ? (
        <InlineMessage message={SettingsMessage.info('ModelGateway does not take payments right now. Redemption codes can still be used on the wallet page.')} />
      ) : topUp.in_app ? (
        <>
          <div className={styles.amounts}>
            {topUpOptions(topUp).map((option) => {
              const discount = topUpDiscountLabel(topUp, option)
              return (
                <button
                  key={option}
                  type="button"
                  aria-pressed={selected === option}
                  className={[styles.amount, selected === option ? styles.amountSelected : ''].filter(Boolean).join(' ')}
                  onClick={() => {
                    setAmount(option)
                    setCustom('')
                  }}
                >
                  <span>{topUpLabel(topUp, option)}</span>
                  {discount !== null ? <span className={styles.discount}>{discount}</span> : null}
                </button>
              )
            })}
          </div>
          <SettingsRow label="Other amount">
            <span className={styles.otherAmount}>
              {topUpIsUSD(topUp) ? <span className={styles.symbol}>$</span> : null}
              <TextField
                placeholder={String(topUp.min_amount)}
                aria-label="Other amount"
                width={100}
                value={custom}
                onChange={(value) => {
                  setCustom(value)
                  setAmount(null)
                }}
              />
              <span className={styles.caption}>{`${topUpLabel(topUp, topUp.min_amount)} to ${topUpLabel(topUp, topUp.max_amount)}`}</span>
            </span>
          </SettingsRow>
          <div className={styles.addRow}>
            {/* `.keyboardShortcut(.defaultAction)`: accent while enabled; a disabled default button loses its accent, like AppKit's. */}
            <Button size="small" variant={valid === null || billingBusy ? 'bordered' : 'prominent'} disabled={valid === null || billingBusy} onClick={() => void addFunds()}>
              Add funds…
            </Button>
            {billingBusy ? <BrailleSpinner /> : null}
            {quote !== null && valid !== null && Math.trunc(quote.amount) === valid ? (
              <span className={styles.captionSecondary}>{`You pay ${formatCurrency(quote.pay_money, quote.currency ?? 'USD')}`}</span>
            ) : null}
          </div>
        </>
      ) : (
        <Button size="small" onClick={() => openGatewayWallet()}>
          Add funds on the website…
        </Button>
      )}
    </SettingsGroup>
  )
}
