import AppKit
import Foundation
import Security
import Sparkle

// Self-updates (ARCHITECTURE.md "Updates"). Sparkle replaces the whole
// bundle, the app and `Contents/Helpers/ctx` together, so the backend always
// speaks the protocol this app expects. Sparkle's own helper processes
// download, verify (EdDSA against `SUPublicEDKey`, then the Developer ID
// signature) and install; this is the one place where code in the app
// reaches the network. The feed and the defaults (check daily, download in
// the background, install on quit) are in apps/macos/Info.plist;
// apps/macos/scripts/release.sh writes and publishes the appcast.

@MainActor
@Observable
final class Updater {
    /// False for builds that must not replace themselves: anything not signed
    /// with a Developer ID (Xcode and local `xcodebuild` builds sign ad hoc)
    /// and `--autotest` runs. The menu item is hidden and Settings says why.
    let isAvailable: Bool
    /// Sparkle's `canCheckForUpdates`: false while a check or an install is under way.
    private(set) var canCheckForUpdates = false
    private(set) var automaticallyChecks = false
    private(set) var automaticallyInstalls = false
    private(set) var lastCheck: Date?

    /// Whether a task is running; a relaunch to install offers to wait for it.
    @ObservationIgnored var isBusy: () -> Bool = { false }
    /// Called once a relaunch waits for the running tasks.
    @ObservationIgnored var onPostpone: () -> Void = {}

    @ObservationIgnored private var controller: SPUStandardUpdaterController?
    @ObservationIgnored private let delegate = UpdaterDelegate()
    @ObservationIgnored private var observations: [NSKeyValueObservation] = []

    /// `CFBundleShortVersionString`: the release version (`release.sh` sets it
    /// from Cargo.toml, so it is also the bundled ctx's version).
    static var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
    }

    init(launchOptions: LaunchOptions = .current) {
        isAvailable = !launchOptions.isAutotest && Self.isDeveloperIDSigned
        guard isAvailable else { return }
        delegate.owner = self
        let controller = SPUStandardUpdaterController(startingUpdater: true, updaterDelegate: delegate, userDriverDelegate: nil)
        self.controller = controller
        let updater = controller.updater
        // Sparkle changes these on the main thread (a check starting or
        // ending, the update alert's "automatically" checkbox).
        let changed: (SPUUpdater) -> Void = { [weak self] _ in
            Task { @MainActor in self?.refresh() }
        }
        observations = [
            updater.observe(\.canCheckForUpdates, options: [.new]) { updater, _ in changed(updater) },
            updater.observe(\.automaticallyChecksForUpdates, options: [.new]) { updater, _ in changed(updater) },
            updater.observe(\.automaticallyDownloadsUpdates, options: [.new]) { updater, _ in changed(updater) },
        ]
        refresh()
    }

    /// Re-reads Sparkle's state; Settings also calls it when it appears.
    func refresh() {
        guard let updater = controller?.updater else { return }
        canCheckForUpdates = updater.canCheckForUpdates
        automaticallyChecks = updater.automaticallyChecksForUpdates
        automaticallyInstalls = updater.automaticallyDownloadsUpdates
        lastCheck = updater.lastUpdateCheckDate
    }

    /// Check for Updates… / Settings → Check Now. While a check is under way
    /// Sparkle brings its window forward instead of starting another one.
    func checkForUpdates() {
        controller?.checkForUpdates(nil)
    }

    func setAutomaticallyChecks(_ on: Bool) {
        controller?.updater.automaticallyChecksForUpdates = on
        refresh()
    }

    func setAutomaticallyInstalls(_ on: Bool) {
        controller?.updater.automaticallyDownloadsUpdates = on
        refresh()
    }

    /// "Install and Relaunch" while an agent is working: restarting stops the
    /// run, so ask. Returns true when the relaunch waits; `install` then runs
    /// once no task is running.
    fileprivate func postponeRelaunch(_ install: @escaping () -> Void) -> Bool {
        guard isBusy() else { return false }
        let alert = NSAlert()
        alert.messageText = "Restart Context to finish updating?"
        alert.informativeText = "A task is still running, and restarting now stops it. Context can restart by itself as soon as every running task has finished."
        alert.addButton(withTitle: "Restart When Done")
        alert.addButton(withTitle: "Restart Now")
        guard alert.runModal() == .alertFirstButtonReturn else { return false }
        onPostpone()
        Task { @MainActor [weak self] in
            while self?.isBusy() == true {
                try? await Task.sleep(for: .seconds(2))
            }
            install()
        }
        return true
    }

    /// The running code satisfies Apple's Developer ID requirement (what
    /// release.sh signs with). Checks only what establishes the identity, not
    /// every sealed resource, so it costs next to nothing at launch.
    private static var isDeveloperIDSigned: Bool {
        var code: SecCode?
        var requirement: SecRequirement?
        let developerID = "anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13]"
        guard SecCodeCopySelf([], &code) == errSecSuccess, let code,
              SecRequirementCreateWithString(developerID as CFString, [], &requirement) == errSecSuccess, let requirement
        else { return false }
        return SecCodeCheckValidity(code, [], requirement) == errSecSuccess
    }
}

/// Sparkle holds its delegate weakly; `Updater` keeps this one alive. Sparkle
/// calls it on the main thread.
private final class UpdaterDelegate: NSObject, SPUUpdaterDelegate {
    weak var owner: Updater?

    /// `CTX_UPDATE_FEED` points a release build at another appcast (a staging
    /// feed, a local test server); updates must still carry a valid signature.
    func feedURLString(for updater: SPUUpdater) -> String? {
        ProcessInfo.processInfo.environment["CTX_UPDATE_FEED"]
    }

    func updater(_ updater: SPUUpdater, shouldPostponeRelaunchForUpdate item: SUAppcastItem, untilInvokingBlock installHandler: @escaping () -> Void) -> Bool {
        MainActor.assumeIsolated {
            owner?.postponeRelaunch(installHandler) ?? false
        }
    }
}
