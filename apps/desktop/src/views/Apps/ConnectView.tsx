import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { backend } from '../../backend/client'
import { errorText } from '../../backend/errors'
import { mcpOAuthReference, type McpCatalogAccess, type McpCatalogApp, type McpDeviceCode, type McpServerConfig } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { copyText } from '../../components/clipboard'
import { isMac } from '../../components/platform'
import { useIsReady } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { LogoView } from '../../theme/logo'
import { AppIcon } from './AppIcon'
import { ConnectButton } from './ConnectButton'
import { InlineMessage } from '../../components/InlineMessage'
import { usePageKeys } from './pageKeys'
import styles from './ConnectView.module.css'

/** "Mac" in the Swift copy ("on this Mac"); "PC" on Windows and Linux. */
const MACHINE = isMac ? 'Mac' : 'PC'

/** `ConnectView.SignIn`: where the device-flow sign-in stands. */
type SignIn =
  | { kind: 'checking' }
  | { kind: 'signedOut' }
  | { kind: 'starting' }
  | { kind: 'waiting'; code: McpDeviceCode }
  | { kind: 'signedIn'; account: string }
  | { kind: 'failed'; message: string }

export interface ConnectViewProps {
  app: McpCatalogApp
  /** The entry when the app is installed already (Update…). */
  existing: McpServerConfig | null
  onCancel: () => void
  onApprove: (inputs: { [key: string]: string }, excludeTools: string[]) => void
}

/** `URL` → the default browser (`NSWorkspace.shared.open`). */
function openURL(url: string): void {
  invoke('open_url', { url }).catch(() => {
    /* Outside Tauri there is nothing to open with. */
  })
}

/** `seed()`: every group on for a new connection; for Update…, the groups whose tools the entry does not exclude. */
function seedGranted(app: McpCatalogApp, existing: McpServerConfig | null): Set<string> {
  const excluded = new Set(existing?.exclude_tools ?? [])
  return new Set(app.access.filter((group) => group.required || !group.tools.every((tool) => excluded.has(tool))).map((group) => group.title))
}

/**
 * `ConnectView` (Views/Apps/AppsView.swift): the connect page (Connect,
 * "+" or Update…) in place of a modal — the Context mark and the app's,
 * "Connect Context to GitHub", where the app runs or the credentials it
 * needs (or the GitHub sign-in through `mcp/oauth/start` + poll), "What
 * Context can access" with a checkbox per group of tools, then Cancel and
 * Approve. Groups left unchecked go to the entry's `exclude_tools`;
 * required groups stay on. Esc cancels, Return approves.
 */
export function ConnectView({ app, existing, onCancel, onApprove }: ConnectViewProps) {
  const isReady = useIsReady()
  const rootRef = useRef<HTMLDivElement>(null)
  const [values, setValues] = useState<{ [key: string]: string }>({})
  const [granted, setGranted] = useState<Set<string>>(() => seedGranted(app, existing))
  const [problem, setProblem] = useState<string | null>(null)
  const problemRef = useRef<HTMLDivElement>(null)

  // The problem is the last thing in the scroll view, which may end below
  // the fold above the pinned buttons: scroll to the end to show it.
  useEffect(() => {
    if (!problem) return
    const scroller = problemRef.current?.closest<HTMLElement>(`.${styles.scroll}`)
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' })
  }, [problem])
  const [auth, setAuthState] = useState<SignIn>({ kind: 'checking' })
  // "Use a personal access token instead".
  const [useToken, setUseToken] = useState(false)
  const authRef = useRef(auth)
  const mounted = useRef(true)

  const setAuth = (next: SignIn) => {
    authRef.current = next
    if (mounted.current) setAuthState(next)
  }

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  /** The app signs in instead of taking its token input. */
  const usesSignIn = app.oauth?.available === true && !useToken
  const isSignedIn = auth.kind === 'signedIn'
  const approveDisabled = !isReady || (usesSignIn && !isSignedIn)

  // loadAccount(): an entry with a pasted token keeps the token field;
  // otherwise ask the backend who Context is signed in as.
  useEffect(() => {
    const oauth = app.oauth
    if (!oauth || !oauth.available) return
    if (existing) {
      const value = existing.env[oauth.input] ?? existing.headers[oauth.input]
      if (value !== undefined && value !== '' && value !== mcpOAuthReference(oauth)) setUseToken(true)
    }
    void (async () => {
      let account: string | null = null
      try {
        account = await backend.mcpOAuthAccount(app.id)
      } catch {
        account = null
      }
      if (authRef.current.kind === 'checking') setAuth(account !== null ? { kind: 'signedIn', account } : { kind: 'signedOut' })
    })()
    // Once per page, like `.task`.
  }, [])

  const openVerification = (code: McpDeviceCode) => {
    void copyText(code.user_code)
    openURL(code.verification_uri)
  }

  const startSignIn = async () => {
    setProblem(null)
    setAuth({ kind: 'starting' })
    try {
      const code = await backend.mcpOAuthStart(app.id)
      setAuth({ kind: 'waiting', code })
      openVerification(code)
    } catch (error) {
      setAuth({ kind: 'failed', message: errorText(error) })
    }
  }

  const switchAccount = async () => {
    try {
      await backend.mcpOAuthSignOut(app.id)
    } catch {
      // Signed out or not, sign in again.
    }
    await startSignIn()
  }

  // pollWhileWaiting(): polls at GitHub's interval until the code is
  // approved, declined or expires; stops when the page leaves or the code changes.
  const waitingCode = auth.kind === 'waiting' ? auth.code : null
  useEffect(() => {
    const code = waitingCode
    if (!code) return
    let cancelled = false
    let timer: number | null = null
    let interval = Math.max(code.interval, 5)
    const deadline = Date.now() + Math.max(code.expires_in, 60) * 1000
    const stillWaiting = () => {
      const current = authRef.current
      return !cancelled && current.kind === 'waiting' && current.code.device_code === code.device_code
    }
    const tick = async () => {
      if (!stillWaiting()) return
      if (Date.now() > deadline) {
        setAuth({ kind: 'failed', message: 'The code expired. Sign in again.' })
        return
      }
      try {
        const poll = await backend.mcpOAuthPoll(app.id, code.device_code)
        if (!stillWaiting()) return
        switch (poll.status) {
          case 'approved':
            setAuth({ kind: 'signedIn', account: poll.account ?? app.name })
            try {
              void getCurrentWindow()
                .setFocus()
                .catch(() => undefined)
            } catch {
              // No window handle (a browser preview).
            }
            return
          case 'denied':
            setAuth({ kind: 'failed', message: 'The sign-in was declined on GitHub.' })
            return
          case 'expired':
            setAuth({ kind: 'failed', message: 'The code expired. Sign in again.' })
            return
          default:
            interval = Math.max(poll.interval ?? interval, 5)
        }
      } catch (error) {
        if (stillWaiting()) setAuth({ kind: 'failed', message: errorText(error) })
        return
      }
      timer = window.setTimeout(() => void tick(), interval * 1000)
    }
    timer = window.setTimeout(() => void tick(), interval * 1000)
    return () => {
      cancelled = true
      if (timer !== null) window.clearTimeout(timer)
    }
  }, [waitingCode?.device_code])

  const submit = () => {
    const inputs: { [key: string]: string } = {}
    for (const [key, value] of Object.entries(values)) {
      if (value.trim() !== '') inputs[key] = value
    }
    const oauth = app.oauth
    if (usesSignIn && oauth) {
      if (!isSignedIn) {
        setProblem(`Sign in with ${app.name} first.`)
        return
      }
      // The server reads the stored token; config.toml only names it.
      inputs[oauth.input] = mcpOAuthReference(oauth)
    }
    // Update…: an empty field keeps what is stored.
    if (existing) {
      for (const input of app.inputs) {
        if (inputs[input.key] !== undefined) continue
        const stored = input.kind === 'header' ? existing.headers[input.key] : existing.env[input.key]
        if (stored !== undefined) inputs[input.key] = stored
      }
    }
    const missing = app.inputs.find((input) => input.required && (inputs[input.key] ?? '') === '')
    if (missing) {
      setProblem(`${missing.label} is required.`)
      return
    }
    const optional = app.access.filter((group) => !group.required)
    if (optional.length > 0 && optional.every((group) => !granted.has(group.title)) && !app.access.some((group) => group.required)) {
      setProblem(`Choose at least one thing ${app.name} can do.`)
      return
    }
    const excluded = app.access.filter((group) => !group.required && !granted.has(group.title)).flatMap((group) => group.tools)
    onApprove(inputs, excluded)
  }

  usePageKeys(rootRef, {
    onEscape: onCancel,
    onReturn: () => {
      if (!approveDisabled) submit()
    },
  })

  const subtitle = app.builtin
    ? `Let the agent use ${app.name} on this ${MACHINE}.`
    : app.inputs.some((input) => input.required)
      ? `Grant Context access to your ${app.name} account.`
      : `Let the agent use ${app.name} in your chats.`

  return (
    <div ref={rootRef} className={styles.page} role="region" aria-label={`Connect ${app.name}`}>
      <div className={styles.scroll}>
        <div className={styles.column}>
          <div className={styles.header}>
            <div className={styles.marks} aria-hidden="true">
              <span className={styles.circle}>
                <LogoView size={20} label="" />
              </span>
              <span className={[styles.circle, styles.overlap].join(' ')}>
                <AppIcon app={app} size={24} bare />
              </span>
            </div>
            <div className={styles.titles}>
              <h1 className={styles.title}>{existing === null ? `Connect Context to ${app.name}` : `Update ${app.name}`}</h1>
              <p className={styles.subtitle}>{subtitle}</p>
            </div>
          </div>
          {app.inputs.length === 0 ? (
            <div className={styles.whereItRuns}>
              <span className={styles.whereIcon}>
                <Icon name={app.builtin ? 'laptop' : 'shippingBox'} size={13} />
              </span>
              <div className={styles.whereText}>
                <span className={styles.label}>{app.builtin ? `This ${MACHINE}` : `Runs on this ${MACHINE}`}</span>
                <span className={styles.caption}>{app.builtin ? 'Built into Context; nothing to install' : 'Started with ' + app.requirements.join(', ')}</span>
              </div>
            </div>
          ) : usesSignIn ? (
            <div className={styles.box}>
              <SignInCard
                app={app}
                auth={auth}
                isReady={isReady}
                onSignIn={() => void startSignIn()}
                onSwitchAccount={() => void switchAccount()}
                onCopyAndOpen={openVerification}
                onCancelWaiting={() => setAuth({ kind: 'signedOut' })}
              />
              {!isSignedIn && auth.kind !== 'checking' ? (
                <LinkButton onClick={() => setUseToken(true)}>Use a personal access token instead</LinkButton>
              ) : null}
            </div>
          ) : (
            <div className={styles.box}>
              {app.inputs.map((input) => (
                <div key={input.key} className={styles.inputGroup}>
                  <label className={styles.label} htmlFor={`connect-${app.id}-${input.key}`}>
                    {input.label + (input.required ? '' : ' (optional)')}
                  </label>
                  <input
                    id={`connect-${app.id}-${input.key}`}
                    className={styles.field}
                    type={input.secret ? 'password' : 'text'}
                    value={values[input.key] ?? ''}
                    placeholder={input.secret && existing !== null ? 'Leave empty to keep the current value' : input.key}
                    spellCheck={false}
                    autoComplete="off"
                    onChange={(event) => setValues((current) => ({ ...current, [input.key]: event.target.value }))}
                  />
                  <span className={styles.help}>{input.help}</span>
                </div>
              ))}
              <span className={styles.help}>{'Saved to ~/.context-agent/config.toml (readable only by you). Enter ${VAR} to read an environment variable instead.'}</span>
              {app.oauth?.available === true ? <LinkButton onClick={() => setUseToken(false)}>{`Sign in with ${app.name} instead`}</LinkButton> : null}
            </div>
          )}
          <div className={styles.access}>
            <span className={styles.accessTitle}>What Context can access</span>
            <div className={styles.accessOuter}>
              <div className={styles.accessInner}>
                {app.access.map((group) => (
                  <AccessRow
                    key={group.title}
                    group={group}
                    isOn={group.required || granted.has(group.title)}
                    onChange={(on) =>
                      setGranted((current) => {
                        const next = new Set(current)
                        if (on) next.add(group.title)
                        else next.delete(group.title)
                        return next
                      })
                    }
                  />
                ))}
              </div>
            </div>
            <span className={styles.accessNote}>You can change this later from the app's page.</span>
          </div>
          {problem ? (
            <div ref={problemRef}>
              <InlineMessage message={{ kind: 'error', text: problem }} />
            </div>
          ) : null}
        </div>
      </div>
      <div className={styles.footer}>
        <ConnectButton prominent={false} onClick={onCancel}>
          Cancel
        </ConnectButton>
        <ConnectButton prominent disabled={approveDisabled} onClick={submit}>
          {existing === null ? 'Approve' : 'Save'}
        </ConnectButton>
      </div>
    </div>
  )
}

interface SignInCardProps {
  app: McpCatalogApp
  auth: SignIn
  isReady: boolean
  onSignIn: () => void
  onSwitchAccount: () => void
  onCopyAndOpen: (code: McpDeviceCode) => void
  onCancelWaiting: () => void
}

/// `signInCard`: who Context acts as. Signed out, a "Sign in with GitHub"
/// row; waiting, the code to enter on GitHub.
function SignInCard({ app, auth, isReady, onSignIn, onSwitchAccount, onCopyAndOpen, onCancelWaiting }: SignInCardProps) {
  switch (auth.kind) {
    case 'checking':
      return (
        <div className={styles.checking}>
          <BrailleSpinner size={11} />
          <span>{`Checking your ${app.name} sign-in…`}</span>
        </div>
      )
    case 'signedIn':
      return (
        <div className={styles.accountRow}>
          <AppIcon app={app} size={18} bare />
          <div className={styles.whereText}>
            <span className={styles.label}>{auth.account}</span>
            <span className={styles.caption}>{`Signed in to ${app.name}`}</span>
          </div>
          <span className={styles.spacer} />
          <Button size="small" onClick={onSwitchAccount}>
            Switch account
          </Button>
        </div>
      )
    case 'waiting':
      return (
        <div className={styles.waiting}>
          <span className={styles.label}>Enter this code on GitHub</span>
          <div className={styles.codeRow}>
            <span className={[styles.code, 'selectable'].join(' ')}>{auth.code.user_code}</span>
            <span className={styles.spacer} />
            <Button size="small" onClick={() => onCopyAndOpen(auth.code)}>
              Copy code and open GitHub
            </Button>
          </div>
          <div className={styles.waitingRow}>
            <BrailleSpinner size={11} />
            <span className={styles.waitingText}>Waiting for you to approve on github.com…</span>
            <span className={styles.spacer} />
            <LinkButton onClick={onCancelWaiting}>Cancel</LinkButton>
          </div>
        </div>
      )
    case 'signedOut':
    case 'starting':
    case 'failed':
      return (
        <>
          <div className={styles.accountRow}>
            <AppIcon app={app} size={18} bare />
            <div className={styles.whereText}>
              <span className={styles.label}>{`Sign in with ${app.name}`}</span>
              <span className={styles.caption}>Approve Context in your browser; no token to copy.</span>
            </div>
            <span className={styles.spacer} />
            {auth.kind === 'starting' ? (
              <BrailleSpinner size={11} />
            ) : (
              <Button size="small" disabled={!isReady} onClick={onSignIn}>
                Sign in…
              </Button>
            )}
          </div>
          {auth.kind === 'failed' ? <InlineMessage message={{ kind: 'error', text: auth.message }} /> : null}
        </>
      )
  }
}

/** `.buttonStyle(.link).font(Theme.Font.caption)`. */
function LinkButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className={styles.link} onClick={onClick}>
      {children}
    </button>
  )
}

interface AccessRowProps {
  group: McpCatalogAccess
  isOn: boolean
  onChange: (on: boolean) => void
}

/// `AccessRow`: a checkbox, a title and a one-line description; the whole
/// row toggles it unless the group is required (always on).
function AccessRow({ group, isOn, onChange }: AccessRowProps) {
  const descriptionId = useId()
  const toggle = () => {
    if (!group.required) onChange(!isOn)
  }
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === ' ') {
      event.preventDefault()
      toggle()
    }
  }
  return (
    <div
      className={styles.accessRow}
      role="checkbox"
      tabIndex={0}
      aria-checked={isOn}
      aria-disabled={group.required || undefined}
      aria-label={group.title}
      aria-describedby={descriptionId}
      onClick={toggle}
      onKeyDown={onKeyDown}
    >
      <span className={[styles.checkbox, isOn ? styles.checked : '', group.required ? styles.checkboxDisabled : ''].filter(Boolean).join(' ')} aria-hidden="true">
        {isOn ? <Icon name="checkmark" size={10} strokeWidth={3.25} /> : null}
      </span>
      <div className={styles.accessText}>
        <span className={styles.label}>{group.title}</span>
        <span id={descriptionId} className={styles.accessDescription}>
          {group.description + (group.required ? ' (always on)' : '')}
        </span>
      </div>
    </div>
  )
}
