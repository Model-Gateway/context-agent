// Port of apps/macos/ContextAgent/App/GatewayState.swift: "Sign in with
// ModelGateway" (apps/macos/ARCHITECTURE.md "Account"), the optional account
// whose device key routes model requests through the gateway and whose usage
// the Account page charts. `useGateway` is the observable state (`GatewayState`)
// plus the derived values the sidebar's account button shows; the exported
// functions are the `AppState` extension's intents. The app never speaks HTTP
// itself and never sees the key: `backend.gateway*` relay to `ctx serve`'s
// `gateway/*` methods, the backend keeps the approved device key in its
// credential store, and `model.credential = "modelgateway"` makes the provider
// use it (no environment variable, no secret-store entry).
//
// Plugged into the app store at import: `extendApp({ isGatewaySignedIn })`
// (model stats show only while signed in) and `onBackendStarted` (retire the
// legacy key variable, then refresh or forget a remembered account).

import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { backend } from '../backend/client'
import { BackendError, errorText } from '../backend/errors'
import {
  normalizeGatewayAccount,
  type GatewayAccount,
  type GatewayBilling,
  type GatewayBillingPreference,
  type GatewayKeySettings,
  type GatewayPlan,
  type GatewayReferral,
  type GatewayTopUpQuote,
  type ResponseReportReason,
  type GatewayUsageReport,
  type JSONObject,
  type ModelSettings,
} from '../backend/types'
import { toasts, type ToastLevel } from '../components/toasts'
import { getPref, setPref, usePreferences } from '../models/preferences'
import { accountInitial, accountName, balanceText as accountBalanceText, formatQuota, usageBreakdownsByRange, type UsageByRange } from '../models/modelGateway'
import { assistantTurnReportKey, type AssistantTurn } from '../models/transcript'
import { extendApp, forwardSignInToHosts, onBackendStarted, touch, useApp, SETTINGS_TAB_ACCOUNT } from './app'
import { isReady } from './selectors'

// MARK: - Constants (`GatewayState` statics)

/** `GatewayState.credentialName`: the backend's stored credential (`model.credential`) holding the device key. */
export const GATEWAY_CREDENTIAL_NAME = 'modelgateway'
/** `GatewayState.defaultBaseURL`: the gateway origin; not user-configurable. */
export const GATEWAY_ORIGIN = 'https://www.modelgateway.co'
/** `AppState.gatewayModelBaseURL`: the OpenAI-compatible endpoint of the gateway. */
export const GATEWAY_MODEL_BASE_URL = GATEWAY_ORIGIN + '/v1'
/** `GatewayState.byokDefaultBaseURL`: the BYOK endpoint when none was saved before routing through the gateway. */
export const BYOK_DEFAULT_BASE_URL = 'https://api.openai.com/v1'
/// `GatewayState.legacyKeyVariable`: the key variable of the retired
/// secret-store-based gateway sign-in; the BYOK key uses `BYOK_KEY_VARIABLE`.
export const LEGACY_GATEWAY_KEY_VARIABLE = 'MODELGATEWAY_API_KEY'
/** `GatewayState.byokKeyVariable`. */
export const BYOK_KEY_VARIABLE = 'OPENAI_API_KEY'
/// `GatewayState.projectHeader`: sent with every model request while per-project
/// usage is on (`model.project_header`); the gateway stores it with its usage rows.
export const GATEWAY_PROJECT_HEADER = 'X-ModelGateway-Project'
/** `GatewayState.usageDays`: 53 weeks, what the heatmap shows. */
export const GATEWAY_USAGE_DAYS = 371

// MARK: - State

/** `GatewayState.Flow`: the device sign-in's progress. */
export type GatewayFlow =
  /** `gateway/device/start` in flight. */
  | { kind: 'starting' }
  /** The code is shown; polling until the browser decides. `expiresAt` is in ms. */
  | { kind: 'waiting'; userCode: string; verificationURL: string; expiresAt: number }
  /** Approved: storing the key and switching the endpoint. */
  | { kind: 'completing' }
  | { kind: 'failed'; message: string }

/** `GatewayState`'s stored properties. */
export interface GatewayFields {
  account: GatewayAccount | null
  flow: GatewayFlow | null
  /** The last usage report as fetched (371 days of hourly rows). */
  usageReport: GatewayUsageReport | null
  /** The last usage report, broken down for each of the Usage card's ranges. */
  usage: UsageByRange | null
  usageLoading: boolean
  usageError: string | null
  accountError: string | null
  refreshing: boolean
  signingOut: boolean
  /** When the account was last refreshed (ms). */
  lastRefreshed: number | null
  /** Settings → Billing: top-up options, plans on sale, the preference. */
  billing: GatewayBilling | null
  billingLoading: boolean
  billingError: string | null
  /** A checkout, plan purchase or preference change in flight. */
  billingBusy: boolean
  /** The Account page's Context credit claim in flight. */
  claimingContextCredit: boolean
  /** The Account page's Invite card (`gateway/referral`). */
  referral: GatewayReferral | null
  referralLoading: boolean
  referralError: string | null
  /** A split change or a code attach in flight. */
  referralBusy: boolean
  /** Settings → Spending: the device key's price limits and spending cap. */
  keySettings: GatewayKeySettings | null
  keySettingsError: string | null
  /** Settings → Spending: the model ids ModelGateway serves, one price limit row each. */
  models: string[]
  /** Assistant messages reported this session (`assistantTurnReportKey`), so their Report button reads Reported. */
  reportedResponses: ReadonlySet<string>
}

/// Values derived from the fields on every change (selectors may read them
/// directly; each is a primitive). The sidebar's account button reads these.
export interface GatewayDerived {
  /** `GatewayState.isSignedIn`. */
  isSignedIn: boolean
  /** `GatewayState.isSigningIn`: a sign-in is under way (not a failed one waiting to be dismissed). */
  isSigningIn: boolean
  /** `GatewayAccount.name` (display name, else username); null when signed out. */
  name: string | null
  /** `GatewayAccount.initial`: the avatar's letter; null when signed out. */
  initial: string | null
  /** The balance ("$24.69", or raw units when the gateway does not show currency); null when signed out. */
  balanceText: string | null
  /** The account button's title: the name, "Signing in to ModelGateway…" or "Sign in with ModelGateway". */
  title: string
  /// The account button's subtitle: "ModelGateway · $X left", "Approve the code
  /// in your browser", "Contacting ModelGateway…" or "Optional · usage and billing".
  subtitle: string
  /** The account button's tooltip. */
  help: string
}

/** `useGateway`'s intents (the same functions as the module's exports). */
export interface GatewayActions {
  /** The sidebar account button: shows the profile page and, landing on Account signed out, starts the device sign-in. */
  openAccount: () => void
  beginGatewaySignIn: () => void
  cancelGatewaySignIn: () => void
  dismissGatewayError: () => void
  signOutOfGateway: () => Promise<void>
  refreshGateway: () => Promise<void>
  refreshGatewayUsage: () => Promise<void>
  claimContextCredit: () => Promise<void>
  refreshGatewayReferral: () => Promise<void>
  setGatewayReferralSplit: (cents: number) => Promise<void>
  attachGatewayReferralCode: (code: string) => Promise<boolean>
  setGatewayRouting: (enabled: boolean) => Promise<void>
  setGatewayProjectAttribution: (enabled: boolean) => Promise<void>
  openGatewayDashboard: () => void
  openGatewayWallet: () => void
  openGatewayPlans: () => void
}

/** The `useGateway` store. */
export type GatewayStore = GatewayFields & GatewayDerived & GatewayActions

function restoredAccount(): GatewayAccount | null {
  const stored = getPref('gatewayAccount')
  if (!stored) return null
  try {
    return normalizeGatewayAccount(stored)
  } catch {
    return null
  }
}

function initialFields(): GatewayFields {
  return {
    account: restoredAccount(),
    flow: null,
    usageReport: null,
    usage: null,
    usageLoading: false,
    usageError: null,
    accountError: null,
    refreshing: false,
    signingOut: false,
    lastRefreshed: null,
    billing: null,
    billingLoading: false,
    billingError: null,
    billingBusy: false,
    claimingContextCredit: false,
    referral: null,
    referralLoading: false,
    referralError: null,
    referralBusy: false,
    keySettings: null,
    keySettingsError: null,
    models: [],
    reportedResponses: new Set(),
  }
}

function flowIsSigningIn(flow: GatewayFlow | null): boolean {
  return flow !== null && flow.kind !== 'failed'
}

function derive(fields: Pick<GatewayFields, 'account' | 'flow'>): GatewayDerived {
  const { account, flow } = fields
  const signingIn = flowIsSigningIn(flow)
  const name = account ? accountName(account) : null
  const balance = account ? accountBalanceText(account) : null
  let subtitle: string
  if (account) subtitle = `ModelGateway · ${balance ?? ''} left`
  else if (flow?.kind === 'waiting') subtitle = 'Approve the code in your browser'
  else if (signingIn) subtitle = 'Contacting ModelGateway…'
  else subtitle = 'Optional · usage and billing'
  return {
    isSignedIn: account !== null,
    isSigningIn: signingIn,
    name,
    initial: account ? accountInitial(account) : null,
    balanceText: balance,
    title: name ?? (signingIn ? 'Signing in to ModelGateway…' : 'Sign in with ModelGateway'),
    subtitle,
    help: account ? 'ModelGateway account, usage and settings' : 'Account and settings',
  }
}

const actions: GatewayActions = {
  openAccount: () => openAccount(),
  beginGatewaySignIn: () => beginGatewaySignIn(),
  cancelGatewaySignIn: () => cancelGatewaySignIn(),
  dismissGatewayError: () => dismissGatewayError(),
  signOutOfGateway: () => signOutOfGateway(),
  refreshGateway: () => refreshGateway(),
  refreshGatewayUsage: () => refreshGatewayUsage(),
  claimContextCredit: () => claimContextCredit(),
  refreshGatewayReferral: () => refreshGatewayReferral(),
  setGatewayReferralSplit: (cents) => setGatewayReferralSplit(cents),
  attachGatewayReferralCode: (code) => attachGatewayReferralCode(code),
  setGatewayRouting: (enabled) => setGatewayRouting(enabled),
  setGatewayProjectAttribution: (enabled) => setGatewayProjectAttribution(enabled),
  openGatewayDashboard: () => openGatewayDashboard(),
  openGatewayWallet: () => openGatewayWallet(),
  openGatewayPlans: () => openGatewayPlans(),
}

/**
 * `GatewayState` (App/GatewayState.swift) as a zustand store: the account, the
 * device flow, usage, billing and key settings, plus the account button's
 * derived `isSignedIn`, `name`, `balanceText`, `title`, `subtitle` and the
 * `openAccount()` intent. Select primitives or stable references.
 */
export const useGateway = create<GatewayStore>()(() => {
  const fields = initialFields()
  return { ...fields, ...derive(fields), ...actions }
})

/** Sets fields and recomputes the derived values. */
function patch(fields: Partial<GatewayFields>): void {
  useGateway.setState((state) => {
    const account = 'account' in fields ? (fields.account ?? null) : state.account
    const flow = 'flow' in fields ? (fields.flow ?? null) : state.flow
    return { ...fields, ...derive({ account, flow }) }
  })
}

function gw(): GatewayStore {
  return useGateway.getState()
}

/** `ToastCenter.show(_:kind:)`. */
function toast(message: string, level: ToastLevel): void {
  toasts.push({ message, level })
}

/** Whether the backend is up (`connection.isReady`). */
function connectionReady(): boolean {
  return isReady(useApp.getState())
}

/** `persistGatewayAccount()`; also bumps the app store, whose model stats depend on `isSignedIn`. */
function persistGatewayAccount(): void {
  setPref('gatewayAccount', gw().account)
  touch()
}

// MARK: - Endpoint helpers

/// `AppState.normalizedGatewayOrigin(_:)`: scheme + host, no trailing slash
/// or `/v1`; the default origin for an empty value.
export function normalizedGatewayOrigin(raw: string): string {
  let text = raw.trim()
  if (text.length === 0) text = GATEWAY_ORIGIN
  const lower = text.toLowerCase()
  if (!lower.startsWith('http://') && !lower.startsWith('https://')) text = 'https://' + text
  while (text.endsWith('/')) text = text.slice(0, -1)
  if (text.endsWith('/v1')) text = text.slice(0, -3)
  while (text.endsWith('/')) text = text.slice(0, -1)
  return text
}

/** `AppState.isGatewayEndpoint(_:)`: whether the user-level model endpoint is the gateway's. */
export function isGatewayEndpoint(model: ModelSettings): boolean {
  return model.credential === GATEWAY_CREDENTIAL_NAME || normalizedGatewayOrigin(model.base_url) === GATEWAY_ORIGIN
}

/** `AppState.gatewayRoutingActive`: signed in and the backend is sending model requests to the gateway. */
export function gatewayRoutingActive(): boolean {
  const model = useApp.getState().config?.model
  if (!model) return false
  return gw().isSignedIn && isGatewayEndpoint(model)
}

/** `gatewayRoutingActive`, subscribed to the config and the account. */
export function useGatewayRoutingActive(): boolean {
  const model = useApp((state) => state.config?.model ?? null)
  const signedIn = useGateway((state) => state.isSignedIn)
  return model !== null && signedIn && isGatewayEndpoint(model)
}

// MARK: - Browser and errors

/** `AppState.openInBrowser(_:)`: opens a web page in the default browser; ignores what is not a URL. */
export function openInBrowser(url: string): void {
  try {
    new URL(url)
  } catch {
    return
  }
  openUrl(url).catch(() => {
    invoke('open_url', { url }).catch(() => {
      /* Outside Tauri (plain Vite) there is nothing to open with. */
    })
  })
}

/** `AppState.gatewayMessage(_:)`: an error in words, "ModelGateway could not be reached: …" for transport failures. */
export function gatewayMessage(error: unknown): string {
  if (error instanceof BackendError) {
    if (error.detail.type === 'rpc') {
      return error.detail.kind === 'transport' ? `ModelGateway could not be reached: ${error.detail.message}` : error.detail.message
    }
    return error.message
  }
  return errorText(error)
}

/** `AppState.isUnauthorized(_:)`: the gateway no longer accepts the device key. */
export function isUnauthorized(error: unknown): boolean {
  return error instanceof BackendError && error.detail.type === 'rpc' && error.detail.kind === 'unauthorized'
}

/** `AppState.isSignedOut(_:)`: the backend has no stored ModelGateway key. */
export function isSignedOut(error: unknown): boolean {
  return error instanceof BackendError && error.detail.type === 'rpc' && error.detail.kind === 'signed_out'
}

function isGatewayRefusal(error: unknown): boolean {
  return error instanceof BackendError && error.detail.type === 'rpc' && error.detail.kind === 'gateway'
}

// MARK: - Account button

/// The sidebar's bottom-left account button (`AccountButton` in
/// Views/Sidebar/SidebarView.swift): shows the profile page on its last tab
/// and, landing on Account while signed out, starts the device sign-in.
export function openAccount(): void {
  useApp.getState().setSidebarItem('account')
  const state = gw()
  if (getPref('settingsTab') === SETTINGS_TAB_ACCOUNT && !state.isSignedIn && !state.isSigningIn) {
    beginGatewaySignIn()
  }
}

// MARK: - Session restore

/// `restoreGatewaySession()`: after the backend is up, refreshes a remembered
/// account, or forgets it when the backend no longer holds the key (signed out
/// elsewhere, a fresh data dir).
export async function restoreGatewaySession(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady()) return
  let signedIn = true
  try {
    signedIn = (await backend.gatewayStatus()).signed_in
  } catch {
    signedIn = true
  }
  if (!signedIn) {
    patch({ account: null, usage: null, usageReport: null })
    persistGatewayAccount()
    return
  }
  await refreshGateway()
}

// MARK: - Sign in

/** The running device flow (`GatewayState.pollTask`); `cancelled` stops it. */
interface PollTask {
  cancelled: boolean
}

let pollTask: PollTask | null = null

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

/// `beginGatewaySignIn()`: starts the device flow — a code pair from the
/// gateway, the browser opened on the approval page, then polling every
/// `interval` seconds until the user decides. Clicking again while a code is
/// pending just reopens the browser.
export function beginGatewaySignIn(): void {
  const flow = gw().flow
  if (flow?.kind === 'waiting') {
    openInBrowser(flow.verificationURL)
    return
  }
  if (gw().isSigningIn) return
  const config = useApp.getState().config
  if (!connectionReady() || config?.mock === true) {
    patch({
      flow: {
        kind: 'failed',
        message:
          config?.mock === true
            ? 'Sign-in needs the real backend; turn off the mock backend in Settings → General.'
            : 'Sign-in needs the backend running; start it and try again.',
      },
    })
    return
  }
  patch({ flow: { kind: 'starting' } })
  if (pollTask) pollTask.cancelled = true
  const task: PollTask = { cancelled: false }
  pollTask = task
  const origin = GATEWAY_ORIGIN
  void (async () => {
    let start
    try {
      start = await backend.gatewayDeviceStart(origin, null)
    } catch (error) {
      if (!task.cancelled) patch({ flow: { kind: 'failed', message: gatewayMessage(error) } })
      return
    }
    if (task.cancelled) return
    const expiresAt = Date.now() + Math.max(start.expires_in, 60) * 1000
    patch({ flow: { kind: 'waiting', userCode: start.user_code, verificationURL: start.verification_uri_complete, expiresAt } })
    openInBrowser(start.verification_uri_complete)
    await pollGateway(task, origin, start.device_code, Math.max(start.interval, 2), expiresAt)
  })()
}

/** `cancelGatewaySignIn()`. */
export function cancelGatewaySignIn(): void {
  if (pollTask) pollTask.cancelled = true
  pollTask = null
  patch({ flow: null })
}

/** `dismissGatewayError()`: clears a failed sign-in's message. */
export function dismissGatewayError(): void {
  if (gw().flow?.kind === 'failed') patch({ flow: null })
}

const EXPIRED_MESSAGE = 'The code expired before it was approved. Start the sign-in again.'

async function pollGateway(task: PollTask, origin: string, deviceCode: string, interval: number, expiresAt: number): Promise<void> {
  let consecutiveFailures = 0
  while (!task.cancelled) {
    await sleep(interval * 1000)
    if (task.cancelled) return
    if (Date.now() >= expiresAt) {
      patch({ flow: { kind: 'failed', message: EXPIRED_MESSAGE } })
      return
    }
    try {
      const poll = await backend.gatewayDevicePoll(origin, deviceCode)
      if (task.cancelled) return
      consecutiveFailures = 0
      switch (poll.status) {
        case 'pending':
          continue
        case 'approved':
          // The backend has stored the key by now.
          patch({ flow: { kind: 'completing' } })
          await completeGatewaySignIn(task, poll.account ?? null)
          return
        case 'denied':
          patch({ flow: { kind: 'failed', message: 'The request was denied in the browser.' } })
          return
        case 'expired':
          patch({ flow: { kind: 'failed', message: EXPIRED_MESSAGE } })
          return
        default:
          patch({ flow: { kind: 'failed', message: `Unexpected reply from ModelGateway: ${poll.status}` } })
          return
      }
    } catch (error) {
      if (task.cancelled) return
      // The gateway refused the code outright: no point polling on.
      if (isGatewayRefusal(error)) {
        patch({ flow: { kind: 'failed', message: gatewayMessage(error) } })
        return
      }
      consecutiveFailures += 1
      if (consecutiveFailures >= 3) {
        patch({ flow: { kind: 'failed', message: gatewayMessage(error) } })
        return
      }
    }
  }
}

async function completeGatewaySignIn(task: PollTask, account: GatewayAccount | null): Promise<void> {
  patch({ account, accountError: null })
  persistGatewayAccount()
  patch({ flow: null })
  if (pollTask === task) pollTask = null
  toast('Signed in to ModelGateway' + (account ? ` as ${accountName(account)}` : ''), 'success')
  if (getPref('routeThroughGateway')) {
    try {
      await applyGatewayEndpoint()
    } catch (error) {
      toast(`Signed in, but the model endpoint could not be switched to ModelGateway: ${gatewayMessage(error)}`, 'error')
    }
  }
  await refreshGateway()
  // Connected hosts use this sign-in too.
  await forwardSignInToHosts()
}

// MARK: - Sign out

/// `signOutOfGateway()`: has the backend revoke the device's key on the
/// gateway and forget it, then puts the previous model endpoint back.
export async function signOutOfGateway(): Promise<void> {
  if (gw().signingOut) return
  patch({ signingOut: true })
  try {
    cancelGatewaySignIn()
    const tokenName = gw().account?.token?.name ?? 'Context'
    if (connectionReady()) {
      try {
        const result = await backend.gatewaySignOut(GATEWAY_ORIGIN)
        if (!result.revoked) {
          toast('The device key could not be revoked on ModelGateway. Hidden device keys are revoked from the gateway when you sign in again.', 'warning')
        }
      } catch (error) {
        toast(`The device key could not be revoked on ModelGateway (${gatewayMessage(error)}); “${tokenName}” may still be active there.`, 'warning')
      }
    } else {
      toast('The backend is not running; the device key stays stored until it is.', 'warning')
    }
    patch({
      account: null,
      usage: null,
      usageReport: null,
      billing: null,
      billingError: null,
      keySettings: null,
      keySettingsError: null,
      usageError: null,
      accountError: null,
      lastRefreshed: null,
    })
    persistGatewayAccount()
    try {
      await restoreEndpointAfterGateway()
    } catch (error) {
      toast(`Signed out, but the previous model endpoint could not be restored: ${gatewayMessage(error)}`, 'error')
    }
    toast('Signed out of ModelGateway', 'info')
    // Connected hosts drop the forwarded key too.
    await forwardSignInToHosts()
  } finally {
    patch({ signingOut: false })
  }
}

// MARK: - Refresh

/** `refreshGateway()`: the account summary, then the usage rows. */
export async function refreshGateway(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady() || gw().refreshing) return
  patch({ refreshing: true })
  try {
    const origin = GATEWAY_ORIGIN
    try {
      const account = await backend.gatewayAccount(origin)
      patch({ account, accountError: null })
      persistGatewayAccount()
    } catch (error) {
      patch({
        accountError: isUnauthorized(error)
          ? "ModelGateway no longer accepts this device's key (it may have been revoked). Sign out and sign in again."
          : isSignedOut(error)
            ? 'The backend has no ModelGateway key stored. Sign out and sign in again.'
            : gatewayMessage(error),
      })
    }
    await fetchUsage(origin)
    patch({ lastRefreshed: Date.now() })
  } finally {
    patch({ refreshing: false })
  }
}

/** `refreshGatewayUsage()`: the usage rows only (the Usage card's Retry). */
export async function refreshGatewayUsage(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady()) return
  await fetchUsage(GATEWAY_ORIGIN)
}

async function fetchUsage(origin: string): Promise<void> {
  if (gw().usageLoading) return
  patch({ usageLoading: true })
  try {
    const end = Math.floor(Date.now() / 1000)
    const start = end - GATEWAY_USAGE_DAYS * 86_400
    try {
      const report = await backend.gatewayUsage(origin, start, end)
      // Folding a year of hourly rows into buckets: once per report, memoised.
      patch({ usageReport: report, usage: usageBreakdownsByRange(report), usageError: null })
    } catch (error) {
      patch({ usageError: gatewayMessage(error) })
    }
  } finally {
    patch({ usageLoading: false })
  }
}

// MARK: - Context credit

/** `claimContextCredit()`: claims the welcome Context credit (the Account page's card). */
export async function claimContextCredit(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady() || gw().claimingContextCredit) return
  patch({ claimingContextCredit: true })
  try {
    try {
      const credit = await backend.gatewayClaimContextCredit(GATEWAY_ORIGIN)
      const account = gw().account
      if (account) {
        patch({ account: { ...account, context_credit: credit } })
        persistGatewayAccount()
      }
      const updated = gw().account
      toast(`Claimed ${updated ? formatQuota(updated, credit.granted) : 'your'} Context credit`, 'success')
    } catch (error) {
      toast(`The credit could not be claimed: ${gatewayMessage(error)}`, 'error')
      // The gateway knows best whether it is claimable; show its view.
      await refreshGateway()
    }
  } finally {
    patch({ claimingContextCredit: false })
  }
}

// MARK: - Referrals

/** `refreshGatewayReferral()`: the Invite card's referees and terms (`gateway/referral`). */
export async function refreshGatewayReferral(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady() || gw().referralLoading) return
  patch({ referralLoading: true })
  try {
    const referral = await backend.gatewayReferral(GATEWAY_ORIGIN)
    patch({ referral, referralError: null })
  } catch (error) {
    patch({ referralError: gatewayMessage(error) })
  } finally {
    patch({ referralLoading: false })
  }
}

/** `setGatewayReferralSplit(_:)`: how much of the match pool future referees get. */
export async function setGatewayReferralSplit(cents: number): Promise<void> {
  if (!gw().isSignedIn || !connectionReady() || gw().referralBusy) return
  patch({ referralBusy: true })
  try {
    const result = await backend.gatewayUpdateReferralSplit(GATEWAY_ORIGIN, cents)
    const referral = gw().referral
    const account = gw().account
    patch({
      referral: referral ? { ...referral, summary: { ...referral.summary, split_referee_cents: result.split_referee_cents } } : null,
      account: account?.referral ? { ...account, referral: { ...account.referral, split_referee_cents: result.split_referee_cents } } : account,
    })
    persistGatewayAccount()
  } catch (error) {
    toast(`The split could not be changed: ${gatewayMessage(error)}`, 'error')
    await refreshGatewayReferral()
  } finally {
    patch({ referralBusy: false })
  }
}

/** `attachGatewayReferralCode(_:)`: adds a friend's code to this (new) account. */
export async function attachGatewayReferralCode(code: string): Promise<boolean> {
  if (!gw().isSignedIn || !connectionReady() || gw().referralBusy) return false
  patch({ referralBusy: true })
  try {
    const result = await backend.gatewayAttachReferralCode(GATEWAY_ORIGIN, code)
    const account = gw().account
    if (account?.referral) {
      patch({ account: { ...account, referral: { ...account.referral, invited_by: result.invited_by, attach_allowed: false } } })
      persistGatewayAccount()
    }
    toast(`${result.invited_by.display_name} is now your referrer`, 'success')
    await refreshGateway()
    return true
  } catch (error) {
    toast(`The code could not be added: ${gatewayMessage(error)}`, 'error')
    return false
  } finally {
    patch({ referralBusy: false })
  }
}

/// `reportResponse(projectId:turn:reason:comment:)`: reports a finished
/// assistant message as a bad response; the gateway counts it against the
/// provider that served it. True once filed.
export async function reportResponse(projectId: string, turn: AssistantTurn, reason: ResponseReportReason, comment: string): Promise<boolean> {
  if (turn.taskId === null || !connectionReady()) return false
  try {
    await backend.gatewayReport(GATEWAY_ORIGIN, projectId, turn.taskId, turn.step, reason, comment)
    patch({ reportedResponses: new Set([...gw().reportedResponses, assistantTurnReportKey(turn)]) })
    toast('Thanks, the response was reported', 'success')
    return true
  } catch (error) {
    toast(`The response could not be reported: ${gatewayMessage(error)}`, 'error')
    return false
  }
}

// MARK: - Settings

/** `setGatewayRouting(_:)`: "Route model requests through ModelGateway". */
export async function setGatewayRouting(enabled: boolean): Promise<void> {
  setPref('routeThroughGateway', enabled)
  if (!gw().isSignedIn) return
  try {
    if (enabled) await applyGatewayEndpoint()
    else await restoreEndpointAfterGateway()
  } catch (error) {
    toast(`The model endpoint could not be changed: ${gatewayMessage(error)}`, 'error')
  }
}

/** `setGatewayProjectAttribution(_:)`: "Attribute usage to projects" (the `X-ModelGateway-Project` header). */
export async function setGatewayProjectAttribution(enabled: boolean): Promise<void> {
  setPref('attributeProjects', enabled)
  if (!gatewayRoutingActive()) return
  try {
    await useApp.getState().patchConfig({ 'model.project_header': enabled ? GATEWAY_PROJECT_HEADER : null })
  } catch (error) {
    toast(`The project header setting could not be saved: ${gatewayMessage(error)}`, 'error')
  }
}

// MARK: - Key settings (Settings → Spending)

/** `refreshGatewayKeySettings()`. */
export async function refreshGatewayKeySettings(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady()) return
  try {
    patch({ keySettings: await backend.gatewayKeySettings(GATEWAY_ORIGIN), keySettingsError: null })
  } catch (error) {
    patch({ keySettingsError: gatewayMessage(error) })
  }
}

/// `refreshGatewayModels()`: `models/list` on the user-level endpoint, kept
/// only while that is the gateway (otherwise it lists the BYOK endpoint's
/// models) and the list came from it rather than the configured-model fallback.
export async function refreshGatewayModels(): Promise<void> {
  if (!gatewayRoutingActive() || !connectionReady()) return
  try {
    const list = await backend.modelsList()
    if (list.source === 'fallback') return
    patch({ models: list.models })
  } catch {
    // Keep the last list.
  }
}

/// `updateGatewayKeySettings(_:)`: saves `values` (`price_limits`,
/// `unlimited_quota`, `remain_quota`) on the device key. Throws.
export async function updateGatewayKeySettings(values: JSONObject): Promise<void> {
  const settings = await backend.gatewayUpdateKeySettings(GATEWAY_ORIGIN, values)
  patch({ keySettings: settings, keySettingsError: null })
}

// MARK: - Billing (Settings → Billing)

/** `refreshGatewayBilling()`. */
export async function refreshGatewayBilling(): Promise<void> {
  if (!gw().isSignedIn || !connectionReady() || gw().billingLoading) return
  patch({ billingLoading: true })
  try {
    patch({ billing: await backend.gatewayBilling(GATEWAY_ORIGIN), billingError: null })
  } catch (error) {
    patch({ billingError: gatewayMessage(error) })
  } finally {
    patch({ billingLoading: false })
  }
}

/** `setGatewayBillingPreference(_:)`: whether requests draw on the plan or the credit balance first. Throws. */
export async function setGatewayBillingPreference(preference: GatewayBillingPreference): Promise<void> {
  if (gw().billingBusy) return
  patch({ billingBusy: true })
  try {
    const result = await backend.gatewayBillingPreference(GATEWAY_ORIGIN, preference)
    const { billing, account } = gw()
    patch({
      billing: billing ? { ...billing, billing_preference: result.billing_preference } : billing,
      account: account ? { ...account, billing_preference: result.billing_preference } : account,
    })
    persistGatewayAccount()
  } finally {
    patch({ billingBusy: false })
  }
}

/** `quoteGatewayTopUp(_:)`: what a top-up of `amount` costs; null on failure. */
export async function quoteGatewayTopUp(amount: number): Promise<GatewayTopUpQuote | null> {
  try {
    return await backend.gatewayBillingQuote(GATEWAY_ORIGIN, amount)
  } catch {
    return null
  }
}

/// `addGatewayFunds(_:)`: opens a checkout page for `amount` in the browser
/// (the wallet page when the gateway cannot make one); the balance grows once
/// paid. Throws.
export async function addGatewayFunds(amount: number): Promise<void> {
  if (gw().billingBusy) return
  if (!(gw().billing?.topup.in_app ?? false)) {
    openGatewayWallet()
    return
  }
  patch({ billingBusy: true })
  try {
    const checkout = await backend.gatewayBillingTopUp(GATEWAY_ORIGIN, amount)
    const url = checkout.checkout_url
    if (!url) throw BackendError.rpc(-32000, 'ModelGateway did not return a checkout page.', 'gateway')
    openInBrowser(url)
  } finally {
    patch({ billingBusy: false })
  }
}

/// `subscribeToGatewayPlan(_:fromBalance:)`: buys `plan` by card in the
/// browser (`stripe`), or from the balance at once. Resolves with whether the
/// plan is active now (a balance purchase). Throws.
export async function subscribeToGatewayPlan(plan: GatewayPlan, fromBalance: boolean): Promise<boolean> {
  if (gw().billingBusy) return false
  patch({ billingBusy: true })
  try {
    const result = await backend.gatewayBillingSubscribe(GATEWAY_ORIGIN, plan.id, fromBalance ? 'balance' : 'stripe')
    if (result.checkout_url) {
      openInBrowser(result.checkout_url)
      return false
    }
    if (result.subscribed === true) {
      patch({ billingBusy: false })
      await refreshGateway()
      await refreshGatewayBilling()
      return true
    }
    throw BackendError.rpc(-32000, 'ModelGateway did not return a checkout page.', 'gateway')
  } finally {
    patch({ billingBusy: false })
  }
}

/** `openGatewayWallet()`: the gateway's own wallet page (other payment methods, redemption codes). */
export function openGatewayWallet(): void {
  const wallet = gw().billing?.wallet_url
  openInBrowser(wallet ? wallet : GATEWAY_ORIGIN + '/wallet')
}

/** `openGatewayDashboard()`. */
export function openGatewayDashboard(): void {
  openInBrowser(GATEWAY_ORIGIN + '/dashboard')
}

/** `openGatewayPlans()`: the wallet page, where plans are bought and listed. */
export function openGatewayPlans(): void {
  openInBrowser(GATEWAY_ORIGIN + '/wallet')
}

// MARK: - Endpoint switching

/// `applyGatewayEndpoint()`: points `model.base_url` at the gateway (keeping
/// the previous URL to restore), names the stored credential as the key
/// source and sets the project header. Idle projects pick the new provider up
/// at once; `api_key_env` is left alone for when the user signs out.
async function applyGatewayEndpoint(): Promise<void> {
  const model = useApp.getState().config?.model
  if (!model) return
  if (!isGatewayEndpoint(model)) setPref('gatewayPreviousEndpoint', { base_url: model.base_url })
  const values: JSONObject = {}
  if (model.base_url !== GATEWAY_MODEL_BASE_URL) values['model.base_url'] = GATEWAY_MODEL_BASE_URL
  if (model.credential !== GATEWAY_CREDENTIAL_NAME) values['model.credential'] = GATEWAY_CREDENTIAL_NAME
  values['model.project_header'] = getPref('attributeProjects') ? GATEWAY_PROJECT_HEADER : null
  await useApp.getState().patchConfig(values)
}

/// `restoreEndpointAfterGateway()`: the endpoint from before sign-in (or the
/// default), credential and project header off; the key variable is as the
/// user left it.
async function restoreEndpointAfterGateway(): Promise<void> {
  const model = useApp.getState().config?.model
  if (!model || !isGatewayEndpoint(model)) return
  const previous = getPref('gatewayPreviousEndpoint')['base_url']
  const baseURL = previous ? previous : BYOK_DEFAULT_BASE_URL
  await useApp.getState().patchConfig({ 'model.base_url': baseURL, 'model.credential': null, 'model.project_header': null })
  setPref('gatewayPreviousEndpoint', {})
}

/// `retireLegacyGatewayKey()`: configurations from the retired
/// secret-store-based gateway sign-in name `MODELGATEWAY_API_KEY` as the BYOK
/// key variable, so the BYOK key field would show that old gateway key. Points
/// the variable back at `OPENAI_API_KEY` and deletes the old entry.
export async function retireLegacyGatewayKey(): Promise<void> {
  const model = useApp.getState().config?.model
  if (!model || model.api_key_env !== LEGACY_GATEWAY_KEY_VARIABLE) return
  try {
    await useApp.getState().patchConfig({ 'model.api_key_env': BYOK_KEY_VARIABLE })
    await invoke<void>('secret_remove', { account: LEGACY_GATEWAY_KEY_VARIABLE })
  } catch (error) {
    toast(`The old ModelGateway key variable could not be retired: ${gatewayMessage(error)}`, 'warning')
  }
}

// MARK: - Contract names (apps/desktop/ARCHITECTURE.md "View phase")

/** Contract name of `beginGatewaySignIn` (the device flow). */
export const startDeviceSignIn = beginGatewaySignIn
/** Contract name of `signOutOfGateway`. */
export const signOut = signOutOfGateway
/** Contract name of `refreshGateway` (account summary, then usage). */
export const refreshAccount = refreshGateway

// MARK: - Registration with the app store

extendApp({ isGatewaySignedIn: () => useGateway.getState().isSignedIn })

// The account summary lives in `Preferences.gatewayAccount` (`persistGatewayAccount`
// writes it on every change). Follow writes made elsewhere — another module
// refreshing the summary, another window — so the page and the sidebar button
// never show a stale account. Our own writes store the same object and are skipped.
usePreferences.subscribe((next, previous) => {
  if (next.gatewayAccount === previous.gatewayAccount || next.gatewayAccount === gw().account) return
  const raw = next.gatewayAccount
  patch({ account: raw === null ? null : normalizeGatewayAccount(raw) })
  if (raw === null) patch({ usage: null, usageReport: null })
})

onBackendStarted(async () => {
  await retireLegacyGatewayKey()
  await restoreGatewaySession()
})
