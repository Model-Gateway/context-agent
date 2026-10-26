import Foundation
import Security
import SwiftUI

// MARK: - KeychainStore

/// API keys live in the legacy (file-based) login keychain under one service
/// name; the account is the environment variable name (`OPENAI_API_KEY`).
enum KeychainStore {
    static let service = "dev.contextagent.desktop"

    struct Failure: Error, LocalizedError, Equatable {
        let status: OSStatus
        var errorDescription: String? {
            if let message = SecCopyErrorMessageString(status, nil) as String? {
                return "Keychain: \(message) (\(status))"
            }
            return "Keychain error \(status)"
        }
    }

    enum Lookup: Equatable {
        case found(String)
        case missing
        /// The keychain refused to answer (locked, user cancelled the prompt, no UI allowed).
        case unavailable(OSStatus)
    }

    /// Statuses that mean "could not consult the keychain" rather than "not stored".
    static let unavailableStatuses: Set<OSStatus> = [errSecAuthFailed, errSecUserCanceled, errSecInteractionNotAllowed]

    static func lookup(account: String) -> Lookup {
        var query = baseQuery(account: account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            if let data = item as? Data, let text = String(data: data, encoding: .utf8) {
                return .found(text)
            }
            return .missing
        case errSecItemNotFound:
            return .missing
        default:
            return unavailableStatuses.contains(status) ? .unavailable(status) : .missing
        }
    }

    /// The stored value, or nil when missing or unavailable.
    static func get(account: String) -> String? {
        if case .found(let value) = lookup(account: account) { return value }
        return nil
    }

    static func isSet(account: String) -> Bool {
        if case .found = lookup(account: account) { return true }
        return false
    }

    /// Adds or replaces the value.
    static func set(_ value: String, account: String) throws {
        let data = Data(value.utf8)
        let update = SecItemUpdate(baseQuery(account: account) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if update == errSecSuccess { return }
        guard update == errSecItemNotFound else { throw Failure(status: update) }
        var attributes = baseQuery(account: account)
        attributes[kSecValueData as String] = data
        attributes[kSecAttrLabel as String] = "Context — \(account)"
        let add = SecItemAdd(attributes as CFDictionary, nil)
        guard add == errSecSuccess else { throw Failure(status: add) }
    }

    /// Removes the value; a missing item is not an error.
    static func remove(account: String) throws {
        let status = SecItemDelete(baseQuery(account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw Failure(status: status) }
    }

    private static func baseQuery(account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecUseDataProtectionKeychain as String: false,
        ]
    }
}

// MARK: - LoginShellEnvironment

/// The user's login-shell environment, captured once per app run by running
/// `/bin/zsh -ilc` with a NUL marker so rc-file chatter is skipped.
enum LoginShellEnvironment {
    static let marker = Data("\u{0}__CTX_ENV__\u{0}".utf8)
    /// `printenv -0` where it exists (GNU coreutils); BSD `env -0` on stock macOS.
    static let script = "printf \"\\0__CTX_ENV__\\0\"; printenv -0 2>/dev/null || env -0"
    static let timeout: TimeInterval = 5

    private static let cached = Task<[String: String], Never>.detached(priority: .utility) {
        capture() ?? ProcessInfo.processInfo.environment
    }

    /// Cached after the first call; falls back to the process environment.
    static func snapshot() async -> [String: String] {
        await cached.value
    }

    /// Parses `<anything><marker>NAME=value\0NAME=value\0…`. Nil when the marker is absent.
    static func parse(_ data: Data) -> [String: String]? {
        guard let markerRange = data.range(of: marker, options: [.backwards]) else { return nil }
        let payload = data[markerRange.upperBound...]
        var result: [String: String] = [:]
        for entry in payload.split(separator: 0, omittingEmptySubsequences: true) {
            let text = String(decoding: entry, as: UTF8.self)
            guard let equals = text.firstIndex(of: "=") else { continue }
            let name = String(text[..<equals])
            guard !name.isEmpty else { continue }
            result[name] = String(text[text.index(after: equals)...])
        }
        return result
    }

    private static func capture() -> [String: String]? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-ilc", script]
        var environment = ProcessInfo.processInfo.environment
        environment["TERM"] = "dumb"
        environment["CTX_DESKTOP"] = "1"
        process.environment = environment
        process.standardInput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        let pipe = Pipe()
        process.standardOutput = pipe

        let lock = NSLock()
        var collected = Data()
        let finished = DispatchSemaphore(value: 0)
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty {
                handle.readabilityHandler = nil
                finished.signal()
                return
            }
            lock.lock()
            collected.append(chunk)
            lock.unlock()
        }

        do {
            try process.run()
        } catch {
            pipe.fileHandleForReading.readabilityHandler = nil
            BackendLog.debug("login shell did not start: \(error.localizedDescription)")
            return nil
        }

        if finished.wait(timeout: .now() + timeout) == .timedOut {
            BackendLog.debug("login shell timed out after \(Int(timeout)) s; terminating")
            if process.isRunning { process.terminate() }
            if finished.wait(timeout: .now() + 1) == .timedOut {
                if process.isRunning { Darwin.kill(process.processIdentifier, SIGKILL) }
                _ = finished.wait(timeout: .now() + 1)
            }
            pipe.fileHandleForReading.readabilityHandler = nil
        }

        lock.lock()
        let output = collected
        lock.unlock()
        guard let parsed = parse(output), !parsed.isEmpty else {
            BackendLog.debug("login shell produced no usable environment (\(output.count) bytes)")
            return nil
        }
        BackendLog.debug("login shell environment captured: \(parsed.count) variables")
        return parsed
    }
}

// MARK: - BackendEnvironment

/// The environment handed to `ctx serve`.
enum BackendEnvironment {
    /// Added to PATH so `ctx` finds cargo/homebrew tools even when the login
    /// shell could not be captured.
    static let extraPathDirectories = ["~/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin"]

    /// Login-shell variables that describe that shell run, not the user's setup.
    static let shellLocalVariables: Set<String> = ["PWD", "OLDPWD", "SHLVL", "_", "TERM", "CTX_DESKTOP"]

    /// Process environment + the login shell's variables and PATH + the API
    /// key variable (Keychain, else the login shell) when `apiKeyEnv` names
    /// one that is not set yet. An app launched from Finder only gets
    /// launchd's PATH, so without the login shell the agent's commands
    /// cannot find pnpm, node, brew tools and the like.
    static func build(apiKeyEnv: String?) async -> [String: String] {
        let process = ProcessInfo.processInfo.environment
        let login = await LoginShellEnvironment.snapshot()
        var environment = merged(process: process, login: login)

        // Keychain beats the login shell; an exported variable beats both.
        if let name = apiKeyEnv?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty,
           (process[name] ?? "").isEmpty {
            let fromKeychain = await Task.detached(priority: .userInitiated) { KeychainStore.get(account: name) }.value
            if let key = fromKeychain, !key.isEmpty {
                environment[name] = key
            }
        }
        return environment
    }

    /// Variables the process lacks come from the login shell; PATH is the
    /// login shell's, then the extra directories, then the process's own.
    static func merged(process: [String: String], login: [String: String]) -> [String: String] {
        var environment = process
        for (name, value) in login where environment[name] == nil && !shellLocalVariables.contains(name) {
            environment[name] = value
        }
        environment["PATH"] = mergedPath(login: login["PATH"], current: process["PATH"])
        return environment
    }

    static func mergedPath(login: String?, current: String?) -> String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let extra = extraPathDirectories.map { $0.hasPrefix("~") ? home + $0.dropFirst() : $0 }
        let split = { (path: String?) in (path ?? "").split(separator: ":").map(String.init).filter { !$0.isEmpty } }
        var seen: Set<String> = []
        return (split(login) + extra + split(current)).filter { seen.insert($0).inserted }.joined(separator: ":")
    }
}
