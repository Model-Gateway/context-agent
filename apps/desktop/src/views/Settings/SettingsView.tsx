// Port of apps/macos/ContextAgent/Views/Settings/SettingsView.swift: the
// profile page (`ProfileView`) with its column of tabs.

import { lazy, Suspense, useLayoutEffect, useRef, useState } from 'react'
import { Skeleton } from '../../components/Skeleton'
import { Tooltip } from '../../components/Tooltip'
import { usePrefState } from '../../models/preferences'
import { useGateway } from '../../state/gateway'
import { Icon, type IconName } from '../../theme/icons'
import styles from './SettingsView.module.css'

// The pages load on first use (views/preload.ts warms them once the app is
// up), so startup does not wait for the forms and the usage charts.
const AccountView = lazy(() => import('../Account/AccountView').then((module) => ({ default: module.AccountView })))
const BillingSettingsSection = lazy(() => import('./BillingSettingsSection').then((module) => ({ default: module.BillingSettingsSection })))
const BYOKSettingsSection = lazy(() => import('./BYOKSettingsSection').then((module) => ({ default: module.BYOKSettingsSection })))
const SpendingSettingsSection = lazy(() => import('./SpendingSettingsSection').then((module) => ({ default: module.SpendingSettingsSection })))
const GeneralSettingsSection = lazy(() => import('./SettingsSections').then((module) => ({ default: module.GeneralSettingsSection })))
const ModelSettingsSection = lazy(() => import('./SettingsSections').then((module) => ({ default: module.ModelSettingsSection })))
const PermissionsSettingsSection = lazy(() => import('./SettingsSections').then((module) => ({ default: module.PermissionsSettingsSection })))
const AppearanceSettingsSection = lazy(() => import('./SettingsSections').then((module) => ({ default: module.AppearanceSettingsSection })))

/** `SettingsTab`: a profile page tab; the raw value is remembered (`settingsTab`). */
export type SettingsTab = 'account' | 'general' | 'model' | 'byok' | 'spending' | 'billing' | 'permissions' | 'appearance'

/** `SettingsTab.allCases`. */
export const SETTINGS_TABS: readonly SettingsTab[] = ['account', 'general', 'model', 'byok', 'spending', 'billing', 'permissions', 'appearance']

/** `SettingsTab.title`. */
export function settingsTabTitle(tab: SettingsTab): string {
  switch (tab) {
    case 'account':
      return 'Account'
    case 'general':
      return 'General'
    case 'model':
      return 'Model'
    case 'byok':
      return 'BYOK'
    case 'spending':
      return 'Spending'
    case 'billing':
      return 'Billing'
    case 'permissions':
      return 'Permissions'
    case 'appearance':
      return 'Appearance'
  }
}

/** `SettingsTab.subtitle`. */
function settingsTabSubtitle(tab: SettingsTab): string | null {
  switch (tab) {
    case 'byok':
      return 'Your own endpoint and key'
    case 'billing':
      return 'ModelGateway'
    default:
      return null
  }
}

/** `SettingsTab.icon`. */
function settingsTabIcon(tab: SettingsTab): IconName {
  switch (tab) {
    case 'account':
      return 'account'
    case 'general':
      return 'settings'
    case 'model':
      return 'model'
    case 'byok':
      return 'byok'
    case 'spending':
      return 'spending'
    case 'billing':
      return 'billing'
    case 'permissions':
      return 'approval'
    case 'appearance':
      return 'appearance'
  }
}

/** `ProfileView.compactWidth`: narrower than this, the tab column shows icons only. */
const COMPACT_WIDTH = 720

/**
 * `ProfileView` (SettingsView.swift): the profile page, opened from the
 * sidebar's account button and by mod+, (`openSettings`): a column of tabs
 * and the selected one. Account is AccountView; the rest are the settings
 * forms. Billing is listed only while signed in to ModelGateway. Narrower
 * than 720 px (a small window, or a wide inspector) the column shows icons
 * only, with the titles as tooltips.
 */
export function ProfileView() {
  const [settingsTab, setSettingsTab] = usePrefState('settingsTab')
  const signedIn = useGateway((s) => s.isSignedIn)
  const rootRef = useRef<HTMLDivElement>(null)
  const [compact, setCompact] = useState(false)

  // `.onGeometryChange(for: Bool.self) { $0.size.width < compactWidth }`.
  useLayoutEffect(() => {
    const element = rootRef.current
    if (!element) return
    const update = () => setCompact(element.getBoundingClientRect().width < COMPACT_WIDTH)
    update()
    const observer = new ResizeObserver(update)
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  const tabs = SETTINGS_TABS.filter((tab) => tab !== 'billing' || signedIn)
  const selected = tabs.find((tab) => tab === settingsTab) ?? 'account'

  return (
    <div ref={rootRef} className={styles.profile} role="group" aria-label="Profile">
      <nav className={[styles.column, compact ? styles.compact : ''].filter(Boolean).join(' ')} aria-label="Profile tabs">
        {tabs.map((tab) => (
          <SettingsTabRow key={tab} tab={tab} isSelected={tab === selected} compact={compact} onSelect={() => setSettingsTab(tab)} />
        ))}
      </nav>
      <div className={styles.divider} aria-hidden="true" />
      {/* Never wider than what is left: a wide form must not push the tab column out of the window. */}
      <div className={styles.content}>
        <Suspense fallback={<PageSkeleton />}>
          <TabContent tab={selected} />
        </Suspense>
      </div>
    </div>
  )
}

/** A page still loading: the settings page skeleton in the page's frame (`SettingsPage`). */
function PageSkeleton() {
  return (
    <div className={styles.loadingPage}>
      <div className={styles.loadingPageContent}>
        <Skeleton.SettingsPage label="Loading" />
      </div>
    </div>
  )
}

function TabContent({ tab }: { tab: SettingsTab }) {
  switch (tab) {
    case 'account':
      return <AccountView />
    case 'general':
      return <GeneralSettingsSection />
    case 'model':
      return <ModelSettingsSection />
    case 'byok':
      return <BYOKSettingsSection />
    case 'spending':
      return <SpendingSettingsSection />
    case 'billing':
      return <BillingSettingsSection />
    case 'permissions':
      return <PermissionsSettingsSection />
    case 'appearance':
      return <AppearanceSettingsSection />
  }
}

/** `SettingsTabRow`: icon, title and subtitle; icon only (title as the tooltip) when compact. */
function SettingsTabRow({ tab, isSelected, compact, onSelect }: { tab: SettingsTab; isSelected: boolean; compact: boolean; onSelect: () => void }) {
  const title = settingsTabTitle(tab)
  const subtitle = settingsTabSubtitle(tab)
  const row = (
    <button
      type="button"
      aria-label={title}
      aria-current={isSelected ? 'page' : undefined}
      className={[styles.tab, isSelected ? styles.selected : '', compact ? styles.tabCompact : ''].filter(Boolean).join(' ')}
      onClick={onSelect}
    >
      <span className={styles.tabIcon} aria-hidden="true">
        <Icon name={settingsTabIcon(tab)} size={compact ? 14 : 13} strokeWidth={2} />
      </span>
      {compact ? null : (
        <span className={styles.tabText}>
          <span className={styles.tabTitle}>{title}</span>
          {subtitle !== null ? <span className={styles.tabSubtitle}>{subtitle}</span> : null}
        </span>
      )}
    </button>
  )
  return (
    <Tooltip content={[title, subtitle].filter((part): part is string => part !== null).join(' · ')} disabled={!compact}>
      {row}
    </Tooltip>
  )
}
