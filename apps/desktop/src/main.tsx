import React from 'react'
import ReactDOM from 'react-dom/client'
import './theme/global.css'

/**
 * Entry point. Outside the Tauri shell (Vite in a plain browser, dev only)
 * the IPC mocks of src/dev/webBridge.ts are installed first, before any
 * module that reads Tauri globals at load time (platform detection) is
 * imported, so App is loaded dynamically.
 */
async function boot(): Promise<void> {
  const inTauri = '__TAURI_INTERNALS__' in window && !('__CTX_WEB_BRIDGE__' in window)
  const preview = import.meta.env.DEV && !inTauri
  if (preview && !document.querySelector('link[rel~="icon"]')) {
    // No favicon in the browser preview: an empty icon, set before the page's
    // load event, keeps the browser from asking for /favicon.ico (a 404 in
    // the console on every load).
    const icon = document.createElement('link')
    icon.rel = 'icon'
    icon.href = 'data:,'
    document.head.appendChild(icon)
  }
  if (preview) {
    const { installWebBridge } = await import('./dev/webBridge')
    await installWebBridge()
  }
  const { default: App } = await import('./App')
  if (preview) {
    // Visual checks read ids from the store (`window.__ctxApp.getState()`).
    const { useApp } = await import('./state/app')
    ;(window as unknown as Record<string, unknown>).__ctxApp = useApp
  }
  ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  )
}

void boot()
