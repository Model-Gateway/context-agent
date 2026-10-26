// Port of apps/macos/ContextAgent/Views/Settings/BYOKSettingsSection.swift.

import { useEffect, useState } from 'react'
import { errorText } from '../../backend/errors'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { setPref, usePref } from '../../models/preferences'
import { useApp } from '../../state/app'
import { anyProjectRunning, apiKeyEnvName, useAnyProjectRunning } from '../../state/selectors'
import { BYOK_DEFAULT_BASE_URL, setGatewayRouting, useGateway, useGatewayRoutingActive } from '../../state/gateway'
import { FormButtons, Help, TextField, Toggle } from './SettingsControls'
import { BackendUnavailable, DoctorResultView, InlineMessage, probeEndpointFromSettings, SettingsGroup, SettingsMessage, SettingsPage, SettingsRow } from './SettingsSections'
import { keyStoreArticle, keyStoreLongName, keyStoreName } from './settingsPlatform'
import styles from './BYOKSettingsSection.module.css'

/**
 * `BYOKSettingsSection` (BYOKSettingsSection.swift): bring your own key: the
 * OpenAI-compatible endpoint, its API key in the OS credential store, and
 * the endpoint probe. While signed in to ModelGateway with routing on,
 * requests use the gateway's key from sign-in; the endpoint fields then
 * edit the endpoint that turning routing off switches to.
 */
export function BYOKSettingsSection() {
  const hasConfig = useApp((s) => s.config !== null)
  const signedIn = useGateway((s) => s.isSignedIn)
  return (
    <SettingsPage>
      {hasConfig ? (
        <>
          {signedIn ? <RoutingGroup /> : null}
          <EndpointGroup />
          <ProbeGroup />
        </>
      ) : (
        <BackendUnavailable skeleton={[3, 2]} />
      )}
    </SettingsPage>
  )
}

// MARK: Routing

function RoutingGroup() {
  const routeRequests = usePref('routeThroughGateway')
  const routingActive = useGatewayRoutingActive()
  return (
    <SettingsGroup
      title="ModelGateway"
      footer="You are signed in to ModelGateway. Turn routing off to send requests to your own endpoint below with your own key; your ModelGateway account stays signed in."
    >
      <div className={styles.routing}>
        <div className={styles.routingText}>
          <span className={styles.routingTitle}>Route model requests through ModelGateway</span>
          <span className={styles.caption}>
            {routingActive ? 'On: requests use your ModelGateway balance or plan.' : 'Off: requests use your own endpoint and key below.'}
          </span>
        </div>
        <span className={styles.spacer} />
        <Toggle
          label="Route model requests through ModelGateway"
          labelHidden
          checked={routeRequests}
          onChange={(enabled) => void setGatewayRouting(enabled)}
        />
      </div>
    </SettingsGroup>
  )
}

// MARK: Endpoint

/// The endpoint saved for BYOK: the configured one, or while routing
/// through ModelGateway the one turning routing off restores.
function useSavedBaseURL(): string {
  const routingActive = useGatewayRoutingActive()
  const previous = usePref('gatewayPreviousEndpoint').base_url
  const configured = useApp((s) => s.config?.model.base_url ?? '')
  if (routingActive) return previous ? previous : BYOK_DEFAULT_BASE_URL
  return configured
}

function EndpointGroup() {
  const routing = useGatewayRoutingActive()
  const ready = useApp((s) => s.connection.kind === 'ready')
  const keyEnv = useApp(apiKeyEnvName)
  const savedBaseURL = useSavedBaseURL()
  const [endpoint, setEndpoint] = useState<{ baseURL: string; synced: string | null }>({ baseURL: '', synced: null })
  const [apiKey, setApiKey] = useState('')
  const [apiKeyStored, setApiKeyStored] = useState<boolean | null>(null)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsMessage | null>(null)

  /** `sync(_:)`: adopts a newly saved URL unless the field holds edits. */
  const sync = (url: string) =>
    setEndpoint((previous) => {
      const keepEdits = previous.synced !== null && previous.baseURL.trim() !== previous.synced
      return { synced: url, baseURL: keepEdits ? previous.baseURL : url }
    })

  // `.onChange(of: savedBaseURL, initial: true)`.
  useEffect(() => {
    sync(savedBaseURL)
  }, [savedBaseURL])

  // `.task(id: appState.apiKeyEnvName)`: whether the credential store holds the key.
  useEffect(() => {
    let live = true
    void useApp
      .getState()
      .isAPIKeyStored()
      .then((stored) => {
        if (live) setApiKeyStored(stored)
      })
    return () => {
      live = false
    }
  }, [keyEnv])

  const trimmedKey = apiKey.trim()
  const isDirty = endpoint.synced !== null && (endpoint.baseURL.trim() !== endpoint.synced || trimmedKey.length > 0)

  const revert = () => {
    if (endpoint.synced === null) return
    setEndpoint({ synced: endpoint.synced, baseURL: endpoint.synced })
    setApiKey('')
    setMessage(null)
  }

  const save = async () => {
    if (saving || !ready || !isDirty) return
    const url = endpoint.baseURL.trim()
    if (url.length === 0) {
      setMessage(SettingsMessage.error('Base URL cannot be empty.'))
      return
    }
    if (!url.startsWith('http://') && !url.startsWith('https://')) {
      setMessage(SettingsMessage.error('Base URL must start with http:// or https://.'))
      return
    }
    const key = trimmedKey
    const routingNow = routing
    const restarts = !anyProjectRunning(useApp.getState())
    setSaving(true)
    setMessage(null)
    try {
      if (url !== endpoint.synced) {
        if (routingNow) {
          setPref('gatewayPreviousEndpoint', { base_url: url })
          sync(url)
        } else {
          await useApp.getState().patchConfig({ 'model.base_url': url })
        }
      }
      if (key.length > 0) {
        await useApp.getState().saveAPIKey(key)
        setApiKey('')
        setApiKeyStored(true)
      }
      if (routingNow) {
        setMessage(SettingsMessage.success('Saved. Turning routing off uses this endpoint.'))
      } else if (key.length > 0 && !restarts) {
        setMessage(SettingsMessage.info('Saved. The key is used once the running task finishes and the backend restarts (General).'))
      } else {
        setMessage(SettingsMessage.success('Saved. Idle projects use the new endpoint now.'))
      }
    } catch (error) {
      setMessage(SettingsMessage.error(errorText(error)))
    } finally {
      setSaving(false)
    }
  }

  const removeKey = async () => {
    if (saving) return
    setSaving(true)
    setMessage(null)
    const restarts = !anyProjectRunning(useApp.getState())
    try {
      await useApp.getState().removeAPIKey()
      setApiKeyStored(false)
      setMessage(
        restarts
          ? SettingsMessage.success(`Key removed from ${keyStoreArticle}.`)
          : SettingsMessage.info(`Key removed from ${keyStoreArticle}. The running backend keeps it until it restarts.`),
      )
    } catch (error) {
      setMessage(SettingsMessage.error(errorText(error)))
    } finally {
      setSaving(false)
    }
  }

  return (
    <SettingsGroup
      title="Your endpoint"
      footer={`Any OpenAI-compatible endpoint (OpenAI, OpenRouter, a local server). The key is stored in ${keyStoreLongName} only. Saving rewrites ~/.context-agent/config.toml; idle projects use it at once, running ones after their task finishes.`}
    >
      {routing ? <InlineMessage message={SettingsMessage.info('Requests go through ModelGateway right now. Turning routing off switches to this endpoint and key.')} /> : null}
      <SettingsRow label="Base URL">
        <TextField
          placeholder={BYOK_DEFAULT_BASE_URL}
          aria-label="Base URL"
          value={endpoint.baseURL}
          onChange={(baseURL) => setEndpoint((previous) => ({ ...previous, baseURL }))}
          onSubmit={() => void save()}
        />
      </SettingsRow>
      <SettingsRow label="API key">
        <div className={styles.keyRow}>
          <TextField
            secure
            placeholder={apiKeyStored === true ? `Stored in ${keyStoreName}; paste a new key to replace it` : 'Paste your API key'}
            aria-label="API key"
            value={apiKey}
            onChange={setApiKey}
            onSubmit={() => void save()}
          />
          {apiKeyStored === true ? (
            <>
              <Chip icon="checkmark" variant="tinted" tint="var(--success)">
                Stored
              </Chip>
              <Button size="small" disabled={saving} onClick={() => void removeKey()}>
                Remove
              </Button>
            </>
          ) : apiKeyStored === false ? (
            <Chip>Not set</Chip>
          ) : (
            <BrailleSpinner size={11} />
          )}
        </div>
      </SettingsRow>
      <FormButtons onSave={() => void save()} onRevert={revert} dirty={isDirty} saving={saving} ready={ready} defaultAction />
      {message ? <InlineMessage message={message} /> : null}
    </SettingsGroup>
  )
}

// MARK: Probe

function ProbeGroup() {
  const doctorRunning = useApp((s) => s.doctorRunning)
  const doctorError = useApp((s) => s.doctorError)
  const doctorReport = useApp((s) => s.doctorReport)
  const ready = useApp((s) => s.connection.kind === 'ready')
  const anyRunning = useAnyProjectRunning()
  const probeDisabled = doctorRunning || anyRunning || !ready
  const probeHelp = !ready
    ? 'The backend is not connected'
    : anyRunning
      ? 'Finishes after the running task'
      : doctorRunning
        ? 'A probe is already running'
        : 'Send a test request to the model endpoint and check the configuration'
  return (
    <SettingsGroup
      title="Endpoint check"
      footer="Runs ctx doctor with a probe against the user-level configuration: real requests go to the endpoint, so it waits while a task is running. The capability profile it finds is saved for later runs."
    >
      <div className={styles.probeRow}>
        <Help text={probeHelp}>
          <Button size="small" disabled={probeDisabled} onClick={() => void probeEndpointFromSettings()}>
            Probe endpoint
          </Button>
        </Help>
        {doctorRunning ? (
          <>
            <BrailleSpinner />
            <span className={styles.captionSecondary}>Probing… this can take up to 2 minutes</span>
          </>
        ) : null}
      </div>
      {doctorError !== null ? <InlineMessage message={SettingsMessage.error(doctorError)} /> : null}
      {doctorReport !== null ? <DoctorResultView report={doctorReport} /> : null}
    </SettingsGroup>
  )
}

// MARK: - SettingsNumber

/** `SettingsNumber`: dollar amounts typed into and shown by the Settings forms. */
export const SettingsNumber = {
  /** "2", "2.5", "$0.50", "1,000" → a non-negative amount; null when it is not one. */
  usd(text: string): number | null {
    let cleaned = text.trim()
    if (cleaned.startsWith('$')) cleaned = cleaned.slice(1)
    cleaned = cleaned.replace(/,/g, '')
    if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(cleaned)) return null
    const value = Number(cleaned)
    if (!Number.isFinite(value) || value < 0) return null
    return value
  },

  /// 2.0 → "2", 0.5 → "0.5" (no grouping, up to six decimals). Always with
  /// a decimal point, so `usd` reads back what this writes.
  format(value: number): string {
    return new Intl.NumberFormat('en-US', { useGrouping: false, minimumFractionDigits: 0, maximumFractionDigits: 6 }).format(value)
  },
} as const
