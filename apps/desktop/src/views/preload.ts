/**
 * Warms the screens that load on first use (the Editor with CodeMirror,
 * Apps, the profile pages, the terminal's xterm.js) once the app is idle
 * after startup, so the first visit shows the screen at once instead of
 * its loading state. Each import is the same module specifier as the
 * matching `React.lazy`, so they share one chunk.
 */
export function preloadScreens(): void {
  const load = () => {
    void import('./Editor/EditorView')
    void import('./Apps/AppsView')
    void import('./Account/AccountView')
    void import('./Settings/SettingsSections')
    void import('./Settings/BYOKSettingsSection')
    void import('./Settings/SpendingSettingsSection')
    void import('./Settings/BillingSettingsSection')
    void import('./Inspector/TerminalHost')
  }
  const idle = (window as Window & { requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number }).requestIdleCallback
  if (idle) idle(load, { timeout: 3000 })
  else window.setTimeout(load, 1500)
}
