// The wording the Settings forms change by platform (apps/desktop/ARCHITECTURE.md
// "Window layout and platform mapping": Windows wording where the platform
// differs). The macOS strings are the Swift views' own.

import { platform as osPlatform } from '@tauri-apps/plugin-os'
import { isMac } from '../../components/platform'

function detectWindows(): boolean {
  try {
    return osPlatform() === 'windows'
  } catch {
    // Outside Tauri (vite in a browser): the user agent.
    return typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent || '')
  }
}

/** True on Windows (the primary target of the Tauri app). */
export const isWindows: boolean = !isMac && detectWindows()

/** "this Mac" in the Swift copy; "this PC" on Windows, "this computer" on Linux. */
export const thisDevice: string = isMac ? 'this Mac' : isWindows ? 'this PC' : 'this computer'

/** "this Mac's" (possessive, for "this Mac's ModelGateway key"). */
export const thisDevices: string = `${thisDevice}'s`

/** Where API keys are stored: "Keychain" on macOS, "Credential Manager" on Windows, "keyring" on Linux. */
export const keyStoreName: string = isMac ? 'Keychain' : isWindows ? 'Credential Manager' : 'keyring'

/** "the macOS Keychain" / "Windows Credential Manager" / "the system keyring". */
export const keyStoreLongName: string = isMac ? 'the macOS Keychain' : isWindows ? 'Windows Credential Manager' : 'the system keyring'

/** "the Keychain" / "Credential Manager" / "the keyring" (after "from"). */
export const keyStoreArticle: string = isMac ? 'the Keychain' : isWindows ? 'Credential Manager' : 'the keyring'

/** The operating system's name in "System follows the macOS appearance". */
export const systemName: string = isMac ? 'macOS' : isWindows ? 'Windows' : 'system'
