# Shared components (F2)

Ports of `apps/macos/ContextAgent/Components/*.swift`, `Theme/Modifiers.swift`
and the small pieces of SwiftUI chrome (buttons, menus, popovers, sheets,
toasts) the macOS views use. Every component is `Name.tsx` + `Name.module.css`,
styled only with the tokens in `src/theme/tokens.css`; icons come from
`src/theme/icons.tsx` (`IconName`). Optional props are marked `?`.

Theme: `src/theme/tokens.css` (colours light / dark, `--font-*`, `--font-size-*`,
metrics, shadows), `src/theme/global.css` (reset, scrollbars, selection, focus
ring, `.selectable`, `.card`, `.hover-highlight`, `.floating-shadow`,
`.hairline-{top,bottom,left,right,y,all}`, `.sr-only`), `src/theme/icons.tsx`
(`Icon`, `Icons`, `IconName`, `starter(kind)`, `fileType(path)`),
`src/theme/logo.tsx` (`LogoView { size? = 22, className?, style?, label? = "Context" }`).

## Controls

- **Button** — `variant? 'plain' | 'bordered' (default) | 'prominent' | 'destructive'`,
  `size? 'small' (20 px) | 'regular' (24 px) | 'large' (30 px)`, `icon? IconName`,
  `loading?` (spinner, disabled), `block?`, `ref?`, plus every `<button>` attribute.
- **IconButton** — `icon: IconName`, `label: string` (required: tooltip + `aria-label`),
  `isActive?` (accent tint, highlight kept), `size? = 16` (button is `size + 12`),
  `shortcut?` ("mod+shift+I", shown in the tooltip), `noTooltip?`,
  `tooltipPlacement? 'top' | 'bottom'`, `ref?`, plus `<button>` attributes.
- **Chip** — `children`, `icon? IconName`, `variant? 'neutral' | 'tinted' | 'badge'`,
  `tint?` (CSS colour of a tinted chip, e.g. `var(--warning)`), `onClick?` (makes it a
  button), `aria-label?`, `title?`, `className?`, `style?`.
- **Kbd** — `shortcut?` ("mod+K" → ⌘K / Ctrl+K), `children?` (literal text),
  `className?`, `aria-hidden? = true`.
- **SearchField** — `placeholder`, `value`, `onChange(value)`, `kbdHint?` ("mod+K",
  shown while empty), `onSubmit?`, `onEscape?` (Esc on an empty field; Esc clears
  first), `onKeyDown?`, `autoFocus?`, `disabled?`, `aria-label?`, `className?`, `ref?`.
- **CopyButton** — `text`, `label? = "Copy"`, `size? = 13`, `className?`. Shows a green
  check and "Copied" for 1.5 s. `copyText(text): Promise<boolean>` (clipboard.ts,
  also re-exported) writes through the Tauri clipboard plugin, falling back to
  `navigator.clipboard`.
- **Segmented** — `options: { value, label, icon?, badge?, shortcut?, disabled? }[]`,
  `value`, `onChange(value)`, `aria-label` (required), `compact?` (title on the
  selected pill only), `disabled?`, `size? 'regular' | 'small'`, `iconSize? = 12`,
  `iconStrokeWidth? = 2.25`, `className?`.
  Left / Right / Home / End move the selection.
- **Switch** — `checked`, `onChange(checked)`, `label` (accessible name), `disabled?`,
  `className?`. The macOS 26 small NSSwitch (44 × 20, pill knob) that Settings,
  Account and Apps all use; Settings' labelled `Toggle` wraps it.
- **SegmentedControl** — `options: { value, label }[]`, `value`, `onChange(value)`,
  `aria-label`, `maxWidth?` (stretch up to it; `Infinity` for no cap; without it the
  control hugs its labels), `disabled?`, `className?`. NSSegmentedControl as macOS 26
  draws it: a 24 px grey track, the selected segment an accent capsule. (`Segmented`
  above is the top bar's tab pill, not this.)
- **ResizeHandle** — `width`, `min`, `max`, `defaultWidth` (double-click / Enter
  restores it), `onResize(width)`, `edge? 'leading' (default: dragging left widens) |
  'trailing'`, `label? = "Resize pane"`, `className?`. An invisible 8 px strip that
  overlaps the hairline by 4 px on each side; arrows move it 8 px (32 with Shift).

## Status and loading

- **StatusPill** — one of `{ status: 'completed' | 'blocked' | 'failed' | 'cancelled' }`,
  `{ running: boolean }` (Running with a spinner / Idle) or `{ text, tint }`; plus
  `className?`, `style?`.
- **BrailleSpinner** — `size? = 12`, `tint? = var(--text-secondary)` (`"inherit"` takes
  the surrounding colour), `className?`, `style?`. ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ at 80 ms, static ⠿
  with Reduce Motion, labelled "Working". Also exports `useReducedMotion()`,
  `brailleFrame(at?)`, `BRAILLE_FRAMES`.
- **ProgressRing** — `fraction` (0…1, or `null` while the total is not known), `size? = 16`,
  `lineWidth? = 2`, `tint? = var(--accent)`, `className?`, `style?`. A track and an arc
  filled to `fraction`; a quarter arc turning once a second while `null` (still with
  Reduce Motion). For a project opening and indexing; other waits use BrailleSpinner.
- **InlineDots** — `tint? = var(--text-tertiary)`, `className?`, `style?`.
- **Skeleton** — the pulse wrapper `Skeleton { label? = "Loading", active? = true,
  children, className?, style? }` with the shapes as members:
  `Skeleton.Bar { width?, height? = 10, radius? = 4 }`,
  `Skeleton.Exchange { question, reply: number[] }`, `Skeleton.Transcript`,
  `Skeleton.ToolCall { label }`, `Skeleton.ChatLoading`, `Skeleton.EmptyChat`,
  `Skeleton.SettingsRows { count? = 2, label? }`,
  `Skeleton.SettingsPage { rows? = [3, 2], label? }` (all take `className?`).
- **EmptyPlaceholder** — `icon: IconName`, `title`, `subtitle?`, `actionTitle?`,
  `onAction?`, `className?`.
- **HalftoneHeader** — `height? = 120`, `className?`, `style?`. Decorative, `aria-hidden`.

## Content

- **DisclosureRow** — `expanded`, `onToggle(expanded)`, `label: ReactNode`,
  `trailing?` (right edge; clicks on it do not toggle), `children?` (shown while
  expanded; opens and closes through `Fade`), `aria-label?`, `className?`, `contentClassName?`.
- **Fade** — `show`, `children`, `variant? 'opacity' | 'moveBottom'`, `className?`.
  SwiftUI's `if … { view.transition(.opacity) }`: content that appears later opens its
  space while fading in, and stays for the fade-out while the space closes (0.18 s).
- **InlineMessage** — `message: { kind: 'success' | 'error' | 'info', text }`
  (`InlineMessageData`). The one-line feedback under a form (Settings, Apps).
- **LineList** — `text`, `maxLines? = 400`, `maxBytes? = 32768`,
  `highlight?(line, index): ReactNode`, `cacheKey?` (names the highlighter so the
  rendered block is shared across mounts), `onOpenFull?`, `wrap? = true`,
  `footer?: ReactNode | ({ omitted, clipped }) => ReactNode`, `className?`.
  `capLines(text, maxLines, maxBytes)` is exported (`LineList.cap`).
- **TailWindow** — `items`, `itemKey(item, index)`, `renderItem(item, index)`,
  `page? = 20`, `limit? = 60`, `threshold? = 600`, `followsBottom? = null` (null uses the
  built-in `FollowState`), `rowGap? = 0`, `renderPlaceholder?(index)`,
  `placeholderRowHeight? = 160`, `placeholderHeight? = 960`, `header?`, `footer?`,
  `className?`, `contentClassName?`, `rowsClassName?`, `style?`, `role?`, `aria-label?`,
  `onAtBottomChange?(atBottom)` (drives "Jump to latest"), `onFollowingChange?`,
  `onFollowsTailChange?`, `ref?: TailWindowHandle` with `jumpToLatest({ animated? })`,
  `loadAround(index, { block?, animated? })`, `stopFollowing()`, `followIfNeeded()`,
  `element`, `atBottom`, `isFollowing`, `followsTail`. The pure window logic is
  `TailWindowModel` (tailWindowModel.ts).

- **MiddleTruncate** — `text`, `className?`, `style?`, `natural?`, `maxWidth?`, `runs?`,
  `lines? = 1`. Every middle-truncated text in the app (`.truncationMode(.middle)`:
  "~/Projects/…/context-agent"), fitted with a canvas measurement on every width change.
  Without `natural` its width comes from the container (a block, or a flex item that
  grows); with `natural` it hugs the whole text like a SwiftUI `Text` in an `HStack`
  and gives way when the row is narrower (`maxWidth` caps it). `runs` are styled
  pieces of `text` cut at the same characters; `lines > 1` wraps and clamps, cutting
  the middle until it fits. The full text stays the accessible name, and copying the
  whole shortened text copies the whole text. `middleTruncated(text, width, font)` is
  exported for rows that measure their own width.

## Floating layers

All of them portal into `document.body`, share the overlay stack in
`overlays.ts` (Esc closes only the topmost layer, an outside press closes the
layers it is outside of, Esc / Enter never reach the composer) and respect
Reduce Motion.

- **Tooltip** — `content`, `shortcut?`, `placement? 'bottom' | 'top'`, `delay? = 400`,
  `disabled?`, `children` (exactly one element). Shows on hover (below the pointer,
  like AppKit and Windows; `top` puts it above) or keyboard focus (under the element),
  hides on press, Esc or scroll. A disabled child still shows its tooltip in every
  engine: it lets the pointer through and the hover is followed from the element under
  it, so no wrapper is needed.
- **Popover** — `open`, `onClose`, `anchorRef`, `placement? 'below' | 'above'`,
  `align? 'start' | 'center' | 'end'`, `width?`, `maxHeight?`, `padded?`,
  `initialFocus?` (ref or `'none'`), `returnFocus? = true`,
  `role? 'dialog' | 'listbox' | 'menu' | 'group'`, `aria-label?`, `aria-labelledby?`,
  `className?`, `style?`, `children`. Focus is trapped while open; closing fades it out
  (0.12 s) drawn from its last content.
- **Menu** — `open`, `onClose`, `items: MenuItem[]`, `anchorRef?` (dropdown) or
  `position? { x, y }` (context menu), `placement?`, `align?`, `minWidth? = 180`,
  `aria-label?`, `className?`. `MenuItem` is `{ label, onSelect, icon?, shortcut?,
  destructive?, disabled?, checked?, help?, id? }`, `{ type: 'separator' }` or
  `{ type: 'header', label }`. Arrows, Home / End, type-ahead, Enter / Space.
- **ContextMenu** — `items: MenuItem[] | () => MenuItem[]`, `disabled?`, `children`,
  `aria-label?`. Opens a `Menu` on the `contextmenu` event (pointer or keyboard).
- **Sheet** — `open`, `onClose` (Esc, backdrop press), `title?`, `children?`, `footer?`
  (right-aligned buttons), `width? = 440`, `initialFocus?`, `dismissOnBackdrop? = true`,
  `aria-label?`, `className?`, `style?`. Hangs just under the top bar (a macOS sheet
  hangs from the title bar), slides down on open and back up on close, drawn from its
  last content. Return and Esc inside reach the sheet's own `onKeyDown` handlers and
  stop at its edge. A destructive `confirm()` starts on Cancel, so Return never
  destroys.
  `confirm({ title, message?, confirmLabel? = "OK", cancelLabel? = "Cancel",
  destructive? }): Promise<boolean>` and `showAlert({ title, message?, details?,
  okLabel? }): Promise<void>` (dialogs.tsx, re-exported from Sheet.tsx) need no host.
- **Toast** — `toast: Toast`, `onDismiss(id)`, `onDetails?(toast)`.
- **ToastHost** — no props; mount once in MainWindow (bottom-right stack). The store
  is `toasts.ts`: `toasts.push({ message, level? 'info' | 'success' | 'warning' |
  'error', details? })`, `toasts.dismiss(id)`, `toasts.dismissAll()`,
  `toasts.setOnDetails(handler)`, and the `useToasts` hook. Identical messages within
  10 s are deduped, each toast auto-dismisses after 5 s, at most 4 are shown.

## Helpers

- `platform.ts` — `isMac`, `modLabel` ("⌘" / "Ctrl"), `revealLabel` ("Reveal in Finder" /
  "Show in Explorer"), `isModKey(event)`, `formatShortcut("mod+shift+K")` (⇧⌘K /
  Ctrl+Shift+K).
- `overlays.ts` — `useOverlay`, `useFocusTrap`, `positionAnchored`, `positionAtPoint`,
  `prefersReducedMotion` for views that build their own floating panels;
  `stopOverlayKeys` (an overlay root's `onKeyDown`: Esc / Return stop at its edge) and
  `restoreFocus(element)` (gives focus back without the keyboard ring, as macOS does
  after a menu or sheet closes).

Icons (`theme/icons.tsx`): `size` is the SF Symbol point size. The `Icon` draws
lucide's 20-unit live area into the box (lucide pads its 24-unit grid by 2), so a glyph
inks about its point size like an SF Symbol and reads 1.2× larger than plain lucide;
pass the Swift `.font(.system(size:))` value, not a compensated one. Custom glyphs on
lucide's grid stand in where lucide has no match: `agents`
(point.3.connected.trianglepath.dotted), `chart` (chart.bar), `terminal`, `context`
(text.magnifyingglass), the effort gauges (gauge.with.dots.needle.0/33/50/67/100percent)
and `boltFill` (bolt.circle.fill); `changes` / `branch` are lucide `Split`
(arrow.triangle.branch).
