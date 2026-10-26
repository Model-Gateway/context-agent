import Foundation
import SwiftUI

/// What the locator needs from `Preferences` (Models/Preferences.swift).
protocol BackendLocatorPreferences {
    var backendPathOverride: String? { get }
}

/// Finds the `ctx` binary. Search order (first executable file wins):
/// the user override, the app bundle (`Contents/Helpers/ctx`,
/// `Contents/MacOS/ctx`), `$CTX_BINARY`, `~/.cargo/bin`, `/opt/homebrew/bin`,
/// `/usr/local/bin`, then a repository checkout (`Cargo.toml` next to
/// `target/release/ctx` or `target/debug/ctx`) found by walking up from the
/// bundle and from the current directory.
enum BackendLocator {
    static let binaryName = "ctx"
    static let buildCommand = "cargo build --release -p ctx-cli"

    static func locate(preferences: any BackendLocatorPreferences) -> (found: URL?, searched: [URL]) {
        let candidates = self.candidates(preferences: preferences)
        var searched: [URL] = []
        for url in candidates {
            searched.append(url)
            if isExecutable(url) {
                return (url, searched)
            }
        }
        return (nil, searched)
    }

    /// Every path that would be tried, in order, without touching the disk
    /// (except for the repository walk, which needs `Cargo.toml` to exist).
    static func candidates(preferences: any BackendLocatorPreferences) -> [URL] {
        var list: [URL] = []
        let fm = FileManager.default

        if let override = preferences.backendPathOverride?.trimmingCharacters(in: .whitespacesAndNewlines), !override.isEmpty {
            list.append(URL(fileURLWithPath: (override as NSString).expandingTildeInPath))
        }

        let bundle = Bundle.main.bundleURL
        list.append(bundle.appendingPathComponent("Contents/Helpers/\(binaryName)"))
        list.append(bundle.appendingPathComponent("Contents/MacOS/\(binaryName)"))

        if let env = ProcessInfo.processInfo.environment["CTX_BINARY"]?.trimmingCharacters(in: .whitespacesAndNewlines), !env.isEmpty {
            list.append(URL(fileURLWithPath: (env as NSString).expandingTildeInPath))
        }

        let home = fm.homeDirectoryForCurrentUser
        list.append(home.appendingPathComponent(".cargo/bin/\(binaryName)"))
        list.append(URL(fileURLWithPath: "/opt/homebrew/bin/\(binaryName)"))
        list.append(URL(fileURLWithPath: "/usr/local/bin/\(binaryName)"))

        list.append(contentsOf: repositoryCandidates(startingAt: bundle))
        list.append(contentsOf: repositoryCandidates(startingAt: URL(fileURLWithPath: fm.currentDirectoryPath)))

        var seen: Set<String> = []
        return list.filter { seen.insert($0.standardizedFileURL.path).inserted }
    }

    /// Walks up from `start`; every ancestor holding a `Cargo.toml` contributes
    /// `target/release/ctx` then `target/debug/ctx`.
    static func repositoryCandidates(startingAt start: URL) -> [URL] {
        let fm = FileManager.default
        var results: [URL] = []
        var dir = start.standardizedFileURL
        var guardCount = 0
        while guardCount < 64 {
            guardCount += 1
            if fm.fileExists(atPath: dir.appendingPathComponent("Cargo.toml").path) {
                results.append(dir.appendingPathComponent("target/release/\(binaryName)"))
                results.append(dir.appendingPathComponent("target/debug/\(binaryName)"))
            }
            let parent = dir.deletingLastPathComponent()
            if parent.path == dir.path { break }
            dir = parent
        }
        return results
    }

    static func isExecutable(_ url: URL) -> Bool {
        var isDirectory: ObjCBool = false
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path, isDirectory: &isDirectory), !isDirectory.boolValue else { return false }
        return fm.isExecutableFile(atPath: url.path)
    }
}
