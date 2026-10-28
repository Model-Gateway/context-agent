import type { CSSProperties } from 'react'
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowUp,
  ArrowUpRight,
  Asterisk,
  BadgeCheck,
  BadgeX,
  Bird,
  Box,
  Braces,
  Brain,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  Circle,
  CircleArrowDown,
  CircleArrowUp,
  CircleCheck,
  CircleDollarSign,
  CircleDot,
  CircleEllipsis,
  CirclePlus,
  CircleStop,
  CircleUser,
  CircleSlash,
  CircleX,
  Clock,
  CodeXml,
  Contrast,
  Copy,
  CopyPlus,
  CreditCard,
  Database,
  Eye,
  FileArchive,
  Flame,
  Download,
  FileBraces,
  FileText,
  Folder,
  FolderPlus,
  Gift,
  GitFork,
  Globe,
  Grid3x3,
  Hammer,
  Hand,
  History,
  Image as ImageIcon,
  Info,
  Key,
  LayoutGrid,
  ListFilter,
  Lock,
  LockOpen,
  LogOut,
  MessageCircleQuestionMark,
  Minimize2,
  Paperclip,
  MessageCircle,
  MessageSquareText,
  Minus,
  OctagonX,
  Paintbrush,
  PanelRight,
  Pencil,
  Pin,
  Play,
  Plus,
  Puzzle,
  RefreshCw,
  Scissors,
  Search,
  Settings,
  ShieldAlert,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Square,
  SquareCheck,
  RotateCw,
  SquareArrowUpRight,
  Stethoscope,
  Table,
  Trash,
  TriangleAlert,
  Undo2,
  UserKey,
  Users,
  WandSparkles,
  Wrench,
  X,
  Zap,
  SquareFunction,
  List,
  Type,
  Variable,
  ALargeSmall,
  TextCursorInput,
  WholeWord,
  Hexagon,
  Split,
  createLucideIcon,
  type LucideIcon,
} from 'lucide-react'
// Apps (catalog app symbols and the connect page's "where it runs" row).
import { Drama, FileDown, House, Laptop, LibraryBig, ListOrdered, Package, Server, SquareMousePointer } from 'lucide-react'
// The command palette's Toggle sidebar (View → Toggle Sidebar has no symbol in Swift).
import { PanelLeft } from 'lucide-react'
// The transcript's Report row (flag).
import { Flag } from 'lucide-react'

// SF Symbols lucide has no close match for, drawn on lucide's 24-unit grid
// (2-unit padding, round caps and joins) so they take the same size and
// stroke as the rest of the catalogue.

/** `point.3.connected.trianglepath.dotted`: three rings on a dotted triangle. */
const PointsTriangleDotted = createLucideIcon('points-triangle-dotted', [
  ['circle', { cx: '5', cy: '6.5', r: '2.5', key: 'left' }],
  ['circle', { cx: '19', cy: '6.5', r: '2.5', key: 'right' }],
  ['circle', { cx: '12', cy: '18', r: '2.5', key: 'bottom' }],
  ['path', { d: 'M9.5 6.5h.01M12 6.5h.01M14.5 6.5h.01M7.7 10.9h.01M9.2 13.4h.01M16.3 10.9h.01M14.8 13.4h.01', key: 'dots' }],
])

/** `chart.bar`: three outlined bars of rising height on one baseline. */
const ChartBarOutline = createLucideIcon('chart-bar-outline', [
  ['rect', { x: '2.5', y: '11', width: '5', height: '9', rx: '1.5', key: 'short' }],
  ['rect', { x: '9.5', y: '7.5', width: '5', height: '12.5', rx: '1.5', key: 'middle' }],
  ['rect', { x: '16.5', y: '4', width: '5', height: '16', rx: '1.5', key: 'tall' }],
])

/** `terminal`: a wide rounded window with a prompt and a cursor line. */
const TerminalWindow = createLucideIcon('terminal-window', [
  ['rect', { x: '2', y: '4', width: '20', height: '16', rx: '3', key: 'frame' }],
  ['path', { d: 'm6.5 9 3 2.5-3 2.5', key: 'prompt' }],
  ['path', { d: 'M12 15h5', key: 'cursor' }],
])

/** `text.magnifyingglass`: a magnifier with two text lines in its lens. */
const TextMagnifier = createLucideIcon('text-magnifier', [
  ['circle', { cx: '10.5', cy: '10.5', r: '7.5', key: 'lens' }],
  ['path', { d: 'm21 21-5.2-5.2', key: 'handle' }],
  ['path', { d: 'M7.5 9h6M7.5 12.5h4.5', key: 'lines' }],
])

/** `gauge.with.dots.needle.<n>percent`: lucide's gauge arc with the needle at the level. */
function gauge(name: string, needle: string): LucideIcon {
  return createLucideIcon(name, [
    ['path', { d: 'M3.34 19a10 10 0 1 1 17.32 0', key: 'arc' }],
    ['path', { d: needle, key: 'needle' }],
    ['circle', { cx: '12', cy: '14', r: '0.6', key: 'hub' }],
  ])
}
const Gauge0 = gauge('gauge-0', 'M12 14 7.24 16.75')
const Gauge33 = gauge('gauge-33', 'M12 14 8.46 9.79')
const Gauge50 = gauge('gauge-50', 'M12 14 12.00 8.50')
const Gauge67 = gauge('gauge-67', 'M12 14 15.54 9.79')
const Gauge100 = gauge('gauge-100', 'M12 14 16.76 16.75')

/** `bolt.circle.fill`: a disc with a bolt (FILL `knockout` cuts the bolt out of it). */
const BoltCircle = createLucideIcon('bolt-circle', [
  ['circle', { cx: '12', cy: '12', r: '10', key: 'disc' }],
  ['path', { d: 'M13.2 6.2 8.4 13h3.4l-1 4.8 4.8-6.8h-3.4z', key: 'bolt' }],
])

/**
 * Every SF Symbol name in the macOS app's `Icons.swift`, mapped one-to-one
 * to a lucide component. The keys are the Swift static names; the two names
 * the macOS ARCHITECTURE.md lists but the Swift file no longer has (`logo`,
 * `tools`) and the toast "error" glyph (`xmark.octagon` in Toasts.swift)
 * are included so callers following the document still find them.
 */
export const Icons = {
  // Icons.swift static lets
  welcome: Hand, // hand.wave
  newChat: Plus,
  projects: FolderPlus, // folder.badge.plus
  changes: Split, // arrow.triangle.branch
  project: Box, // cube
  inspector: PanelRight, // sidebar.right
  settings: Settings, // gearshape
  search: Search, // magnifyingglass
  filter: ListFilter, // line.3.horizontal.decrease
  send: ArrowUp,
  stop: Square, // stop.fill (filled, see FILL)
  queued: Clock,
  attach: Plus,
  model: Asterisk,
  copy: Copy, // doc.on.doc
  check: CircleCheck, // checkmark.circle
  failed: CircleX, // xmark.circle
  stopped: CircleStop, // stop.circle
  approval: Hand, // hand.raised
  permissionAuto: Zap, // bolt
  permissionBypass: ShieldAlert, // exclamationmark.shield
  effort: Gauge50, // gauge.with.dots.needle.50percent
  agents: PointsTriangleDotted, // point.3.connected.trianglepath.dotted
  agentExplore: Search,
  agentImplement: Hammer,
  validationPass: BadgeCheck, // checkmark.seal
  validationFail: BadgeX, // xmark.seal
  chevronDown: ChevronDown,
  chevronRight: ChevronRight,
  folder: Folder,
  remoteHost: Server, // server.rack
  home: House, // house
  showHidden: Eye, // eye
  branch: Split, // arrow.triangle.branch
  pin: Pin,
  close: X, // xmark
  refresh: RotateCw, // arrow.clockwise
  warning: TriangleAlert, // exclamationmark.triangle
  info: Info, // info.circle
  thinking: Brain,
  file: FileText, // doc.text
  editor: CodeXml, // chevron.left.forwardslash.chevron.right
  save: Download, // square.and.arrow.down (a tray with an arrow into it)
  terminal: TerminalWindow, // terminal
  browser: Globe,
  mcp: Puzzle, // puzzlepiece.extension
  apps: LayoutGrid, // square.grid.2x2
  run: Play,
  memory: BrainCircuit, // brain.head.profile
  context: TextMagnifier, // text.magnifyingglass
  prompt: FileText, // doc.plaintext

  // Used by the shared components.
  copied: Check, // checkmark
  clear: CircleX, // xmark.circle.fill (filled, see FILL)
  openFull: SquareArrowUpRight, // arrow.up.forward.square

  // Names the views used to write inline.
  chat: MessageCircle, // bubble.left
  pinned: Pin, // pin.fill (filled, see FILL)
  key: Key,
  doctor: Stethoscope,
  chart: ChartBarOutline, // chart.bar
  trash: Trash, // trash
  follow: ArrowDownToLine, // arrow.down.to.line
  chevronUp: ChevronUp,
  arrowDown: ArrowDown,
  addCustom: CirclePlus, // plus.circle
  fix: Wrench, // wrench.and.screwdriver
  refactor: RefreshCw, // arrow.triangle.2.circlepath
  pricing: CircleDollarSign, // dollarsign.circle
  byok: Key,
  spending: Gauge33, // gauge.with.dots.needle.33percent
  billing: CreditCard,
  appearance: Contrast, // circle.lefthalf.filled
  radioOn: CircleDot, // largecircle.fill.circle
  radioOff: Circle,
  externalLink: ArrowUpRight, // arrow.up.forward
  edit: Pencil,
  undo: Undo2, // arrow.uturn.backward
  // Changes tab git actions
  stage: Plus,
  unstage: Minus,
  discard: Undo2, // arrow.uturn.backward
  commit: CircleCheck, // checkmark.circle
  push: CircleArrowUp, // arrow.up.circle
  pull: CircleArrowDown, // arrow.down.circle
  fetch: RefreshCw, // arrow.triangle.2.circlepath
  generate: Sparkles,
  commits: History, // clock.arrow.circlepath
  newBranch: CopyPlus, // plus.square.on.square
  // Account (ModelGateway)
  account: CircleUser, // person.crop.circle
  signIn: UserKey, // person.badge.key
  signOut: LogOut, // rectangle.portrait.and.arrow.right
  gateway: Globe, // network (a globe, not lucide Network)
  usage: Grid3x3, // square.grid.3x3.fill (filled, see FILL)
  plan: CreditCard,
  contextCredit: Gift,
  invite: Users, // person.2
  worktree: GitFork, // arrow.branch

  // ARCHITECTURE.md names and the toast error glyph.
  logo: Asterisk,
  tools: Sparkles,
  error: OctagonX, // xmark.octagon

  // starter(kind) glyphs not covered above.
  eye: Eye, // review
  scissors: Scissors, // cleanup
  textBubble: MessageSquareText, // text.bubble (default starter)

  // fileType(path) glyphs not covered above.
  swift: Bird, // "swift" (the bird of the Swift logo)
  paintbrush: Paintbrush, // css
  richText: FileText, // doc.richtext (markdown)
  json: FileBraces, // curlybraces.square
  config: SlidersHorizontal, // slider.horizontal.3
  image: ImageIcon, // photo
  table: Table, // tablecells
  database: Database, // cylinder
  code: Braces, // curlybraces

  // SF Symbols the Swift views write inline (not in Icons.swift), so the
  // ported views still go through this catalogue.
  checkmark: Check, // checkmark (menus, selected rows)
  checkFill: CircleCheck, // checkmark.circle.fill (filled, see FILL)
  failedFill: CircleX, // xmark.circle.fill (editor error count; filled, see FILL)
  warningFill: TriangleAlert, // exclamationmark.triangle.fill (filled, see FILL)
  question: MessageCircleQuestionMark, // questionmark.bubble (question card)
  checkboxOn: SquareCheck, // checkmark.square.fill (question card multi-select; filled, see FILL)
  checkboxOff: Square, // square (question card multi-select)
  collapseAll: Minimize2, // arrow.down.right.and.arrow.up.left (Files tab, "compacted" chip)
  toolUse: Wrench, // wrench.adjustable (model hover card "Tools" chip)
  vision: Eye, // eye (model hover card "Vision" chip)
  openWeights: LockOpen, // lock.open (model hover card "Open" chip)
  paperclip: Paperclip, // paperclip ("Drop to attach")
  archive: FileArchive, // doc.zipper (media viewer for archives)
  boltShield: ShieldCheck, // bolt.shield (app permissions)
  boltFill: BoltCircle, // bolt.circle.fill (bypassed tool permission; filled, see FILL)
  more: CircleEllipsis, // ellipsis.circle
  effortAuto: WandSparkles, // wand.and.stars
  effortNone: CircleSlash, // circle.slash
  effortMinimal: Gauge0, // gauge.with.dots.needle.0percent
  effortLow: Gauge33, // gauge.with.dots.needle.33percent
  effortMedium: Gauge50, // gauge.with.dots.needle.50percent
  effortHigh: Gauge67, // gauge.with.dots.needle.67percent
  effortExtraHigh: Gauge100, // gauge.with.dots.needle.100percent
  effortMax: Flame, // flame
  // Inspector (Files tab, Browser tab).
  folderFill: Folder, // folder.fill (an expanded folder; filled, see FILL)
  chevronLeft: ChevronLeft, // chevron.left (Browser back)
  lock: Lock, // lock (https address)

  // Apps: the SF Symbols `mcp/catalog` names for its apps (AppIcon's
  // fallback when an app has no logo) and the connect page's row.
  computerUse: SquareMousePointer, // macwindow.and.cursorarrow
  books: LibraryBig, // books.vertical
  docDown: FileDown, // arrow.down.doc
  theaterMasks: Drama, // theatermasks
  listNumber: ListOrdered, // list.number
  laptop: Laptop, // laptopcomputer
  shippingBox: Package, // shippingbox
  // Editor: the completion popup's kinds (CompletionItem.systemImage in
  // CompletionPanel.swift); struct / class use `shippingBox`, modules `folder`.
  symbolFunction: SquareFunction, // function
  symbolEnum: List, // list.bullet
  symbolTypeAlias: Type, // t.square
  symbolVariable: Variable, // v.square
  symbolKeyword: ALargeSmall, // textformat
  symbolSnippet: TextCursorInput, // text.insert
  symbolWord: WholeWord, // text.word.spacing
  symbolOther: Hexagon, // circle.hexagongrid
  // The Windows / Linux caption buttons (WindowControls; macOS has traffic lights).
  windowMinimize: Minus,
  windowMaximize: Square, // outlined (no FILL entry)
  windowRestore: Copy, // two overlapping squares
  sidebarLeft: PanelLeft, // sidebar.left (palette: Toggle sidebar)
  report: Flag, // flag
} as const satisfies Record<string, LucideIcon>

/** A key of the `Icons` catalogue. */
export type IconName = keyof typeof Icons

/**
 * Names whose SF Symbol is a `.fill` variant. `solid` fills the glyph with
 * the current colour; `knockout` additionally draws the inner strokes in
 * the surface colour (xmark.circle.fill: a filled disc with a cut-out X).
 */
const FILL: Partial<Record<IconName, 'solid' | 'knockout'>> = {
  stop: 'solid',
  pinned: 'solid',
  clear: 'knockout',
  usage: 'knockout',
  checkFill: 'knockout',
  failedFill: 'knockout',
  warningFill: 'knockout',
  checkboxOn: 'knockout',
  boltFill: 'knockout',
  folderFill: 'solid',
}

/** `Icons.starter(kind)`: the glyph of a `suggest/starters` kind. */
export function starter(kind: string): IconName {
  switch (kind) {
    case 'fix':
      return 'fix'
    case 'explain':
      return 'context'
    case 'refactor':
      return 'refactor'
    case 'test':
      return 'validationPass'
    case 'docs':
      return 'file'
    case 'review':
      return 'eye'
    case 'feature':
      return 'generate'
    case 'cleanup':
      return 'scissors'
    default:
      return 'textBubble'
  }
}

/** `Icons.fileType(path)`: the file-type glyph for a path, by extension. */
export function fileType(path: string): IconName {
  const name = path.split(/[\\/]/).pop() ?? ''
  const dot = name.lastIndexOf('.')
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
  switch (ext) {
    case 'html':
    case 'htm':
    case 'xhtml':
    case 'xml':
    case 'svg':
    case 'vue':
    case 'svelte':
    case 'jsx':
    case 'tsx':
    case 'astro':
      return 'editor'
    case 'swift':
      return 'swift'
    case 'css':
    case 'scss':
    case 'sass':
    case 'less':
      return 'paintbrush'
    case 'md':
    case 'markdown':
    case 'mdx':
    case 'rst':
      return 'richText'
    case 'txt':
    case 'log':
      return 'prompt'
    case 'json':
    case 'jsonc':
      return 'json'
    case 'toml':
    case 'yaml':
    case 'yml':
    case 'ini':
    case 'cfg':
    case 'conf':
    case 'plist':
    case 'env':
    case 'lock':
      return 'config'
    case 'sh':
    case 'bash':
    case 'zsh':
    case 'fish':
    case 'ps1':
      return 'terminal'
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'heic':
    case 'avif':
    case 'ico':
    case 'bmp':
    case 'tiff':
      return 'image'
    case 'csv':
    case 'tsv':
    case 'xlsx':
      return 'table'
    case 'sql':
    case 'db':
    case 'sqlite':
      return 'database'
    case 'rs':
    case 'py':
    case 'js':
    case 'mjs':
    case 'cjs':
    case 'ts':
    case 'go':
    case 'c':
    case 'h':
    case 'cc':
    case 'cpp':
    case 'hpp':
    case 'm':
    case 'mm':
    case 'java':
    case 'kt':
    case 'kts':
    case 'rb':
    case 'php':
    case 'cs':
    case 'scala':
    case 'lua':
    case 'dart':
    case 'zig':
    case 'ex':
    case 'exs':
    case 'hs':
      return 'code'
    default:
      return 'file'
  }
}

/**
 * The optical size. lucide draws in a 24-unit box with 2 units of padding
 * on every side, so a glyph at `size` px has about 0.85 × size of ink,
 * while an SF Symbol at `.font(.system(size:))` inks about its point size
 * (a 16 pt gearshape is 18 pt across). Drawing only the 20-unit live area
 * into the box makes `size` mean the Swift point size: every glyph reads
 * 1.2× larger than plain lucide, in the same layout box. The ink may touch
 * the box's edge, so the SVG does not clip.
 */
const OPTICAL_VIEWBOX = '2 2 20 20'

export interface IconProps {
  name: IconName
  /** The SF Symbol point size (`.font(.system(size:))`); 16 by default (12 in chips, 14 in buttons). */
  size?: number
  /** Stroke width in lucide's 24-unit grid; 1.75 by default (≈ SF Symbol medium). */
  strokeWidth?: number
  className?: string
  style?: CSSProperties
  /** Decorative by default; pass a label to expose the icon on its own. */
  'aria-label'?: string
  'aria-hidden'?: boolean
  title?: string
}

/**
 * `Image(systemName:)` of the macOS app: renders the lucide glyph for a
 * catalogue name in the current text colour. Decorative (aria-hidden)
 * unless given an `aria-label`.
 */
export function Icon({ name, size = 16, strokeWidth = 1.75, className, style, title, ...aria }: IconProps) {
  const Component = Icons[name]
  const fill = FILL[name]
  const label = aria['aria-label']
  const hidden = aria['aria-hidden'] ?? !label
  const fillProps =
    fill === 'solid'
      ? { fill: 'currentColor' }
      : fill === 'knockout'
        ? { fill: 'currentColor', stroke: 'var(--surface)' }
        : {}
  return (
    <Component
      size={size}
      strokeWidth={strokeWidth}
      viewBox={OPTICAL_VIEWBOX}
      overflow="visible"
      className={className}
      style={style}
      aria-hidden={hidden || undefined}
      aria-label={label}
      role={label ? 'img' : undefined}
      focusable="false"
      {...fillProps}
    >
      {title ? <title>{title}</title> : null}
    </Component>
  )
}
