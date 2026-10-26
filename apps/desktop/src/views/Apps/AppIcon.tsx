import type { CSSProperties } from 'react'
import type { McpCatalogApp } from '../../backend/types'
import { Icon, Icons, type IconName } from '../../theme/icons'
import githubLogo from '../../assets/logos/logo-github.svg'
import mcpLogo from '../../assets/logos/logo-mcp.svg'
import playwrightLogo from '../../assets/logos/logo-playwright.svg'
import upstashLogo from '../../assets/logos/logo-upstash.svg'
import styles from './AppIcon.module.css'

/** The brand marks of `Resources/Assets.xcassets/Logos` (`logo-<name>`), by name. */
const LOGOS: Readonly<Record<string, string>> = {
  github: githubLogo,
  mcp: mcpLogo,
  playwright: playwrightLogo,
  upstash: upstashLogo,
}

/// The catalog's own app → logo pairs (`crates/tools/src/mcp/catalog.rs`),
/// so `appLogo` also answers for an app id when a caller only has that
/// (the composer's `@` menu lists servers by name).
const CATALOG_LOGOS: Readonly<Record<string, string>> = {
  context7: 'upstash',
  fetch: 'mcp',
  github: 'github',
  memory: 'mcp',
  playwright: 'playwright',
  'sequential-thinking': 'mcp',
}

/// The SF Symbols `mcp/catalog` names for its apps (`CatalogApp.icon`),
/// mapped to the `Icons` catalogue; an unknown symbol draws `Icons.mcp`.
const SYMBOLS: Readonly<Record<string, IconName>> = {
  'macwindow.and.cursorarrow': 'computerUse',
  'books.vertical': 'books',
  'arrow.down.doc': 'docDown',
  'chevron.left.forwardslash.chevron.right': 'editor',
  brain: 'thinking',
  theatermasks: 'theaterMasks',
  'list.number': 'listNumber',
  'puzzlepiece.extension': 'mcp',
  'square.grid.2x2': 'apps',
}

/// The bundled logo for `id`: a logo name as the catalog gives it
/// (`McpCatalogApp.logo`: "github"), its asset name ("logo-github") or a
/// catalog app id ("context7" → the Upstash mark). Null when the app has
/// no bundled mark (`NSImage(named:) == nil` in AppIcon.swift), so the
/// caller falls back to the app's symbol. Shared with the composer's `@` menu.
export function appLogo(id: string | null | undefined): string | null {
  if (!id) return null
  const name = id.startsWith('logo-') ? id.slice('logo-'.length) : id
  return LOGOS[name] ?? LOGOS[CATALOG_LOGOS[name] ?? ''] ?? null
}

/** An SF Symbol name (as `mcp/catalog` sends it) or an `IconName` → the glyph to draw. */
function symbolIcon(symbol: string | null | undefined): IconName {
  if (!symbol) return 'mcp'
  if (Object.prototype.hasOwnProperty.call(Icons, symbol)) return symbol as IconName
  return SYMBOLS[symbol] ?? 'mcp'
}

export interface AppIconProps {
  /** `AppIcon(app:size:bare:)`: takes the app's `icon`, `logo` and `logo_monochrome`. */
  app?: McpCatalogApp | null
  /** `AppIcon(symbol:)`: an SF Symbol name as `mcp/catalog` sends it, or an `IconName` (`"mcp"`). */
  symbol?: string | null
  /** A logo name (`logo-<name>` in the asset catalog); ignored when `app` is given. */
  logo?: string | null
  /** The logo is a single-colour mark, tinted like a template image. */
  monochrome?: boolean
  /** Side length in px. */
  size: number
  /** The mark alone in `accent` (inside a chip), without the tile. */
  bare?: boolean
  className?: string
  style?: CSSProperties
}

/**
 * `AppIcon` (Views/Apps/AppsView.swift): an app's tile — its brand logo
 * from the bundled assets (tinted when it is a single-colour mark) when the
 * catalog names one, else its symbol in `accent` on a `surface` tile with a
 * hairline and a 27 % corner radius. `bare` draws the mark alone. Shared
 * with the composer's `@` menu. Decorative (hidden from assistive tech).
 */
export function AppIcon({ app, symbol, logo, monochrome, size, bare = false, className, style }: AppIconProps) {
  const logoName = app ? app.logo : logo
  const isMonochrome = app ? app.logo_monochrome : (monochrome ?? false)
  const glyph = symbolIcon(app ? app.icon : symbol)
  const source = logoName ? (LOGOS[logoName] ?? null) : null
  const radius = size * 0.27
  const frame: CSSProperties = { width: size, height: size, ...style }

  if (source) {
    const markSize = bare ? size : size - 2 * size * 0.22
    const mark = isMonochrome ? (
      <span
        className={styles.mask}
        style={{ width: markSize, height: markSize, WebkitMaskImage: `url("${source}")`, maskImage: `url("${source}")` }}
      />
    ) : (
      <img className={styles.image} src={source} alt="" draggable={false} style={{ width: markSize, height: markSize }} />
    )
    if (bare) {
      return (
        <span className={[styles.bare, className ?? ''].filter(Boolean).join(' ')} style={frame} aria-hidden="true">
          {mark}
        </span>
      )
    }
    return (
      <span
        className={[styles.tile, styles.logoTile, className ?? ''].filter(Boolean).join(' ')}
        style={{ ...frame, borderRadius: radius }}
        aria-hidden="true"
      >
        {mark}
      </span>
    )
  }
  if (bare) {
    return (
      <span className={[styles.bare, className ?? ''].filter(Boolean).join(' ')} style={frame} aria-hidden="true">
        <Icon name={glyph} size={size * 0.75} />
      </span>
    )
  }
  return (
    <span className={[styles.tile, className ?? ''].filter(Boolean).join(' ')} style={{ ...frame, borderRadius: radius }} aria-hidden="true">
      <Icon name={glyph} size={size * 0.45} />
    </span>
  )
}
