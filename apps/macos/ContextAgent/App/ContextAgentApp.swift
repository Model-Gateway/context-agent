import SwiftUI
import AppKit
import Foundation

/// Application entry point. Owns the root `AppState` and the main window
/// scene (settings live on its profile page, `ProfileView`), starts the backend once the first window appears
/// and hands control to `UITestDriver` when `--autotest` was given.
@main
struct ContextAgentApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var appState: AppState

    init() {
        // The backend child's stdin may close at any time; a write into a dead
        // pipe must surface as EPIPE (BackendError.transport), never kill the app.
        signal(SIGPIPE, SIG_IGN)
        // Opt out of AppKit window-state restoration: this app persists the
        // window frame (`setFrameAutosaveName("Main")`) and the open projects
        // (`Preferences`) itself. Stale or foreign saved state for this bundle
        // id makes AppKit "restore" zero windows, after which SwiftUI never
        // opens the WindowGroup window (observed on macOS 26 after an earlier
        // dev build). Registered (volatile) defaults are consulted before the
        // persisted domain, so nothing is written to the user's preferences.
        UserDefaults.standard.register(defaults: ["ApplePersistenceIgnoreState": true])
        let state = AppState()
        _appState = State(initialValue: state)
        AppDelegate.openHandler = { url in
            Task { @MainActor in await state.openProject(url: url) }
        }
        AppDelegate.appearanceHandler = { state.preferences.applyAppearance() }
        // `--autotest` only: fail fast with a diagnosis when no window ever
        // appears (the normal start below never runs then).
        UITestDriver.armWindowWatchdog(state)
    }

    var body: some Scene {
        WindowGroup {
            MainWindow()
            .environment(appState)
            .task {
                // A second main window (SwiftUI opens one for an open-file
                // event that `AppDelegate` already handled) must never
                // restart a healthy backend: the launch start runs once.
                guard appState.claimBackendStart() else { return }
                BackendLog.debug("main window appeared; starting backend")
                if !UITestDriver.hasStarted {
                    await appState.startBackend()
                    BackendLog.debug("startBackend finished: \(appState.connectionLabel)")
                }
                UITestDriver.startIfRequested(appState)
            }
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1180, height: 780)
        // Folders handed to a running instance (`open -a`, a Dock drop) are
        // opened as projects by `AppDelegate.application(_:openFiles:)`; the
        // window group must not open a second main window for them.
        .handlesExternalEvents(matching: [])
        .commands {
            AppCommands(appState: appState)
        }
    }
}

/// AppKit delegate behind `@NSApplicationDelegateAdaptor`.
///
/// AppKit treats every command-line argument that does not start with `-`
/// and names an existing path as a document to open and delivers it through
/// `application(_:openFiles:)` instead of opening the untitled window. SwiftUI
/// then never opens the `WindowGroup` window, so a launch such as
/// `Context --mock --project /repo --autotest /dir` ran without any
/// window at all. This delegate absorbs those "documents" (the values of the
/// launch options are ignored; anything else — a folder dropped on the Dock
/// icon, `open -a "Context" folder` — opens as a project) and then asks
/// SwiftUI's own delegate for the untitled window when none is visible.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Opens a dropped / `open`ed path as a project; set by `ContextAgentApp.init`.
    static var openHandler: ((URL) -> Void)?
    /// Applies the saved Appearance setting; set by `ContextAgentApp.init`.
    static var appearanceHandler: (@MainActor () -> Void)?

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Before any window is shown, so a Light / Dark override never flashes.
        Self.appearanceHandler?()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        Self.ensureMainWindow()
    }

    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        let launchPaths = LaunchOptions.current.argumentPaths
        for name in filenames {
            let url = URL(fileURLWithPath: name).standardizedFileURL
            guard !launchPaths.contains(url.path) else { continue }
            Self.openHandler?(url)
        }
        sender.reply(toOpenOrPrint: .success)
        Self.ensureMainWindow()
    }

    /// On a plain launch SwiftUI has already opened the main window by the
    /// time `applicationDidFinishLaunching` runs, so this is a no-op; after a
    /// launch with "documents" it opens the window SwiftUI skipped, and
    /// after the window was closed ⌘, reopens it.
    static func ensureMainWindow() {
        DispatchQueue.main.async {
            let hasVisibleWindow = NSApp.windows.contains { $0.isVisible && !($0 is NSPanel) }
            guard !hasVisibleWindow else { return }
            BackendLog.debug("no main window after launch; opening the untitled window")
            _ = NSApp.delegate?.applicationOpenUntitledFile?(NSApp)
        }
    }
}
