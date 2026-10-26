// Port of apps/macos/ContextAgent/Models/PermissionProfiles.swift: the
// permission profiles the composer's permissions popover and Settings →
// Permissions offer (the TUI's `/approval` names), each with its icon and
// description, plus `AppState.approvalModeLabel` that titles them.

import type { IconName } from '../theme/icons'

/** `PermissionProfileOption`: one permission profile with its icon and description. */
export interface PermissionProfileOption {
  /** The profile name `permissions/set` takes (`approval_required`, …). */
  id: string
  title: string
  /** `systemImage`: a key of the `Icons` catalogue. */
  icon: IconName
  description: string
}

/// `AppState.approvalModeLabel(_:)`: "Ask" / "Auto" / "Bypass", else the
/// configured name with underscores as spaces.
export function approvalModeLabel(profile: string): string {
  switch (profile) {
    case 'approval_required':
      return 'Ask'
    case 'workspace_autonomous':
      return 'Auto'
    case 'bypass_permissions':
      return 'Bypass'
    default:
      return profile.replace(/_/g, ' ')
  }
}

/** `PermissionProfileOption.standard`: Ask and Auto. */
export const STANDARD_PERMISSION_PROFILES: readonly PermissionProfileOption[] = [
  {
    id: 'approval_required',
    title: approvalModeLabel('approval_required'),
    icon: 'approval',
    description: 'Asks for edits and commands.',
  },
  {
    id: 'workspace_autonomous',
    title: approvalModeLabel('workspace_autonomous'),
    icon: 'permissionAuto',
    description: 'Asks for risky commands.',
  },
]

/** `PermissionProfileOption.bypass` (tinted `warning` where it is shown). */
export const BYPASS_PERMISSION_PROFILE: PermissionProfileOption = {
  id: 'bypass_permissions',
  title: approvalModeLabel('bypass_permissions'),
  icon: 'permissionBypass',
  description: 'Never asks.',
}

/// `PermissionProfileOption.all`: every profile the app offers, as in the
/// TUI's `/approval` list: Ask and Auto, then Bypass.
export const ALL_PERMISSION_PROFILES: readonly PermissionProfileOption[] = [...STANDARD_PERMISSION_PROFILES, BYPASS_PERMISSION_PROFILE]

/** `PermissionProfileOption.options(current:)`: `all`, plus an unknown configured profile when it is the current value. */
export function permissionProfileOptions(current: string): PermissionProfileOption[] {
  if (ALL_PERMISSION_PROFILES.some((option) => option.id === current)) return [...ALL_PERMISSION_PROFILES]
  return [
    ...ALL_PERMISSION_PROFILES,
    { id: current, title: approvalModeLabel(current), icon: 'approval', description: 'Set in config.toml.' },
  ]
}

/** `PermissionProfileOption.systemImage(for:)`: the icon for a profile name (the composer and top-bar chips). */
export function permissionProfileIcon(profile: string): IconName {
  return ALL_PERMISSION_PROFILES.find((option) => option.id === profile)?.icon ?? 'approval'
}
