// Port of apps/macos/ContextAgent/Models/ModelsDev.swift: models.dev
// metadata (https://models.dev/api.json) for the model picker's hover cards,
// the composer's effort menu and image warning, and the display names used
// across the app. Fetched by the webview (as the Swift app fetches it
// itself), cached in localStorage for a day; lookups never touch the network.
// Views that show models.dev data subscribe with `useModelsDevVersion()`,
// which changes whenever a catalogue (or a logo failure) arrives.

import { create } from 'zustand'

/// `ModelsDevInfo`: what the model picker's hover card shows for one model:
/// models.dev's name, short description, limits and capability flags, plus
/// the lab whose logo is drawn.
export interface ModelsDevInfo {
  id: string
  name: string
  description: string | null
  /** models.dev provider id of the lab (`openai`, `moonshotai`); names the logo. */
  labId: string
  labName: string
  contextWindow: number | null
  maxOutput: number | null
  toolCall: boolean
  reasoning: boolean
  /// The `reasoning_effort` values the model accepts, weakest first
  /// (`reasoning_options`): empty without effort control, null when
  /// models.dev does not say.
  effortLevels: string[] | null
  /** Takes images (the composer warns when it does not). */
  vision: boolean
  openWeights: boolean
  knowledge: string | null
  releaseDate: string | null
  /** USD per million tokens. */
  inputCost: number | null
  outputCost: number | null
}

/** `ModelsDevInfo.vision`, by the name the composer uses. */
export function modelSupportsImages(info: ModelsDevInfo): boolean {
  return info.vision
}

// MARK: - Index

/** One model entry as listed by one provider (`ModelsDevIndex.Raw`); also the cache format. */
interface RawModel {
  id: string
  name: string | null
  description: string | null
  canonical: string | null
  contextWindow: number | null
  maxOutput: number | null
  toolCall: boolean
  reasoning: boolean
  effortLevels: string[] | null
  vision: boolean
  openWeights: boolean
  knowledge: string | null
  releaseDate: string | null
  inputCost: number | null
  outputCost: number | null
}

/** `ModelsDevIndex.Provider`; also the cache format (`models` as an object). */
interface RawProvider {
  name: string
  api: string | null
  models: Record<string, RawModel>
}

/** The trimmed api.json kept in localStorage. */
interface CompactRoot {
  providers: Record<string, RawProvider>
}

type Json = Record<string, unknown>

function isObject(value: unknown): value is Json {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function str(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

function num(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function int(value: unknown): number | null {
  const n = num(value)
  return n === null ? null : Math.trunc(n)
}

/** `ModelsDevIndex.raw(key:_:)`. */
function rawModel(key: string, model: Json): RawModel {
  const limit = isObject(model.limit) ? model.limit : null
  const cost = isObject(model.cost) ? model.cost : null
  const modalities = isObject(model.modalities) ? model.modalities : null
  const inputs = Array.isArray(modalities?.input) ? (modalities.input as unknown[]).filter((entry): entry is string => typeof entry === 'string') : []
  const reasoning = model.reasoning === true
  // `reasoning_options: [{"type": "effort", "values": [...]}, {"type": "toggle"}, …]`;
  // a model that does not reason has no effort to set.
  const options = model.reasoning_options
  let effortLevels: string[] | null
  if (Array.isArray(options) && options.every(isObject)) {
    effortLevels = (options as Json[])
      .filter((option) => option.type === 'effort')
      .flatMap((option) => (Array.isArray(option.values) && option.values.every((v) => typeof v === 'string') ? (option.values as string[]) : []))
  } else {
    effortLevels = reasoning ? null : []
  }
  const description = str(model.description)
  return {
    id: str(model.id) ?? key,
    name: str(model.name),
    description: description !== null && description.length > 0 ? description : null,
    canonical: str(model.canonical_model_id),
    contextWindow: int(limit?.context),
    maxOutput: int(limit?.output),
    toolCall: model.tool_call === true,
    reasoning,
    effortLevels,
    vision: inputs.includes('image') || model.attachment === true,
    openWeights: model.open_weights === true,
    knowledge: str(model.knowledge),
    releaseDate: str(model.release_date),
    inputCost: num(cost?.input),
    outputCost: num(cost?.output),
  }
}

/** api.json → the fields this app reads; null when it holds no provider. */
function compactRoot(root: unknown): CompactRoot | null {
  if (!isObject(root)) return null
  const providers: Record<string, RawProvider> = {}
  for (const [providerId, value] of Object.entries(root)) {
    if (!isObject(value) || !isObject(value.models)) continue
    const models: Record<string, RawModel> = {}
    for (const [key, model] of Object.entries(value.models)) {
      if (isObject(model)) models[key] = rawModel(key, model)
    }
    providers[providerId] = { name: str(value.name) ?? providerId, api: str(value.api), models }
  }
  return Object.keys(providers).length > 0 ? { providers } : null
}

/** A (provider id, model key) pair. */
interface Listing {
  providerId: string
  key: string
}

/** `ModelsDevIndex`: the parsed api.json, keyed for the lookups `info(for:baseURL:)` tries. */
class ModelsDevIndex {
  private readonly providers: Map<string, RawProvider>
  /** Normalized API base URL → provider id. */
  private readonly providersByAPI = new Map<string, string>()
  /** Lowercased bare model id → "lab/id" from every `canonical_model_id`. */
  private readonly canonicalByBareId = new Map<string, string>()
  /// Lowercased model id → (provider id, model id) of an entry without a
  /// canonical link (a first-party listing), native-SDK providers first.
  private readonly firstPartyById = new Map<string, Listing>()
  /// Lowercased model id → (provider id, model id) of any listing, for ids
  /// only resellers list (e.g. a coding plan's alias of a lab model).
  private readonly anyById = new Map<string, Listing>()

  private constructor(root: CompactRoot) {
    this.providers = new Map(Object.entries(root.providers))
    for (const [providerId, provider] of this.providers) {
      if (provider.api !== null) this.providersByAPI.set(normalizedURL(provider.api), providerId)
    }
    // Deterministic order: native-SDK providers (no `api`) win ties.
    const ordered = [...this.providers.keys()].sort((a, b) => {
      const aNative = this.providers.get(a)?.api === null
      const bNative = this.providers.get(b)?.api === null
      if (aNative !== bNative) return aNative ? -1 : 1
      return a < b ? -1 : a > b ? 1 : 0
    })
    for (const providerId of ordered) {
      const provider = this.providers.get(providerId)
      if (!provider) continue
      for (const [key, raw] of Object.entries(provider.models)) {
        const canonical = raw.canonical
        const slash = canonical?.indexOf('/') ?? -1
        if (canonical !== null && slash >= 0) {
          const lab = canonical.slice(0, slash)
          const bare = canonical.slice(slash + 1)
          if (this.providers.get(lab)?.models[bare] !== undefined && !this.canonicalByBareId.has(bare.toLowerCase())) {
            this.canonicalByBareId.set(bare.toLowerCase(), canonical)
          }
        } else if (!this.firstPartyById.has(key.toLowerCase())) {
          this.firstPartyById.set(key.toLowerCase(), { providerId, key })
        }
        if (!this.anyById.has(key.toLowerCase())) this.anyById.set(key.toLowerCase(), { providerId, key })
      }
    }
  }

  /** `ModelsDevIndex(data:)`: null when the catalogue holds no provider. */
  static from(root: CompactRoot | null): ModelsDevIndex | null {
    return root && Object.keys(root.providers).length > 0 ? new ModelsDevIndex(root) : null
  }

  /// 1. the provider whose API base URL is the endpoint's; 2. "lab/id" ids;
  /// 3. a canonical lab entry for the bare id; 4. any first-party listing;
  /// 5. any listing at all.
  info(modelId: string, baseURL: string): ModelsDevInfo | null {
    const id = modelId.replace(/^[\p{Zs}\t]+|[\p{Zs}\t]+$/gu, '')
    if (id.length === 0) return null
    const byAPI = this.providersByAPI.get(normalizedURL(baseURL))
    if (byAPI !== undefined) {
      const raw = this.lookup(byAPI, id)
      if (raw) return this.resolve(raw, byAPI)
    }
    const slash = id.indexOf('/')
    if (slash >= 0) {
      const lab = id.slice(0, slash)
      const bare = id.slice(slash + 1)
      const raw = this.lookup(lab, bare)
      if (raw) return this.resolve(raw, lab)
      return this.info(bare, '')
    }
    const canonical = this.canonicalByBareId.get(id.toLowerCase())
    if (canonical !== undefined) {
      const cut = canonical.indexOf('/')
      if (cut >= 0) {
        const lab = canonical.slice(0, cut)
        const raw = this.lookup(lab, canonical.slice(cut + 1))
        if (raw) return this.resolve(raw, lab)
      }
    }
    for (const table of [this.firstPartyById, this.anyById]) {
      const listing = table.get(id.toLowerCase())
      if (listing) {
        const raw = this.providers.get(listing.providerId)?.models[listing.key]
        if (raw) return this.resolve(raw, listing.providerId)
      }
    }
    return null
  }

  private lookup(providerId: string, id: string): RawModel | null {
    const models = this.providers.get(providerId)?.models
    if (!models) return null
    const exact = models[id]
    if (exact) return exact
    const lowered = id.toLowerCase()
    for (const [key, raw] of Object.entries(models)) if (key.toLowerCase() === lowered) return raw
    return null
  }

  /** Fills gaps from the canonical lab entry; the logo is the lab's. */
  private resolve(raw: RawModel, providerId: string): ModelsDevInfo {
    let labId = providerId
    let lab: RawModel | null = null
    const canonical = raw.canonical
    const slash = canonical?.indexOf('/') ?? -1
    if (canonical !== null && slash >= 0) {
      const labKey = canonical.slice(0, slash)
      const entry = this.lookup(labKey, canonical.slice(slash + 1))
      if (entry) {
        labId = labKey
        lab = entry
      }
    }
    const ownName = raw.name !== null && raw.name !== raw.id ? raw.name : null
    const picked = ownName ?? lab?.name ?? null
    const name = picked !== null ? cleanName(picked) : ModelNames.pretty(raw.id)
    return {
      id: raw.id,
      name,
      description: raw.description ?? lab?.description ?? null,
      labId,
      labName: this.providers.get(labId)?.name ?? labId,
      contextWindow: raw.contextWindow ?? lab?.contextWindow ?? null,
      maxOutput: raw.maxOutput ?? lab?.maxOutput ?? null,
      toolCall: raw.toolCall,
      reasoning: raw.reasoning,
      // The endpoint's own listing may accept fewer values than the lab's.
      effortLevels: raw.effortLevels ?? lab?.effortLevels ?? null,
      vision: raw.vision,
      openWeights: raw.openWeights || lab?.openWeights === true,
      knowledge: raw.knowledge ?? lab?.knowledge ?? null,
      releaseDate: raw.releaseDate ?? lab?.releaseDate ?? null,
      inputCost: raw.inputCost,
      outputCost: raw.outputCost,
    }
  }
}

/** `ModelsDevIndex.cleanName(_:)`: "Claude Sonnet 4.5 (latest)" → "Claude Sonnet 4.5". */
function cleanName(name: string): string {
  const suffix = ' (latest)'
  return name.endsWith(suffix) ? name.slice(0, -suffix.length) : name
}

/** `ModelsDevIndex.normalizedURL(_:)`: lowercased, scheme and trailing slashes dropped. */
export function normalizedURL(url: string): string {
  let s = url.replace(/^[\p{Zs}\t]+|[\p{Zs}\t]+$/gu, '').toLowerCase()
  for (const scheme of ['https://', 'http://']) if (s.startsWith(scheme)) s = s.slice(scheme.length)
  while (s.endsWith('/')) s = s.slice(0, -1)
  return s
}

// MARK: - Catalogue

/** localStorage key of the cached catalogue (`models-dev.json` in Application Support for the Swift app). */
const CACHE_KEY = 'ctx.modelsDevCache'

interface CacheEntry {
  fetchedAt: number
  root: CompactRoot
}

function readCache(): CacheEntry | null {
  try {
    const text = localStorage.getItem(CACHE_KEY)
    if (text === null) return null
    const parsed: unknown = JSON.parse(text)
    if (!isObject(parsed) || typeof parsed.fetchedAt !== 'number' || !isObject(parsed.root) || !isObject(parsed.root.providers)) return null
    return parsed as unknown as CacheEntry
  } catch {
    return null
  }
}

function writeCache(entry: CacheEntry): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(entry))
  } catch {
    // Over quota or blocked: the catalogue stays in memory for this run.
  }
}

/** Changes whenever the catalogue or a logo's state changes, for views to subscribe to. */
const useModelsDevStore = create<{ version: number }>(() => ({ version: 0 }))

function bump(): void {
  useModelsDevStore.setState((state) => ({ version: state.version + 1 }))
}

/** Subscribes a view to the models.dev catalogue (re-renders when it loads). */
export function useModelsDevVersion(): number {
  return useModelsDevStore((state) => state.version)
}

/// `ModelsDevCatalog`: loads the catalogue alongside the model list
/// (`AppState.prefetchModels`), cached for a day. Failures keep whatever is
/// loaded; names fall back to `ModelNames.pretty`.
export class ModelsDevCatalog {
  static readonly apiURL = 'https://models.dev/api.json'
  /** Seconds a cached catalogue stays fresh. */
  static readonly maxAge = 24 * 60 * 60

  private index: ModelsDevIndex | null = null
  private fetchedAt: number | null = null
  private loading: Promise<void> | null = null
  /** Labs whose logo failed to load (no retry this run). */
  private failedLogos = new Set<string>()

  /** True once a catalogue (cached or fetched) is loaded. */
  get isLoaded(): boolean {
    return this.index !== null
  }

  /// `load(force:)`: reads the cache the first time, then fetches when it is
  /// missing, older than a day or `force` (the picker's Refresh).
  async load(force = false): Promise<void> {
    if (this.loading) {
      await this.loading
      if (!force) return
    }
    const task = this.performLoad(force)
    this.loading = task
    try {
      await task
    } finally {
      if (this.loading === task) this.loading = null
    }
  }

  private async performLoad(force: boolean): Promise<void> {
    if (this.index === null) {
      const cached = readCache()
      const index = cached ? ModelsDevIndex.from(cached.root) : null
      if (cached && index) {
        this.index = index
        this.fetchedAt = cached.fetchedAt
        bump()
      }
    }
    const fresh = this.fetchedAt !== null && (Date.now() - this.fetchedAt) / 1000 < ModelsDevCatalog.maxAge
    if (!force && this.index !== null && fresh) return
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), 30_000)
    try {
      const response = await fetch(ModelsDevCatalog.apiURL, { cache: 'no-store', signal: controller.signal })
      if (response.status !== 200) return
      const root = compactRoot(await response.json())
      const index = ModelsDevIndex.from(root)
      if (!root || !index) return
      const now = Date.now()
      writeCache({ fetchedAt: now, root })
      this.index = index
      this.fetchedAt = now
      bump()
    } catch {
      // Offline or blocked: keep the cached index (or none).
    } finally {
      clearTimeout(timer)
    }
  }

  // MARK: - Lookups

  /** `info(for:baseURL:)`: models.dev's entry for a model the endpoint at `baseURL` lists. */
  info(modelId: string, baseURL = ''): ModelsDevInfo | null {
    return this.index?.info(modelId, baseURL) ?? null
  }

  /** `displayName(for:baseURL:)`: "GPT-4.1", "Kimi For Coding HighSpeed"; `ModelNames.pretty` when models.dev has no entry. */
  displayName(modelId: string, baseURL = ''): string {
    if (modelId.length === 0) return modelId
    return this.info(modelId, baseURL)?.name ?? ModelNames.pretty(modelId)
  }

  /// `logo(for:)`: the lab's logo as a URL for an `<img>` (draw it as a mask
  /// to tint it like the Swift template image); null once it failed.
  logo(labId: string): string | null {
    if (labId.length === 0 || this.failedLogos.has(labId)) return null
    return `https://models.dev/logos/${encodeURIComponent(labId)}.svg`
  }

  /** The `<img>` for `logo(labId)` failed: no retry this run. */
  markLogoFailed(labId: string): void {
    if (this.failedLogos.has(labId)) return
    this.failedLogos.add(labId)
    bump()
  }
}

/** The app's one catalogue (`AppState.modelsDev`). */
export const modelsDev = new ModelsDevCatalog()

// MARK: - Display names

const ACRONYMS = new Set(['gpt', 'glm', 'llm', 'ai', 'vl', 'tts', 'oss', 'api', 'moe', 'qwq'])
const DIGIT = /^\p{N}$/u

function isDigits(text: string): boolean {
  const chars = Array.from(text)
  return chars.length > 0 && chars.every((ch) => DIGIT.test(ch))
}

/** `ModelNames`. */
export const ModelNames = {
  /// Readable fallback for ids models.dev does not know: "gpt-5.6-sol" →
  /// "GPT 5.6 Sol", "claude-sonnet-4-5" → "Claude Sonnet 4.5",
  /// "meta/llama-3.3-70b" → "Llama 3.3 70B". Single-letter-plus-digit ids
  /// ("o3", "o4-mini") keep their lowercase lead.
  pretty(id: string): string {
    let base = id
    const slash = base.lastIndexOf('/')
    if (slash >= 0) base = base.slice(slash + 1)
    const tokens = base.split(/[-_ ]/).filter((token) => token.length > 0)
    if (tokens.length === 0) return id
    const words: string[] = []
    for (const token of tokens) {
      // "4", "5" → "4.5" (versions written with dashes); dates stay apart.
      const last = words[words.length - 1]
      if (Array.from(token).length === 1 && isDigits(token) && last !== undefined && Array.from(last).length === 1 && isDigits(last)) {
        words[words.length - 1] = `${last}.${token}`
        continue
      }
      words.push(prettyWord(token))
    }
    return words.join(' ')
  },
}

function prettyWord(token: string): string {
  const lower = token.toLowerCase()
  if (ACRONYMS.has(lower)) return lower.toUpperCase()
  const chars = Array.from(lower)
  const first = chars[0] ?? ''
  if (DIGIT.test(first)) {
    // "70b" → "70B", "4o" stays.
    if (lower.endsWith('b') && chars.slice(0, -1).every((ch) => DIGIT.test(ch) || ch === '.')) return lower.toUpperCase()
    return lower
  }
  // "o3", "o4" (OpenAI reasoning series) stay lowercase.
  if (chars.length >= 2 && first === 'o' && chars.slice(1).every((ch) => DIGIT.test(ch))) return lower
  return first.toUpperCase() + chars.slice(1).join('')
}

// MARK: - Formatting

/** `ModelsDevInfo.tokens(_:)`: "1M", "200K", "32K". */
export function modelsDevTokens(count: number): string {
  if (count >= 1_000_000) {
    const millions = (count / 1_000_000).toFixed(1)
    return (millions.endsWith('.0') ? millions.slice(0, -2) : millions) + 'M'
  }
  // Powers of two read as such: 32 768 → "32K", 200 000 → "200K".
  if (count >= 1_000) return `${count % 1_024 === 0 ? count / 1_024 : Math.round(count / 1_000)}K`
  return `${count}`
}

/** `ModelsDevInfo.costLine`: "$2 in · $8 out per M"; null without a price. */
export function modelsDevCostLine(info: ModelsDevInfo): string | null {
  if (info.inputCost === null || info.outputCost === null) return null
  const money = (value: number) => (Math.round(value) === value ? `$${Math.trunc(value)}` : `$${value.toFixed(2)}`)
  return `${money(info.inputCost)} in · ${money(info.outputCost)} out per M`
}
