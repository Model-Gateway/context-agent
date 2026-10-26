import SwiftUI
import Foundation

/// Command-line and environment switches read once at launch
/// (ARCHITECTURE.md, "Self-test mode"):
///
/// - `--mock` or `CTX_DESKTOP_MOCK=1`: start the backend with `ctx serve --mock`.
/// - `--project <path>`: open this folder at launch instead of the remembered projects.
/// - `--autotest <dir>`: run the scripted self-test (`UITestDriver`), write
///   screenshots and `result.json` into `dir`, then exit.
/// - `--binary <path>`: backend binary override for this launch only.
///
/// `CTX_DATA_DIR` is not parsed here: `AppState.startBackend` passes it
/// through to the child verbatim.
struct LaunchOptions: Equatable {
    /// `--mock` / `CTX_DESKTOP_MOCK=1`.
    var mock: Bool = false
    /// `--project <path>` (tilde expanded, otherwise as given).
    var projectPath: String?
    /// `--autotest <dir>` (tilde expanded, otherwise as given).
    var autotestDirectory: String?
    /// `--binary <path>` (tilde expanded, otherwise as given).
    var binaryOverride: String?

    /// The options of the running process.
    static let current = LaunchOptions(
        arguments: ProcessInfo.processInfo.arguments,
        environment: ProcessInfo.processInfo.environment
    )

    init() {}

    /// Parses `arguments` (the first element is the executable and is skipped)
    /// and `environment`. A flag that expects a value is ignored when the value
    /// is missing or looks like another flag.
    init(arguments: [String], environment: [String: String]) {
        let args = Array(arguments.dropFirst())
        var index = 0
        while index < args.count {
            let arg = args[index]
            let next: String? = {
                guard args.indices.contains(index + 1) else { return nil }
                let candidate = args[index + 1]
                return candidate.hasPrefix("--") ? nil : candidate
            }()
            switch arg {
            case "--mock":
                mock = true
            case "--project":
                if let next {
                    projectPath = Self.expand(next)
                    index += 1
                }
            case "--autotest":
                if let next {
                    autotestDirectory = Self.expand(next)
                    index += 1
                }
            case "--binary":
                if let next {
                    binaryOverride = Self.expand(next)
                    index += 1
                }
            default:
                // `--flag=value` forms and unknown switches (Xcode / AppKit add
                // their own, e.g. -NSDocumentRevisionsDebugMode) are ignored.
                if let value = Self.value(of: "--project", in: arg) { projectPath = value }
                else if let value = Self.value(of: "--autotest", in: arg) { autotestDirectory = value }
                else if let value = Self.value(of: "--binary", in: arg) { binaryOverride = value }
            }
            index += 1
        }
        if let env = environment["CTX_DESKTOP_MOCK"], Self.isTruthy(env) {
            mock = true
        }
    }

    /// True when `--autotest <dir>` was given.
    var isAutotest: Bool { autotestDirectory != nil }

    /// `--project` as a file URL, when given.
    var projectURL: URL? {
        projectPath.map { URL(fileURLWithPath: $0) }
    }

    /// `--autotest` as a file URL, when given.
    var autotestURL: URL? {
        autotestDirectory.map { URL(fileURLWithPath: $0) }
    }

    /// `--binary` as a file URL, when given.
    var binaryURL: URL? {
        binaryOverride.map { URL(fileURLWithPath: $0) }
    }

    /// Standardized paths of every option value (`--project`, `--autotest`,
    /// `--binary`). AppKit also hands these to `application(_:openFiles:)` as
    /// documents to open; `AppDelegate` uses this set to ignore them.
    var argumentPaths: Set<String> {
        Set([projectPath, autotestDirectory, binaryOverride].compactMap { $0 }.map { URL(fileURLWithPath: $0).standardizedFileURL.path })
    }

    // MARK: - Helpers

    private static func expand(_ path: String) -> String {
        (path as NSString).expandingTildeInPath
    }

    /// `--flag=value` → `value`.
    private static func value(of flag: String, in argument: String) -> String? {
        let prefix = flag + "="
        guard argument.hasPrefix(prefix) else { return nil }
        let value = String(argument.dropFirst(prefix.count))
        return value.isEmpty ? nil : expand(value)
    }

    private static func isTruthy(_ value: String) -> Bool {
        switch value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "1", "true", "yes", "on": return true
        default: return false
        }
    }
}
