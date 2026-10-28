import Foundation

/// "Install ctx on host" (docs/remote-hosts.md): puts the static Linux build
/// of this app's ctx version on a host that has none, or an older one,
/// without Rust, a checkout or root there. The archive comes from the GitHub
/// release of the app's version (.github/workflows/release-linux.yml) and is
/// downloaded here, so the host needs no internet access; it is unpacked
/// over the host's own ssh settings into `~/.local/bin`, the first directory
/// of `BackendLaunch.remotePath`.
enum RemoteInstaller {
    /// `<base>/v<version>/ctx-<target>.tar.gz`.
    static let releaseBase = "https://github.com/Model-Gateway/context-agent/releases/download"
    /// Where the binary goes on the host (for messages; the script spells it out).
    static let installDirectory = "~/.local/bin"

    enum InstallError: LocalizedError {
        case unsupported(String)
        case download(String)
        case remote(String)

        var errorDescription: String? {
            switch self {
            case .unsupported(let platform):
                return "There is no ctx build for \(platform.isEmpty ? "this host" : platform). Automatic install covers Linux on x86_64 and arm64; install ctx there yourself (cargo install --locked --git https://github.com/Model-Gateway/context-agent ctx-cli) or set the host's ctx path."
            case .download(let message), .remote(let message):
                return message
            }
        }
    }

    /// `uname -sm` → the release target; nil for a system without a build.
    static func target(uname: String) -> String? {
        let words = uname.split(whereSeparator: \.isWhitespace).map { $0.lowercased() }
        guard words.count >= 2, words[0] == "linux" else { return nil }
        switch words[1] {
        case "x86_64", "amd64": return "x86_64-unknown-linux-musl"
        case "aarch64", "arm64": return "aarch64-unknown-linux-musl"
        default: return nil
        }
    }

    /// "0.1.4" < "0.1.5" by numeric components (anything after a `-` or `+`
    /// is ignored); false when either is not a version ("unknown").
    static func isOlder(_ version: String, than other: String) -> Bool {
        func parts(_ text: String) -> [Int]? {
            let core = text.split(whereSeparator: { $0 == "-" || $0 == "+" }).first.map(String.init) ?? ""
            let numbers = core.split(separator: ".").map { Int($0) }
            guard !numbers.isEmpty, numbers.allSatisfy({ $0 != nil }) else { return nil }
            return numbers.compactMap { $0 }
        }
        guard let left = parts(version), let right = parts(other) else { return false }
        for index in 0..<max(left.count, right.count) {
            let a = index < left.count ? left[index] : 0
            let b = index < right.count ? right[index] : 0
            if a != b { return a < b }
        }
        return false
    }

    /// The archive for `target`: from the app version's release, or from the
    /// directory `$CTX_REMOTE_RELEASE_URL` names (a `file://` URL in tests).
    static func archiveURL(version: String, target: String) -> URL? {
        let name = "ctx-\(target).tar.gz"
        if let override = ProcessInfo.processInfo.environment["CTX_REMOTE_RELEASE_URL"]?.trimmingCharacters(in: .whitespacesAndNewlines), !override.isEmpty {
            return URL(string: override.hasSuffix("/") ? override + name : override + "/" + name)
        }
        return URL(string: "\(releaseBase)/v\(version)/\(name)")
    }

    /// Unpacks the archive on stdin next to the old binary, checks that the
    /// new one runs there (a `noexec` home fails here, not at the next
    /// connect), then renames it over `~/.local/bin/ctx`. Prints the
    /// version as its last line. Plain POSIX sh, run with `sh -c` whatever
    /// the login shell is.
    static let installScript = """
    set -e
    dir="$HOME/.local/bin"
    mkdir -p "$dir"
    tmp=$(mktemp -d "$dir/.ctx-install.XXXXXX")
    trap 'rm -rf "$tmp"' EXIT
    tar -xzf - -C "$tmp"
    chmod 755 "$tmp/ctx"
    "$tmp/ctx" --version
    mv -f "$tmp/ctx" "$dir/ctx"
    """

    /// Probes the host, downloads the matching archive and installs it;
    /// returns what the new binary printed for `--version` ("ctx 0.1.6").
    static func install(on host: SSHHost, version: String, environment: [String: String]) async throws -> String {
        let probe = try await ssh(host, command: "uname -sm", input: nil, environment: environment)
        guard probe.code == 0 else {
            throw InstallError.remote(failureText(probe, doing: "ask the host what it runs"))
        }
        let platform = probe.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let target = target(uname: platform) else { throw InstallError.unsupported(platform) }
        guard let url = archiveURL(version: version, target: target) else {
            throw InstallError.download("No download address for ctx \(version) (\(target)).")
        }

        let archive: Data
        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            if let http = response as? HTTPURLResponse, http.statusCode != 200 {
                throw InstallError.download(http.statusCode == 404
                    ? "The v\(version) release has no Linux build of ctx (\(url.lastPathComponent)). Update Context, or install ctx on the host yourself."
                    : "Downloading \(url.absoluteString) failed (HTTP \(http.statusCode)).")
            }
            archive = data
        } catch let error as InstallError {
            throw error
        } catch {
            throw InstallError.download("Could not download \(url.absoluteString): \(error.localizedDescription)")
        }

        let result = try await ssh(host, command: "sh -c " + BackendLaunch.shellQuote(installScript), input: archive, environment: environment)
        guard result.code == 0 else {
            throw InstallError.remote(failureText(result, doing: "install ctx in \(installDirectory)"))
        }
        let printed = result.stdout.split(whereSeparator: \.isNewline).last.map(String.init) ?? ""
        return printed.isEmpty ? "ctx \(version)" : printed
    }

    private struct Output {
        var code: Int32
        var stdout: String
        var stderr: String
    }

    private static func failureText(_ output: Output, doing what: String) -> String {
        let tail = output.stderr.split(whereSeparator: \.isNewline).suffix(6).joined(separator: "\n")
        let head = "Could not \(what) (ssh exited with code \(output.code))."
        return tail.isEmpty ? head : head + "\n" + tail
    }

    /// `ssh <host's options> -- <address> <command>` with `input` on stdin,
    /// the same way the app connects (`BackendLaunch.sshArguments`).
    private static func ssh(_ host: SSHHost, command: String, input: Data?, environment: [String: String]) async throws -> Output {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let process = Process()
                process.executableURL = BackendLaunch.sshBinary
                process.arguments = BackendLaunch.sshArguments(host: host) + [command]
                process.environment = environment
                let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
                process.standardInput = stdin
                process.standardOutput = stdout
                process.standardError = stderr
                do {
                    try process.run()
                } catch {
                    continuation.resume(throwing: InstallError.remote("Could not run \(BackendLaunch.sshBinary.path): \(error.localizedDescription)"))
                    return
                }
                // Both streams drain while stdin is written, so neither side
                // can stall on a full pipe; SIGPIPE is ignored app-wide, so a
                // host that hangs up early only fails the write.
                let group = DispatchGroup()
                var out = Data(), err = Data()
                DispatchQueue.global().async(group: group) { out = stdout.fileHandleForReading.readDataToEndOfFile() }
                DispatchQueue.global().async(group: group) { err = stderr.fileHandleForReading.readDataToEndOfFile() }
                if let input {
                    try? stdin.fileHandleForWriting.write(contentsOf: input)
                }
                try? stdin.fileHandleForWriting.close()
                group.wait()
                process.waitUntilExit()
                continuation.resume(returning: Output(code: process.terminationStatus, stdout: String(decoding: out, as: UTF8.self), stderr: String(decoding: err, as: UTF8.self)))
            }
        }
    }
}
