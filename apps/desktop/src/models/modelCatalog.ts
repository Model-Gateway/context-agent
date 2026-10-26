// Port of apps/macos/ContextAgent/Models/ModelCatalog.swift: the model
// picker's data: the endpoint's `models/list` result, recents per base URL
// (the `modelRecents` preference), fuzzy ranking with match ranges and prefix
// grouping once the list grows past 12 entries.
//
// `ModelCatalog` is a plain class held by `ProjectState` and mutated in
// place (the host bumps its version after `apply` and friends); recents are
// read from the preferences store, so a view that shows them also
// subscribes with `usePref('modelRecents')`.

import type { ModelStat, ModelSwitchResult, ModelsList } from '../backend/types'
import { promoteRecentModel, recentModels, removeRecentModel } from './preferences'

/** A run of matched characters in `display`: code-point offsets, `end` exclusive. */
export interface MatchRange {
  start: number
  end: number
}

/// `ModelCatalog.Match`: one ranked entry. `name` is the model id the
/// endpoint lists; `display` the readable name the row shows ("GPT-4.1").
/// `ranges` are the matched character runs in `display` (for highlighting;
/// empty when only the id matched).
export interface ModelMatch {
  /** `id`. */
  name: string
  display: string
  ranges: MatchRange[]
  score: number
}

/** One piece of `ModelMatch.segments`. */
export interface MatchSegment {
  text: string
  matched: boolean
}

/** `Match.init(name:display:ranges:score:)`. */
export function makeModelMatch(name: string, display: string | null, ranges: MatchRange[], score: number): ModelMatch {
  return { name, display: display ?? name, ranges, score }
}

/** `Match.segments`: `display` split into matched and unmatched runs (bold the matched ones). */
export function matchSegments(match: ModelMatch): MatchSegment[] {
  const chars = Array.from(match.display)
  const out: MatchSegment[] = []
  let cursor = 0
  for (const range of match.ranges) {
    if (cursor < range.start) out.push({ text: chars.slice(cursor, range.start).join(''), matched: false })
    out.push({ text: chars.slice(range.start, range.end).join(''), matched: true })
    cursor = range.end
  }
  if (cursor < chars.length) out.push({ text: chars.slice(cursor).join(''), matched: false })
  return out
}

/** `Section.Kind`. */
export type ModelSectionKind = 'recent' | 'group' | 'all'

/** `ModelCatalog.Section`: "Recent", a prefix group ("gpt", "claude") or the flat "Models" list. */
export interface ModelSection {
  kind: ModelSectionKind
  title: string
  items: ModelMatch[]
  /** `"\(kind)-\(title)"`. */
  id: string
}

function section(kind: ModelSectionKind, title: string, items: ModelMatch[]): ModelSection {
  return { kind, title, items, id: `${kind}-${title}` }
}

/** The readable name of a model id (`ModelsDevCatalog.displayName`); identity by default. */
export type DisplayName = (name: string) => string

const identity: DisplayName = (name) => name

/** `ModelCatalog`: the picker's list for one project. */
export class ModelCatalog {
  /** Lists longer than this are grouped by prefix. */
  static readonly groupingThreshold = 12
  /** A loaded list (and its stats) is refetched after this long (seconds). */
  static readonly maxAge = 60 * 60

  /** Sorted, deduped names from the endpoint (current model first when missing). */
  models: string[] = []
  /** The project's current model as the backend reports it. */
  current: string | null = null
  /** `endpoint` / `cached` / `fallback`. */
  source: string | null = null
  /** True while `models/list` is in flight. */
  loading = false
  /** `models/list.error` (the list still succeeded) or the RPC failure. */
  error: string | null = null
  /** ModelGateway's savings and 24 h error rate per model (`models/list.stats`). */
  stats: Record<string, ModelStat> = {}
  lastLoaded: Date | null = null
  /** Result of the last successful switch (capability summary line). */
  lastSwitch: ModelSwitchResult | null = null
  /** Inline failure of the last `models/switch`. */
  switchError: string | null = null

  private _baseURL: string

  constructor(baseURL: string) {
    this._baseURL = baseURL
  }

  /** The endpoint the recents are keyed by. A new endpoint makes the list stale. */
  get baseURL(): string {
    return this._baseURL
  }

  set baseURL(value: string) {
    if (value === this._baseURL) return
    this._baseURL = value
    this.invalidate()
  }

  // MARK: - Recents

  /** Recently used models for `baseURL`, most recent first. */
  get recents(): string[] {
    return recentModels(this._baseURL)
  }

  /** Called after a successful switch only. */
  promote(name: string): void {
    promoteRecentModel(name, this._baseURL)
  }

  removeRecent(name: string): void {
    removeRecentModel(name, this._baseURL)
  }

  // MARK: - Applying results

  apply(list: ModelsList): void {
    const seen = new Set<string>()
    const names = list.models.filter((name) => {
      if (seen.has(name)) return false
      seen.add(name)
      return true
    })
    if (!names.includes(list.current) && list.current.length > 0) names.unshift(list.current)
    this.models = names
    this.current = list.current
    this.source = list.source
    this.error = list.error ?? null
    this.stats = list.stats ?? {}
    this.lastLoaded = new Date()
  }

  /** Everything the picker can show when the list failed: current + recents. */
  get fallbackNames(): string[] {
    const seen = new Set<string>()
    const all = [...(this.current !== null ? [this.current] : []), ...this.recents, ...this.models]
    return all.filter((name) => {
      if (name.length === 0 || seen.has(name)) return false
      seen.add(name)
      return true
    })
  }

  /// True when the list has to be fetched: never loaded, invalidated (new
  /// endpoint or backend), the last fetch failed, or it is older than
  /// `maxAge` (so the stats' error rates follow the backend's hourly cache).
  /// The picker's Refresh forces a fetch.
  get needsLoad(): boolean {
    return (
      this.lastLoaded === null ||
      this.error !== null ||
      this.models.length === 0 ||
      (Date.now() - this.lastLoaded.getTime()) / 1000 > ModelCatalog.maxAge
    )
  }

  /// A fetch the picker shows as "Loading models…": only when there is no
  /// list to show yet, so background refreshes stay silent.
  get showsLoading(): boolean {
    return this.loading && this.models.length === 0
  }

  /** Marks the list stale; the next prefetch refetches it. */
  invalidate(): void {
    this.lastLoaded = null
  }

  // MARK: - Ranking

  /** Models matching `query`, best first (all models when the query is empty). */
  ranked(query: string, displayName: DisplayName = identity): ModelMatch[] {
    return ModelCatalog.rank(query, this.fallbackNames, displayName)
  }

  /// Matches the readable name and the id; the better score wins, and only
  /// a display-name match is highlighted.
  static rank(query: string, names: readonly string[], displayName: DisplayName = identity): ModelMatch[] {
    const trimmed = trimWhitespaces(query)
    if (trimmed.length === 0) return names.map((name) => makeModelMatch(name, displayName(name), [], 0))
    const matches: ModelMatch[] = []
    for (const name of names) {
      const display = displayName(name)
      const onDisplay = ModelCatalog.fuzzyMatch(trimmed, display)
      const onId = display === name ? null : ModelCatalog.fuzzyMatch(trimmed, name)
      if (onDisplay && onDisplay.score >= (onId?.score ?? Number.MIN_SAFE_INTEGER)) {
        matches.push(makeModelMatch(name, display, onDisplay.ranges, onDisplay.score))
      } else if (onId) {
        matches.push(makeModelMatch(name, display, [], onId.score))
      }
    }
    return matches.sort((a, b) => {
      if (a.score !== b.score) return b.score - a.score
      const aLength = Array.from(a.name).length
      const bLength = Array.from(b.name).length
      if (aLength !== bLength) return aLength - bLength
      return a.name < b.name ? -1 : a.name > b.name ? 1 : 0
    })
  }

  /// Case-insensitive subsequence match. Higher is better: exact > prefix >
  /// substring > scattered; bonuses for word-boundary and consecutive hits.
  static fuzzyMatch(query: string, candidate: string): { score: number; ranges: MatchRange[] } | null {
    const q = Array.from(query).map((ch) => ch.toLowerCase())
    const c = Array.from(candidate).map((ch) => ch.toLowerCase())
    if (q.length === 0 || q.length > c.length) return null

    if (sameChars(q, c)) return { score: 1000, ranges: [{ start: 0, end: c.length }] }

    // Exact substring: prefer the earliest, boundary-aligned occurrence.
    const start = substringStart(q, c)
    if (start !== null) {
      const boundary = start === 0 || isBoundary(c, start)
      const base = start === 0 ? 800 : boundary ? 650 : 500
      const score = base - Math.min(start, 100) - Math.min(c.length - q.length, 100)
      return { score, ranges: [{ start, end: start + q.length }] }
    }

    // Scattered subsequence with greedy boundary preference.
    const positions: number[] = []
    let ci = 0
    for (const ch of q) {
      let found: number | null = null
      let probe = ci
      while (probe < c.length) {
        if (c[probe] === ch) {
          if (found === null) found = probe
          if (isBoundary(c, probe)) {
            found = probe
            break
          }
        }
        probe += 1
        // Do not scan far past the first hit looking for a boundary.
        if (found !== null && probe - found > 8) break
      }
      if (found === null) return null
      positions.push(found)
      ci = found + 1
    }

    let score = 300
    let previous = -2
    for (const p of positions) {
      if (p === previous + 1) score += 15
      if (isBoundary(c, p)) score += 10
      previous = p
    }
    const last = positions[positions.length - 1]
    score -= Math.min(last !== undefined ? last - positions.length : 0, 100)
    score -= Math.min(c.length - q.length, 60)
    return { score, ranges: mergedRanges(positions) }
  }

  // MARK: - Grouping

  /** "gpt-4.1" → "gpt", "meta/llama-3" → "meta", "o3" → "o3". */
  static prefix(name: string): string {
    const cut = name.search(/[-/]/)
    return cut > 0 ? name.slice(0, cut) : name
  }

  /** Prefix groups when there are more than 12 matches, else null (flat list). */
  static grouped(matches: readonly ModelMatch[]): ModelSection[] | null {
    if (matches.length <= ModelCatalog.groupingThreshold) return null
    const order: string[] = []
    const buckets = new Map<string, ModelMatch[]>()
    for (const match of matches) {
      const key = ModelCatalog.prefix(match.name).toLowerCase()
      let bucket = buckets.get(key)
      if (!bucket) {
        bucket = []
        buckets.set(key, bucket)
        order.push(key)
      }
      bucket.push(match)
    }
    // Singletons collapse into "Other" so the list does not become one header per model.
    const sections: ModelSection[] = []
    const other: ModelMatch[] = []
    for (const key of [...order].sort(compareStrings)) {
      const items = buckets.get(key) ?? []
      if (items.length === 1) other.push(...items)
      else sections.push(section('group', key, items))
    }
    if (other.length > 0) sections.push(section('group', 'Other', [...other].sort((a, b) => compareStrings(a.name, b.name))))
    return sections
  }

  /// What the picker renders for `query`: a "Recent" section (only without a
  /// query), then prefix groups or one flat section.
  sections(query: string, displayName: DisplayName = identity): ModelSection[] {
    const out: ModelSection[] = []
    const trimmed = trimWhitespaces(query)
    const recent = this.recents
    if (trimmed.length === 0 && recent.length > 0) {
      out.push(
        section(
          'recent',
          'Recent',
          recent.map((name) => makeModelMatch(name, displayName(name), [], 0)),
        ),
      )
    }
    const matches = this.ranked(trimmed, displayName)
    const groups = ModelCatalog.grouped(matches)
    if (groups) out.push(...groups)
    else out.push(section('all', 'Models', matches))
    return out
  }

  /// True when the typed text names no listed model exactly, so the picker
  /// offers "Use “<typed>” as the model name".
  offersCustomName(query: string): boolean {
    const trimmed = trimWhitespaces(query)
    if (trimmed.length === 0) return false
    const lowered = trimmed.toLowerCase()
    return !this.fallbackNames.some((name) => name.toLowerCase() === lowered)
  }
}

// MARK: - Private helpers

/** `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not newlines. */
function trimWhitespaces(text: string): string {
  return text.replace(/^[\p{Zs}\t]+|[\p{Zs}\t]+$/gu, '')
}

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}

function sameChars(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false
  return true
}

function substringStart(q: readonly string[], c: readonly string[]): number | null {
  if (q.length > c.length) return null
  let best: number | null = null
  for (let i = 0; i + q.length <= c.length; i += 1) {
    let equal = true
    for (let j = 0; j < q.length; j += 1) {
      if (c[i + j] !== q[j]) {
        equal = false
        break
      }
    }
    if (equal) {
      if (best === null) best = i
      if (i === 0 || isBoundary(c, i)) return i
    }
  }
  return best
}

const LETTER = /^\p{L}/u
const NUMBER = /^\p{N}/u

function isBoundary(c: readonly string[], index: number): boolean {
  if (!(index > 0 && index < c.length)) return index === 0
  const prev = c[index - 1] ?? ''
  const cur = c[index] ?? ''
  if (prev === '-' || prev === '/' || prev === '.' || prev === '_' || prev === ':' || prev === ' ') return true
  if (LETTER.test(prev) && NUMBER.test(cur)) return true
  if (NUMBER.test(prev) && LETTER.test(cur)) return true
  return false
}

function mergedRanges(positions: readonly number[]): MatchRange[] {
  const ranges: MatchRange[] = []
  let runStart: number | null = null
  let previous = -2
  for (const p of positions) {
    if (p === previous + 1) {
      previous = p
      continue
    }
    if (runStart !== null) ranges.push({ start: runStart, end: previous + 1 })
    runStart = p
    previous = p
  }
  if (runStart !== null) ranges.push({ start: runStart, end: previous + 1 })
  return ranges
}
