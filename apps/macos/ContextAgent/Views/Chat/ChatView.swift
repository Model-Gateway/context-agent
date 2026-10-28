import SwiftUI
import AppKit

// The Chat tab: the committed transcript (VStack + day separators), the
// live turn, auto-follow with a "Jump to latest" pill, the ⌘F find bar, the
// model-mismatch notice, the Retry / Continue action row and the composer
// dock (queue row, status bar, composer) at the bottom.

/// Picks the Chat tab's content: the selected chat, the empty state for the
/// selected project, or a placeholder when no project is open.
struct ChatTabView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let chat = appState.selectedChat {
            ChatView(chat: chat)
                .id(chat.sessionId)
        } else if let project = appState.selectedProject {
            EmptyChatView(project: project)
                .id(project.id)
        } else {
            EmptyPlaceholder(systemImage: Icons.folder, title: "No project open", subtitle: "Open a repository folder to start a chat.", actionTitle: "Open a project…") {
                Task { await appState.chooseProject() }
            }
            .background(Theme.canvas)
        }
    }
}

/// One chat session: transcript above, composer dock below.
struct ChatView: View {
    var chat: ChatState

    /// Posting this notification (AppCommands ⌘F) opens the find bar of the visible chat.
    static let findNotification = Notification.Name("ContextAgent.findInChat")

    @Environment(AppState.self) private var appState
    @State private var findVisible = false
    @State private var findQuery = ""
    @State private var findCursor = 0

    init(chat: ChatState) {
        self.chat = chat
    }

    var body: some View {
        @Bindable var appState = appState
        let matches = currentMatches
        VStack(spacing: 0) {
            if findVisible {
                FindBar(
                    query: $findQuery,
                    matchCount: matches.count,
                    cursor: clampedCursor(matches.count),
                    onPrevious: { step(-1, count: matches.count) },
                    onNext: { step(1, count: matches.count) },
                    onClose: closeFind
                )
            }
            TranscriptPane(
                chat: chat,
                findQuery: findVisible ? trimmedQuery : "",
                currentMatch: matches.isEmpty ? nil : matches[clampedCursor(matches.count)]
            )
            .id(chat.sessionId)
            ComposerDock(chat: chat)
        }
        .environment(\.fileReferences, fileReferences)
        .background(Theme.canvas)
        .background(hiddenShortcuts)
        .sheet(item: $appState.rewindRequest) { request in
            RewindSheet(request: request)
                .environment(appState)
        }
        .onReceive(NotificationCenter.default.publisher(for: Self.findNotification)) { _ in showFind() }
        .onChange(of: findQuery) { _, _ in findCursor = 0 }
        .onChange(of: chat.sessionId) { _, _ in closeFind() }
    }

    /// File names in the transcript resolve against the chat's project
    /// (and its changed files) and open through `AppState.reveal`.
    private var fileReferences: FileReferenceContext? {
        guard let project = appState.project(for: chat.projectId) else { return nil }
        return FileReferenceContext(
            root: project.root,
            changedPaths: { project.gitStatus?.files.map(\.path) ?? [] },
            perform: { [appState] path, action in appState.reveal(path: path, in: project, action: action) }
        )
    }

    // MARK: Find

    private var trimmedQuery: String {
        findQuery.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var currentMatches: [FindMatch] {
        guard findVisible else { return [] }
        return FindMatch.all(in: chat.transcript.items, query: trimmedQuery)
    }

    private func clampedCursor(_ count: Int) -> Int {
        guard count > 0 else { return 0 }
        return ((findCursor % count) + count) % count
    }

    private func step(_ delta: Int, count: Int) {
        guard count > 0 else { return }
        findCursor = ((clampedCursor(count) + delta) % count + count) % count
    }

    private func showFind() {
        findVisible = true
    }

    private func closeFind() {
        findVisible = false
        findCursor = 0
    }

    /// ⌘G / ⌘⇧G cycle the matches while the find bar is open. ⌘F itself is
    /// AppCommands' (it posts `findNotification`).
    private var hiddenShortcuts: some View {
        Group {
            if findVisible {
                Button("Next match") { step(1, count: currentMatches.count) }
                    .keyboardShortcut("g", modifiers: .command)
                Button("Previous match") { step(-1, count: currentMatches.count) }
                    .keyboardShortcut("g", modifiers: [.command, .shift])
            }
        }
        .frame(width: 0, height: 0)
        .opacity(0)
        .accessibilityHidden(true)
    }
}

// MARK: - Transcript rows

/// Dispatches one committed `TranscriptItem` to its view.
struct TranscriptRow: View {
    var item: TranscriptItem
    var chat: ChatState
    /// Find-in-chat query (empty when the find bar is closed).
    var findQuery: String
    /// True for the item the find cursor points at.
    var isCurrentMatch: Bool

    init(item: TranscriptItem, chat: ChatState, findQuery: String = "", isCurrentMatch: Bool = false) {
        self.item = item
        self.chat = chat
        self.findQuery = findQuery
        self.isCurrentMatch = isCurrentMatch
    }

    var body: some View {
        content
            .overlay {
                if isCurrentMatch {
                    RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
                        .strokeBorder(Theme.accent, lineWidth: 1.5)
                        .padding(-8)
                        .allowsHitTesting(false)
                }
            }
    }

    @ViewBuilder private var content: some View {
        switch item {
        case .user(let turn):
            UserMessageView(turn: turn, highlight: highlight, chat: chat)
        case .assistant(let turn):
            AssistantMessageView(turn: turn, projectId: chat.projectId)
        case .tools(let group):
            ToolTimelineView(group: group)
        case .agents(let group):
            AgentTimelineView(group: group)
        case .approval(let approval):
            ApprovalCardView(item: approval, chat: chat)
        case .outcome(let outcome):
            OutcomeCardView(item: outcome, chat: chat)
        case .notice(let notice):
            NoticeView(item: notice, highlight: highlight)
        }
    }

    private var highlight: String? {
        findQuery.isEmpty ? nil : findQuery
    }
}

/// The streaming part of the current run: the open tool group, then the
/// streaming assistant turn (or a compact "Investigating…" row while the
/// model is being called). `onGrow` fires whenever the live content grows so
/// the pane can keep following.
struct LiveTurnView: View {
    var chat: ChatState
    var onGrow: (() -> Void)?

    init(chat: ChatState, onGrow: (() -> Void)? = nil) {
        self.chat = chat
        self.onGrow = onGrow
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            if let tools = chat.liveTools, !tools.calls.isEmpty {
                ToolTimelineView(group: tools)
            }
            if let agents = chat.liveAgents, !agents.agents.isEmpty {
                AgentTimelineView(group: agents)
            }
            if let live = chat.live, !live.isEmpty {
                AssistantMessageView(turn: live)
            } else if let label = workingLabel {
                HStack(spacing: 8) {
                    BrailleSpinner(size: 13)
                    Text("\(label)…")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                }
                .padding(.vertical, 2)
                .accessibilityElement(children: .combine)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onChange(of: growth) { _, _ in onGrow?() }
    }

    private struct Growth: Equatable {
        var text: Int
        var reasoning: Int
        var calls: Int
        var running: Int
        var thinking: Bool
        var agentCalls: Int
    }

    private var growth: Growth {
        Growth(
            text: chat.live?.text.utf8.count ?? 0,
            reasoning: chat.live?.reasoning.utf8.count ?? 0,
            calls: chat.liveTools?.calls.count ?? 0,
            running: chat.liveTools?.runningCount ?? 0,
            thinking: chat.live?.isThinking ?? false,
            agentCalls: chat.liveAgents?.agents.reduce(0) { $0 + $1.calls.count } ?? 0
        )
    }

    /// "Preparing" / "Investigating" while nothing streams and no tool runs.
    private var workingLabel: String? {
        guard let progress = chat.status.progress else { return nil }
        if chat.pendingApproval != nil || chat.pendingQuestion != nil { return nil }
        if let tools = chat.liveTools, tools.runningCount > 0 { return nil }
        if let agents = chat.liveAgents, agents.runningCount > 0 { return nil }
        return progress.state.label
    }
}

// MARK: - Transcript pane

private struct TranscriptPane: View {
    var chat: ChatState
    var findQuery: String
    var currentMatch: FindMatch?

    @Environment(AppState.self) private var appState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var follow = FollowState()
    @State private var mismatchDismissed = false
    /// The committed items loaded into views: the newest 20 when the chat
    /// opens, 20 more each time the user scrolls up to them, at most 60
    /// (`TailWindow`).
    @State private var window: TailWindow
    /// The loaded rows' height plus one row spacing per row.
    @State private var rows = MeasuredRows()

    init(chat: ChatState, findQuery: String, currentMatch: FindMatch?) {
        self.chat = chat
        self.findQuery = findQuery
        self.currentMatch = currentMatch
        _window = State(initialValue: TailWindow(count: chat.transcript.count, page: 20, limit: 60))
    }

    private static let bottomId = "transcript-bottom"

    /// Unloaded messages, while the user scrolls to them: exchanges of
    /// varied shapes, 132 pt plus the transcript's 20 pt and 8 pt between.
    private static let skeletonExchangeHeight: CGFloat = 160
    private static let skeletonShapes: [(question: CGFloat, reply: [CGFloat])] = [
        (0.42, [1, 0.94, 0.62]), (0.3, [0.97, 1, 0.88, 0.4]), (0.55, [0.9, 0.7]), (0.36, [1, 0.96, 0.8, 0.92, 0.5]),
    ]

    private static func skeletonExchange(_ index: Int) -> some View {
        let shape = skeletonShapes[index % skeletonShapes.count]
        return TranscriptSkeleton.Exchange(question: shape.question, reply: shape.reply)
            .padding(.vertical, 14)
    }
    private static let scrollSpace = "transcript-scroll"

    var body: some View {
        GeometryReader { outer in
            ScrollViewReader { proxy in
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: 20) {
                        // Committed rows are windowed: only the newest items
                        // are loaded (more as the user scrolls up, the
                        // oldest or newest unloaded past 60, `TailWindow`)
                        // and the others stand as empty space at the average
                        // row height, so a long chat costs what a short one
                        // does on every streamed update and the scroller
                        // stays the size of the whole chat. Not lazy: the
                        // window keeps the row count small, and rows measured
                        // exactly let the scroll view keep the visible rows
                        // in place as items load and unload above or below
                        // them (a lazy stack anchors on estimated heights and
                        // jumps).
                        let range = window.range(count: chat.transcript.count)
                        header
                        TailWindowSpacer(height: window.spacerAbove, rowsEdge: .bottom, placeholderRowHeight: Self.skeletonExchangeHeight, placeholder: Self.skeletonExchange)
                        VStack(alignment: .leading, spacing: 20) {
                            TranscriptRows(
                                items: chat.transcript.items,
                                range: range,
                                chat: chat,
                                findQuery: findQuery,
                                currentMatchId: currentMatch?.itemId
                            )
                        }
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { height in
                            rows = MeasuredRows(height: height + 20, count: range.count)
                        }
                        TailWindowSpacer(height: window.spacerBelow(count: chat.transcript.count), rowsEdge: .top, placeholderRowHeight: Self.skeletonExchangeHeight, placeholder: Self.skeletonExchange)
                        LiveTurnView(chat: chat) { followIfNeeded(proxy) }
                        if showActionRow {
                            ActionRow(chat: chat)
                        }
                        Color.clear
                            .frame(height: 1)
                            .id(Self.bottomId)
                    }
                    .padding(.horizontal, 24)
                    .padding(.top, 20)
                    .padding(.bottom, 12)
                    .frame(maxWidth: Theme.Metrics.contentMaxWidth + 48)
                    .frame(maxWidth: .infinity)
                    // Content growth (a card expanding in place, streamed
                    // text) and scrolling both move the content's bottom
                    // edge in the scroll space; that edge decides whether
                    // the "Jump to latest" pill shows.
                    .onGeometryChange(for: CGFloat.self) { proxy in
                        proxy.frame(in: .named(Self.scrollSpace)).maxY
                    } action: { maxY in
                        follow.update(contentMaxY: maxY, viewport: outer.size.height)
                    }
                    // Loaded history (and rows that size in after the first
                    // pass) grows the content after the count-change scroll
                    // already ran against the old height: keep the bottom in
                    // view while following. Scrolling away (wheel, scroller,
                    // trackpad) or jumping to a find match stops following
                    // first, so lazy rows measuring in leave the view alone.
                    .onGeometryChange(for: CGFloat.self) { geometry in
                        geometry.size.height
                    } action: { _ in
                        if follow.followLatest { proxy.scrollTo(Self.bottomId, anchor: .bottom) }
                    }
                }
                .defaultScrollAnchor(.bottom, for: .initialOffset)
                .tailWindow($window, count: chat.transcript.count, rows: rows, followsBottom: follow.followLatest)
                .onChange(of: window.followsTail, initial: true) { _, atTail in follow.windowAtTail = atTail }
                .coordinateSpace(name: Self.scrollSpace)
                .onChange(of: outer.size.height) { _, height in
                    follow.update(viewport: height)
                }
                .onChange(of: chat.transcript.count) { _, _ in
                    if follow.followLatest { scrollToBottom(proxy, animated: false, settle: true) }
                }
                .onChange(of: chat.status.isRunning) { _, running in
                    if running { scrollToBottom(proxy, animated: true, settle: true) }
                }
                .onChange(of: currentMatch) { _, match in
                    guard let match else { return }
                    follow.stop()
                    // A match outside the loaded items loads a page around it first.
                    if let index = chat.transcript.items.firstIndex(where: { $0.id == match.itemId }), !window.range(count: chat.transcript.count).contains(index) {
                        window.reveal(index, count: chat.transcript.count)
                        Task { @MainActor in
                            try? await Task.sleep(for: .milliseconds(34))
                            proxy.scrollTo(match.itemId, anchor: .center)
                        }
                    } else {
                        withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(match.itemId, anchor: .center) }
                    }
                }
                .onChange(of: outer.frame(in: .global)) { _, frame in
                    follow.paneFrame = frame
                }
                .onAppear {
                    follow.paneFrame = outer.frame(in: .global)
                    follow.installMonitor()
                    scrollToBottom(proxy, animated: false, settle: true)
                }
                .onDisappear { follow.removeMonitor() }
                .overlay(alignment: .bottom) {
                    if !follow.atBottom {
                        JumpToLatestPill { scrollToBottom(proxy, animated: true, settle: true) }
                            .padding(.bottom, 12)
                            .transition(reduceMotion ? .opacity : .opacity.combined(with: .move(edge: .bottom)))
                    }
                }
                .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: follow.atBottom)
            }
        }
    }

    // MARK: Pieces

    @ViewBuilder private var header: some View {
        if chat.status == .loadingHistory {
            TranscriptSkeleton()
        }
        if let error = chat.historyError {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: Icons.warning)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.danger)
                    .accessibilityHidden(true)
                Text("Could not load history: \(error)")
                    .font(.system(size: 12.5))
                    .foregroundStyle(Theme.danger)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
                Spacer(minLength: 8)
                Button("Retry") {
                    let id = chat.sessionId
                    Task { await appState.select(session: id) }
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        if let mismatch = modelMismatch, !mismatchDismissed {
            ModelMismatchNotice(sessionModel: appState.modelDisplayName(mismatch.session), projectModel: appState.modelDisplayName(mismatch.project)) {
                withAnimation(.easeOut(duration: 0.15)) { mismatchDismissed = true }
            }
        }
    }

    /// "This chat was started with X; new messages use Y" when they differ.
    private var modelMismatch: (session: String, project: String)? {
        let sessionModel = chat.summary.model.trimmingCharacters(in: .whitespaces)
        guard !sessionModel.isEmpty,
              let projectModel = appState.project(for: chat.projectId)?.info.model,
              !projectModel.isEmpty,
              projectModel != sessionModel else { return nil }
        return (sessionModel, projectModel)
    }

    /// Retry / Continue / Open Settings after a failed, cancelled or blocked run.
    private var showActionRow: Bool {
        if chat.status.isRunning || chat.status == .loadingHistory { return false }
        if chat.lastError != nil { return true }
        if case .finished(let status) = chat.status, status != .completed { return true }
        return false
    }

    // MARK: Scrolling

    private func followIfNeeded(_ proxy: ScrollViewProxy) {
        guard follow.followLatest else { return }
        proxy.scrollTo(Self.bottomId, anchor: .bottom)
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy, animated: Bool, settle: Bool) {
        follow.resume()
        // Scrolled up past the newest items: load the newest page again.
        if !window.followsTail { window.reset(count: chat.transcript.count) }
        if animated {
            withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo(Self.bottomId, anchor: .bottom) }
        } else {
            proxy.scrollTo(Self.bottomId, anchor: .bottom)
        }
        guard settle else { return }
        // Rows can size in after the first pass: settle once layout has caught up.
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(80))
            guard follow.followLatest else { return }
            proxy.scrollTo(Self.bottomId, anchor: .bottom)
        }
    }
}

/// The committed items, each under its day separator when the calendar day
/// changes. A view of its own so that what only the pane reads (follow state,
/// run status, the header's app state) does not rebuild every row.
private struct TranscriptRows: View {
    var items: [TranscriptItem]
    /// The loaded slice of `items`.
    var range: Range<Int>
    var chat: ChatState
    var findQuery: String
    var currentMatchId: String?

    var body: some View {
        ForEach(Self.entries(items, range)) { entry in
            VStack(alignment: .leading, spacing: 20) {
                if let label = entry.separator {
                    DaySeparator(label: label)
                }
                TranscriptRow(
                    item: entry.item,
                    chat: chat,
                    findQuery: findQuery,
                    isCurrentMatch: currentMatchId == entry.id
                )
            }
            .id(entry.id)
        }
    }

    private struct Entry: Identifiable {
        var item: TranscriptItem
        var separator: String?
        var id: String { item.id }
    }

    /// A centred day label before the first item (unless today) and whenever the calendar day changes.
    private static func entries(_ items: [TranscriptItem], _ range: Range<Int>) -> [Entry] {
        let calendar = Calendar.current
        var previous: Date? = range.lowerBound > 0 ? items[range.lowerBound - 1].date : nil
        return items[range].map { item in
            let date = item.date
            let separator: String?
            if let previous {
                separator = calendar.isDate(previous, inSameDayAs: date) ? nil : TranscriptTime.dayLabel(date)
            } else {
                separator = calendar.isDateInToday(date) ? nil : TranscriptTime.dayLabel(date)
            }
            previous = date
            return Entry(item: item, separator: separator)
        }
    }
}

/// Auto-follow state. The user's intent to leave the bottom is read from
/// scroll-wheel events over the pane and from live scrolls of its scroll
/// view (dragging the scroller, a trackpad gesture); content growth alone
/// never disables following, and reaching the bottom again re-enables it.
@MainActor
@Observable
private final class FollowState {
    var followLatest = true
    var atBottom = true
    /// Whether the loaded items reach the newest one (`TailWindow`): the
    /// bottom of a window that stops short of it is not the latest.
    @ObservationIgnored var windowAtTail = true {
        didSet { if windowAtTail != oldValue { evaluate() } }
    }
    @ObservationIgnored var paneFrame: CGRect = .zero
    @ObservationIgnored private var monitor: Any?
    @ObservationIgnored private var liveScrollObservers: [NSObjectProtocol] = []
    @ObservationIgnored private var contentMaxY: CGFloat = 0
    @ObservationIgnored private var viewport: CGFloat = 0
    /// The user's last scroll went towards earlier content: being near the
    /// bottom then (on the way up, or as content shrinks) does not mean
    /// they want to follow again. A scroll down, or a gesture ending at the
    /// bottom, clears it.
    @ObservationIgnored private var leftBottom = false
    /// The transcript's scroll view is in a live scroll (a scroller drag, a
    /// trackpad gesture with its momentum).
    @ObservationIgnored private weak var liveScrollView: NSScrollView?

    /// The content's bottom edge in the scroll space and the viewport height.
    func update(contentMaxY: CGFloat, viewport: CGFloat) {
        self.contentMaxY = contentMaxY
        self.viewport = viewport
        evaluate()
    }

    func update(viewport: CGFloat) {
        self.viewport = viewport
        evaluate()
    }

    /// A find match away from the bottom.
    func stop() {
        leftBottom = true
        followLatest = false
    }

    /// Following again (Jump to latest, a new run): the user asked for the bottom.
    func resume() {
        leftBottom = false
        followLatest = true
    }

    /// Reaching the bottom re-enables following, unless the user is
    /// scrolling or last scrolled up: a gesture that starts at the bottom
    /// spends its first frames within reach of it, and re-enabling there
    /// would pull the view back down on the next streamed line.
    private func evaluate() {
        let near = windowAtTail && contentMaxY - viewport < 48
        if near != atBottom { atBottom = near }
        if near, !followLatest, !leftBottom, liveScrollView == nil { followLatest = true }
    }

    func installMonitor() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
            self?.userScrolled(event)
            return event
        }
        liveScrollObservers = [
            NotificationCenter.default.addObserver(forName: NSScrollView.willStartLiveScrollNotification, object: nil, queue: .main) { [weak self] note in
                guard let scrollView = note.object as? NSScrollView else { return }
                MainActor.assumeIsolated { self?.liveScrollStarted(scrollView) }
            },
            NotificationCenter.default.addObserver(forName: NSScrollView.didEndLiveScrollNotification, object: nil, queue: .main) { [weak self] note in
                guard let scrollView = note.object as? NSScrollView else { return }
                MainActor.assumeIsolated { self?.liveScrollEnded(scrollView) }
            },
        ]
    }

    func removeMonitor() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        liveScrollObservers.forEach(NotificationCenter.default.removeObserver)
        monitor = nil
        liveScrollObservers = []
    }

    /// A live scroll of the transcript's own scroll view (the one whose
    /// frame holds the pane's centre): the user is moving the view.
    private func liveScrollStarted(_ scrollView: NSScrollView) {
        guard !paneFrame.isEmpty, let content = scrollView.window?.contentView else { return }
        let frame = scrollView.convert(scrollView.bounds, to: content)
        let centre = CGPoint(x: paneFrame.midX, y: content.isFlipped ? paneFrame.midY : content.bounds.height - paneFrame.midY)
        guard frame.contains(centre) else { return }
        liveScrollView = scrollView
        leftBottom = true
        followLatest = false
    }

    /// The gesture is over (momentum included): ending at the bottom follows again.
    private func liveScrollEnded(_ scrollView: NSScrollView) {
        guard scrollView === liveScrollView else { return }
        liveScrollView = nil
        leftBottom = !(windowAtTail && contentMaxY - viewport < 48)
        evaluate()
    }

    private func userScrolled(_ event: NSEvent) {
        // Positive deltaY reveals earlier content (already adjusted for natural scrolling).
        guard event.scrollingDeltaY != 0, contains(event) else { return }
        if event.scrollingDeltaY > 0 {
            leftBottom = true
            followLatest = false
        } else {
            leftBottom = false
        }
    }

    private func contains(_ event: NSEvent) -> Bool {
        guard !paneFrame.isEmpty, let window = event.window, let content = window.contentView else { return true }
        let local = content.convert(event.locationInWindow, from: nil)
        let point = CGPoint(x: local.x, y: content.isFlipped ? local.y : content.bounds.height - local.y)
        return paneFrame.contains(point)
    }
}

// MARK: - Find

private struct FindMatch: Equatable {
    var itemId: String
    var occurrence: Int

    static func all(in items: [TranscriptItem], query: String) -> [FindMatch] {
        guard !query.isEmpty else { return [] }
        var out: [FindMatch] = []
        for item in items {
            let count = TranscriptHighlight.occurrences(of: query, in: searchableText(item))
            for occurrence in 0..<count {
                out.append(FindMatch(itemId: item.id, occurrence: occurrence))
            }
        }
        return out
    }

    static func searchableText(_ item: TranscriptItem) -> String {
        switch item {
        case .user(let turn):
            return turn.text
        case .assistant(let turn):
            return turn.text
        case .tools(let group):
            return group.calls.map { ToolMetadata.rowLabel(for: $0) + " " + $0.summary }.joined(separator: "\n")
        case .agents(let group):
            return group.agents.map { agent in
                ([agent.name, agent.objective, agent.report?.summary ?? ""] + agent.calls.map { ToolMetadata.rowLabel(for: $0) + " " + $0.summary }).joined(separator: "\n")
            }.joined(separator: "\n")
        case .approval(let approval):
            return approval.request.summary + "\n" + approval.request.details
        case .outcome(let outcome):
            return outcome.outcome.summary + "\n" + outcome.outcome.modifiedFiles.joined(separator: "\n")
        case .notice(let notice):
            return (notice.lines + notice.detailLines).joined(separator: "\n")
        }
    }
}

private struct FindBar: View {
    @Binding var query: String
    var matchCount: Int
    var cursor: Int
    var onPrevious: () -> Void
    var onNext: () -> Void
    var onClose: () -> Void

    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: Icons.search)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .accessibilityHidden(true)
                TextField("Find in chat", text: $query)
                    .textFieldStyle(.plain)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                    .focused($focused)
                    .onSubmit(onNext)
                    .onExitCommand(perform: onClose)
                    .accessibilityLabel("Find in chat")
                if !query.isEmpty {
                    Button {
                        query = ""
                    } label: {
                        Image(systemName: Icons.clear)
                            .font(.system(size: 12))
                            .foregroundStyle(Theme.textTertiary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear search")
                }
            }
            .padding(.horizontal, 10)
            .frame(height: 28)
            .frame(maxWidth: 320)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                    .strokeBorder(focused ? Theme.borderStrong : Theme.border, lineWidth: 1)
            )

            Text(countLabel)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .monospacedDigit()
                .frame(minWidth: 64, alignment: .leading)
                .accessibilityLabel(countLabel.isEmpty ? "No search" : countLabel)

            IconButton(Icons.chevronUp, label: "Previous match", size: 12, action: onPrevious)
                .disabled(matchCount == 0)
            IconButton(Icons.chevronDown, label: "Next match", size: 12, action: onNext)
                .disabled(matchCount == 0)

            Spacer(minLength: 8)

            Button("Done", action: onClose)
                .buttonStyle(.bordered)
                .controlSize(.small)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Theme.sidebar)
        .hairline(.bottom)
        .onAppear {
            Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(40))
                focused = true
            }
        }
    }

    private var countLabel: String {
        if query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "" }
        if matchCount == 0 { return "No matches" }
        return "\(cursor + 1) of \(matchCount)"
    }
}

// MARK: - Small pieces

private struct DaySeparator: View {
    var label: String

    var body: some View {
        HStack(spacing: 12) {
            line
            Text(label)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize()
            line
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label)
    }

    private var line: some View {
        Rectangle()
            .fill(Theme.border)
            .frame(height: 1)
            .frame(maxWidth: .infinity)
    }
}

private struct ModelMismatchNotice: View {
    var sessionModel: String
    var projectModel: String
    var onDismiss: () -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: Icons.info)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            Text("This chat was started with \(sessionModel); new messages use \(projectModel)")
                .font(.system(size: 12.5))
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            IconButton(Icons.close, label: "Dismiss", size: 11, action: onDismiss)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .accessibilityElement(children: .contain)
    }
}

/// Retry · Continue (blocked) · Open Settings → Model (provider_auth).
private struct ActionRow: View {
    var chat: ChatState

    @Environment(AppState.self) private var appState

    var body: some View {
        HStack(spacing: 8) {
            Button {
                Task { await appState.retry(chat: chat) }
            } label: {
                Label("Retry", systemImage: Icons.refresh)
            }
            .disabled(chat.transcript.lastUserText == nil)
            .help("Send the last message again")

            if chat.status == .finished(.blocked) {
                Button("Continue") {
                    Task { await appState.send("Continue with the unresolved items") }
                }
                .help("Ask the agent to continue with the unresolved items")
            }

            if showsModelSettings {
                Button("Open Settings → Model") {
                    appState.openSettings(.model)
                }
            }
            Spacer(minLength: 0)
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .padding(.top, 2)
    }

    /// The endpoint is the likely cause: no key at all, an auth failure, or
    /// a probe that could not reach or use the endpoint.
    private var showsModelSettings: Bool {
        if appState.needsAPIKey { return true }
        switch chat.lastErrorKind {
        case "provider_auth"?, "provider_unsupported"?, "provider_connection"?: return true
        default: return false
        }
    }
}

private struct JumpToLatestPill: View {
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: Icons.arrowDown)
                    .font(.system(size: 11, weight: .semibold))
                Text("Jump to latest")
                    .font(.system(size: 12, weight: .medium))
            }
            .foregroundStyle(Theme.textPrimary)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(Theme.surface, in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.border, lineWidth: 1))
            .floatingShadow()
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Jump to latest")
    }
}

// MARK: - Composer dock

/// The composer, centred under the transcript. The
/// "Queued · N" row is part of `ComposerView(chat:)` (V4), which renders it
/// whenever `chat.queued` is non-empty, so it is not repeated here.
private struct ComposerDock: View {
    var chat: ChatState

    var body: some View {
        VStack(spacing: 10) {
            if let question = chat.pendingQuestion {
                QuestionCardView(request: question, chat: chat)
                    .id(question.requestId)
                    .transition(.opacity.combined(with: .move(edge: .bottom)))
            }
            ComposerView(chat: chat)
        }
        .animation(.easeOut(duration: 0.18), value: chat.pendingQuestion?.requestId)
        .frame(maxWidth: Theme.Metrics.contentMaxWidth)
        .padding(.horizontal, 24)
        .padding(.top, 8)
        .padding(.bottom, 16)
        .frame(maxWidth: .infinity)
    }
}

// MARK: - Previews

@MainActor
private enum ChatPreviewData {
    static func make(streaming: Bool) -> (AppState, ChatState) {
        let appState = AppState()
        let info = ProjectInfo(
            projectId: "0123456789abcdef01234567",
            root: "/Users/me/Projects/context-agent",
            name: "context-agent",
            git: GitInfo(branch: "main", head: "862959a1", dirtyFiles: 3),
            index: IndexStats(generation: 12, files: 310, symbols: 4102, chunks: 5110, relationships: 900, embeddings: 5110, lastFullIndexAt: nil, languages: []),
            indexing: false,
            model: "gpt-4.1",
            permissionProfile: "approval_required",
            sandbox: "none",
            validationCommands: [ValidationCommand(command: "cargo test", kind: "test", cwd: ".", source: "Cargo.toml")],
            languages: [],
            buildFiles: ["Cargo.toml"],
            fileCount: 310,
            runningTask: nil
        )
        let project = ProjectState(info: info, models: ModelCatalog(preferences: appState.preferences, baseURL: ""))
        appState.projects.append(project)
        appState.selectedProjectId = project.id
        let summary = SessionSummary(id: "s1", model: "gpt-4.1-mini", mode: "desktop", title: "Fix spacing on cards", createdAt: "2026-10-20T09:00:00Z", updatedAt: "2026-10-21T10:00:00Z", activeTaskId: nil, taskCount: 1, lastStatus: .completed)
        project.upsert(session: summary)
        let chat = project.chat(for: summary.id)
        appState.selectedSessionId = summary.id

        let yesterday = Date().addingTimeInterval(-86_400)
        chat.transcript.append(.user(UserTurn(text: "Fix the failing test in cards.rs and run cargo test to verify", date: yesterday)))
        var read = ToolCallItem(callId: "call_1", name: "read_file", arguments: .object(["path": .string("src/cards.rs")]), status: .succeeded, summary: "read 120 lines", durationMs: 12)
        read.startedAt = yesterday
        var run = ToolCallItem(callId: "call_2", name: "execute_command", arguments: .object(["command": .string("cargo test")]), status: .succeeded, summary: "exit 0", durationMs: 4200)
        run.startedAt = yesterday
        run.output = ToolOutput(content: "running 14 tests\ntest result: ok. 14 passed", fullLength: 42, isError: false, tool: "execute_command")
        chat.transcript.append(.tools(ToolGroup(calls: [read, run], filesChanged: ["src/cards.rs"], commandsRun: 1, startedAt: yesterday, endedAt: yesterday.addingTimeInterval(6), terminal: .done)))
        chat.transcript.append(.assistant(AssistantTurn(
            text: "The comparator sorted by `created_at` while the fixture only sets `updated_at`.\n\n- Edited `src/cards.rs`\n- Ran `cargo test` (14 passed)",
            isStreaming: false, step: 2, date: yesterday.addingTimeInterval(8)
        )))
        let record = ValidationRecord(command: "cargo test", passed: true, exitCode: 0, durationMs: 4200, summary: "14 passed")
        let outcome = TaskOutcome(
            taskId: "t1", status: .completed, summary: "Fixed the sort order in `src/cards.rs`.", modifiedFiles: ["src/cards.rs"],
            validation: [record], unresolved: [], steps: 4, modelCalls: 3, toolCalls: 2,
            usage: TokenUsage(promptTokens: 38_000, completionTokens: 4_100, cachedPromptTokens: nil, reasoningTokens: nil, source: .reported),
            costUsd: 0.12, durationMs: 372_000
        )
        chat.transcript.append(.outcome(OutcomeItem(outcome: outcome, date: yesterday.addingTimeInterval(10))))
        chat.transcript.append(.notice(NoticeItem(lines: ["Approval mode set to Auto"], detailLines: ["checkpoint: before apply_patch src/cards.rs"])))

        if streaming {
            chat.transcript.append(.user(UserTurn(text: "Now explain how the index generation counter works")))
            chat.status = .running(RunProgress(state: .investigating, step: 1, startedAt: Date().addingTimeInterval(-12)))
            var listing = ToolCallItem(callId: "call_3", name: "search_code", arguments: .object(["query": .string("generation")]))
            listing.status = .running
            chat.liveTools = ToolGroup(calls: [listing], startedAt: Date().addingTimeInterval(-3))
            chat.live = AssistantTurn(text: "The counter lives in the index store and", reasoning: "Need to find where generation is bumped.", isStreaming: true, isThinking: false, step: 1, thinkingStarted: Date().addingTimeInterval(-2), thinkingDuration: 0.8)
            chat.queued = [OutgoingPrompt(text: "Also document it in README.md")]
        } else {
            chat.status = .finished(.completed)
        }
        return (appState, chat)
    }
}

#Preview("Chat · finished") {
    let (appState, chat) = ChatPreviewData.make(streaming: false)
    return ChatView(chat: chat)
        .frame(width: 960, height: 760)
        .environment(appState)
}

#Preview("Chat · streaming") {
    let (appState, chat) = ChatPreviewData.make(streaming: true)
    return ChatView(chat: chat)
        .frame(width: 960, height: 760)
        .environment(appState)
}

