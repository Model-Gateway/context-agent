import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import type { GatewayAccount, GatewayContextCredit, GatewaySubscription } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { CopyButton } from '../../components/CopyButton'
import { IconButton } from '../../components/IconButton'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { SegmentedControl } from '../../components/SegmentedControl'
import { Switch } from '../../components/Switch'
import { Tooltip } from '../../components/Tooltip'
import {
  balanceUSD,
  billingPreferenceTitle,
  breakdownSeries,
  formatInteger,
  formatQuota,
  subscriptionDaysLeft,
  subscriptionEndDate,
  subscriptionIsUnlimited,
  subscriptionName,
  subscriptionNextResetDate,
  subscriptionPriceTitle,
  subscriptionRemaining,
  subscriptionResetTitle,
  subscriptionUsedFraction,
  totalsIsEmpty,
  UNATTRIBUTED,
  usageMetricFormat,
  usageMetricTitle,
  usageMetricValue,
  usageRangeCaption,
  usageRangeTitle,
  usageScopeTitle,
  UsageFormat,
  usedUSD,
  USAGE_METRICS,
  USAGE_RANGES,
  USAGE_SCOPES,
  type UsageBreakdown,
  type UsageMetric,
  type UsageRange,
  type UsageScope,
  type UsageSeries,
} from '../../models/modelGateway'
import { usePref } from '../../models/preferences'
import { useApp } from '../../state/app'
import {
  beginGatewaySignIn,
  cancelGatewaySignIn,
  claimContextCredit,
  dismissGatewayError,
  GATEWAY_MODEL_BASE_URL,
  GATEWAY_ORIGIN,
  openGatewayDashboard,
  openInBrowser,
  refreshGateway,
  refreshGatewayUsage,
  setGatewayProjectAttribution,
  setGatewayRouting,
  signOutOfGateway,
  useGateway,
  useGatewayRoutingActive,
} from '../../state/gateway'
import { useIsReady } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { UsageBarChart } from './UsageBarChart'
import { UsageHeatmap } from './UsageHeatmap'
import styles from './AccountView.module.css'

/** `Settings → Billing`'s tab id (`SettingsTab.billing.rawValue`). */
const SETTINGS_TAB_BILLING = 'billing'

/** An account older than this is refreshed when the page opens (sign-in refreshes itself). */
const STALE_AFTER_MS = 60_000

/**
 * `AccountView` (Views/Account/AccountView.swift): the profile page's Account
 * tab, opened from the sidebar's bottom-left button. Signed out: the "Sign in
 * with ModelGateway" card (and the device code while a sign-in runs). Signed
 * in, top to bottom: the account header, the Context credit (the welcome
 * offer, then what is left of it), Plan (active subscriptions, or pay as you
 * go), Usage (24h / 7d / 30d bar charts or an all-time heatmap: total, by
 * model, by project; cost / tokens / requests), Settings and Sign out.
 */
export function AccountView() {
  const signedIn = useGateway((state) => state.isSignedIn)
  const hasPlans = useGateway((state) => state.account?.subscriptions != null)

  // `.task(id: gateway.isSignedIn)`: opening the page refreshes a stale account.
  useEffect(() => {
    if (!signedIn) return
    const last = useGateway.getState().lastRefreshed
    if (last === null || Date.now() - last > STALE_AFTER_MS) void refreshGateway()
  }, [signedIn])

  return (
    <div className={styles.page} role="region" aria-label="Account">
      <div className={styles.column}>
        {signedIn ? (
          <>
            <AccountHeader />
            <ContextCreditCard />
            {hasPlans ? <PlanCard /> : null}
            <UsageCard />
            <GatewaySettingsCard />
            <SignOutCard />
          </>
        ) : (
          <SignInCard />
        )}
      </div>
    </div>
  )
}

// MARK: - Shared pieces

const TIME_FORMAT = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' })
const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', year: 'numeric' })

/** `formatted(date: .abbreviated, time: .shortened)`: "Oct 24, 2026 at 3:42 PM". */
function formatDateTime(time: number): string {
  const date = new Date(time)
  return `${DATE_FORMAT.format(date)} at ${TIME_FORMAT.format(date)}`
}

function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(' ')
}

/** A warning glyph (12 px, `padding(.top, 1)`) beside a message: the account, usage and sign-in errors. */
function MessageRow({ tone, children }: { tone: 'warning' | 'danger'; children: ReactNode }) {
  return (
    <div className={styles.messageRow}>
      <Icon name="warning" size={12} strokeWidth={2} className={cx(styles.messageIcon, tone === 'danger' ? styles.danger : styles.warning)} />
      {children}
    </div>
  )
}

/** A card header: 13 px glyph in `textSecondary`, the heading, then whatever trails it. */
function CardHeader({ icon, title, children }: { icon: 'plan' | 'usage' | 'settings'; title: string; children?: ReactNode }) {
  return (
    <div className={styles.cardHeader}>
      <Icon name={icon} size={13} strokeWidth={2} className={styles.headerIcon} />
      <h3 className={styles.heading}>{title}</h3>
      {children}
    </div>
  )
}

/** The 6 px capsule bar of the credit and plan quotas; never narrower than 4 px. */
function CapsuleBar({ fraction, tone, label }: { fraction: number; tone: 'accent' | 'warning' | 'danger'; label?: string }) {
  const percent = Math.max(0, Math.min(1, fraction)) * 100
  return (
    <div className={styles.capsule} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true}>
      <div className={cx(styles.capsuleFill, styles[tone])} style={{ width: `max(4px, ${percent}%)` }} />
    </div>
  )
}

/// `.lineLimit(1).truncationMode(.middle)`: one line, shortened in the middle
/// (components/MiddleTruncate), as wide as the text and shrinking in a row.
function MiddleText({ text, className, selectable = false }: { text: string; className?: string; selectable?: boolean }) {
  return <MiddleTruncate natural text={text} className={cx(selectable && 'selectable', className)} />
}

// MARK: - Signed out

/** `SignInCard`: the optional sign-in, the device code while it runs, or why it failed. */
function SignInCard() {
  const flow = useGateway((state) => state.flow)

  let body: ReactNode
  if (flow === null) {
    body = <SignInButtons />
  } else {
    switch (flow.kind) {
      case 'starting':
        body = (
          <div className={styles.progressRow}>
            <BrailleSpinner size={12} />
            <span className={styles.secondaryText}>Contacting ModelGateway…</span>
          </div>
        )
        break
      case 'waiting':
        body = <WaitingForApproval userCode={flow.userCode} url={flow.verificationURL} expiresAt={flow.expiresAt} />
        break
      case 'completing':
        body = (
          <div className={styles.progressRow}>
            <BrailleSpinner size={12} />
            <span className={styles.secondaryText}>Approved. Finishing sign-in…</span>
          </div>
        )
        break
      case 'failed':
        body = (
          <div className={styles.stack10}>
            <MessageRow tone="danger">
              <span className={cx(styles.secondaryText, styles.dangerText, 'selectable')}>{flow.message}</span>
            </MessageRow>
            <SignInButtons />
          </div>
        )
        break
    }
  }

  return (
    <section className={cx(styles.card, styles.stack16)}>
      <div className={styles.signInHeader}>
        <span className={styles.gatewayBadge} aria-hidden="true">
          <Icon name="gateway" size={22} />
        </span>
        <div className={styles.stack6}>
          <h2 className={styles.title}>Sign in with ModelGateway</h2>
          <p className={styles.secondaryText}>
            Optional. Signing in routes model requests through your ModelGateway account and shows your usage here, by model and by project. Context works
            without it, with any OpenAI-compatible endpoint from the Model tab.
          </p>
        </div>
      </div>
      {body}
    </section>
  )
}

function SignInButtons() {
  const ready = useIsReady()
  return (
    <div className={styles.buttonRow}>
      <Button
        variant="prominent"
        icon="signIn"
        disabled={!ready}
        onClick={() => {
          dismissGatewayError()
          beginGatewaySignIn()
        }}
      >
        Sign in with ModelGateway
      </Button>
      <Button variant="bordered" onClick={() => openInBrowser(GATEWAY_ORIGIN + '/register')}>
        Create an account…
      </Button>
      {!ready ? <span className={styles.captionTertiary}>Waiting for the backend…</span> : null}
    </div>
  )
}

function WaitingForApproval({ userCode, url, expiresAt }: { userCode: string; url: string; expiresAt: number }) {
  return (
    <div className={styles.stack12}>
      <p className={styles.secondaryText}>Your browser opened ModelGateway. Confirm that it shows this code, then approve:</p>
      <div className={styles.codeRow}>
        <span className={cx(styles.userCode, 'selectable')}>
          <span aria-hidden="true">{userCode}</span>
          <span className="sr-only">{`Code ${Array.from(userCode).join(' ')}`}</span>
        </span>
        <CopyButton text={userCode} label="Copy code" />
      </div>
      <div className={styles.progressRow}>
        <BrailleSpinner size={11} tint="var(--text-secondary)" />
        <span className={styles.captionTertiary}>{`Waiting for approval · expires ${TIME_FORMAT.format(new Date(expiresAt))}`}</span>
      </div>
      <div className={styles.buttonRow}>
        <Button size="small" icon="externalLink" onClick={() => openInBrowser(url)}>
          Open ModelGateway again
        </Button>
        <Button size="small" onClick={() => cancelGatewaySignIn()}>
          Cancel
        </Button>
      </div>
    </div>
  )
}

// MARK: - Account header

/** `AccountHeader`: avatar, name, group, @username · email, the four figures, Refresh and the dashboard link. */
function AccountHeader() {
  const account = useGateway((state) => state.account)
  const accountError = useGateway((state) => state.accountError)
  const refreshing = useGateway((state) => state.refreshing)
  const initial = useGateway((state) => state.initial)
  const name = useGateway((state) => state.name)

  return (
    <section className={cx(styles.card, styles.stack14)}>
      {account ? (
        <>
          <div className={styles.headerRow}>
            <span className={styles.avatar} aria-hidden="true">
              {initial}
            </span>
            <div className={styles.headerText}>
              <div className={styles.nameRow}>
                <h2 className={cx(styles.title, styles.oneLine)}>{name}</h2>
                {account.group.length > 0 ? (
                  <Tooltip content="Group">
                    <Chip>{account.group}</Chip>
                  </Tooltip>
                ) : null}
              </div>
              <MiddleText
                text={[account.username.length === 0 ? null : '@' + account.username, account.email.length === 0 ? null : account.email]
                  .filter((part): part is string => part !== null)
                  .join(' · ')}
                className={styles.secondaryText}
                selectable
              />
            </div>
            <span className={styles.spacer} />
            <div className={styles.headerButtons}>
              {refreshing ? (
                <span className={styles.spinnerSlot}>
                  <BrailleSpinner size={11} tint="var(--text-secondary)" />
                </span>
              ) : (
                <IconButton icon="refresh" label="Refresh account and usage" onClick={() => void refreshGateway()} />
              )}
              <IconButton icon="externalLink" label="Open the ModelGateway dashboard" onClick={() => openGatewayDashboard()} />
            </div>
          </div>
          <AccountStats account={account} />
        </>
      ) : null}
      {accountError !== null ? (
        <MessageRow tone="warning">
          <span className={cx(styles.captionSecondary, 'selectable')}>{accountError}</span>
        </MessageRow>
      ) : null}
    </section>
  )
}

function AccountStats({ account }: { account: GatewayAccount }) {
  const currency = account.display_in_currency
  const token = account.token
  return (
    <div className={styles.stats}>
      <Stat title="Balance" value={currency ? UsageFormat.usd(balanceUSD(account)) : UsageFormat.compact(account.quota)} detail="remaining" />
      <span className={styles.statDivider} />
      <Stat title="Spent" value={currency ? UsageFormat.usd(usedUSD(account)) : UsageFormat.compact(account.used_quota)} detail="all time" />
      <span className={styles.statDivider} />
      <Stat title="Requests" value={formatInteger(account.request_count)} detail="all time" />
      {token ? (
        <>
          <span className={styles.statDivider} />
          <Stat
            title="This device"
            value={currency ? UsageFormat.usd(token.used_quota / Math.max(account.quota_per_unit, 1)) : UsageFormat.compact(token.used_quota)}
            detail={token.name}
          />
        </>
      ) : null}
    </div>
  )
}

function Stat({ title, value, detail }: { title: string; value: string; detail: string }) {
  return (
    <div className={styles.stat} role="group" aria-label={`${title}: ${value}, ${detail}`}>
      <span className={styles.captionTertiary}>{title}</span>
      <span className={cx(styles.statValue, styles.oneLine)}>{value}</span>
      <MiddleText text={detail} className={styles.captionSecondary} />
    </div>
  )
}

// MARK: - Context credit

/// `ContextCreditCard`: the welcome Context credit, an offer the user claims
/// once a card or crypto top-up has gone through, then what is left of it.
/// It pays for requests made with this app's device key only, before any plan
/// or the balance. Hidden once spent, and for gateways that predate it.
function ContextCreditCard() {
  const account = useGateway((state) => state.account)
  const credit = account?.context_credit
  if (!account || !credit) return null
  if (!(credit.claimed ? credit.balance > 0 : credit.offer_quota > 0)) return null
  return (
    <section className={cx(styles.card, styles.creditCard)}>
      <span className={styles.creditBadge} aria-hidden="true">
        <Icon name="contextCredit" size={18} />
      </span>
      {credit.claimed ? <CreditRemaining credit={credit} account={account} /> : <CreditOffer credit={credit} account={account} />}
    </section>
  )
}

function CreditOffer({ credit, account }: { credit: GatewayContextCredit; account: GatewayAccount }) {
  const claiming = useGateway((state) => state.claimingContextCredit)
  const ready = useIsReady()
  const offer = formatQuota(account, credit.offer_quota)
  return (
    <div className={cx(styles.creditBody, styles.stack12)}>
      <div className={styles.stack4}>
        <h3 className={styles.heading}>{`${offer} of free Context credit`}</h3>
        <p className={styles.secondaryText}>
          {credit.eligible
            ? 'Thanks for trying Context! Claim 10$ for use within Context'
            : "Add funds by card or crypto once to unlock it. Context's requests are then paid from this credit before your balance. It only works in Context."}
        </p>
      </div>
      <div className={styles.buttonRow}>
        <Tooltip content={credit.eligible ? 'Add the credit to this account' : 'Available after a card or crypto top-up'}>
          <span className={styles.tooltipHost}>
            <Button variant="prominent" disabled={!credit.eligible || claiming || !ready} onClick={() => void claimContextCredit()}>
              {claiming ? (
                <span className={styles.inlineLabel}>
                  <BrailleSpinner size={11} tint="var(--on-accent)" />
                  Claiming…
                </span>
              ) : (
                `Claim ${offer}`
              )}
            </Button>
          </span>
        </Tooltip>
        {!credit.eligible ? <Button onClick={() => useApp.getState().openSettings(SETTINGS_TAB_BILLING)}>Add funds…</Button> : null}
      </div>
    </div>
  )
}

function CreditRemaining({ credit, account }: { credit: GatewayContextCredit; account: GatewayAccount }) {
  const fraction = credit.granted > 0 ? Math.min(1, credit.balance / credit.granted) : 0
  const left = `${formatQuota(account, credit.balance)} of ${formatQuota(account, credit.granted)} left`
  return (
    <div className={cx(styles.creditBody, styles.stack8)} role="group" aria-label={`Context credit, ${left}`}>
      <div className={styles.baselineRow}>
        <h3 className={styles.heading}>Context credit</h3>
        <span className={styles.spacer} />
        <span className={cx(styles.secondaryText, styles.oneLine)}>{left}</span>
      </div>
      <CapsuleBar fraction={fraction} tone="accent" />
      <p className={styles.captionTertiary}>Pays for Context&apos;s requests before your balance. It cannot be used with other API keys.</p>
    </div>
  )
}

// MARK: - Plan

/** `PlanCard`: each active subscription with its quota, or "Pay as you go"; hidden for gateways without plans. */
function PlanCard() {
  const account = useGateway((state) => state.account)
  const subscriptions = account?.subscriptions
  if (!account || !subscriptions) return null
  const billingTitle = billingPreferenceTitle(account)
  return (
    <section className={cx(styles.card, styles.stack14)}>
      <CardHeader icon="plan" title={subscriptions.length > 1 ? 'Plans' : 'Plan'}>
        <span className={styles.spacer} />
        <Tooltip content="Add funds, buy a plan and choose whether credits or the plan are used first">
          <Button size="small" onClick={() => useApp.getState().openSettings(SETTINGS_TAB_BILLING)}>
            Billing settings…
          </Button>
        </Tooltip>
      </CardHeader>
      {subscriptions.length === 0 ? (
        <div className={styles.stack4}>
          <span className={styles.rowTitle}>Pay as you go</span>
          <p className={styles.captionSecondary}>
            {`No active plan. Requests are billed to your balance (${account.display_in_currency ? UsageFormat.usd(balanceUSD(account)) : UsageFormat.compact(account.quota)} left).`}
          </p>
        </div>
      ) : (
        <>
          {subscriptions.map((subscription, index) => (
            <Fragment key={subscription.id}>
              <SubscriptionRow subscription={subscription} account={account} />
              {index < subscriptions.length - 1 ? <hr className={styles.divider} /> : null}
            </Fragment>
          ))}
          {billingTitle !== null ? <p className={styles.captionTertiary}>{`Billing: ${billingTitle} · balance ${formatQuota(account, account.quota)}`}</p> : null}
        </>
      )}
    </section>
  )
}

function SubscriptionRow({ subscription, account }: { subscription: GatewaySubscription; account: GatewayAccount }) {
  const price = subscriptionPriceTitle(subscription)
  const end = subscriptionEndDate(subscription)
  const days = subscriptionDaysLeft(subscription)
  const reset = subscriptionNextResetDate(subscription)
  const resetTitle = subscriptionResetTitle(subscription)
  const fraction = subscriptionUsedFraction(subscription)
  const used = `${formatQuota(account, subscription.amount_used)} of ${formatQuota(account, subscription.amount_total)} used · ${formatQuota(account, subscriptionRemaining(subscription))} left`
  const usedText = <span className={styles.captionSecondary}>{used}</span>
  const resetText = reset !== null ? <span className={styles.captionTertiary}>{`Resets ${formatDateTime(reset)}`}</span> : null

  return (
    <div className={styles.stack8} role="group">
      <div className={styles.baselineRow}>
        <span className={cx(styles.rowTitle, styles.oneLine)}>{subscriptionName(subscription)}</span>
        <Chip variant="tinted" tint="var(--success)">
          Active
        </Chip>
        {price !== null ? <span className={cx(styles.secondaryText, styles.oneLine)}>{price}</span> : null}
        <span className={styles.spacer} />
        {end !== null && days !== null ? (
          <Tooltip content={`Until ${formatDateTime(end)}`}>
            <span className={cx(styles.caption, days <= 3 ? styles.warning : styles.secondaryColor, styles.noShrink)}>{`${days === 1 ? '1 day' : `${days} days`} left`}</span>
          </Tooltip>
        ) : null}
      </div>
      {subscription.subtitle.length > 0 ? <p className={cx(styles.captionTertiary, styles.twoLines)}>{subscription.subtitle}</p> : null}
      {subscriptionIsUnlimited(subscription) ? (
        <p className={styles.captionSecondary}>{`Unlimited quota · ${formatQuota(account, subscription.amount_used)} used`}</p>
      ) : (
        <>
          <CapsuleBar fraction={fraction} tone={fraction > 0.9 ? 'danger' : fraction > 0.7 ? 'warning' : 'accent'} label={`Plan quota used ${Math.trunc(fraction * 100)} percent`} />
          <div className={styles.quotaRow}>
            {subscription.amount_reserved > 0 ? (
              <Tooltip content={`${formatQuota(account, subscription.amount_reserved)} held for requests in flight`}>{usedText}</Tooltip>
            ) : (
              usedText
            )}
            <span className={styles.spacer} />
            {resetText !== null && resetTitle !== null ? <Tooltip content={resetTitle}>{resetText}</Tooltip> : resetText}
          </div>
        </>
      )}
    </div>
  )
}

// MARK: - Usage

/** Series shown before "Show all". */
const COLLAPSED_COUNT = 6

const RANGE_OPTIONS: ReadonlyArray<{ value: UsageRange; label: string }> = USAGE_RANGES.map((range) => ({ value: range, label: usageRangeTitle(range) }))
const SCOPE_OPTIONS: ReadonlyArray<{ value: UsageScope; label: string }> = USAGE_SCOPES.map((scope) => ({ value: scope, label: usageScopeTitle(scope) }))
const METRIC_OPTIONS: ReadonlyArray<{ value: UsageMetric; label: string }> = USAGE_METRICS.map((metric) => ({ value: metric, label: usageMetricTitle(metric) }))

/** `UsageCard`: the period, scope and metric pickers over the charts. */
function UsageCard() {
  const usage = useGateway((state) => state.usage)
  const usageLoading = useGateway((state) => state.usageLoading)
  const usageError = useGateway((state) => state.usageError)
  const [range, setRange] = useState<UsageRange>('all')
  const [scope, setScope] = useState<UsageScope>('total')
  const [metric, setMetric] = useState<UsageMetric>('cost')
  const [showAll, setShowAll] = useState(false)

  const breakdown = usage?.[range] ?? null
  let body: ReactNode
  if (breakdown) {
    body = <UsageContent usage={breakdown} scope={scope} metric={metric} showAll={showAll} onToggleShowAll={() => setShowAll((value) => !value)} />
  } else if (usageLoading) {
    body = (
      <div className={styles.usageLoading}>
        <BrailleSpinner size={12} />
        <span className={styles.secondaryText}>Loading usage…</span>
      </div>
    )
  } else if (usageError !== null) {
    body = <UsageErrorRow error={usageError} />
  } else {
    body = <p className={styles.secondaryText}>Usage appears here once ModelGateway has recorded requests from this account.</p>
  }

  return (
    <section className={cx(styles.card, styles.stack14)}>
      <CardHeader icon="usage" title="Usage">
        {usageLoading ? <BrailleSpinner size={11} tint="var(--text-secondary)" /> : null}
        <span className={styles.spacer} />
        <Tooltip content="Period to chart">
          <SegmentedControl
            options={RANGE_OPTIONS}
            value={range}
            onChange={(next) => {
              setRange(next)
              setShowAll(false)
            }}
            aria-label="Period"
          />
        </Tooltip>
      </CardHeader>
      {/* Side by side, or stacked when the page is narrow (a wide inspector). */}
      <div className={styles.pickers}>
        <SegmentedControl
          options={SCOPE_OPTIONS}
          value={scope}
          onChange={(next) => {
            setScope(next)
            setShowAll(false)
          }}
          aria-label="Scope"
        />
        <SegmentedControl options={METRIC_OPTIONS} value={metric} onChange={setMetric} aria-label="Metric" />
      </div>
      {body}
      {usageError !== null && usage !== null ? <UsageErrorRow error={usageError} /> : null}
    </section>
  )
}

function emptyMessage(usage: UsageBreakdown, scope: UsageScope): string {
  if (usage.rowCount === 0) return 'No usage recorded in the last year. Requests routed through ModelGateway show up here within the hour.'
  if (totalsIsEmpty(usage.total.total)) return `No usage in the ${usageRangeCaption(usage.range).toLowerCase()}. Pick a longer period to see earlier requests.`
  return scope === 'project' ? 'No project breakdown yet.' : 'No model breakdown yet.'
}

function UsageContent({
  usage,
  scope,
  metric,
  showAll,
  onToggleShowAll,
}: {
  usage: UsageBreakdown
  scope: UsageScope
  metric: UsageMetric
  showAll: boolean
  onToggleShowAll: () => void
}) {
  const series = breakdownSeries(usage, scope)
  if (series.length === 0 || (scope === 'total' && totalsIsEmpty(usage.total.total))) {
    return (
      <div className={styles.stack12}>
        <UsageChart series={usage.total} usage={usage} metric={metric} />
        <p className={styles.captionTertiary}>{emptyMessage(usage, scope)}</p>
      </div>
    )
  }
  if (scope === 'total') return <UsageChart series={usage.total} usage={usage} metric={metric} />
  const extra = series.slice(COLLAPSED_COUNT)
  return (
    <div className={styles.stack18}>
      {series.slice(0, COLLAPSED_COUNT).map((item) => (
        <UsageSeriesRow key={item.id} item={item} usage={usage} metric={metric} />
      ))}
      {extra.length > 0 ? (
        // A new period or scope starts collapsed without animating (`onChange { showAll = false }`).
        <Collapsible key={`${usage.range}:${scope}`} open={showAll}>
          {extra.map((item) => (
            <UsageSeriesRow key={item.id} item={item} usage={usage} metric={metric} />
          ))}
        </Collapsible>
      ) : null}
      {series.length > COLLAPSED_COUNT ? (
        <button type="button" className={styles.linkButton} onClick={onToggleShowAll}>
          {showAll ? 'Show fewer' : `Show all ${series.length}`}
        </button>
      ) : null}
      {scope === 'project' && series.some((item) => item.title === UNATTRIBUTED) ? (
        <p className={styles.captionTertiary}>
          “Unattributed” is usage without a project name: requests from other clients, or made while “Attribute usage to projects” was off.
        </p>
      ) : null}
    </div>
  )
}

type CollapsiblePhase = 'closed' | 'opening' | 'open' | 'closing'

/// The series past the first six: `withAnimation(.easeOut(duration: 0.15)) {
/// showAll.toggle() }` fades them in or out while the list grows or shrinks to
/// fit. Collapsed, they are not rendered at all.
function Collapsible({ open, children }: { open: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const [phase, setPhase] = useState<CollapsiblePhase>(open ? 'open' : 'closed')
  if (open && (phase === 'closed' || phase === 'closing')) setPhase('opening')
  if (!open && (phase === 'open' || phase === 'opening')) setPhase('closing')

  useLayoutEffect(() => {
    const element = ref.current
    if (!element || (phase !== 'opening' && phase !== 'closing')) return
    const opening = phase === 'opening'
    const done: CollapsiblePhase = opening ? 'open' : 'closed'
    if (typeof element.animate !== 'function' || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setPhase(done)
      return
    }
    const height = `${opening ? element.scrollHeight : element.getBoundingClientRect().height}px`
    element.style.overflow = 'hidden'
    const animation = element.animate(
      opening ? [{ height: '0px', opacity: 0 }, { height, opacity: 1 }] : [{ height, opacity: 1 }, { height: '0px', opacity: 0 }],
      // Closing holds the last frame until the rows unmount.
      { duration: 150, easing: 'ease-out', fill: opening ? 'none' : 'forwards' },
    )
    animation.onfinish = () => setPhase(done)
    return () => {
      animation.onfinish = null
      animation.cancel()
      element.style.overflow = ''
    }
  }, [phase])

  if (phase === 'closed') return null
  return (
    <div ref={ref} className={styles.collapsible}>
      <div className={styles.collapsibleRows}>{children}</div>
    </div>
  )
}

/** One model's or project's row: title, figure, share chip and its own chart. */
const UsageSeriesRow = memo(function UsageSeriesRow({ item, usage, metric }: { item: UsageSeries; usage: UsageBreakdown; metric: UsageMetric }) {
  return (
    <div className={styles.stack8}>
      <div className={styles.seriesHeader}>
        <MiddleText text={item.title} className={cx(styles.rowTitle, item.title === UNATTRIBUTED && styles.secondaryColor)} />
        <span className={styles.spacer} />
        <span className={styles.seriesValue}>{usageMetricFormat(metric, item.total, usage.quotaPerUnit)}</span>
        <ShareChip item={item} usage={usage} metric={metric} />
      </div>
      <UsageChart series={item} usage={usage} metric={metric} showsTotal={false} />
    </div>
  )
})

/** The range's bar chart, or the all-time heatmap. */
function UsageChart({ series, usage, metric, showsTotal = true }: { series: UsageSeries; usage: UsageBreakdown; metric: UsageMetric; showsTotal?: boolean }) {
  if (usage.range === 'all') return <UsageHeatmap series={series} metric={metric} quotaPerUnit={usage.quotaPerUnit} showsTotal={showsTotal} />
  return <UsageBarChart series={series} range={usage.range} buckets={usage.bucketStarts} metric={metric} quotaPerUnit={usage.quotaPerUnit} showsTotal={showsTotal} />
}

/** "42%" of the scope's total, by the chosen metric. */
function ShareChip({ item, usage, metric }: { item: UsageSeries; usage: UsageBreakdown; metric: UsageMetric }) {
  const total = usageMetricValue(metric, usage.total.total, usage.quotaPerUnit)
  const value = usageMetricValue(metric, item.total, usage.quotaPerUnit)
  const percent = total > 0 ? Math.round((value / total) * 100) : 0
  return (
    <Tooltip content={`Share of all usage in this period, by ${usageMetricTitle(metric).toLowerCase()}`}>
      <Chip>{`${percent}%`}</Chip>
    </Tooltip>
  )
}

function UsageErrorRow({ error }: { error: string }) {
  return (
    <MessageRow tone="warning">
      <span className={cx(styles.captionSecondary, styles.messageText, 'selectable')}>{`Usage could not be loaded: ${error}`}</span>
      <Button size="small" onClick={() => void refreshGatewayUsage()}>
        Retry
      </Button>
    </MessageRow>
  )
}

// MARK: - Settings

/** `GatewaySettingsCard`: "Route model requests through ModelGateway" and "Attribute usage to projects". */
function GatewaySettingsCard() {
  const routing = usePref('routeThroughGateway')
  const attribution = usePref('attributeProjects')
  const model = useApp((state) => state.config?.model ?? null)
  const routingActive = useGatewayRoutingActive()

  let routingDetail: string
  if (model === null) routingDetail = 'The backend is not running.'
  else if (routingActive)
    routingDetail = `Model requests go to ${model.base_url} with your ModelGateway key. Turning this off restores the previous endpoint from Settings → Model.`
  else if (routing) routingDetail = `Switching the endpoint to ${GATEWAY_MODEL_BASE_URL}…`
  else routingDetail = `Off: requests keep using ${model.base_url} and your own key. Usage shown here only covers requests that went through ModelGateway.`

  return (
    <section className={cx(styles.card, styles.stack14)}>
      <CardHeader icon="settings" title="Settings" />
      <SettingRow
        title="Route model requests through ModelGateway"
        detail={routingDetail}
        control={<Switch checked={routing} label="Route model requests through ModelGateway" onChange={(on) => void setGatewayRouting(on)} />}
      />
      <SettingRow
        title="Attribute usage to projects"
        detail="Sends each request's project name as the “X-ModelGateway-Project” header, so Usage can show a per-project breakdown. Nothing else about the project is sent."
        control={
          <Switch checked={attribution} label="Attribute usage to projects" disabled={!routing} onChange={(on) => void setGatewayProjectAttribution(on)} />
        }
      />
    </section>
  )
}

function SettingRow({ title, detail, control }: { title: string; detail: string; control: ReactNode }) {
  return (
    <div className={styles.settingRow}>
      <div className={styles.stack3}>
        <span className={styles.settingTitle}>{title}</span>
        <span className={styles.captionTertiary}>{detail}</span>
      </div>
      <span className={styles.spacer} />
      <span className={styles.settingControl}>{control}</span>
    </div>
  )
}

// MARK: - Sign out

/** `SignOutCard`: revokes the device key, forgets it and restores the previous endpoint. */
function SignOutCard() {
  const signingOut = useGateway((state) => state.signingOut)
  return (
    <section className={cx(styles.card, styles.signOutCard)}>
      <div className={styles.stack3}>
        <span className={styles.settingTitle}>Sign out</span>
        <span className={styles.captionTertiary}>Sign out of Context.</span>
      </div>
      <span className={styles.spacer} />
      <Button className={styles.dangerButton} disabled={signingOut} onClick={() => void signOutOfGateway()}>
        <span className={styles.inlineLabel}>
          {signingOut ? <BrailleSpinner size={10} tint="var(--text-secondary)" /> : <Icon name="signOut" size={14} strokeWidth={2} />}
          <span className={styles.baselineText}>Sign out</span>
        </span>
      </Button>
    </section>
  )
}
