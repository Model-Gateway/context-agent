import { useState, type FormEvent } from 'react'
import { errorText } from '../../backend/errors'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Tooltip } from '../../components/Tooltip'
import { useApp } from '../../state/app'
import { apiKeyEnvName, useAnyProjectRunning } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { isMac } from '../../components/platform'
import { platform as osPlatform } from '@tauri-apps/plugin-os'
import styles from './ConnectModelCard.module.css'

/// Where `secret_set` keeps the key: "Keychain" in the Swift copy, the
/// platform's own store elsewhere (Windows Credential Manager, the Secret
/// Service keyring on Linux).
const CREDENTIAL_STORE: { inYour: string; inThe: string } = (() => {
  if (isMac) return { inYour: 'in your Keychain', inThe: 'in the Keychain' }
  let windows = false
  try {
    windows = osPlatform() === 'windows'
  } catch {
    windows = typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)
  }
  return windows
    ? { inYour: 'in Windows Credential Manager', inThe: 'in Windows Credential Manager' }
    : { inYour: 'in your keyring', inThe: 'in the keyring' }
})()

/**
 * `ConnectModelCard` (Components/ConnectModelCard.swift), shared by the
 * Welcome screen and the empty chat while `needsAPIKey`: the configured base
 * URL and model name (read-only), an API key field, Save (the OS credential
 * store, then a backend restart; disabled with "Finishes after the running
 * task" while any project runs) and a link to Settings → Model.
 */
export function ConnectModelCard() {
  const baseURL = useApp((state) => state.config?.model.base_url ?? null)
  const modelName = useApp((state) => state.config?.model.name ?? null)
  const keyVariable = useApp(apiKeyEnvName)
  const busy = useAnyProjectRunning()
  const [key, setKey] = useState('')
  const [saving, setSaving] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  const trimmed = key.trim()
  const canSave = !saving && !busy && trimmed.length > 0

  const save = (event?: FormEvent) => {
    event?.preventDefault()
    if (!canSave) return
    setSaving(true)
    setFailure(null)
    void (async () => {
      try {
        await useApp.getState().saveAPIKey(trimmed)
        setKey('')
        setSaved(true)
      } catch (error) {
        setFailure(errorText(error))
      }
      setSaving(false)
    })()
  }

  return (
    <section className={styles.card} aria-label="Connect a model">
      <div className={styles.header}>
        <Icon name="key" size={14} strokeWidth={2.1} className={styles.keyIcon} />
        <h2 className={styles.title}>Connect a model</h2>
      </div>
      <p className={styles.description}>No API key is set for the configured endpoint. Paste one below; it is stored {CREDENTIAL_STORE.inYour}.</p>
      <form className={styles.grid} onSubmit={save}>
        <span className={styles.label}>Base URL</span>
        <MiddleTruncate text={baseURL ?? '—'} className={`${styles.value} selectable`} />
        <span className={styles.label}>Model</span>
        <span className={`${styles.value} ${styles.tail} selectable`}>{modelName ?? '—'}</span>
        <span className={styles.label}>API key</span>
        <div className={styles.keyRow}>
          <input
            type="password"
            className={styles.secureField}
            placeholder="Paste your API key"
            value={key}
            onChange={(event) => setKey(event.target.value)}
            aria-label={`API key for ${keyVariable}`}
            autoComplete="off"
            spellCheck={false}
          />
          <Tooltip content={busy ? 'Finishes after the running task' : `Store the key ${CREDENTIAL_STORE.inThe} and restart the backend`}>
            <button type="submit" className={styles.save} disabled={!canSave}>
              {saving ? <BrailleSpinner size={11} tint="inherit" /> : null}
              <span>{saved ? 'Saved' : 'Save'}</span>
            </button>
          </Tooltip>
        </div>
      </form>
      {busy ? <p className={styles.caption}>Finishes after the running task</p> : null}
      {failure !== null ? <p className={`${styles.failure} selectable`}>{failure}</p> : null}
      <Tooltip content="Change the base URL, model name or key variable">
        <button type="button" className={styles.link} onClick={() => useApp.getState().openSettings('model')}>
          <span>Open Settings → Model</span>
          <Icon name="externalLink" size={10} strokeWidth={2.5} />
        </button>
      </Tooltip>
    </section>
  )
}
