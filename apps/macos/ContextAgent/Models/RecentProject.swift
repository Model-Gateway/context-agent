import SwiftUI
import Foundation

/// One remembered repository root (Welcome grid, sidebar "pinned", Open Recent).
struct RecentProject: Codable, Equatable, Hashable, Identifiable {
    var path: String
    var name: String
    var lastOpened: Date
    var pinned: Bool = false

    var id: String { path }
    var url: URL { URL(fileURLWithPath: path) }

    /// "~/Projects/context-agent"
    var abbreviatedPath: String {
        (path as NSString).abbreviatingWithTildeInPath
    }

    var exists: Bool {
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory) && isDirectory.boolValue
    }
}

/// `~/Library/Application Support/ContextAgent/recent-projects.json`:
/// `[{path, name, lastOpened, pinned}]`, newest first, at most 30 entries.
@MainActor
@Observable
final class RecentProjectStore {
    static let maxCount = 30
    static let fileName = "recent-projects.json"

    private(set) var items: [RecentProject] = []

    @ObservationIgnored private let fileURL: URL
    @ObservationIgnored private var saveTask: Task<Void, Never>?

    /// Pinned first (by last opened), then the rest by last opened.
    var sorted: [RecentProject] {
        items.sorted { a, b in
            if a.pinned != b.pinned { return a.pinned }
            return a.lastOpened > b.lastOpened
        }
    }

    var pinned: [RecentProject] { sorted.filter(\.pinned) }

    init(fileURL: URL? = nil) {
        self.fileURL = fileURL ?? Self.defaultFileURL()
        load()
    }

    static func defaultFileURL() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("ContextAgent").appendingPathComponent(fileName)
    }

    func item(for path: String) -> RecentProject? {
        items.first { $0.path == path }
    }

    /// Records an open: moves (or inserts) the entry to the front with the current date.
    func touch(path: String, name: String) {
        let standardized = Self.standardize(path)
        if let index = items.firstIndex(where: { $0.path == standardized }) {
            var entry = items[index]
            entry.lastOpened = Date()
            entry.name = name
            items.remove(at: index)
            items.insert(entry, at: 0)
        } else {
            items.insert(RecentProject(path: standardized, name: name, lastOpened: Date()), at: 0)
        }
        trim()
        scheduleSave()
    }

    func remove(path: String) {
        let standardized = Self.standardize(path)
        items.removeAll { $0.path == standardized }
        scheduleSave()
    }

    func setPinned(_ pinned: Bool, path: String) {
        let standardized = Self.standardize(path)
        guard let index = items.firstIndex(where: { $0.path == standardized }) else { return }
        items[index].pinned = pinned
        scheduleSave()
    }

    func togglePin(path: String) {
        let standardized = Self.standardize(path)
        guard let entry = item(for: standardized) else { return }
        setPinned(!entry.pinned, path: standardized)
    }

    /// Open Recent → Clear Menu: drops everything that is not pinned.
    func clearUnpinned() {
        items.removeAll { !$0.pinned }
        scheduleSave()
    }

    func clearAll() {
        items.removeAll()
        scheduleSave()
    }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        if let decoded = try? decoder.decode([RecentProject].self, from: data) {
            var seen: Set<String> = []
            items = decoded.filter { seen.insert($0.path).inserted }
            trim()
        }
    }

    private func trim() {
        guard items.count > Self.maxCount else { return }
        // Keep pinned entries; drop the oldest unpinned ones.
        var kept: [RecentProject] = []
        var overflow = items.count - Self.maxCount
        for entry in items.reversed() {
            if overflow > 0, !entry.pinned {
                overflow -= 1
                continue
            }
            kept.insert(entry, at: 0)
        }
        items = kept
    }

    private func scheduleSave() {
        saveTask?.cancel()
        let snapshot = items
        let url = fileURL
        saveTask = Task.detached(priority: .utility) {
            try? await Task.sleep(nanoseconds: 200_000_000)
            guard !Task.isCancelled else { return }
            Self.write(snapshot, to: url)
        }
    }

    nonisolated private static func write(_ items: [RecentProject], to url: URL) {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(items) else { return }
        let directory = url.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try? data.write(to: url, options: [.atomic])
    }

    nonisolated static func standardize(_ path: String) -> String {
        URL(fileURLWithPath: (path as NSString).expandingTildeInPath).standardizedFileURL.path
    }
}
