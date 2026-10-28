import SwiftUI
import AppKit
import ApplicationServices
import Foundation

/// Scripted self-test (`--autotest <dir>`, ARCHITECTURE.md "Self-test mode").
///
/// Drives `AppState` exactly like a user would — wait for the backend, open
/// the project, start a chat, send a prompt, wait for `task/finished`, open
/// the model picker, visit the Changes and Project tabs — and captures a PNG
/// of the main window after each step with
/// `NSView.bitmapImageRepForCachingDisplay` + `cacheDisplay` (no screen
/// recording permission needed). It writes `result.json`
/// (`{ok, steps, transcriptItems, transcript, error}`) into the directory,
/// logs every step to stderr and exits 0 on success, 1 on failure.
@MainActor
enum UITestDriver {
    static let prompt = "Summarize this repository in three bullet points"
    /// Poll interval for every wait (100 ms).
    static let pollInterval: UInt64 = 100_000_000
    static let windowTimeout: TimeInterval = 15
    static let backendTimeout: TimeInterval = 90
    static let taskTimeout: TimeInterval = 180
    /// How long the backend gets to shut down cleanly before the process exits.
    static let shutdownTimeout: TimeInterval = 6

    /// One scripted step, as written to `result.json`.
    struct Step: Encodable {
        var name: String
        var ok: Bool
        var detail: String?
        var screenshot: String?
        var elapsedMs: Int
    }

    struct Result: Encodable {
        var ok: Bool
        var steps: [Step]
        /// Number of committed transcript items in the test chat.
        var transcriptItems: Int
        /// One short label per transcript item ("user", "assistant", "tools(2)", "outcome(completed)").
        var transcript: [String]
        var error: String?
        var screenshots: [String]
        var directory: String
        var project: String?
        var finishedAt: String
    }

    /// True once the script has been started (by either entry point below).
    private(set) static var hasStarted = false

    /// Starts the self-test when `--autotest <dir>` was given; a no-op
    /// otherwise and when it already runs. Called from `ContextAgentApp`
    /// after `startBackend()` returned in the first window's `.task`.
    static func startIfRequested(_ appState: AppState) {
        guard LaunchOptions.current.isAutotest, !hasStarted else { return }
        hasStarted = true
        Task { @MainActor in
            await run(appState: appState)
        }
    }

    /// Fallback entry point armed from `ContextAgentApp.init` when `--autotest`
    /// was given: if no window appeared after `windowTimeout` (SwiftUI does not
    /// open the launch window while the console is locked, for example), the
    /// backend is started and the script runs anyway so the run fails fast
    /// with a diagnosis in `result.json` instead of hanging until the caller's
    /// timeout. A no-op when the normal entry point ran first.
    static func armWindowWatchdog(_ appState: AppState) {
        guard LaunchOptions.current.isAutotest else { return }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: UInt64(windowTimeout * 1_000_000_000))
            guard !hasStarted else { return }
            hasStarted = true
            FileHandle.standardError.write(Data("[autotest] no window appeared within \(Int(windowTimeout)) s; starting the backend and the script from the watchdog\n".utf8))
            await appState.startBackend()
            await run(appState: appState)
        }
    }

    /// Whether the login session's screen is locked (`CGSessionCopyCurrentDictionary`).
    /// A locked console is the usual reason the launch window never appears.
    static func screenIsLocked() -> Bool {
        guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else { return false }
        if let flag = session["CGSSessionScreenIsLocked"] as? Bool { return flag }
        if let number = session["CGSSessionScreenIsLocked"] as? NSNumber { return number.boolValue }
        return false
    }

    /// Runs the whole script and terminates the process. Returns only when
    /// `--autotest` was not given.
    static func run(appState: AppState) async {
        guard let directory = LaunchOptions.current.autotestURL else { return }
        let session = Session(appState: appState, directory: directory)
        let ok = await session.run()
        await session.finish(ok: ok)
    }
}

// MARK: - Session

@MainActor
private final class Session {
    private let appState: AppState
    private let directory: URL
    private var steps: [UITestDriver.Step] = []
    /// The step whose action is running (`note` / `fail` / `capture` annotate it).
    private var current: UITestDriver.Step?
    private var screenshots: [String] = []
    private var error: String?
    private var projectRoot: String?
    private var chat: ChatState?
    private let startedAt = Date()

    init(appState: AppState, directory: URL) {
        self.appState = appState
        self.directory = directory
    }

    // MARK: Script

    /// Every step in order; stops at the first failure.
    func run() async -> Bool {
        log("self-test started; output directory \(directory.path)")
        let bench: [(String, () async -> Bool)] = [
            ("prepare-directory", prepareDirectory),
            ("wait-for-window", waitForWindow),
            ("wait-for-backend", waitForBackend),
            ("open-project", openProject),
            ("02-empty-chat", { await self.newChat(screenshot: "02-empty-chat.png") }),
            ("send-prompt", sendPrompt),
            ("wait-for-finish", waitForFinish),
            ("perf-bench", perfBench),
        ]
        let script: [(String, () async -> Bool)] = ProcessInfo.processInfo.environment["CTX_PERFBENCH"] == "1" ? bench : [
            ("prepare-directory", prepareDirectory),
            ("wait-for-window", waitForWindow),
            ("wait-for-backend", waitForBackend),
            ("open-project", openProject),
            ("01-welcome", { await self.showWelcome(screenshot: "01-welcome.png") }),
            ("01b-projects", { await self.selectTab(.projects, screenshot: "01b-projects.png") }),
            ("02-empty-chat", { await self.newChat(screenshot: "02-empty-chat.png") }),
            ("02b-attachments", { await self.attachments(screenshot: "02b-attachments.png", card: "02c-attachment-card.png") }),
            ("send-prompt", sendPrompt),
            ("03-running", { await self.captureWhenLive(maxWait: 1.5, screenshot: "03-running.png") }),
            ("wait-for-finish", waitForFinish),
            ("04-finished", { await self.captureFinished(after: 1.0, screenshot: "04-finished.png") }),
            ("05-model-picker", { await self.modelPicker(screenshot: "05-model-picker.png") }),
            ("05b-model-card", { await self.modelCard(screenshot: "05b-model-card.png") }),
            ("06-changes", { await self.changesTab(screenshot: "06-changes.png") }),
            ("07-project", { await self.selectTab(.project, screenshot: "07-project.png") }),
            ("08-worktree", { await self.worktree(screenshot: "08-worktree.png") }),
            ("09-editor-rust", { await self.editorHighlighting("crates/lsp/src/catalog.rs", screenshot: "09-editor-rust.png") }),
            ("09b-editor-swift", { await self.editorHighlighting("apps/macos/ContextAgent/Views/Editor/TreeSitterHighlighter.swift", screenshot: "09b-editor-swift.png") }),
            ("10-profile", { await self.profilePage(screenshot: "10-profile.png", compact: "10b-profile-compact.png") }),
            ("click-search", clickSearchField),
            ("double-click-bar", doubleClickTopBar),
        ]
        for (name, action) in script {
            let began = Date()
            current = UITestDriver.Step(name: name, ok: false, detail: nil, screenshot: nil, elapsedMs: 0)
            let ok = await action()
            var step = current ?? UITestDriver.Step(name: name, ok: ok, detail: nil, screenshot: nil, elapsedMs: 0)
            step.ok = ok
            step.elapsedMs = Int(Date().timeIntervalSince(began) * 1000)
            steps.append(step)
            current = nil
            if ok {
                log("ok   \(name) (\(step.elapsedMs) ms)\(step.detail.map { " — " + $0 } ?? "")")
            } else {
                let detail = step.detail ?? "failed"
                if error == nil { error = "\(name): \(detail)" }
                log("FAIL \(name): \(detail)")
                return false
            }
        }
        return true
    }

    /// Writes `result.json`, stops the backend and exits the process.
    func finish(ok: Bool) async {
        writeResult(ok: ok)
        let total = Int(Date().timeIntervalSince(startedAt))
        log(ok ? "self-test passed in \(total) s" : "self-test FAILED in \(total) s: \(error ?? "unknown error")")
        await stopBackend()
        if ok {
            NSApp.terminate(nil)
            // `terminate` normally never returns; make the exit code deterministic if it does.
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            exit(0)
        } else {
            exit(1)
        }
    }

    // MARK: Steps

    private func prepareDirectory() async -> Bool {
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            return true
        } catch {
            return fail("could not create \(directory.path): \(error.localizedDescription)")
        }
    }

    private func waitForWindow() async -> Bool {
        let deadline = Date().addingTimeInterval(UITestDriver.windowTimeout)
        while Date() < deadline {
            if let window = mainWindow(), let view = window.contentView, view.bounds.width > 0, view.bounds.height > 0 {
                return note("\(Int(view.bounds.width))×\(Int(view.bounds.height)) pt")
            }
            await sleep(nanoseconds: UITestDriver.pollInterval)
        }
        let hint = UITestDriver.screenIsLocked()
            ? "the screen is locked, so SwiftUI did not open the launch window; unlock the session and run again"
            : "\(NSApp.windows.count) NSApp windows, none visible"
        return fail("no visible window after \(Int(UITestDriver.windowTimeout)) s — \(hint)")
    }

    private func waitForBackend() async -> Bool {
        let deadline = Date().addingTimeInterval(UITestDriver.backendTimeout)
        while Date() < deadline {
            switch appState.connection {
            case .ready(let result):
                return note("ctx \(result.ctxVersion) · protocol \(result.protocolVersion)\(result.mock ? " · mock" : "") · model \(result.model.name)")
            case .failed(let failure):
                if appState.backendStarting { break }
                return fail("backend failure — \(failure.title): \(Self.clip(failure.detail, 600)); stderr: \(Self.clip(appState.backend.stderrLog.suffix(8).joined(separator: " | "), 600))")
            case .connecting:
                break
            }
            await sleep(nanoseconds: UITestDriver.pollInterval)
        }
        return fail("backend not ready after \(Int(UITestDriver.backendTimeout)) s (state: \(appState.connectionLabel))")
    }

    private func openProject() async -> Bool {
        let url = LaunchOptions.current.projectURL ?? Self.defaultProjectURL()
        await appState.openProject(url: url)
        guard let project = appState.selectedProject else {
            return fail("project did not open: \(url.path)")
        }
        projectRoot = project.root
        return note("\(project.name) at \(project.root) · \(project.info.fileCount) files · model \(project.info.model)")
    }

    private func showWelcome(screenshot: String) async -> Bool {
        appState.sidebarItem = .welcome
        await settle()
        return capture(screenshot)
    }

    private func newChat(screenshot: String) async -> Bool {
        guard let project = appState.selectedProject else { return fail("no selected project") }
        appState.select(project: project.id, restoreSession: false)
        appState.newChat()
        await settle()
        guard appState.selectedSessionId == nil, appState.sidebarItem == .chat else {
            return fail("new chat did not reach the empty state (session \(appState.selectedSessionId ?? "nil"), tab \(appState.sidebarItem.rawValue))")
        }
        return capture(screenshot)
    }

    /// Attaches a few of the project's files to the empty-state draft,
    /// captures the composer chips, renders the first chip's hover card in
    /// its panel (hover cannot be scripted), then clears the draft again.
    private func attachments(screenshot: String, card: String) async -> Bool {
        guard let project = appState.selectedProject else { return fail("no selected project") }
        let root = project.rootURL
        let candidates = ["README.md", "Cargo.toml", "docs", "crates"]
            .map { root.appendingPathComponent($0) }
            .filter { FileManager.default.fileExists(atPath: $0.path) }
        guard let first = candidates.first else { return note("no README.md / Cargo.toml / docs / crates in the project; skipped") }
        appState.pendingAttachments[project.id] = ComposerAttachment.appending(candidates, to: [])
        defer { appState.pendingAttachments[project.id] = nil }
        await settle(seconds: 0.8)
        guard capture(screenshot) else { return false }
        let attachment = ComposerAttachment(url: first)
        let preview = await AttachmentPreview.load(attachment)
        guard let window = mainWindow(), let anchor = window.contentView else { return fail("no window to anchor the card") }
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        await settle(seconds: 0.3)
        HoverCardPanel.shared.show(AttachmentHoverCard(attachment: attachment, preview: preview), beside: anchor, placement: .below)
        await settle(seconds: 0.6)
        defer { HoverCardPanel.shared.hide() }
        guard capturePanel(card) else { return false }
        return note("\(candidates.count) attachments · card for \(attachment.name): \(preview.meta.joined(separator: " · "))")
    }

    /// Writes the visible hover-card panel to `name`.
    private func capturePanel(_ name: String) -> Bool {
        guard let panelView = NSApp.windows.first(where: { $0 is NSPanel && $0.isVisible })?.contentView,
              let rep = panelView.bitmapImageRepForCachingDisplay(in: panelView.bounds) else {
            return fail("the hover card panel did not appear")
        }
        panelView.cacheDisplay(in: panelView.bounds, to: rep)
        guard let data = rep.representation(using: .png, properties: [:]) else {
            return fail("could not encode \(name)")
        }
        do {
            try data.write(to: directory.appendingPathComponent(name), options: .atomic)
        } catch {
            return fail("could not write \(name): \(error.localizedDescription)")
        }
        screenshots.append(name)
        current?.screenshot = name
        return true
    }

    /// `CTX_PERFBENCH=1`: grows the chat and measures it (`PerfBench`),
    /// writing `perf.json` and a screenshot of the grown chat.
    private func perfBench() async -> Bool {
        guard let chat else { return fail("no chat") }
        let phases = await PerfBench.run(appState: appState, chat: chat) { log($0) }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        do {
            try encoder.encode(phases).write(to: directory.appendingPathComponent("perf.json"), options: .atomic)
        } catch {
            return fail("could not write perf.json: \(error.localizedDescription)")
        }
        _ = capture("perf-grown.png")
        return note("\(phases.count) phases, \(chat.transcript.count) items")
    }

    private func sendPrompt() async -> Bool {
        await appState.send(UITestDriver.prompt)
        guard let chat = appState.selectedChat else {
            return fail("send did not create a session (connection: \(appState.connectionLabel))")
        }
        self.chat = chat
        if let lastError = chat.lastError, !chat.status.isRunning {
            return fail("task/run failed: \(lastError)")
        }
        return note("session \(chat.sessionId) · status \(Self.describe(chat.status))")
    }

    /// Captures as soon as the run shows something live (streaming text or a
    /// tool call), at the latest after `maxWait`: the mock finishes a whole
    /// run in about a second, so a fixed delay would only ever show the end.
    private func captureWhenLive(maxWait: TimeInterval, screenshot: String) async -> Bool {
        let deadline = Date().addingTimeInterval(maxWait)
        var sawLive = false
        while Date() < deadline, let chat {
            if !chat.status.isRunning { break }
            if chat.live != nil || chat.liveTools != nil {
                sawLive = true
                break
            }
            await sleep(nanoseconds: 50_000_000)
        }
        // Let SwiftUI render the live content (deltas flush every 33 ms).
        if sawLive { await settle(seconds: 0.2) }
        if let chat { _ = note("status \(Self.describe(chat.status))\(sawLive ? " · live content on screen" : "")") }
        return capture(screenshot)
    }

    /// Settles, opens the output card of the first tool row that read
    /// README.md (an in-process accessibility press, as a click would), then
    /// captures the finished transcript.
    private func captureFinished(after seconds: TimeInterval, screenshot: String) async -> Bool {
        await sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
        var details: [String] = []
        if let chat { details.append("status \(Self.describe(chat.status))") }
        if clickAccessibilityButton(labelContaining: "README.md") {
            await settle(seconds: 0.5)
            details.append("tool output card opened")
        } else {
            details.append("tool output card NOT opened (row not found through accessibility)")
        }
        _ = note(details.joined(separator: " · "))
        return capture(screenshot)
    }

    /// Changes tab with the first changed file selected, so the diff pane renders.
    private func changesTab(screenshot: String) async -> Bool {
        appState.sidebarItem = .changes
        await settle(seconds: 0.4)
        guard appState.sidebarItem == .changes else { return fail("tab did not switch to changes") }
        if let project = appState.selectedProject {
            if project.gitStatus == nil { await appState.refreshGit(project: project) }
            if let first = project.gitStatus?.files.first {
                await appState.selectChange(path: first.path, in: project)
                _ = note("selected \(first.path) · diff \(project.diff.map { "\($0.diff.count) chars" } ?? "nil")\(project.diffError.map { " · error: \($0)" } ?? "")")
            } else {
                _ = note("no changed files")
            }
        }
        await settle(seconds: 0.6)
        return capture(screenshot)
    }

    /// Clicks the first accessibility button whose description or title
    /// contains `fragment`. The element is located through the process's own
    /// `AXUIElement` tree (inspecting one's own process needs no
    /// assistive-access permission; SwiftUI's in-process `accessibilityChildren`
    /// stays empty until an AX client asks), then a mouse down/up pair is sent
    /// to the window at the element's centre, which triggers the SwiftUI
    /// `Button` exactly like a click (`accessibilityPerformPress` does not).
    /// When nothing matches, the tree is dumped to `ax-dump.txt` for diagnosis.
    private func clickAccessibilityButton(labelContaining fragment: String) -> Bool {
        guard let window = mainWindow() else { return false }
        let application = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
        var windowsRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(application, kAXWindowsAttribute as CFString, &windowsRef) == .success,
              let axWindows = windowsRef as? [AXUIElement] else {
            log("accessibility: could not list the application's windows")
            return false
        }
        func attribute(_ element: AXUIElement, _ name: String) -> String {
            var value: CFTypeRef?
            guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else { return "" }
            return value as? String ?? ""
        }
        var visited = 0
        var dump: [String] = []
        var found: AXUIElement?
        func walk(_ element: AXUIElement, depth: Int) {
            guard found == nil, depth < 40, visited < 20_000 else { return }
            visited += 1
            let role = attribute(element, kAXRoleAttribute as String)
            let text = [attribute(element, kAXDescriptionAttribute as String), attribute(element, kAXTitleAttribute as String)]
                .filter { !$0.isEmpty }
                .joined(separator: " | ")
            if dump.count < 800 {
                dump.append(String(repeating: "  ", count: depth) + "\(role) \(Self.clip(text, 100))")
            }
            if role == kAXButtonRole as String, text.localizedCaseInsensitiveContains(fragment) {
                found = element
                return
            }
            var childrenRef: CFTypeRef?
            guard AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &childrenRef) == .success else { return }
            for child in (childrenRef as? [AXUIElement]) ?? [] {
                walk(child, depth: depth + 1)
            }
        }
        for axWindow in axWindows {
            walk(axWindow, depth: 0)
        }
        guard let found, let frame = Self.axFrame(found) else {
            let url = directory.appendingPathComponent("ax-dump.txt")
            try? dump.joined(separator: "\n").write(to: url, atomically: true, encoding: .utf8)
            log("accessibility: no button containing “\(fragment)” among \(visited) elements; tree in \(url.path)")
            return false
        }
        // AX frames have a top-left origin on the primary screen; AppKit's screen space a bottom-left one.
        let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
        let centre = window.convertPoint(fromScreen: NSPoint(x: frame.midX, y: primaryHeight - frame.midY))
        let time = ProcessInfo.processInfo.systemUptime
        guard let down = NSEvent.mouseEvent(with: .leftMouseDown, location: centre, modifierFlags: [], timestamp: time, windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1),
              let up = NSEvent.mouseEvent(with: .leftMouseUp, location: centre, modifierFlags: [], timestamp: time + 0.05, windowNumber: window.windowNumber, context: nil, eventNumber: 2, clickCount: 1, pressure: 0) else {
            log("accessibility: could not synthesize the click events")
            return false
        }
        window.sendEvent(down)
        window.sendEvent(up)
        log("accessibility: clicked “\(Self.clip(attribute(found, kAXDescriptionAttribute as String), 80))” at (\(Int(centre.x)), \(Int(centre.y))) in window coordinates")
        return true
    }

    /// Screen frame (top-left origin) of an accessibility element.
    private static func axFrame(_ element: AXUIElement) -> CGRect? {
        var positionRef: CFTypeRef?
        var sizeRef: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, kAXPositionAttribute as CFString, &positionRef) == .success,
              AXUIElementCopyAttributeValue(element, kAXSizeAttribute as CFString, &sizeRef) == .success,
              let positionValue = positionRef, CFGetTypeID(positionValue) == AXValueGetTypeID(),
              let sizeValue = sizeRef, CFGetTypeID(sizeValue) == AXValueGetTypeID() else { return nil }
        var position = CGPoint.zero
        var size = CGSize.zero
        guard AXValueGetValue(unsafeDowncast(positionValue, to: AXValue.self), .cgPoint, &position),
              AXValueGetValue(unsafeDowncast(sizeValue, to: AXValue.self), .cgSize, &size) else { return nil }
        return CGRect(origin: position, size: size)
    }

    private func waitForFinish() async -> Bool {
        guard let chat else { return fail("no chat to wait for") }
        let deadline = Date().addingTimeInterval(UITestDriver.taskTimeout)
        while Date() < deadline {
            if case .failed(let failure) = appState.connection {
                return fail("backend failed while the task ran — \(failure.title): \(Self.clip(failure.detail, 400))")
            }
            switch chat.status {
            case .finished(let status):
                let detail = "status \(status.rawValue) · \(chat.transcript.count) transcript items · \(chat.transcript.allToolCalls.count) tool calls"
                    + (chat.lastError.map { " · error: \(Self.clip($0, 300))" } ?? "")
                if status == .failed, let lastError = chat.lastError {
                    return fail("task failed: \(Self.clip(lastError, 400))")
                }
                return note(detail)
            case .idle:
                if let lastError = chat.lastError {
                    return fail("task/run failed: \(Self.clip(lastError, 400))")
                }
            case .running, .loadingHistory:
                break
            }
            await sleep(nanoseconds: UITestDriver.pollInterval)
        }
        return fail("task did not finish within \(Int(UITestDriver.taskTimeout)) s (status \(Self.describe(chat.status)))")
    }

    private func modelPicker(screenshot: String) async -> Bool {
        appState.modelPickerVisible = true
        await settle(seconds: 0.6)
        let ok = capture(screenshot)
        appState.modelPickerVisible = false
        await settle(seconds: 0.2)
        return ok
    }

    /// Shows the picker's hover card for the project model in its panel and
    /// captures the panel (hover cannot be scripted). Needs models.dev; offline it notes the skip and passes.
    private func modelCard(screenshot: String) async -> Bool {
        await appState.modelsDev.load()
        guard let model = appState.selectedProject?.info.model, let info = appState.modelInfo(model) else {
            return note("no models.dev entry for the project model (offline?); card not rendered")
        }
        _ = appState.modelsDev.logo(for: info.labId)
        await settle(seconds: 1.0)
        // Through the real hover panel, so layout-driven state (the
        // capability row's overflow) has settled when it is captured.
        guard let window = mainWindow(), let anchor = window.contentView else { return fail("no window to anchor the card") }
        // The panel hides while the app is inactive.
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        await settle(seconds: 0.4)
        HoverCardPanel.shared.show(ModelHoverCard(modelId: model, name: info.name, info: info, stat: appState.modelStat(model), catalog: appState.modelsDev).environment(appState), beside: anchor)
        await settle(seconds: 0.8)
        defer { HoverCardPanel.shared.hide() }
        guard capturePanel(screenshot) else { return false }
        log("rendered \(screenshot) for \(info.name) (\(info.labId))")
        return true
    }

    /// Clicks the top bar's search field (centred in the window, 26 pt from
    /// the top) through `NSWindow.sendEvent`, so AppKit's title-strip routing
    /// applies, and expects the command palette to open.
    private func clickSearchField() async -> Bool {
        guard let window = mainWindow(), let content = window.contentView else { return fail("no window") }
        appState.paletteVisible = false
        let point = NSPoint(x: content.bounds.midX, y: content.bounds.height - 26)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0) else {
                return fail("could not create the click")
            }
            window.sendEvent(event)
            await settle(seconds: 0.05)
        }
        await settle(seconds: 0.4)
        let opened = appState.paletteVisible
        appState.paletteVisible = false
        return opened ? note("palette opened") : fail("clicking the search field did not open the palette")
    }

    /// Double-clicks empty top-bar space (between the search field and the
    /// right-hand controls) and expects the window to zoom, then zooms it
    /// back.
    private func doubleClickTopBar() async -> Bool {
        guard let window = mainWindow(), let content = window.contentView else { return fail("no window") }
        let action = UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") ?? "Maximize"
        guard action != "Minimize", action != "None", action != "Do Nothing" else {
            return note("title-bar double-click is set to “\(action)” in System Settings; skipped")
        }
        // An inactive window swallows the first click on anything but a control.
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        await settle(seconds: 0.3)
        let wasZoomed = window.isZoomed
        let point = NSPoint(x: content.bounds.midX + 195, y: content.bounds.height - 26)
        for clicks in 1...2 {
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                guard let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: type == .leftMouseDown ? 1 : 0) else {
                    return fail("could not create the click")
                }
                window.sendEvent(event)
                await settle(seconds: 0.03)
            }
        }
        await settle(seconds: 1.0)
        let zoomed = window.isZoomed != wasZoomed
        if zoomed {
            window.zoom(nil)
            await settle(seconds: 0.8)
        }
        return zoomed ? note("window zoomed and restored") : fail("double-clicking empty top-bar space did not zoom the window")
    }

    /// A parallel session: creates a worktree of the project (nested under
    /// it in the sidebar, selected, empty chat), captures the window, then
    /// removes it together with its branch and expects the parent back.
    private func worktree(screenshot: String) async -> Bool {
        guard let parent = appState.selectedProject else { return fail("no selected project") }
        guard parent.isGitRepository else { return note("skipped: the project is not a git repository") }
        let before = appState.projects.count
        guard let worktree = await appState.newWorktree(in: parent, name: "Self test") else { return fail("worktree/create failed (see the toasts)") }
        await settle(seconds: 0.8)
        guard worktree.isWorktree, worktree.parentRoot == parent.root else { return fail("the new project is not a worktree of \(parent.name): \(worktree.info.git.map { "\($0)" } ?? "no git")") }
        guard appState.selectedProjectId == worktree.id, appState.selectedSessionId == nil else { return fail("the worktree's empty chat is not selected") }
        guard appState.topLevelProjects.contains(where: { $0.id == parent.id }), !appState.topLevelProjects.contains(where: { $0.id == worktree.id }) else { return fail("the worktree is not nested under its parent") }
        let branch = worktree.branch ?? "?"
        let captured = capture(screenshot)
        await appState.removeWorktree(worktree, deleteBranch: true)
        await settle(seconds: 0.4)
        guard appState.projects.count == before, appState.project(for: worktree.id) == nil else { return fail("the worktree project is still open after worktree/remove") }
        guard !FileManager.default.fileExists(atPath: worktree.root) else { return fail("the worktree folder still exists: \(worktree.root)") }
        guard appState.selectedProjectId == parent.id else { return fail("the parent was not reselected after the removal") }
        _ = note("created \(branch) at \(worktree.root), nested under \(parent.name), removed with its branch")
        return captured
    }

    /// Opens `path` in the editor and checks tree-sitter coloured it: the
    /// regex highlighter paints the text storage, tree-sitter the layout
    /// manager's temporary attributes, with types and functions as well.
    private func editorHighlighting(_ path: String, screenshot: String) async -> Bool {
        guard let project = appState.selectedProject else { return fail("no selected project") }
        let previous = appState.sidebarItem
        appState.openInEditor(path: path, line: 1, in: project)
        var colors: Set<String> = []
        for _ in 0..<40 where colors.count < 5 {
            await settle(seconds: 0.25)
            colors = editorTemporaryColors()
        }
        let captured = capture(screenshot)
        appState.sidebarItem = previous
        guard colors.count >= 5 else { return fail("\(path) has \(colors.count) tree-sitter colours, expected 5 or more") }
        _ = note("\(path): \(colors.count) tree-sitter colours")
        return captured
    }

    /// Distinct temporary foreground colours in the first 8000 characters of the editor.
    private func editorTemporaryColors() -> Set<String> {
        func find(_ view: NSView) -> CodeTextView? {
            if let textView = view as? CodeTextView { return textView }
            for subview in view.subviews { if let found = find(subview) { return found } }
            return nil
        }
        guard let content = mainWindow()?.contentView, let textView = find(content), let layoutManager = textView.layoutManager else { return [] }
        let limit = min(textView.string.utf16.count, 8000)
        var colors: Set<String> = []
        var index = 0
        while index < limit {
            var range = NSRange()
            let attributes = layoutManager.temporaryAttributes(atCharacterIndex: index, effectiveRange: &range)
            if let color = (attributes[.foregroundColor] as? NSColor)?.usingColorSpace(.sRGB) {
                colors.insert(String(format: "%.3f %.3f %.3f", color.redComponent, color.greenComponent, color.blueComponent))
            }
            index = max(NSMaxRange(range), index + 1)
        }
        return colors
    }

    /// The profile page's Account tab with a sample signed-in account (held
    /// in memory only), beside the inspector at its default width, then with
    /// the inspector as wide as it goes so the tab column turns icon-only.
    /// Restores the account, tab and inspector afterwards.
    private func profilePage(screenshot: String, compact: String) async -> Bool {
        let previous = (item: appState.sidebarItem, tab: appState.preferences.settingsTab, visible: appState.inspectorVisible, width: appState.inspectorWidth)
        let gateway = appState.gateway
        let saved = (account: gateway.account, usage: gateway.usage, refreshed: gateway.lastRefreshed)
        defer {
            gateway.account = saved.account
            gateway.usage = saved.usage
            gateway.lastRefreshed = saved.refreshed
            appState.preferences.settingsTab = previous.tab
            appState.inspectorWidth = previous.width
            appState.inspectorVisible = previous.visible
            appState.sidebarItem = previous.item
        }
        if gateway.account == nil {
            gateway.account = GatewayAccount(id: 1, username: "selftest", displayName: "Self Test", email: "selftest@example.com", group: "default", quota: 12_345_000, usedQuota: 4_200_000, requestCount: 1_284, token: nil, billingPreference: nil, subscriptions: nil)
            let today = Calendar.current.startOfDay(for: Date())
            let rows = (0..<200).compactMap { back -> GatewayUsageRow? in
                guard back % 3 != 1, let day = Calendar.current.date(byAdding: .day, value: -back, to: today) else { return nil }
                return GatewayUsageRow(createdAt: Int64(day.timeIntervalSince1970) + 36_000, modelName: back % 2 == 0 ? "gpt-4.1" : "claude-sonnet-4.5", project: "context-agent", count: back % 5 + 1, quota: (back * 37 % 600 + 50) * 500, tokenUsed: back * 911 % 20_000 + 300)
            }
            gateway.usage = UsageBreakdown.byRange(report: GatewayUsageReport(startTimestamp: 0, endTimestamp: Int64(Date().timeIntervalSince1970), rows: rows))
            gateway.lastRefreshed = Date()
        }
        appState.preferences.settingsTab = SettingsTab.account.rawValue
        appState.inspectorVisible = true
        appState.inspectorWidth = Theme.Metrics.inspectorWidth
        appState.sidebarItem = .account
        await settle(seconds: 0.6)
        guard capture(screenshot) else { return false }
        appState.inspectorWidth = 10_000
        await settle(seconds: 0.6)
        return capture(compact)
    }

    private func selectTab(_ item: SidebarItem, screenshot: String) async -> Bool {
        appState.sidebarItem = item
        await settle(seconds: 0.6)
        guard appState.sidebarItem == item else { return fail("tab did not switch to \(item.rawValue)") }
        return capture(screenshot)
    }

    // MARK: Screenshots

    /// Renders the main window's content view into a PNG in the output directory.
    private func capture(_ name: String) -> Bool {
        guard let window = mainWindow() else { return fail("no window to capture for \(name)") }
        // The frame view (the content view's superview) also draws the
        // traffic lights over the top bar; fall back to the content view.
        guard let content = window.contentView else { return fail("window has no content view for \(name)") }
        let view = content.superview.flatMap { $0.bounds.size == content.bounds.size ? $0 : nil } ?? content
        window.layoutIfNeeded()
        let bounds = view.bounds
        guard bounds.width > 0, bounds.height > 0 else { return fail("window content is empty (\(bounds.size)) for \(name)") }
        guard let rep = view.bitmapImageRepForCachingDisplay(in: bounds) else { return fail("could not allocate a bitmap for \(name)") }
        view.cacheDisplay(in: bounds, to: rep)
        let composed = Self.compose(main: rep, window: window, contentView: view) ?? rep
        guard let data = composed.representation(using: .png, properties: [:]) else { return fail("could not encode \(name) as PNG") }
        let url = directory.appendingPathComponent(name)
        do {
            try data.write(to: url, options: .atomic)
        } catch {
            return fail("could not write \(url.path): \(error.localizedDescription)")
        }
        screenshots.append(name)
        current?.screenshot = name
        log("captured \(name) (\(rep.pixelsWide)×\(rep.pixelsHigh) px, \(data.count) bytes)")
        return true
    }

    private func mainWindow() -> NSWindow? {
        let windows = NSApp.windows
        return windows.first { $0.isVisible && !($0 is NSPanel) } ?? windows.first
    }

    /// Draws every other visible window of the app (popovers are separate
    /// windows) over the main window's bitmap at its screen position, back to
    /// front, so a screenshot shows the model picker. Nil when there is none.
    private static func compose(main: NSBitmapImageRep, window: NSWindow, contentView: NSView) -> NSBitmapImageRep? {
        let others = NSApp.windows.filter { $0 !== window && $0.isVisible && $0.contentView != nil }
        guard !others.isEmpty else { return nil }
        let contentOnScreen = window.convertToScreen(contentView.convert(contentView.bounds, to: nil))
        let size = contentView.bounds.size
        guard size.width > 0, size.height > 0,
              let canvas = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: main.pixelsWide, pixelsHigh: main.pixelsHigh, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return nil }
        canvas.size = size
        guard let context = NSGraphicsContext(bitmapImageRep: canvas) else { return nil }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        // Draw in points: scale the context when it was handed over in pixel units.
        let ctm = context.cgContext.ctm
        if abs(ctm.a - 1) < 0.01, abs(ctm.d) - 1 < 0.01 {
            context.cgContext.scaleBy(x: CGFloat(main.pixelsWide) / size.width, y: CGFloat(main.pixelsHigh) / size.height)
        }
        main.draw(in: NSRect(origin: .zero, size: size))
        var drawn = 0
        for other in others.sorted(by: { $0.orderedIndex > $1.orderedIndex }) {
            guard let frameView = other.contentView?.superview ?? other.contentView else { continue }
            let frame = other.frame
            guard frame.intersects(contentOnScreen), frame.width > 0, frame.height > 0,
                  let rep = frameView.bitmapImageRepForCachingDisplay(in: frameView.bounds) else { continue }
            frameView.cacheDisplay(in: frameView.bounds, to: rep)
            // Screen coordinates have a bottom-left origin; a flipped context counts from the top.
            let y = context.isFlipped ? contentOnScreen.maxY - frame.maxY : frame.minY - contentOnScreen.minY
            let origin = NSPoint(x: frame.minX - contentOnScreen.minX, y: y)
            rep.draw(in: NSRect(origin: origin, size: frame.size))
            drawn += 1
        }
        NSGraphicsContext.restoreGraphicsState()
        return drawn > 0 ? canvas : nil
    }

    // MARK: Result

    private func writeResult(ok: Bool) {
        let items = chat?.transcript.items ?? []
        let result = UITestDriver.Result(
            ok: ok,
            steps: steps,
            transcriptItems: items.count,
            transcript: items.map(Self.label(for:)),
            error: error,
            screenshots: screenshots,
            directory: directory.path,
            project: projectRoot,
            finishedAt: ISO8601DateFormatter().string(from: Date())
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        do {
            let data = try encoder.encode(result)
            try data.write(to: directory.appendingPathComponent("result.json"), options: .atomic)
            log("wrote \(directory.appendingPathComponent("result.json").path)")
        } catch {
            log("could not write result.json: \(error.localizedDescription)")
        }
    }

    private func stopBackend() async {
        // Remote hosts first: their `ssh` children must not outlive the app.
        for host in appState.hosts.hosts where appState.remotes[host.id]?.client.isRunning == true {
            await appState.disconnect(host)
            log("disconnected \(host.displayName)")
        }
        guard appState.backend.state != .stopped else { return }
        let backend = appState.backend
        let stopper = Task { @MainActor in await backend.stop() }
        let deadline = Date().addingTimeInterval(UITestDriver.shutdownTimeout)
        while Date() < deadline, backend.state != .stopped {
            await sleep(nanoseconds: UITestDriver.pollInterval)
        }
        if backend.state != .stopped {
            stopper.cancel()
            log("backend did not stop within \(Int(UITestDriver.shutdownTimeout)) s; exiting anyway")
        } else {
            log("backend stopped")
        }
    }

    // MARK: Helpers

    /// Records a detail on the current step and returns true.
    private func note(_ detail: String) -> Bool {
        current?.detail = detail
        return true
    }

    /// Records a failure detail on the current step and returns false.
    private func fail(_ detail: String) -> Bool {
        current?.detail = detail
        current?.ok = false
        return false
    }

    private func settle(seconds: TimeInterval = 0.4) async {
        await sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    }

    private func sleep(nanoseconds: UInt64) async {
        try? await Task.sleep(nanoseconds: nanoseconds)
    }

    private func log(_ message: String) {
        let stamp = String(format: "%7.2fs", Date().timeIntervalSince(startedAt))
        FileHandle.standardError.write(Data("[autotest \(stamp)] \(message)\n".utf8))
    }

    /// Nearest ancestor of the bundle or the current directory holding `Cargo.toml`, else the current directory.
    private static func defaultProjectURL() -> URL {
        let fm = FileManager.default
        let cwd = URL(fileURLWithPath: fm.currentDirectoryPath)
        for start in [Bundle.main.bundleURL, cwd] {
            var dir = start.standardizedFileURL
            var guardCount = 0
            while guardCount < 64 {
                guardCount += 1
                if fm.fileExists(atPath: dir.appendingPathComponent("Cargo.toml").path) { return dir }
                let parent = dir.deletingLastPathComponent()
                if parent.path == dir.path { break }
                dir = parent
            }
        }
        return cwd
    }

    private static func describe(_ status: ChatStatus) -> String {
        switch status {
        case .idle: return "idle"
        case .loadingHistory: return "loadingHistory"
        case .running(let p): return "running(\(p.state.rawValue.lowercased()), step \(p.step))"
        case .finished(let s): return "finished(\(s.rawValue))"
        }
    }

    private static func label(for item: TranscriptItem) -> String {
        switch item {
        case .user(let u): return u.isRuntime ? "user(runtime)" : "user"
        case .assistant: return "assistant"
        case .tools(let g): return "tools(\(g.calls.count))"
        case .agents(let g): return "agents(\(g.agents.count))"
        case .approval(let a): return "approval(\(a.decision?.rawValue ?? "pending"))"
        case .outcome(let o): return "outcome(\(o.outcome.status.rawValue)\(o.summaryRepeatsReply ? ", summary repeats reply" : ""))"
        case .notice(let n): return n.isError ? "notice(error)" : "notice"
        }
    }

    private static func clip(_ text: String, _ limit: Int) -> String {
        let flat = text.replacingOccurrences(of: "\n", with: " ")
        return flat.count <= limit ? flat : String(flat.prefix(limit)) + "…"
    }
}
