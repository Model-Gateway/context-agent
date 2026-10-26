import { writeText } from '@tauri-apps/plugin-clipboard-manager'

/**
 * `NSPasteboard.general.setString` of the macOS app: writes `text` to the
 * system clipboard through the Tauri clipboard plugin, falling back to
 * `navigator.clipboard` when the plugin is unavailable (Vite in a plain
 * browser, or the plugin refused). Resolves false when neither worked.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    await writeText(text)
    return true
  } catch {
    // Not running under Tauri: fall through to the web clipboard.
  }
  try {
    if (typeof navigator !== 'undefined' && navigator.clipboard) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    // Permission denied or no focus.
  }
  return false
}
