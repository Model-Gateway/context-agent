import { platform as osPlatform } from '@tauri-apps/plugin-os'

/**
 * Platform facts the shared components need (`Platform` in the macOS
 * ARCHITECTURE.md's platform mapping): which modifier key is `mod`, how
 * shortcuts are spelled, and the file-manager wording.
 */

function detectMac(): boolean {
  try {
    return osPlatform() === 'macos'
  } catch {
    // Outside Tauri (vite in a browser) the plugin throws; fall back to the UA.
    if (typeof navigator === 'undefined') return false
    const ua = navigator.userAgent || ''
    const plat = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ?? navigator.platform ?? ''
    return /Mac|iPhone|iPad/i.test(plat) || /Macintosh|Mac OS X/i.test(ua)
  }
}

/** True on macOS, where `mod` is ⌘ and paths are revealed in Finder. */
export const isMac: boolean = detectMac()

/** The `mod` key's label: "⌘" on macOS, "Ctrl" elsewhere. */
export const modLabel: string = isMac ? '⌘' : 'Ctrl'

/** "Reveal in Finder" on macOS, "Show in Explorer" on Windows and Linux. */
export const revealLabel: string = isMac ? 'Reveal in Finder' : 'Show in Explorer'

/** Whether a keyboard event carries the platform's `mod` modifier. */
export function isModKey(event: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return isMac ? event.metaKey : event.ctrlKey
}

const MAC_MODIFIERS: Record<string, string> = { ctrl: '⌃', alt: '⌥', shift: '⇧', mod: '⌘', cmd: '⌘', meta: '⌘' }
const WIN_MODIFIERS: Record<string, string> = { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', mod: 'Ctrl', cmd: 'Ctrl', meta: 'Win' }
/** macOS orders modifiers ⌃ ⌥ ⇧ ⌘. */
const MAC_ORDER = ['ctrl', 'alt', 'shift', 'mod']
/** Windows and Linux spell Ctrl first: Ctrl+Shift+K, Ctrl+Alt+S. */
const WIN_ORDER = ['mod', 'ctrl', 'alt', 'shift']

const MAC_KEYS: Record<string, string> = {
  enter: '⏎',
  return: '⏎',
  backspace: '⌫',
  delete: '⌦',
  escape: 'Esc',
  esc: 'Esc',
  tab: '⇥',
  space: 'Space',
  arrowup: '↑',
  arrowdown: '↓',
  arrowleft: '←',
  arrowright: '→',
  up: '↑',
  down: '↓',
  left: '←',
  right: '→',
}
const WIN_KEYS: Record<string, string> = {
  enter: 'Enter',
  return: 'Enter',
  backspace: 'Backspace',
  delete: 'Del',
  escape: 'Esc',
  esc: 'Esc',
  tab: 'Tab',
  space: 'Space',
  arrowup: '↑',
  arrowdown: '↓',
  arrowleft: '←',
  arrowright: '→',
  up: '↑',
  down: '↓',
  left: '←',
  right: '→',
}

/**
 * Spells a `mod+shift+K`-style shortcut for the platform: "⇧⌘K" on macOS,
 * "Ctrl+Shift+K" on Windows and Linux. Already-spelled strings ("⌘K") pass
 * through unchanged.
 */
export function formatShortcut(shortcut: string): string {
  if (!shortcut.includes('+') || shortcut === '+') return shortcut
  const parts = shortcut.split('+').map((part) => part.trim()).filter(Boolean)
  const modifierNames = new Set(['ctrl', 'control', 'alt', 'option', 'shift', 'mod', 'cmd', 'meta', 'command'])
  const normalise = (part: string): string => {
    const lower = part.toLowerCase()
    if (lower === 'control') return 'ctrl'
    if (lower === 'option') return 'alt'
    if (lower === 'command' || lower === 'cmd' || lower === 'meta') return isMac ? 'mod' : lower === 'meta' ? 'meta' : 'mod'
    return lower
  }
  const modifiers = parts.filter((part) => modifierNames.has(part.toLowerCase())).map(normalise)
  const keys = parts.filter((part) => !modifierNames.has(part.toLowerCase()))
  const order = isMac ? MAC_ORDER : WIN_ORDER
  const labels = isMac ? MAC_MODIFIERS : WIN_MODIFIERS
  const keyLabels = isMac ? MAC_KEYS : WIN_KEYS
  const sorted = [...new Set(modifiers)].sort((a, b) => order.indexOf(a) - order.indexOf(b))
  const modifierText = sorted.map((modifier) => labels[modifier] ?? modifier)
  const keyText = keys.map((key) => keyLabels[key.toLowerCase()] ?? (key.length === 1 ? key.toUpperCase() : key))
  return isMac ? [...modifierText, ...keyText].join('') : [...modifierText, ...keyText].join('+')
}
