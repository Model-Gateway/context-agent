import SwiftUI
import Foundation

/// The model picker's data: the endpoint's `models/list` result, recents per
/// base URL (from `Preferences`), fuzzy ranking with match ranges and prefix
/// grouping once the list grows past 12 entries.
@MainActor
@Observable
final class ModelCatalog {
    /// Lists longer than this are grouped by prefix.
    static let groupingThreshold = 12
    /// A loaded list (and its stats) is refetched after this long.
    static let maxAge: TimeInterval = 60 * 60

    /// One ranked entry. `name` is the model id the endpoint lists; `display`
    /// the readable name the row shows ("GPT-4.1"). `ranges` are the matched
    /// character runs in `display` (for highlighting; empty when only the id
    /// matched); `segments` is the same split ready for `Text`.
    struct Match: Identifiable, Equatable {
        var name: String
        var display: String
        var ranges: [Range<String.Index>]
        var score: Int

        var id: String { name }

        init(name: String, display: String? = nil, ranges: [Range<String.Index>], score: Int) {
            self.name = name
            self.display = display ?? name
            self.ranges = ranges
            self.score = score
        }

        var segments: [(text: String, matched: Bool)] {
            var out: [(String, Bool)] = []
            var cursor = display.startIndex
            for range in ranges {
                if cursor < range.lowerBound { out.append((String(display[cursor..<range.lowerBound]), false)) }
                out.append((String(display[range]), true))
                cursor = range.upperBound
            }
            if cursor < display.endIndex { out.append((String(display[cursor...]), false)) }
            return out
        }

        /// `display` with the matched runs in bold, for a one-line `Text`.
        var attributed: AttributedString {
            var result = AttributedString()
            for segment in segments {
                var piece = AttributedString(segment.text)
                if segment.matched { piece.inlinePresentationIntent = .stronglyEmphasized }
                result.append(piece)
            }
            return result
        }

        static func == (lhs: Match, rhs: Match) -> Bool {
            lhs.name == rhs.name && lhs.display == rhs.display && lhs.score == rhs.score && lhs.ranges == rhs.ranges
        }
    }

    /// A picker section: "Recent", a prefix group ("gpt", "claude") or the flat "Models" list.
    struct Section: Identifiable, Equatable {
        enum Kind: Equatable { case recent, group, all }
        var kind: Kind
        var title: String
        var items: [Match]
        var id: String { "\(kind)-\(title)" }
    }

    // MARK: - State

    /// Sorted, deduped names from the endpoint (current model first when missing).
    var models: [String] = []
    /// The project's current model as the backend reports it.
    var current: String?
    /// `endpoint` / `cached` / `fallback`.
    var source: String?
    /// True while `models/list` is in flight.
    var loading = false
    /// `models/list.error` (the list still succeeded) or the RPC failure.
    var error: String?
    /// ModelGateway's savings and 24 h error rate per model (`models/list.stats`).
    var stats: [String: ModelStat] = [:]
    var lastLoaded: Date?
    /// Result of the last successful switch (capability summary line).
    var lastSwitch: ModelSwitchResult?
    /// Inline failure of the last `models/switch`.
    var switchError: String?

    /// The endpoint the recents are keyed by. A new endpoint makes the list stale.
    var baseURL: String {
        didSet { if oldValue != baseURL { invalidate() } }
    }

    @ObservationIgnored private let preferences: Preferences

    init(preferences: Preferences, baseURL: String) {
        self.preferences = preferences
        self.baseURL = baseURL
    }

    // MARK: - Recents

    /// Recently used models for `baseURL`, most recent first, limited to names still listed when a list exists.
    var recents: [String] {
        preferences.recentModels(for: baseURL)
    }

    /// Called after a successful switch only.
    func promote(_ name: String) {
        preferences.promoteRecentModel(name, baseURL: baseURL)
    }

    func removeRecent(_ name: String) {
        preferences.removeRecentModel(name, baseURL: baseURL)
    }

    // MARK: - Applying results

    func apply(_ list: ModelsList) {
        var seen: Set<String> = []
        var names = list.models.filter { seen.insert($0).inserted }
        if !names.contains(list.current), !list.current.isEmpty {
            names.insert(list.current, at: 0)
        }
        models = names
        current = list.current
        source = list.source
        error = list.error
        stats = list.stats ?? [:]
        lastLoaded = Date()
    }

    /// Everything the picker can show when the list failed: current + recents.
    var fallbackNames: [String] {
        var seen: Set<String> = []
        return ([current].compactMap { $0 } + recents + models).filter { !$0.isEmpty && seen.insert($0).inserted }
    }

    /// True when the list has to be fetched: never loaded, invalidated (new
    /// endpoint or backend), the last fetch failed, or it is older than
    /// `maxAge` (so the stats' error rates follow the backend's hourly
    /// cache). The picker's Refresh forces a fetch.
    var needsLoad: Bool {
        lastLoaded == nil || error != nil || models.isEmpty
            || lastLoaded.map { Date().timeIntervalSince($0) > Self.maxAge } == true
    }

    /// A fetch the picker shows as "Loading models…": only when there is no
    /// list to show yet, so background refreshes stay silent.
    var showsLoading: Bool {
        loading && models.isEmpty
    }

    /// Marks the list stale; the next `AppState.prefetchModels` refetches it.
    func invalidate() {
        lastLoaded = nil
    }

    // MARK: - Ranking

    /// Models matching `query`, best first (all models when the query is empty).
    func ranked(query: String, displayName: (String) -> String = { $0 }) -> [Match] {
        Self.rank(query: query, names: fallbackNames, displayName: displayName)
    }

    /// Matches the readable name and the id; the better score wins, and only
    /// a display-name match is highlighted.
    static func rank(query: String, names: [String], displayName: (String) -> String = { $0 }) -> [Match] {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty {
            return names.map { Match(name: $0, display: displayName($0), ranges: [], score: 0) }
        }
        var matches: [Match] = []
        for name in names {
            let display = displayName(name)
            let onDisplay = fuzzyMatch(trimmed, in: display)
            let onId = display == name ? nil : fuzzyMatch(trimmed, in: name)
            if let onDisplay, onDisplay.score >= (onId?.score ?? Int.min) {
                matches.append(Match(name: name, display: display, ranges: onDisplay.ranges, score: onDisplay.score))
            } else if let onId {
                matches.append(Match(name: name, display: display, ranges: [], score: onId.score))
            }
        }
        return matches.sorted { a, b in
            if a.score != b.score { return a.score > b.score }
            if a.name.count != b.name.count { return a.name.count < b.name.count }
            return a.name < b.name
        }
    }

    /// Case-insensitive subsequence match. Higher is better: exact > prefix >
    /// substring > scattered; bonuses for word-boundary and consecutive hits.
    static func fuzzyMatch(_ query: String, in candidate: String) -> (score: Int, ranges: [Range<String.Index>])? {
        let q = Array(query.lowercased())
        let c = Array(candidate.lowercased())
        guard !q.isEmpty, q.count <= c.count else { return nil }

        if q == c {
            return (1000, [candidate.startIndex..<candidate.endIndex])
        }

        // Exact substring: prefer the earliest, boundary-aligned occurrence.
        if let start = substringStart(q, in: c) {
            let boundary = start == 0 || isBoundary(c, at: start)
            let base = start == 0 ? 800 : (boundary ? 650 : 500)
            let score = base - min(start, 100) - min(c.count - q.count, 100)
            return (score, [offsetRange(candidate, start, start + q.count)])
        }

        // Scattered subsequence with greedy boundary preference.
        var positions: [Int] = []
        var ci = 0
        for ch in q {
            var found: Int?
            var probe = ci
            while probe < c.count {
                if c[probe] == ch {
                    if found == nil { found = probe }
                    if isBoundary(c, at: probe) { found = probe; break }
                }
                probe += 1
                // Do not scan far past the first hit looking for a boundary.
                if let f = found, probe - f > 8 { break }
            }
            guard let hit = found else { return nil }
            positions.append(hit)
            ci = hit + 1
        }

        var score = 300
        var previous = -2
        for p in positions {
            if p == previous + 1 { score += 15 }
            if isBoundary(c, at: p) { score += 10 }
            previous = p
        }
        score -= min(positions.last.map { $0 - positions.count } ?? 0, 100)
        score -= min(c.count - q.count, 60)
        return (score, mergedRanges(candidate, positions))
    }

    // MARK: - Grouping

    /// "gpt-4.1" → "gpt", "meta/llama-3" → "meta", "o3" → "o3".
    static func prefix(of name: String) -> String {
        if let cut = name.firstIndex(where: { $0 == "-" || $0 == "/" }), cut > name.startIndex {
            return String(name[..<cut])
        }
        return name
    }

    /// Prefix groups when there are more than 12 matches, else nil (flat list).
    static func grouped(_ matches: [Match]) -> [Section]? {
        guard matches.count > groupingThreshold else { return nil }
        var order: [String] = []
        var buckets: [String: [Match]] = [:]
        for match in matches {
            let key = prefix(of: match.name).lowercased()
            if buckets[key] == nil { order.append(key) }
            buckets[key, default: []].append(match)
        }
        // Singletons collapse into "Other" so the list does not become one header per model.
        var sections: [Section] = []
        var other: [Match] = []
        for key in order.sorted() {
            let items = buckets[key] ?? []
            if items.count == 1 {
                other.append(contentsOf: items)
            } else {
                sections.append(Section(kind: .group, title: key, items: items))
            }
        }
        if !other.isEmpty {
            sections.append(Section(kind: .group, title: "Other", items: other.sorted { $0.name < $1.name }))
        }
        return sections
    }

    /// What the picker renders for `query`: a "Recent" section (only without a
    /// query), then prefix groups or one flat section.
    func sections(query: String, displayName: (String) -> String = { $0 }) -> [Section] {
        var out: [Section] = []
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        let recent = recents
        if trimmed.isEmpty, !recent.isEmpty {
            out.append(Section(kind: .recent, title: "Recent", items: recent.map { Match(name: $0, display: displayName($0), ranges: [], score: 0) }))
        }
        let matches = ranked(query: trimmed, displayName: displayName)
        if let groups = Self.grouped(matches) {
            out.append(contentsOf: groups)
        } else {
            out.append(Section(kind: .all, title: "Models", items: matches))
        }
        return out
    }

    /// True when the typed text names no listed model exactly, so the picker
    /// offers "Use “<typed>” as the model name".
    func offersCustomName(for query: String) -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return false }
        return !fallbackNames.contains { $0.caseInsensitiveCompare(trimmed) == .orderedSame }
    }

    // MARK: - Private helpers

    private static func substringStart(_ q: [Character], in c: [Character]) -> Int? {
        guard q.count <= c.count else { return nil }
        var best: Int?
        var i = 0
        while i + q.count <= c.count {
            if Array(c[i..<(i + q.count)]) == q {
                if best == nil { best = i }
                if i == 0 || isBoundary(c, at: i) { return i }
            }
            i += 1
        }
        return best
    }

    private static func isBoundary(_ c: [Character], at index: Int) -> Bool {
        guard index > 0, index < c.count else { return index == 0 }
        let prev = c[index - 1]
        let cur = c[index]
        if prev == "-" || prev == "/" || prev == "." || prev == "_" || prev == ":" || prev == " " { return true }
        if prev.isLetter && cur.isNumber { return true }
        if prev.isNumber && cur.isLetter { return true }
        return false
    }

    private static func offsetRange(_ s: String, _ start: Int, _ end: Int) -> Range<String.Index> {
        let lower = s.index(s.startIndex, offsetBy: start)
        let upper = s.index(s.startIndex, offsetBy: end)
        return lower..<upper
    }

    private static func mergedRanges(_ s: String, _ positions: [Int]) -> [Range<String.Index>] {
        var ranges: [Range<String.Index>] = []
        var runStart: Int?
        var previous = -2
        for p in positions {
            if p == previous + 1 {
                previous = p
                continue
            }
            if let start = runStart { ranges.append(offsetRange(s, start, previous + 1)) }
            runStart = p
            previous = p
        }
        if let start = runStart { ranges.append(offsetRange(s, start, previous + 1)) }
        return ranges
    }
}
