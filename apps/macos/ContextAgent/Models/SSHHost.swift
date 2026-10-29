import Foundation
import SwiftUI

/// A machine the app can run `ctx serve` on over SSH (Settings → Remote
/// hosts), the way Claude Code and Codex open sessions on a remote host.
/// `address` is what `ssh` is given (a hostname, an IP, or a `Host` alias
/// from `~/.ssh/config`, which supplies the user, port, key and jumps); the
/// optional fields override that file. `ctxPath` is the remote `ctx`
/// binary, by name on the remote PATH unless given.
struct SSHHost: Codable, Equatable, Hashable, Identifiable {
    var id: UUID = UUID()
    /// Shown in the sidebar and the recents; defaults to the address.
    var name: String
    var address: String
    var user: String = ""
    /// 0 = ssh's default (or the config file's).
    var port: Int = 0
    /// `-i`: a private key file ("~/.ssh/id_ed25519"); empty = ssh's choice.
    var identityFile: String = ""
    /// The remote `ctx` binary; empty = "ctx" on the remote PATH (plus the
    /// usual install directories, see `BackendLaunch.remotePath`).
    var ctxPath: String = ""
    /// Extra `ssh` options, one per entry ("-o ProxyJump=bastion").
    var extraArguments: [String] = []

    init(id: UUID = UUID(), name: String, address: String, user: String = "", port: Int = 0, identityFile: String = "", ctxPath: String = "", extraArguments: [String] = []) {
        self.id = id
        self.name = name
        self.address = address
        self.user = user
        self.port = port
        self.identityFile = identityFile
        self.ctxPath = ctxPath
        self.extraArguments = extraArguments
    }

    var displayName: String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? address : trimmed
    }

    /// "alice@devbox:2222" — what ssh will connect to, for the forms and tooltips.
    /// The editor's single "SSH host" field: "alice@devbox" when a user is
    /// set, else the address.
    var login: String {
        let user = user.trimmingCharacters(in: .whitespaces)
        let address = address.trimmingCharacters(in: .whitespaces)
        return user.isEmpty ? address : "\(user)@\(address)"
    }

    /// Splits the "SSH host" field ("root@192.168.1.67", "devbox") into
    /// user and address at the last `@`.
    static func splitLogin(_ text: String) -> (user: String, address: String) {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard let at = trimmed.lastIndex(of: "@"), at != trimmed.startIndex else {
            return ("", trimmed.hasPrefix("@") ? String(trimmed.dropFirst()) : trimmed)
        }
        return (String(trimmed[..<at]), String(trimmed[trimmed.index(after: at)...]))
    }

    var target: String {
        var text = user.isEmpty ? address : "\(user)@\(address)"
        if port > 0 { text += ":\(port)" }
        return text
    }

    /// Whether the fields describe something ssh can be given.
    var isValid: Bool {
        let trimmed = address.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty && !trimmed.contains(where: \.isWhitespace) && !trimmed.hasPrefix("-") && (0...65535).contains(port)
    }

    /// Trimmed fields, the name defaulted from the address.
    func normalized() -> SSHHost {
        var host = self
        host.address = address.trimmingCharacters(in: .whitespacesAndNewlines)
        host.name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if host.name.isEmpty { host.name = host.address }
        host.user = user.trimmingCharacters(in: .whitespacesAndNewlines)
        host.identityFile = identityFile.trimmingCharacters(in: .whitespacesAndNewlines)
        host.ctxPath = ctxPath.trimmingCharacters(in: .whitespacesAndNewlines)
        host.extraArguments = extraArguments.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        return host
    }
}

/// A project on a remote host, as the preferences remember it (`ssh://<host
/// id>/abs/path`): the open-project list and the recents are shared with
/// local folders, which are plain paths.
struct RemoteProjectRef: Equatable, Hashable {
    var hostId: UUID
    var path: String

    static let scheme = "ctx-ssh"

    var encoded: String {
        "\(Self.scheme)://\(hostId.uuidString)\(path.hasPrefix("/") ? path : "/" + path)"
    }

    init(hostId: UUID, path: String) {
        self.hostId = hostId
        self.path = path
    }

    /// Nil for a local path.
    init?(encoded: String) {
        let prefix = Self.scheme + "://"
        guard encoded.hasPrefix(prefix) else { return nil }
        let rest = encoded.dropFirst(prefix.count)
        guard let slash = rest.firstIndex(of: "/"), let id = UUID(uuidString: String(rest[..<slash])) else { return nil }
        hostId = id
        path = String(rest[slash...])
    }

    static func isRemote(_ encoded: String) -> Bool {
        encoded.hasPrefix(scheme + "://")
    }
}

/// `~/Library/Application Support/ContextAgent/ssh-hosts.json`: the
/// configured hosts, in the order they were added.
@MainActor
@Observable
final class SSHHostStore {
    static let fileName = "ssh-hosts.json"

    private(set) var hosts: [SSHHost] = []

    @ObservationIgnored private let fileURL: URL
    @ObservationIgnored private var saveTask: Task<Void, Never>?

    init(fileURL: URL? = nil) {
        self.fileURL = fileURL ?? Self.defaultFileURL()
        load()
    }

    static func defaultFileURL() -> URL {
        RecentProjectStore.defaultFileURL().deletingLastPathComponent().appendingPathComponent(fileName)
    }

    func host(_ id: UUID) -> SSHHost? {
        hosts.first { $0.id == id }
    }

    /// Adds, or replaces the host with the same id.
    func upsert(_ host: SSHHost) {
        let normalized = host.normalized()
        if let index = hosts.firstIndex(where: { $0.id == normalized.id }) {
            hosts[index] = normalized
        } else {
            hosts.append(normalized)
        }
        scheduleSave()
    }

    func remove(_ id: UUID) {
        hosts.removeAll { $0.id == id }
        scheduleSave()
    }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        if let decoded = try? JSONDecoder().decode([SSHHost].self, from: data) {
            var seen: Set<UUID> = []
            hosts = decoded.filter { seen.insert($0.id).inserted }
        }
    }

    private func scheduleSave() {
        saveTask?.cancel()
        let snapshot = hosts
        let url = fileURL
        saveTask = Task.detached(priority: .utility) {
            try? await Task.sleep(nanoseconds: 200_000_000)
            guard !Task.isCancelled else { return }
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            guard let data = try? encoder.encode(snapshot) else { return }
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? data.write(to: url, options: [.atomic])
        }
    }
}

// MARK: - ~/.ssh/config

/// The `Host` aliases of the user's SSH configuration, offered as
/// suggestions when adding a host (patterns with wildcards are skipped;
/// `Include`d files are read one level deep).
enum SSHConfigHosts {
    static func aliases(configURL: URL? = nil) -> [String] {
        let url = configURL ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ssh/config")
        guard let text = try? String(contentsOf: url, encoding: .utf8) else { return [] }
        var names: [String] = []
        var seen: Set<String> = []
        collect(text, into: &names, seen: &seen, base: url.deletingLastPathComponent(), depth: 0)
        return names
    }

    private static func collect(_ text: String, into names: inout [String], seen: inout Set<String>, base: URL, depth: Int) {
        for rawLine in text.split(whereSeparator: \.isNewline) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { continue }
            // "Host devbox" or "Host=devbox"; several names on one line.
            let parts = line.replacingOccurrences(of: "=", with: " ").split(whereSeparator: \.isWhitespace).map(String.init)
            guard let keyword = parts.first?.lowercased() else { continue }
            if keyword == "host" {
                for name in parts.dropFirst() where !name.contains(where: { "*?!".contains($0) }) && seen.insert(name).inserted {
                    names.append(name)
                }
            } else if keyword == "include", depth == 0 {
                for pattern in parts.dropFirst() {
                    let expanded = (pattern as NSString).expandingTildeInPath
                    let fileURL = expanded.hasPrefix("/") ? URL(fileURLWithPath: expanded) : base.appendingPathComponent(expanded)
                    if let included = try? String(contentsOf: fileURL, encoding: .utf8) {
                        collect(included, into: &names, seen: &seen, base: base, depth: depth + 1)
                    }
                }
            }
        }
    }
}
