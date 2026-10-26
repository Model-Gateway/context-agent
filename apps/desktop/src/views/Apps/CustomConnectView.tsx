import { useEffect, useRef, useState, type ReactNode } from 'react'
import { normalizeMcpServerConfig, type McpServerConfig } from '../../backend/types'
import { useIsReady } from '../../state/selectors'
import { LogoView } from '../../theme/logo'
import { AppIcon } from './AppIcon'
import { ConnectButton } from './ConnectButton'
import { InlineMessage } from '../../components/InlineMessage'
import { usePageKeys } from './pageKeys'
import styles from './ConnectView.module.css'

export interface CustomConnectViewProps {
  /** Names already used by servers or catalog apps. */
  taken: ReadonlySet<string>
  onCancel: () => void
  onConnect: (name: string, server: McpServerConfig) => void
}

/** `URL(string:)?.host`, lower-cased; null when the text is not a URL with a host. */
function hostOf(text: string): string | null {
  try {
    const host = new URL(text).hostname.toLowerCase()
    return host === '' ? null : host
  } catch {
    return null
  }
}

/** `CustomConnectView.isValidName(_:)`: the same rule as `McpServerConfig::valid_name` in ctx-core. */
function isValidName(name: string): boolean {
  return name !== '' && name.length <= 32 && /^[A-Za-z0-9_-]+$/.test(name)
}

/** `CustomConnectView.suggestedName(for:taken:)`: `https://mcp.linear.app/sse` → `linear`, made unique among `taken`. */
function suggestedName(url: string, taken: ReadonlySet<string>): string {
  const host = hostOf(url.trim())
  if (host === null) return ''
  const labels = host.split('.').filter((label) => label !== '' && !['mcp', 'www', 'api'].includes(label))
  const base = Array.from(labels[0] ?? host)
    .map((character) => (/^[A-Za-z0-9-]$/.test(character) ? character : '-'))
    .slice(0, 28)
    .join('')
  if (base === '') return ''
  let candidate = base
  let n = 2
  while (taken.has(candidate)) {
    candidate = `${base}-${n}`
    n += 1
  }
  return candidate
}

/**
 * `CustomConnectView` (Views/Apps/AppsView.swift): the connect page for a
 * server of the user's own (Connect beside the search field) — its MCP
 * URL, a name (from the URL's host until edited) and an optional header for
 * a key, then Cancel and Connect. The entry is an http
 * `[mcp.servers.<name>]` table, the same as `ctx mcp add --url`.
 */
export function CustomConnectView({ taken, onCancel, onConnect }: CustomConnectViewProps) {
  const isReady = useIsReady()
  const rootRef = useRef<HTMLDivElement>(null)
  const urlRef = useRef<HTMLInputElement>(null)
  const [url, setURL] = useState('')
  const [name, setName] = useState('')
  // The name was typed, so the URL no longer fills it.
  const [nameEdited, setNameEdited] = useState(false)
  const [headerName, setHeaderName] = useState('Authorization')
  const [headerValue, setHeaderValue] = useState('')
  const [problem, setProblem] = useState<string | null>(null)
  const problemRef = useRef<HTMLDivElement>(null)

  // The problem is the last thing in the scroll view, which may end below
  // the fold above the pinned buttons: scroll to the end to show it.
  useEffect(() => {
    if (!problem) return
    const scroller = problemRef.current?.closest<HTMLElement>(`.${styles.scroll}`)
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' })
  }, [problem])

  useEffect(() => {
    urlRef.current?.focus()
  }, [])

  const connectDisabled = !isReady || url.trim() === ''

  const submit = () => {
    const address = url.trim()
    let parsed: URL | null = null
    try {
      parsed = new URL(address)
    } catch {
      parsed = null
    }
    if (!parsed || !['http:', 'https:'].includes(parsed.protocol.toLowerCase()) || parsed.hostname === '') {
      setProblem('Enter a URL starting with https:// (or http://).')
      return
    }
    const key = name.trim()
    if (!isValidName(key)) {
      setProblem('Names use letters, digits, - and _ only (at most 32 characters).')
      return
    }
    if (taken.has(key)) {
      setProblem(`${key} is already used; choose another name.`)
      return
    }
    const headers: { [name: string]: string } = {}
    const value = headerValue.trim()
    if (value !== '') {
      const header = headerName.trim()
      if (header === '' || /[:\s]/.test(header)) {
        setProblem('Enter the header\'s name, such as Authorization.')
        return
      }
      headers[header] = value
    }
    setProblem(null)
    onConnect(key, normalizeMcpServerConfig({ transport: 'http', url: address, headers }))
  }

  usePageKeys(rootRef, {
    onEscape: onCancel,
    onReturn: () => {
      if (!connectDisabled) submit()
    },
  })

  return (
    <div ref={rootRef} className={styles.page} role="region" aria-label="Connect a custom app">
      <div className={styles.scroll}>
        <div className={styles.column}>
          <div className={styles.header}>
            <div className={styles.marks} aria-hidden="true">
              <span className={styles.circle}>
                <LogoView size={20} label="" />
              </span>
              <span className={[styles.circle, styles.overlap].join(' ')}>
                <AppIcon symbol="mcp" size={24} bare />
              </span>
            </div>
            <div className={styles.titles}>
              <h1 className={styles.title}>Connect a custom app</h1>
              <p className={styles.subtitle}>Add a remote MCP server by its URL. Its tools become available in every chat.</p>
            </div>
          </div>
          <div className={styles.box}>
            <Field id="custom-url" label="Server URL" help="The server's streamable HTTP endpoint, such as https://mcp.example.com/mcp.">
              <input
                id="custom-url"
                ref={urlRef}
                className={styles.field}
                type="text"
                value={url}
                placeholder="https://"
                spellCheck={false}
                autoComplete="off"
                onChange={(event) => {
                  const value = event.target.value
                  setURL(value)
                  if (!nameEdited) setName(suggestedName(value, taken))
                }}
              />
            </Field>
            <Field id="custom-name" label="Name" help="How the agent's tools are prefixed. Letters, digits, - and _ only.">
              <input
                id="custom-name"
                className={styles.field}
                type="text"
                value={name}
                placeholder="my-server"
                spellCheck={false}
                autoComplete="off"
                onChange={(event) => {
                  setName(event.target.value)
                  setNameEdited(true)
                }}
              />
            </Field>
            <div className={styles.inputGroup}>
              <span className={styles.label}>Header (optional)</span>
              <div className={styles.headerFields}>
                <input
                  className={[styles.field, styles.headerName].join(' ')}
                  type="text"
                  value={headerName}
                  placeholder="Header"
                  aria-label="Header name"
                  spellCheck={false}
                  autoComplete="off"
                  onChange={(event) => setHeaderName(event.target.value)}
                />
                <input
                  className={styles.field}
                  type="password"
                  value={headerValue}
                  placeholder="Bearer …"
                  aria-label="Header value"
                  autoComplete="off"
                  onChange={(event) => setHeaderValue(event.target.value)}
                />
              </div>
              <span className={styles.help}>
                {
                  'Sent with every request, for servers that take an API key. Saved to ~/.context-agent/config.toml (readable only by you); enter ${VAR} to read an environment variable instead.'
                }
              </span>
            </div>
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
        <ConnectButton prominent disabled={connectDisabled} onClick={submit}>
          Connect
        </ConnectButton>
      </div>
    </div>
  )
}

/** `field(_:help:content:)`: a label, the input, the help line. */
function Field({ id, label, help, children }: { id: string; label: string; help: string; children: ReactNode }) {
  return (
    <div className={styles.inputGroup}>
      <label className={styles.label} htmlFor={id}>
        {label}
      </label>
      {children}
      <span className={styles.help}>{help}</span>
    </div>
  )
}
