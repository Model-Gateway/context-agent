import SwiftUI
import AppKit

/// The main window (ARCHITECTURE.md "Window layout"): the window-wide
/// `TopBar` (52 pt, traffic lights at its left) over `SidebarView` (248 pt)
/// · `DetailView` · `InspectorView` (320 pt, resizable) in one `HStack`, the toast
/// overlay (bottom-right), the ⌘K command palette overlay and the window configuration (frame autosave
/// "Main", hidden-title-bar tweaks).
struct MainWindow: View {
    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// The lower-discount question just answered, kept off screen until its
    /// answer is sent.
    @State private var answeredDiscountDrop: String?

    private var paneAnimation: Animation? { reduceMotion ? nil : .easeInOut(duration: 0.2) }

    /// The remote folder picker sheet (`AppState.remotePicker`).
    private var remotePickerBinding: Binding<RemotePickerRequest?> {
        Binding(get: { appState.remotePicker }, set: { appState.remotePicker = $0 })
    }

    var body: some View {
        VStack(spacing: 0) {
            TopBar()
            panes
        }
        // `.hiddenTitleBar` still insets content below the (invisible) title
        // bar; the top bar owns that strip itself, with the traffic lights
        // over its leading inset.
        .ignoresSafeArea(.container, edges: .top)
        .background(Theme.canvas)
        .overlay(alignment: .bottomTrailing) {
            ToastOverlay()
        }
        .overlay {
            if appState.paletteVisible {
                CommandPalette()
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.15), value: appState.paletteVisible)
        .onOpenURL { url in
            guard url.isFileURL else { return }
            Task { await appState.openProject(url: url) }
        }
        .sheet(item: remotePickerBinding) { request in
            RemoteFolderPicker(request: request)
                .environment(appState)
        }
        .background(WindowConfigurator())
        .frame(minWidth: 960, minHeight: 600)
        .onAppear {
            AppCommands.installKeyMonitor(appState: appState)
        }
        .alert(
            appState.pendingDiscountDrop?.request.summary ?? "",
            isPresented: discountDropPresented,
            presenting: appState.pendingDiscountDrop
        ) { pending in
            Button("Continue") { answerDiscountDrop(pending, .approve) }
                .keyboardShortcut(.defaultAction)
            Button("Always Continue") { answerDiscountDrop(pending, .approveForSession) }
            Button("Cancel", role: .cancel) { answerDiscountDrop(pending, .deny) }
        } message: { pending in
            Text("\(pending.request.details)\n\nContinue uses them for the rest of this task in “\(pending.chat.title)”. Always Continue stops asking; turn it back on in Settings → Spending.")
        }
    }

    /// A run's "go on at a much lower discount?" question (see
    /// `AppState.pendingDiscountDrop`).
    private var discountDropPresented: Binding<Bool> {
        Binding(
            get: { appState.pendingDiscountDrop.map { $0.request.id != answeredDiscountDrop } ?? false },
            set: { _ in }
        )
    }

    private func answerDiscountDrop(_ pending: (chat: ChatState, request: ApprovalRequest), _ decision: ApprovalDecision) {
        answeredDiscountDrop = pending.request.id
        Task { await appState.respond(approval: pending.request.id, in: pending.chat, decision: decision) }
    }

    /// Sidebar · centre pane · inspector under the top bar.
    private var panes: some View {
        GeometryReader { geometry in
            panes(in: geometry.size.width)
        }
    }

    private func panes(in totalWidth: CGFloat) -> some View {
        let maxInspector = Self.maxInspectorWidth(total: totalWidth, sidebarVisible: appState.sidebarVisible)
        let inspectorWidth = min(max(appState.inspectorWidth, Theme.Metrics.inspectorMinWidth), maxInspector)
        return HStack(spacing: 0) {
            if appState.sidebarVisible {
                SidebarView()
                    .frame(width: Theme.Metrics.sidebarWidth)
                    .hairline(.trailing)
                    .transition(reduceMotion ? .opacity : .move(edge: .leading))
            }
            // `minWidth: 0` + `clipped()`: content wider than the pane (a tab
            // at its minimum with both side panes open) must not widen the
            // HStack, which would centre it in the window and clip the sidebar.
            DetailView()
                .frame(minWidth: 0, maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
                .layoutPriority(1)
            if appState.inspectorVisible {
                InspectorView()
                    .frame(width: inspectorWidth)
                    .hairline(.leading)
                    .overlay(alignment: .leading) {
                        PaneResizeHandle(width: inspectorWidth, range: Theme.Metrics.inspectorMinWidth...maxInspector) { width in
                            appState.inspectorWidth = width
                        }
                    }
                    .transition(reduceMotion ? .opacity : .move(edge: .trailing))
            }
        }
        .animation(paneAnimation, value: appState.sidebarVisible)
        .animation(paneAnimation, value: appState.inspectorVisible)
    }

    /// The inspector never squeezes the centre pane below 420 pt.
    private static func maxInspectorWidth(total: CGFloat, sidebarVisible: Bool) -> CGFloat {
        let available = total - (sidebarVisible ? Theme.Metrics.sidebarWidth : 0) - 420
        return max(Theme.Metrics.inspectorMinWidth, min(Theme.Metrics.inspectorMaxWidth, available))
    }

    /// Opens every dropped file URL as a project (files open their parent
    /// directory, see `AppState.openProject`). Returns false when nothing in
    /// the drop is a file URL so the drop is refused.
    @discardableResult
    static func openDroppedFolders(_ providers: [NSItemProvider], appState: AppState) -> Bool {
        let candidates = providers.filter { $0.canLoadObject(ofClass: URL.self) }
        guard !candidates.isEmpty else { return false }
        for provider in candidates {
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url, url.isFileURL else { return }
                Task { @MainActor in
                    await appState.openProject(url: url)
                }
            }
        }
        return true
    }
}

/// Accent frame + caption shown while a folder is dragged over a drop target
/// in the Welcome screen.
struct DropHighlight: View {
    var message: String

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous)
        shape
            .strokeBorder(Theme.accent, lineWidth: 2)
            .background(Theme.accent.opacity(0.06), in: shape)
            .padding(8)
            .overlay {
                Text(message)
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .background(Theme.surface, in: Capsule())
                    .floatingShadow()
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

// MARK: - Pane resizing

/// Invisible strip over a trailing pane's leading hairline: dragging it left
/// widens the pane, double-clicking restores the default width.
private struct PaneResizeHandle: View {
    var width: CGFloat
    var range: ClosedRange<CGFloat>
    var defaultWidth: CGFloat = Theme.Metrics.inspectorWidth
    var onResize: (CGFloat) -> Void

    @State private var startWidth: CGFloat?
    @State private var hovering = false
    @State private var showingCursor = false

    var body: some View {
        Color.clear
            .frame(width: 8)
            .contentShape(Rectangle())
            .offset(x: -4)
            .onHover { inside in
                hovering = inside
                updateCursor()
            }
            .gesture(
                DragGesture(minimumDistance: 1, coordinateSpace: .global)
                    .onChanged { value in
                        let start = startWidth ?? width
                        startWidth = start
                        onResize(min(max(start - value.translation.width, range.lowerBound), range.upperBound))
                    }
                    .onEnded { _ in
                        startWidth = nil
                        updateCursor()
                    }
            )
            .onTapGesture(count: 2) {
                onResize(min(max(defaultWidth, range.lowerBound), range.upperBound))
            }
            .onDisappear {
                if showingCursor { NSCursor.pop() }
            }
            .accessibilityHidden(true)
    }

    /// Keeps the resize cursor while hovering or dragging (the pointer can
    /// outrun the strip mid-drag).
    private func updateCursor() {
        let wants = hovering || startWidth != nil
        if wants, !showingCursor {
            NSCursor.resizeLeftRight.push()
            showingCursor = true
        } else if !wants, showingCursor {
            NSCursor.pop()
            showingCursor = false
        }
    }
}

// MARK: - Window configuration

/// Looks up the hosting `NSWindow` once it exists and applies the settings
/// SwiftUI has no modifier for: frame autosave name "Main", transparent
/// title bar (the sidebar draws under the traffic lights), dragging by the
/// background and a minimum size.
private struct WindowConfigurator: NSViewRepresentable {
    final class Coordinator {
        var configured = false
        var observers: [NSObjectProtocol] = []

        deinit {
            observers.forEach(NotificationCenter.default.removeObserver)
        }
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        let coordinator = context.coordinator
        Task { @MainActor in
            WindowConfigurator.configure(windowOf: view, coordinator: coordinator)
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        guard !context.coordinator.configured else { return }
        let coordinator = context.coordinator
        Task { @MainActor in
            WindowConfigurator.configure(windowOf: nsView, coordinator: coordinator)
        }
    }

    @MainActor
    private static func configure(windowOf view: NSView, coordinator: Coordinator) {
        guard !coordinator.configured, let window = view.window else { return }
        coordinator.configured = true
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        // Only the top bar's empty space moves the window (`TitleBarArea`).
        window.isMovableByWindowBackground = false
        window.minSize = NSSize(width: 960, height: 600)
        _ = window.setFrameAutosaveName("Main")
        TrafficLights.align(in: window)
        // AppKit lays the buttons out again on resize, full screen and focus changes.
        let names: [Notification.Name] = [
            NSWindow.didResizeNotification, NSWindow.didEndLiveResizeNotification,
            NSWindow.didExitFullScreenNotification, NSWindow.didBecomeKeyNotification,
            NSWindow.didResignKeyNotification,
        ]
        coordinator.observers = names.map { name in
            NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak window] _ in
                MainActor.assumeIsolated {
                    if let window { TrafficLights.align(in: window) }
                }
            }
        }
    }
}

/// Moves the close / minimise / zoom buttons so they sit centred on the
/// 52 pt top bar (AppKit centres them in its 32 pt title strip) and start
/// over the sidebar's row icons. Their strip stays put, so clicks below it
/// still reach the bar.
@MainActor
enum TrafficLights {
    /// Leading edge of the close button: the sidebar's icon column (10 pt
    /// section padding + 8 pt row padding), so the buttons sit above it.
    static let leading: CGFloat = 18
    /// Gap between button origins (AppKit's own spacing).
    static let pitch: CGFloat = 20
    /// Centre of the buttons, from the window top: the bar's middle, less 1 pt
    /// so the 14 pt buttons stay inside their 32 pt strip.
    static let centerFromTop: CGFloat = 25

    static func align(in window: NSWindow) {
        guard !window.styleMask.contains(.fullScreen) else { return }
        let types: [NSWindow.ButtonType] = [.closeButton, .miniaturizeButton, .zoomButton]
        for (index, type) in types.enumerated() {
            guard let button = window.standardWindowButton(type), let strip = button.superview else { continue }
            var frame = button.frame
            frame.origin.x = leading + CGFloat(index) * pitch
            let top = centerFromTop - frame.height / 2
            frame.origin.y = strip.isFlipped ? top : strip.bounds.height - top - frame.height
            if button.frame != frame { button.setFrameOrigin(frame.origin) }
        }
    }
}

// MARK: - Toasts

/// Bottom-right stack of `ToastCenter` toasts.
private struct ToastOverlay: View {
    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let toasts = appState.toasts.toasts
        VStack(alignment: .trailing, spacing: 8) {
            ForEach(toasts) { toast in
                ToastView(toast: toast)
                    .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
            }
        }
        .padding(16)
        .animation(reduceMotion ? nil : .easeOut(duration: 0.2), value: toasts)
        .allowsHitTesting(!toasts.isEmpty)
    }
}

private struct ToastView: View {
    let toast: Toast
    @Environment(AppState.self) private var appState

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: toast.kind.systemImage)
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(toast.kind.tint)
                .padding(.top, 1)
                .accessibilityHidden(true)
            Text(verbatim: toast.message)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
                .selectableText()
            if toast.hasDetails {
                Button("Details") {
                    appState.toasts.showDetails(toast)
                }
                .buttonStyle(.plain)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.accent)
                .padding(.top, 1)
            }
            IconButton(Icons.close, label: "Dismiss", size: 11) {
                appState.toasts.dismiss(toast.id)
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, 8)
        .padding(.vertical, 10)
        .frame(maxWidth: 380, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .floatingShadow()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(toast.message)
    }
}

// MARK: - Preview

#Preview("Main window") {
    let state = MainWindowPreview.appState()
    return MainWindow()
        .environment(state)
        .frame(width: 1180, height: 780)
}

#Preview("Main window · backend failed") {
    let state = MainWindowPreview.appState(failure: .notFound(searched: ["~/.cargo/bin/ctx", "/opt/homebrew/bin/ctx", "/usr/local/bin/ctx"]))
    return MainWindow()
        .environment(state)
        .frame(width: 1180, height: 780)
}

/// Sample state for the previews of the shell views. It never touches the
/// user's defaults or recents file (separate defaults suite, temporary
/// recents file) and never spawns the backend.
@MainActor
enum MainWindowPreview {
    static func appState(failure: BackendFailure? = nil, needsAPIKey: Bool = false, withProjects: Bool = true) -> AppState {
        let defaults = UserDefaults(suiteName: "dev.contextagent.desktop.preview") ?? .standard
        let recentsURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("ContextAgentPreview", isDirectory: true)
            .appendingPathComponent("recent-projects.json")
        let state = AppState(
            preferences: Preferences(defaults: defaults),
            backend: BackendClient(),
            recents: RecentProjectStore(fileURL: recentsURL),
            toasts: ToastCenter(),
            notifier: TaskNotifier()
        )
        let config = InitializeResult(
            protocolVersion: 1, ctxVersion: "0.1.0", dataDir: "/Users/me/.context-agent",
            configFile: "/Users/me/.context-agent/config.toml", configSources: [], mock: false,
            model: ModelSettings(name: "gpt-4.1", baseUrl: "https://api.openai.com/v1", apiKeyEnv: "OPENAI_API_KEY", apiKeySet: !needsAPIKey, contextWindow: 128_000, maxOutputTokens: 16_000, reasoningEffort: nil),
            permissions: PermissionSettings(profile: "approval_required", allowedCommands: [], deniedCommands: [], allowNetworkCommands: false),
            pricing: PricingSettings(inputPerMillion: 2, cachedInputPerMillion: 0.5, outputPerMillion: 8)
        )
        state.config = config
        if let failure {
            state.connection = .failed(failure)
        } else {
            state.connection = .ready(config)
        }

        state.recents.touch(path: "/Users/me/Projects/context-agent", name: "context-agent")
        state.recents.touch(path: "/Users/me/Projects/growth-campaign", name: "growth-campaign")
        state.recents.touch(path: "/Users/me/Projects/content-engine", name: "content-engine")
        state.recents.setPinned(true, path: "/Users/me/Projects/content-engine")

        guard withProjects else {
            state.sidebarItem = .welcome
            return state
        }
        
        let now = Date()
        func stamp(_ secondsAgo: TimeInterval) -> String {
            ISO8601DateFormatter().string(from: now.addingTimeInterval(-secondsAgo))
        }

        let info = ProjectInfo(
            projectId: "a1b2c3", root: "/Users/me/Projects/context-agent", name: "context-agent",
            git: GitInfo(branch: "main", head: "862959a1", dirtyFiles: 3),
            index: IndexStats(generation: 12, files: 310, symbols: 4102, chunks: 5110, relationships: 900, embeddings: 5110, lastFullIndexAt: nil, languages: [LanguageCount(language: "rust", files: 120)]),
            indexing: false, model: "gpt-4.1", permissionProfile: "approval_required", sandbox: "none",
            validationCommands: [ValidationCommand(command: "cargo test", kind: "test", cwd: ".", source: "Cargo.toml")],
            languages: [LanguageCount(language: "rust", files: 120)], buildFiles: ["Cargo.toml"], fileCount: 310, runningTask: nil
        )
        let project = ProjectState(info: info, models: ModelCatalog(preferences: state.preferences, baseURL: config.model.baseUrl))
        project.sessions = [
            SessionSummary(id: "s1", model: "gpt-4.1", mode: "desktop", title: "Fix spacing on cards", createdAt: stamp(7200), updatedAt: stamp(120), activeTaskId: nil, taskCount: 2, lastStatus: .completed),
            SessionSummary(id: "s2", model: "gpt-4.1", mode: "desktop", title: "Need better empty state", createdAt: stamp(9000), updatedAt: stamp(3600), activeTaskId: nil, taskCount: 1, lastStatus: .failed),
            SessionSummary(id: "s3", model: "gpt-4.1", mode: "desktop", title: "Update sidebar structure so the recents list stays readable", createdAt: stamp(86_400), updatedAt: stamp(30), activeTaskId: "t3", taskCount: 1, lastStatus: nil),
            SessionSummary(id: "s4", model: "gpt-4.1", mode: "desktop", title: "Mobile nav feels cramped", createdAt: stamp(200_000), updatedAt: stamp(172_900), activeTaskId: nil, taskCount: 1, lastStatus: .blocked),
        ]
        project.tasks = [
            TaskSummary(id: "t1", sessionId: "s1", objective: "Fix spacing on the dashboard cards", category: "localized_edit", state: .completed, status: .completed, summary: "Adjusted the card padding and gap in CardGrid.swift", createdAt: stamp(7200), updatedAt: stamp(7000), completedAt: stamp(7000), modifiedFiles: ["src/CardGrid.swift"], unresolved: []),
            TaskSummary(id: "t2", sessionId: "s2", objective: "Design a better empty state for the inbox", category: "feature", state: .failed, status: .failed, summary: nil, createdAt: stamp(9000), updatedAt: stamp(8900), completedAt: nil, modifiedFiles: [], unresolved: ["Needs product copy"]),
        ]
        project.runningSessionIds = ["s3"]
        let running = project.chat(for: "s3")
        running.status = .running(RunProgress(state: .implementing, step: 3, startedAt: now.addingTimeInterval(-95)))
        running.contextStats = ContextStats(estimatedPromptTokens: 12_400, stablePrefixTokens: 3_100, repositorySnippets: 14, repositoryTokens: 6_200, memoryRecords: 3, memoryTokens: 420, conversationMessages: 9, conversationTokens: 2_600, retrievalBudgetTokens: 24_000, contextWindow: 128_000, compacted: false)
        running.usage = TokenUsage(promptTokens: 38_000, completionTokens: 4_100, cachedPromptTokens: 9_000, reasoningTokens: nil, source: .reported)
        running.costUsd = 0.12
        running.addLog("task started: Update sidebar structure", date: now.addingTimeInterval(-95))
        running.addLog("state: investigating", date: now.addingTimeInterval(-94))
        running.addLog("context ~12400 tok · 14 snippets · 3 memories", date: now.addingTimeInterval(-93))
        running.addLog("→ read_file {\"path\":\"src/sidebar.rs\"}", date: now.addingTimeInterval(-90))
        running.addLog("  ✓ 212 lines (18 ms)", date: now.addingTimeInterval(-89))
        running.addLog("→ execute_command {\"command\":\"cargo test\"}", date: now.addingTimeInterval(-40))
        running.addLog("  ✗ exit 101 (1200 ms)", date: now.addingTimeInterval(-38), level: "warn")
        let failed = project.chat(for: "s2")
        failed.unseen = .error
        let blocked = project.chat(for: "s4")
        blocked.unseen = .finished(.blocked)

        let secondInfo = ProjectInfo(
            projectId: "d4e5f6", root: "/Users/me/Projects/growth-campaign", name: "growth-campaign",
            git: GitInfo(branch: "feature/landing", head: "0badf00d", dirtyFiles: 0),
            index: IndexStats(generation: 3, files: 88, symbols: 900, chunks: 1200, relationships: 120, embeddings: 1200, lastFullIndexAt: nil, languages: []),
            indexing: true, model: "gpt-4.1-mini", permissionProfile: "workspace_autonomous", sandbox: "none",
            validationCommands: [], languages: [], buildFiles: ["package.json"], fileCount: 88, runningTask: nil
        )
        let second = ProjectState(info: secondInfo, models: ModelCatalog(preferences: state.preferences, baseURL: config.model.baseUrl))
        second.sessions = [
            SessionSummary(id: "g1", model: "gpt-4.1-mini", mode: "desktop", title: "Generate onboarding copy", createdAt: stamp(5000), updatedAt: stamp(4000), activeTaskId: nil, taskCount: 1, lastStatus: .completed),
        ]
        second.indexProgress = IndexProgress(projectId: "d4e5f6", done: 40, total: 88)
        let waiting = second.chat(for: "g1")
        waiting.pendingApproval = ApprovalRequest(id: "approval_call_2", tool: "execute_command", risk: .execute, summary: "run: npm test", details: "npm test", reason: "Commands need approval in Ask mode.")
        waiting.unseen = .approval

        state.projects = [project, second]
        state.selectedProjectId = project.id
        state.sidebarItem = .welcome
        state.backendLog.append(LogLine(id: 1, date: now.addingTimeInterval(-200), text: "capability profile loaded from cache (probed 2d ago)", level: "info"))
        state.backendLog.append(LogLine(id: 2, date: now.addingTimeInterval(-199), text: "reasoning controls unsupported by this endpoint; ignoring reasoning_effort", level: "warn"))
        state.toasts.show("Index rebuilt in 1.2 s", kind: .success)
        return state
    }
}
