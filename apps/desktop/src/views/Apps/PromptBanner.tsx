import type { McpCatalogApp } from '../../backend/types'
import bannerDusk from '../../assets/banners/banner-app-dusk.jpg'
import bannerEmber from '../../assets/banners/banner-app-ember.jpg'
import bannerForest from '../../assets/banners/banner-app-forest.jpg'
import bannerPastel from '../../assets/banners/banner-app-pastel.jpg'
import bannerMarketplace from '../../assets/banners/banner-marketplace.jpg'
import { AppIcon } from './AppIcon'
import styles from './PromptBanner.module.css'

type Sky = 'dusk' | 'pastel' | 'ember' | 'forest'

const SKIES: readonly Sky[] = ['dusk', 'pastel', 'ember', 'forest']

const SKY_IMAGES: Readonly<Record<Sky, string>> = {
  dusk: bannerDusk,
  pastel: bannerPastel,
  ember: bannerEmber,
  forest: bannerForest,
}

/** The skies chosen for the catalog's own apps. */
const CHOSEN: Readonly<Record<string, Sky>> = {
  'computer-use': 'ember',
  context7: 'forest',
  fetch: 'pastel',
  github: 'dusk',
  memory: 'dusk',
  playwright: 'forest',
  'sequential-thinking': 'pastel',
}

/** The marketplace's three pills sit offset like a conversation. */
const OFFSETS: readonly number[] = [-12, 10, -4]

/// `PromptBanner.appBanner(for:)`: the sky behind an app's banner, chosen
/// per catalog app and, for any other id, a stable pick (the sum of its
/// Unicode scalars) so the same app keeps the same one.
function appBanner(id: string): Sky {
  const chosen = CHOSEN[id]
  if (chosen) return chosen
  let sum = 0
  for (const character of id) sum += character.codePointAt(0) ?? 0
  return SKIES[sum % SKIES.length] ?? 'dusk'
}

export interface PromptBannerProps {
  /** Marketplace: the first three featured apps; app page: the app. */
  apps: McpCatalogApp[]
  variant: 'marketplace' | 'app'
}

/**
 * `PromptBanner` (Views/Apps/AppsView.swift): example prompts, each a pill
 * (logo + name chip, then the prompt), over a photographic background —
 * `banner-marketplace` on the marketplace, one of the `banner-app-*` skies
 * on an app's page. Marketplace: three stacked and offset; app page: one.
 * The image fills and overflows the 14 px-rounded banner and takes no clicks.
 */
export function PromptBanner({ apps, variant }: PromptBannerProps) {
  const image = variant === 'marketplace' ? bannerMarketplace : SKY_IMAGES[appBanner(apps[0]?.id ?? '')]
  return (
    <div
      className={[styles.banner, variant === 'marketplace' ? styles.marketplace : styles.app].join(' ')}
      role="group"
      aria-label="Example prompts"
    >
      <img className={styles.image} src={image} alt="" draggable={false} aria-hidden="true" />
      <span className={styles.stroke} aria-hidden="true" />
      {apps.map((app, index) => (
        <div
          key={app.id}
          className={styles.pill}
          style={variant === 'marketplace' ? { transform: `translateX(${OFFSETS[index % 3] ?? 0}px)` } : undefined}
        >
          <span className={styles.chip}>
            <AppIcon app={app} size={16} bare />
            <span className={styles.chipName}>{app.name}</span>
          </span>
          <span className={styles.prompt}>{app.example_prompt === '' ? app.tagline : app.example_prompt}</span>
        </div>
      ))}
    </div>
  )
}
