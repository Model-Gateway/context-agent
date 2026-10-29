import SwiftUI
import AppKit
import Foundation

/// What the model picker's hover card shows for one model: models.dev's
/// name, short description, limits and capability flags, plus the lab whose
/// logo is drawn.
struct ModelsDevInfo: Sendable, Equatable {
    var id: String
    var name: String
    var description: String?
    /// models.dev provider id of the lab (`openai`, `moonshotai`); names the logo.
    var labId: String
    var labName: String
    var contextWindow: Int?
    var maxOutput: Int?
    var toolCall: Bool
    var reasoning: Bool
    /// The `reasoning_effort` values the model accepts, weakest first
    /// (`reasoning_options`): empty without effort control, nil when
    /// models.dev does not say.
    var effortLevels: [String]? = nil
    var vision: Bool
    var openWeights: Bool
    var knowledge: String?
    var releaseDate: String?
    /// USD per million tokens.
    var inputCost: Double?
    var outputCost: Double?
}

/// models.dev metadata (https://models.dev/api.json) for the picker's cards
/// and the display names used across the app. Loaded alongside the model
/// list (`AppState.prefetchModels`), cached on disk for a day, parsed off the
/// main actor. Lookups never touch the network; logos load lazily.
@MainActor
@Observable
final class ModelsDevCatalog {
    static let apiURL = URL(string: "https://models.dev/api.json")!
    static let maxAge: TimeInterval = 24 * 60 * 60

    private var index: ModelsDevIndex?
    /// Provider id → logo; `nil` once a fetch failed (no retry this run).
    private var logos: [String: NSImage?] = [:]
    @ObservationIgnored private var loadTask: Task<Void, Never>?
    @ObservationIgnored private var logoTasks: Set<String> = []
    @ObservationIgnored private var fetchedAt: Date?
    @ObservationIgnored private let cacheURL: URL

    init(cacheURL: URL? = nil) {
        self.cacheURL = cacheURL ?? RecentProjectStore.defaultFileURL()
            .deletingLastPathComponent()
            .appendingPathComponent("models-dev.json")
    }

    // MARK: - Loading

    /// Reads the disk cache the first time, then fetches when it is missing,
    /// older than a day or `force` (the picker's Refresh). Failures keep
    /// whatever is loaded; names fall back to `ModelNames.pretty`.
    func load(force: Bool = false) async {
        if let loadTask {
            await loadTask.value
            if !force { return }
        }
        let task = Task { await performLoad(force: force) }
        loadTask = task
        await task.value
        loadTask = nil
    }

    private func performLoad(force: Bool) async {
        let url = cacheURL
        if index == nil {
            let cached = await Task.detached(priority: .utility) { () -> (ModelsDevIndex, Date)? in
                guard let data = try? Data(contentsOf: url),
                      let index = ModelsDevIndex(data: data) else { return nil }
                let modified = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                return (index, modified ?? .distantPast)
            }.value
            if let cached {
                index = cached.0
                fetchedAt = cached.1
            }
        }
        let fresh = fetchedAt.map { Date().timeIntervalSince($0) < Self.maxAge } ?? false
        guard force || index == nil || !fresh else { return }
        do {
            var request = URLRequest(url: Self.apiURL, timeoutInterval: 30)
            request.cachePolicy = .reloadIgnoringLocalCacheData
            let (data, response) = try await URLSession.shared.data(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { return }
            let parsed = await Task.detached(priority: .utility) { () -> ModelsDevIndex? in
                guard let index = ModelsDevIndex(data: data) else { return nil }
                try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
                try? data.write(to: url, options: .atomic)
                return index
            }.value
            if let parsed {
                index = parsed
                fetchedAt = Date()
            }
        } catch {
            // Offline or blocked: keep the cached index (or none).
        }
    }

    // MARK: - Lookups

    /// models.dev's entry for a model the endpoint at `baseURL` lists.
    func info(for modelId: String, baseURL: String) -> ModelsDevInfo? {
        index?.info(for: modelId, baseURL: baseURL)
    }

    /// "GPT-4.1", "Kimi For Coding HighSpeed"; `ModelNames.pretty` when models.dev has no entry.
    func displayName(for modelId: String, baseURL: String) -> String {
        guard !modelId.isEmpty else { return modelId }
        return info(for: modelId, baseURL: baseURL)?.name ?? ModelNames.pretty(modelId)
    }

    /// The lab's logo (template image), fetched on first use.
    func logo(for labId: String) -> NSImage? {
        if let cached = logos[labId] { return cached }
        fetchLogo(labId)
        return nil
    }

    private func fetchLogo(_ labId: String) {
        guard !logoTasks.contains(labId), let url = URL(string: "https://models.dev/logos/\(labId).svg") else { return }
        logoTasks.insert(labId)
        Task {
            var image: NSImage?
            if let (data, response) = try? await URLSession.shared.data(from: url),
               (response as? HTTPURLResponse)?.statusCode == 200,
               let svg = NSImage(data: data) {
                svg.isTemplate = true
                image = svg
            }
            logos[labId] = .some(image)
        }
    }
}

// MARK: - Index

/// The parsed api.json, keyed for the three lookups `info(for:baseURL:)` tries.
struct ModelsDevIndex: Sendable {
    private struct Provider: Sendable {
        var name: String
        var api: String?
        var models: [String: Raw]
    }

    /// One model entry as listed by one provider.
    private struct Raw: Sendable {
        var id: String
        var name: String?
        var description: String?
        var canonical: String?
        var contextWindow: Int?
        var maxOutput: Int?
        var toolCall: Bool
        var reasoning: Bool
        var effortLevels: [String]?
        var vision: Bool
        var openWeights: Bool
        var knowledge: String?
        var releaseDate: String?
        var inputCost: Double?
        var outputCost: Double?
    }

    private var providers: [String: Provider] = [:]
    /// Normalized API base URL → provider id.
    private var providersByAPI: [String: String] = [:]
    /// Lowercased bare model id → "lab/id" from every `canonical_model_id`.
    private var canonicalByBareId: [String: String] = [:]
    /// Lowercased model id → (provider id, model id) of an entry without a
    /// canonical link (a first-party listing), native-SDK providers first.
    private var firstPartyById: [String: (String, String)] = [:]
    /// Lowercased model id → (provider id, model id) of any listing, for ids
    /// only resellers list (e.g. a coding plan's alias of a lab model).
    private var anyById: [String: (String, String)] = [:]

    init?(data: Data) {
        guard let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        for (providerId, value) in root {
            guard let provider = value as? [String: Any], let models = provider["models"] as? [String: Any] else { continue }
            var raws: [String: Raw] = [:]
            for (key, value) in models {
                guard let model = value as? [String: Any] else { continue }
                raws[key] = Self.raw(key: key, model)
            }
            let api = provider["api"] as? String
            providers[providerId] = Provider(name: provider["name"] as? String ?? providerId, api: api, models: raws)
            if let api { providersByAPI[Self.normalizedURL(api)] = providerId }
        }
        guard !providers.isEmpty else { return nil }
        // Deterministic order: native-SDK providers (no `api`) win ties.
        let ordered = providers.keys.sorted { a, b in
            let aNative = providers[a]?.api == nil, bNative = providers[b]?.api == nil
            return aNative != bNative ? aNative : a < b
        }
        for providerId in ordered {
            guard let provider = providers[providerId] else { continue }
            for (key, raw) in provider.models {
                if let canonical = raw.canonical, let slash = canonical.firstIndex(of: "/") {
                    let lab = String(canonical[..<slash])
                    let bare = String(canonical[canonical.index(after: slash)...])
                    if providers[lab]?.models[bare] != nil, canonicalByBareId[bare.lowercased()] == nil {
                        canonicalByBareId[bare.lowercased()] = canonical
                    }
                } else if firstPartyById[key.lowercased()] == nil {
                    firstPartyById[key.lowercased()] = (providerId, key)
                }
                if anyById[key.lowercased()] == nil {
                    anyById[key.lowercased()] = (providerId, key)
                }
            }
        }
    }

    /// 1. the provider whose API base URL is the endpoint's; 2. "lab/id" ids;
    /// 3. a canonical lab entry for the bare id; 4. any first-party listing;
    /// 5. any listing at all.
    func info(for modelId: String, baseURL: String) -> ModelsDevInfo? {
        let id = modelId.trimmingCharacters(in: .whitespaces)
        guard !id.isEmpty else { return nil }
        if let providerId = providersByAPI[Self.normalizedURL(baseURL)],
           let raw = lookup(providerId, id) {
            return resolve(raw, providerId: providerId)
        }
        if let slash = id.firstIndex(of: "/") {
            let lab = String(id[..<slash])
            let bare = String(id[id.index(after: slash)...])
            if let raw = lookup(lab, bare) { return resolve(raw, providerId: lab) }
            return info(for: bare, baseURL: "")
        }
        if let canonical = canonicalByBareId[id.lowercased()], let slash = canonical.firstIndex(of: "/") {
            let lab = String(canonical[..<slash])
            if let raw = lookup(lab, String(canonical[canonical.index(after: slash)...])) {
                return resolve(raw, providerId: lab)
            }
        }
        for table in [firstPartyById, anyById] {
            if let (providerId, key) = table[id.lowercased()], let raw = providers[providerId]?.models[key] {
                return resolve(raw, providerId: providerId)
            }
        }
        return nil
    }

    private func lookup(_ providerId: String, _ id: String) -> Raw? {
        guard let models = providers[providerId]?.models else { return nil }
        if let raw = models[id] { return raw }
        return models.first { $0.key.caseInsensitiveCompare(id) == .orderedSame }?.value
    }

    /// Fills gaps from the canonical lab entry; the logo is the lab's.
    private func resolve(_ raw: Raw, providerId: String) -> ModelsDevInfo {
        var labId = providerId
        var lab: Raw?
        if let canonical = raw.canonical, let slash = canonical.firstIndex(of: "/") {
            let labKey = String(canonical[..<slash])
            if let entry = lookup(labKey, String(canonical[canonical.index(after: slash)...])) {
                labId = labKey
                lab = entry
            }
        }
        let name = (raw.name.flatMap { $0 == raw.id ? nil : $0 } ?? lab?.name).map(Self.cleanName) ?? ModelNames.pretty(raw.id)
        return ModelsDevInfo(
            id: raw.id,
            name: name,
            description: raw.description ?? lab?.description,
            labId: labId,
            labName: providers[labId]?.name ?? labId,
            contextWindow: raw.contextWindow ?? lab?.contextWindow,
            maxOutput: raw.maxOutput ?? lab?.maxOutput,
            toolCall: raw.toolCall,
            reasoning: raw.reasoning,
            // The endpoint's own listing may accept fewer values than the lab's.
            effortLevels: raw.effortLevels ?? lab?.effortLevels,
            vision: raw.vision,
            openWeights: raw.openWeights || lab?.openWeights == true,
            knowledge: raw.knowledge ?? lab?.knowledge,
            releaseDate: raw.releaseDate ?? lab?.releaseDate,
            inputCost: raw.inputCost,
            outputCost: raw.outputCost
        )
    }

    private static func raw(key: String, _ model: [String: Any]) -> Raw {
        let limit = model["limit"] as? [String: Any]
        let cost = model["cost"] as? [String: Any]
        let inputs = (model["modalities"] as? [String: Any])?["input"] as? [String] ?? []
        let reasoning = model["reasoning"] as? Bool ?? false
        // `reasoning_options: [{"type": "effort", "values": [...]}, {"type": "toggle"}, …]`;
        // a model that does not reason has no effort to set.
        let effortLevels: [String]? = (model["reasoning_options"] as? [[String: Any]]).map { options in
            options.filter { $0["type"] as? String == "effort" }.flatMap { $0["values"] as? [String] ?? [] }
        } ?? (reasoning ? nil : [])
        return Raw(
            id: model["id"] as? String ?? key,
            name: model["name"] as? String,
            description: (model["description"] as? String).flatMap { $0.isEmpty ? nil : $0 },
            canonical: model["canonical_model_id"] as? String,
            contextWindow: (limit?["context"] as? NSNumber)?.intValue,
            maxOutput: (limit?["output"] as? NSNumber)?.intValue,
            toolCall: model["tool_call"] as? Bool ?? false,
            reasoning: reasoning,
            effortLevels: effortLevels,
            vision: inputs.contains("image") || (model["attachment"] as? Bool ?? false),
            openWeights: model["open_weights"] as? Bool ?? false,
            knowledge: model["knowledge"] as? String,
            releaseDate: model["release_date"] as? String,
            inputCost: (cost?["input"] as? NSNumber)?.doubleValue,
            outputCost: (cost?["output"] as? NSNumber)?.doubleValue
        )
    }

    /// "Claude Sonnet 4.5 (latest)" → "Claude Sonnet 4.5".
    private static func cleanName(_ name: String) -> String {
        let suffix = " (latest)"
        return name.hasSuffix(suffix) ? String(name.dropLast(suffix.count)) : name
    }

    /// Lowercased, scheme and trailing slashes dropped.
    static func normalizedURL(_ url: String) -> String {
        var s = url.trimmingCharacters(in: .whitespaces).lowercased()
        for scheme in ["https://", "http://"] where s.hasPrefix(scheme) {
            s.removeFirst(scheme.count)
        }
        while s.hasSuffix("/") { s.removeLast() }
        return s
    }
}

// MARK: - Display names

enum ModelNames {
    private static let acronyms: Set<String> = ["gpt", "glm", "llm", "ai", "vl", "tts", "oss", "api", "moe", "qwq"]

    /// Readable fallback for ids models.dev does not know:
    /// "gpt-5.6-sol" → "GPT 5.6 Sol", "claude-sonnet-4-5" → "Claude Sonnet 4.5",
    /// "meta/llama-3.3-70b" → "Llama 3.3 70B". Single-letter-plus-digit ids
    /// ("o3", "o4-mini") keep their lowercase lead.
    static func pretty(_ id: String) -> String {
        var base = id
        if let slash = base.lastIndex(of: "/") { base = String(base[base.index(after: slash)...]) }
        let tokens = base.split(whereSeparator: { $0 == "-" || $0 == "_" || $0 == " " }).map(String.init)
        guard !tokens.isEmpty else { return id }
        var words: [String] = []
        for token in tokens {
            // "4", "5" → "4.5" (versions written with dashes); dates stay apart.
            if token.count == 1, token.allSatisfy(\.isNumber), let last = words.last,
               last.count == 1, last.allSatisfy(\.isNumber) {
                words[words.count - 1] = "\(last).\(token)"
                continue
            }
            words.append(word(token))
        }
        return words.joined(separator: " ")
    }

    private static func word(_ token: String) -> String {
        let lower = token.lowercased()
        if acronyms.contains(lower) { return lower.uppercased() }
        if let first = lower.first, first.isNumber {
            // "70b" → "70B", "4o" stays.
            if lower.hasSuffix("b"), lower.dropLast().allSatisfy({ $0.isNumber || $0 == "." }) { return lower.uppercased() }
            return lower
        }
        // "o3", "o4" (OpenAI reasoning series) stay lowercase.
        if lower.count >= 2, lower.first == "o", lower.dropFirst().allSatisfy({ $0.isNumber }) { return lower }
        return lower.prefix(1).uppercased() + lower.dropFirst()
    }
}

// MARK: - Formatting

extension ModelsDevInfo {
    /// "1M", "200K", "32K".
    static func tokens(_ count: Int) -> String {
        if count >= 1_000_000 {
            let millions = String(format: "%.1f", Double(count) / 1_000_000)
            return (millions.hasSuffix(".0") ? String(millions.dropLast(2)) : millions) + "M"
        }
        // Powers of two read as such: 32 768 → "32K", 200 000 → "200K".
        if count >= 1_000 { return "\(count % 1_024 == 0 ? count / 1_024 : Int((Double(count) / 1_000).rounded()))K" }
        return "\(count)"
    }

    /// "$2 in · $8 out per M"; nil without a price.
    var costLine: String? {
        guard let inputCost, let outputCost else { return nil }
        return "\(Self.money(inputCost)) in · \(Self.money(outputCost)) out per M"
    }

    /// "$2", "$0.20".
    static func money(_ value: Double) -> String {
        value.rounded() == value ? "$\(Int(value))" : String(format: "$%.2f", value)
    }
}
