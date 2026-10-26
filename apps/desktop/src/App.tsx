import { useEffect } from 'react'
import { applyAppearance, getPref } from './models/preferences'
import { launchOptions, useApp } from './state/app'
import { MainWindow } from './views/MainWindow'
import { preloadScreens } from './views/preload'
import { checkRecentFolders } from './views/Welcome/RecentCard'

// Before the first paint, so a Light / Dark override never flashes
// (`AppDelegate.applicationWillFinishLaunching` → `applyAppearance`).
applyAppearance()

/**
 * The app root (App/ContextAgentApp.swift): the main window, and the
 * backend started once when it first appears (`claimBackendStart` keeps a
 * remount, React's development double effect included, from starting it
 * again). With `--autotest <dir>` the scripted self-test (`selftest.ts`,
 * `UITestDriver.startIfRequested`) runs once the start returned.
 */
export default function App() {
  useEffect(() => {
    const state = useApp.getState()
    if (!state.claimBackendStart()) return
    // The recents' folder checks, before Welcome first draws its cards.
    checkRecentFolders(getPref('recents').map((recent) => recent.path))
    void state.startBackend().finally(() => {
      // The lazily loaded screens, once startup's own work is done.
      preloadScreens()
      void launchOptions().then((options) => {
        if (options.autotest === null) return
        // Loaded only for a self-test run, so it stays out of the main chunk.
        void import('./selftest').then((selftest) => selftest.startSelfTestIfRequested())
      })
    })
  }, [])
  return <MainWindow />
}
