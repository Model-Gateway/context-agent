import Foundation

/// How a `ctx serve` child is started: the local binary, or `ssh` running
/// `ctx serve` on a remote host, which speaks the same protocol over the
/// SSH session's stdin and stdout (docs/desktop-protocol.md "Remote hosts").
enum BackendLaunch: Equatable {
    case local(binary: URL)
    case ssh(SSHHost)

    /// `/usr/bin/ssh`, or `$CTX_SSH_BINARY` (the self-test points it at a
    /// script that runs the local `ctx` so the transport is exercised
    /// without a server).
    static var sshBinary: URL {
        if let override = ProcessInfo.processInfo.environment["CTX_SSH_BINARY"]?.trimmingCharacters(in: .whitespacesAndNewlines), !override.isEmpty {
            return URL(fileURLWithPath: (override as NSString).expandingTildeInPath)
        }
        return URL(fileURLWithPath: "/usr/bin/ssh")
    }

    /// Directories a non-interactive remote shell may lack on its PATH but
    /// where `ctx` is usually installed.
    static let remotePath = "$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH"

    /// `ssh` options every connection gets: no password prompts (there is
    /// nobody to answer them; keys and the agent work), a bounded connect,
    /// keep-alives that notice a dropped link, no tty.
    static let sshOptions: [String] = [
        "-o", "BatchMode=yes",
        "-o", "ConnectTimeout=20",
        "-o", "ServerAliveInterval=15",
        "-o", "ServerAliveCountMax=4",
        "-o", "ClearAllForwardings=yes",
        "-T",
    ]

    var executable: URL {
        switch self {
        case .local(let binary): return binary
        case .ssh: return Self.sshBinary
        }
    }

    var host: SSHHost? {
        if case .ssh(let host) = self { return host }
        return nil
    }

    var isRemote: Bool { host != nil }

    /// The child's argument list for `ctx serve <serveArguments>`.
    func arguments(serve serveArguments: [String]) -> [String] {
        switch self {
        case .local:
            return serveArguments
        case .ssh(let host):
            return Self.sshArguments(host: host) + [Self.remoteCommand(host: host, serve: serveArguments)]
        }
    }

    /// Everything before the remote command: the fixed options, the host's
    /// port, key, user and extras, then the address after `--` so an
    /// address can never be read as an option.
    static func sshArguments(host: SSHHost) -> [String] {
        var arguments = sshOptions
        if host.port > 0 { arguments += ["-p", String(host.port)] }
        if !host.identityFile.isEmpty { arguments += ["-i", (host.identityFile as NSString).expandingTildeInPath] }
        if !host.user.isEmpty { arguments += ["-l", host.user] }
        arguments += host.extraArguments
        arguments += ["--", host.address]
        return arguments
    }

    /// The command the remote login shell runs: `ctx serve …` with the
    /// install directories on the PATH. Every piece is quoted for a POSIX
    /// shell (`$HOME`/`$PATH` in the PATH prefix are meant to expand there).
    static func remoteCommand(host: SSHHost, serve serveArguments: [String]) -> String {
        let binary = host.ctxPath.isEmpty ? "ctx" : host.ctxPath
        let words = ([binary] + serveArguments).map(shellQuote).joined(separator: " ")
        return "env PATH=\"\(remotePath)\" \(words)"
    }

    /// Single-quoted unless the word is plain (letters, digits, `-_./~=:`).
    static func shellQuote(_ word: String) -> String {
        let plain = word.unicodeScalars.allSatisfy { scalar in
            CharacterSet.alphanumerics.contains(scalar) || "-_./~=:+@%".unicodeScalars.contains(scalar)
        }
        if plain, !word.isEmpty, !word.hasPrefix("~") { return word }
        if word.hasPrefix("~/"), plain { return "~/" + shellQuote(String(word.dropFirst(2))) }
        return "'" + word.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    /// "ctx" or "ssh devbox" for messages.
    var label: String {
        switch self {
        case .local(let binary): return binary.path
        case .ssh(let host): return "ssh \(host.target)"
        }
    }
}
