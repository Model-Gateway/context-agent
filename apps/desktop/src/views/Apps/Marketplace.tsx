import { memo, useMemo, useState, type MouseEvent } from 'react'
import { mcpServerTarget, type McpCatalogApp } from '../../backend/types'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Tooltip } from '../../components/Tooltip'
import { useApp } from '../../state/app'
import { useConnectionLabel, useIsReady } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { AppIcon } from './AppIcon'
import { InlineMessage, type InlineMessageData as AppsMessage } from '../../components/InlineMessage'
import { PromptBanner } from './PromptBanner'
import { StandingChip } from './StandingChip'
import { standingEquals, standingIsInstalled, standingOf, standingTitle, useCatalog, type AppStanding } from './standing'
import styles from './Marketplace.module.css'

export interface MarketplaceProps {
  /** App ids with an install / patch in flight. */
  busy: ReadonlySet<string>
  message: AppsMessage | null
  onOpen: (app: McpCatalogApp) => void
  onInstall: (app: McpCatalogApp) => void
  onConnectCustom: () => void
}

/**
 * `Marketplace` (Views/Apps/AppsView.swift): the title, the search field
 * with Connect beside it, the banner of example prompts (while not
 * searching), feedback, then the catalog two columns wide ("Featured" or
 * "Results") and the user's own servers.
 */
export function Marketplace({ busy, message, onOpen, onInstall, onConnectCustom }: MarketplaceProps) {
  const isReady = useIsReady()
  const connectionLabel = useConnectionLabel()
  const catalog = useCatalog()
  const config = useApp((state) => state.config)
  const servers = useApp((state) => state.mcpServers)
  const [query, setQuery] = useState('')

  // The banner shows apps with a logo first, Computer Use last.
  const featured = useMemo(
    () =>
      [...catalog].sort((a, b) => {
        if (a.builtin !== b.builtin) return a.builtin ? 1 : -1
        const rankA = a.logo != null ? 0 : 1
        const rankB = b.logo != null ? 0 : 1
        if (rankA !== rankB) return rankA - rankB
        return a.name < b.name ? -1 : a.name > b.name ? 1 : 0
      }),
    [catalog],
  )

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (q === '') return catalog
    return catalog.filter((app) => [app.name, app.tagline, app.category, app.vendor, app.description].some((field) => field.toLowerCase().includes(q)))
  }, [catalog, query])

  const custom = useMemo(() => {
    const catalogIds = new Set(catalog.map((app) => app.id))
    const all = config?.mcp.servers ?? {}
    return Object.keys(all)
      .filter((name) => all[name]?.app == null && !catalogIds.has(name))
      .sort()
  }, [catalog, config])

  return (
    <div className={styles.scroll}>
      <div className={styles.column}>
        <h1 className={styles.title}>Connect the tools your agent already uses</h1>
        <div className={styles.searchRow}>
          <div className={styles.search}>
            <Icon name="search" size={12} className={styles.magnifier} />
            <input
              className={styles.searchInput}
              type="text"
              value={query}
              placeholder="Search apps…"
              aria-label="Search apps"
              spellCheck={false}
              autoComplete="off"
              onChange={(event) => setQuery(event.target.value)}
            />
            {query !== '' ? (
              <button type="button" className={styles.clear} aria-label="Clear search" onClick={() => setQuery('')}>
                <Icon name="clear" size={13} />
              </button>
            ) : null}
          </div>
          <Tooltip content="Connect your own MCP server by its URL">
            <button type="button" className={styles.connect} disabled={!isReady} onClick={onConnectCustom}>
              <Icon name="newChat" size={13} strokeWidth={2} />
              <span>Connect</span>
            </button>
          </Tooltip>
        </div>
        {query === '' ? <PromptBanner apps={featured.slice(0, 3)} variant="marketplace" /> : null}
        {message ? <InlineMessage message={message} /> : null}
        {!isReady ? (
          <EmptyPlaceholder icon="apps" title="Apps need the backend" subtitle={connectionLabel} className={styles.placeholder} />
        ) : catalog.length === 0 ? (
          <div className={styles.loading}>
            <BrailleSpinner />
            <span>Loading the catalog…</span>
          </div>
        ) : (
          <>
            <section className={styles.section} aria-label={query === '' ? 'Featured' : 'Results'}>
              <h2 className={styles.heading}>{query === '' ? 'Featured' : 'Results'}</h2>
              {filtered.length === 0 ? (
                <p className={styles.noMatch}>No app matches “{query}”.</p>
              ) : (
                <div className={styles.grid}>
                  {filtered.map((app) => (
                    <MarketplaceRow
                      key={app.id}
                      app={app}
                      standing={standingOf(config, servers, app.id)}
                      busy={busy.has(app.id)}
                      onOpen={onOpen}
                      onInstall={onInstall}
                    />
                  ))}
                </div>
              )}
            </section>
            {custom.length > 0 && query === '' ? (
              <section className={styles.customSection} aria-label="Your servers">
                <h2 className={styles.heading}>Your servers</h2>
                <p className={styles.customNote}>Added with Connect, ctx mcp add or in ~/.context-agent/config.toml.</p>
                <div className={styles.customCard}>
                  {custom.map((name, index) => {
                    const server = config?.mcp.servers[name]
                    if (!server) return null
                    return (
                      <div key={name}>
                        <div className={styles.customRow}>
                          <AppIcon symbol="mcp" size={32} />
                          <div className={styles.customText}>
                            <span className={styles.customName}>{name}</span>
                            <MiddleTruncate text={mcpServerTarget(server)} className={styles.customTarget} />
                          </div>
                          <StandingChip standing={standingOf(config, servers, name)} />
                        </div>
                        {index < custom.length - 1 ? <div className={styles.divider} /> : null}
                      </div>
                    )
                  })}
                </div>
              </section>
            ) : null}
          </>
        )}
      </div>
    </div>
  )
}

interface MarketplaceRowProps {
  app: McpCatalogApp
  standing: AppStanding
  busy: boolean
  onOpen: (app: McpCatalogApp) => void
  onInstall: (app: McpCatalogApp) => void
}

/** `MarketplaceRow.standingTint`. */
function standingTint(standing: AppStanding): string {
  switch (standing.kind) {
    case 'connected':
    case 'installed':
      return 'var(--text-secondary)'
    case 'failed':
      return 'var(--danger)'
    default:
      return 'var(--text-tertiary)'
  }
}

/// `MarketplaceRow`: icon tile, name, one-line tagline, then "+" (install)
/// or a check (installed); the whole row opens the app's page. The row's
/// hit area is its own button under the content (not a `role="button"`
/// wrapping the "+" button), so the two controls are siblings.
const MarketplaceRow = memo(
  function MarketplaceRow({ app, standing, busy, onOpen, onInstall }: MarketplaceRowProps) {
    const install = (event: MouseEvent<HTMLButtonElement>) => {
      event.stopPropagation()
      onInstall(app)
    }
    return (
      <div className={styles.row}>
        <button type="button" className={styles.rowHit} aria-label={`${app.name}, ${standingTitle(standing)}`} onClick={() => onOpen(app)} />
        <AppIcon app={app} size={44} />
        <div className={styles.rowText} aria-hidden="true">
          <span className={styles.rowName}>{app.name}</span>
          <span className={styles.rowTagline}>{app.tagline}</span>
        </div>
        {busy ? (
          <span className={styles.trailing} aria-hidden="true">
            <BrailleSpinner size={12} />
          </span>
        ) : standingIsInstalled(standing) ? (
          <span className={styles.trailing} style={{ color: standingTint(standing) }} aria-hidden="true">
            <Icon name="checkmark" size={11} strokeWidth={2.5} />
          </span>
        ) : (
          <button type="button" className={styles.install} aria-label={`Connect ${app.name}`} onClick={install}>
            <Icon name="newChat" size={12} strokeWidth={2.25} />
          </button>
        )}
      </div>
    )
  },
  (previous, next) =>
    previous.app === next.app &&
    previous.busy === next.busy &&
    previous.onOpen === next.onOpen &&
    previous.onInstall === next.onInstall &&
    standingEquals(previous.standing, next.standing),
)
