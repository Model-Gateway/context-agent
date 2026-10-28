import SwiftUI
import Foundation

/// One remembered repository root (Welcome grid, sidebar "pinned", Open
/// Recent): a local folder, or a folder on a remote host (`hostId`, with
/// the host's name at the time for the card when the host is gone).
struct RecentProject: Codable, Equatable, Hashable, Identifiable {
    var path: String
    var name: String
    var lastOpened: Date
    var pinned: Bool = false
    var hostId: UUID? = nil
    var hostName: String? = nil

    var id: String { Self.key(path: path, hostId: hostId) }
    var url: URL { URL(fileURLWithPath: path) }
    var isRemote: Bool { hostId != nil }

    /// What `openProjectPaths` and the recents are keyed by: the path, or
    /// `ctx-ssh://<host>/path` for a remote folder.
    static func key(path: String, hostId: UUID?) -> String {
        hostId.map { RemoteProjectRef(hostId: $0, path: path).encoded } ?? path
    }

    /// "~/Projects/context-agent", or "devbox:~/code/app" for a remote folder
    /// (with its home abbreviated when the host's home directory is known).
    var abbreviatedPath: String {
        guard isRemote else { return (path as NSString).abbreviatingWithTildeInPath }
        return "\(hostName ?? "ssh"):\(path)"
    }

    /// A local folder that is still there; remote folders are taken as
    /// existing (the host decides when they are opened).
    var exists: Bool {
        guard !isRemote else { return true }
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

    func item(for path: String, hostId: UUID? = nil) -> RecentProject? {
        items.first { $0.path == path && $0.hostId == hostId }
    }

    /// Records an open: moves (or inserts) the entry to the front with the
    /// current date. A remote folder is keyed by host and path and keeps
    /// the host's name for its card.
    func touch(path: String, name: String, host: SSHHost? = nil) {
        let standardized = host == nil ? Self.standardize(path) : path
        if let index = items.firstIndex(where: { $0.path == standardized && $0.hostId == host?.id }) {
            var entry = items[index]
            entry.lastOpened = Date()
            entry.name = name
            entry.hostName = host?.displayName
            items.remove(at: index)
            items.insert(entry, at: 0)
        } else {
            items.insert(RecentProject(path: standardized, name: name, lastOpened: Date(), hostId: host?.id, hostName: host?.displayName), at: 0)
        }
        trim()
        scheduleSave()
    }

    func remove(path: String, hostId: UUID? = nil) {
        let standardized = hostId == nil ? Self.standardize(path) : path
        items.removeAll { $0.path == standardized && $0.hostId == hostId }
        scheduleSave()
    }

    func remove(_ recent: RecentProject) {
        remove(path: recent.path, hostId: recent.hostId)
    }

    func setPinned(_ pinned: Bool, path: String, hostId: UUID? = nil) {
        let standardized = hostId == nil ? Self.standardize(path) : path
        guard let index = items.firstIndex(where: { $0.path == standardized && $0.hostId == hostId }) else { return }
        items[index].pinned = pinned
        scheduleSave()
    }

    func togglePin(path: String, hostId: UUID? = nil) {
        let standardized = hostId == nil ? Self.standardize(path) : path
        guard let entry = item(for: standardized, hostId: hostId) else { return }
        setPinned(!entry.pinned, path: standardized, hostId: hostId)
    }

    func togglePin(_ recent: RecentProject) {
        togglePin(path: recent.path, hostId: recent.hostId)
    }

    /// Renames the host on every entry of a host (Settings edits).
    func renameHost(_ id: UUID, to name: String) {
        var changed = false
        for index in items.indices where items[index].hostId == id && items[index].hostName != name {
            items[index].hostName = name
            changed = true
        }
        if changed { scheduleSave() }
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
            items = decoded.filter { seen.insert($0.id).inserted }
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
