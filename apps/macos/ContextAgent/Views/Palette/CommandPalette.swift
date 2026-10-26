import SwiftUI
import AppKit

/// ⌘K (ARCHITECTURE.md "Command palette"): a floating 560 pt panel over the
/// window with a search field, "Chats" (titles plus the project's task
/// objectives / summaries from `ProjectState.tasks`, matched snippet,
/// relative time) and "Actions" (New chat, Open project…, Switch model…,
/// Rebuild index, Toggle inspector, Run doctor, Settings…). ↑↓ move, Return
/// activates, Esc closes (also handled by the AppCommands key monitor).
struct CommandPalette: View {
    @Environment(AppState.self) private var appState
    @State private var query = ""
    @State private var selection = 0
    /// Bumped by ↑↓ and typing; only these scroll the selection into view
    /// (hover selects without scrolling, so the list never moves under the cursor).
    @State private var scrollRequest = 0
    /// Screen location of the last hover that changed the selection; a hover
    /// at the same spot comes from rows scrolling under a still cursor.
    @State private var lastHoverLocation: CGPoint?
    @FocusState private var searchFocused: Bool

    var body: some View {
        let results = results(for: query)
        ZStack(alignment: .top) {
            Color.black.opacity(0.18)
                .ignoresSafeArea()
                .onTapGesture { close() }
                .accessibilityHidden(true)
            panel(results)
                .padding(.top, 72)
        }
        .onChange(of: query) { _, _ in
            selection = 0
            scrollRequest += 1
        }
        .onAppear {
            Task { @MainActor in
                try? await Task.sleep(nanoseconds: 50_000_000)
                searchFocused = true
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Command palette")
    }

    // MARK: - Panel

    private func panel(_ results: [PaletteRow]) -> some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous)
        return VStack(spacing: 0) {
            searchField(results)
                .padding(12)
            Rectangle()
                .fill(Theme.border)
                .frame(height: 1)
                .accessibilityHidden(true)
            if results.isEmpty {
                Text("No chats or actions match “\(query.trimmingCharacters(in: .whitespaces))”")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textTertiary)
                    .padding(24)
            } else {
                list(results)
                    .frame(height: listHeight(results))
            }
            footer
        }
        .frame(width: 560)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .floatingShadow()
        .shadow(color: Color.black.opacity(0.12), radius: 30, x: 0, y: 16)
    }

    private func searchField(_ results: [PaletteRow]) -> some View {
        HStack(spacing: 8) {
            Image(systemName: Icons.search)
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            TextField("Search chats and actions…", text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 16))
                .foregroundStyle(Theme.textPrimary)
                .focused($searchFocused)
                .onSubmit { activate(selected(in: results)) }
                .onKeyPress(.downArrow) {
                    move(1, count: results.count)
                    return .handled
                }
                .onKeyPress(.upArrow) {
                    move(-1, count: results.count)
                    return .handled
                }
                .onKeyPress(.escape) {
                    close()
                    return .handled
                }
                .onExitCommand { close() }
                .accessibilityLabel("Search chats and actions")
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: Icons.clear)
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.textTertiary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Clear search")
            }
            Text("esc")
                .kbd()
                .accessibilityHidden(true)
        }
    }

    private func list(_ results: [PaletteRow]) -> some View {
        ScrollViewReader { proxy in
            ScrollView(.vertical) {
                // Not lazy: rows differ in height (with or without a snippet),
                // and a lazy stack re-measured them while scrolling, which made
                // the scroll stall and jump.
                VStack(alignment: .leading, spacing: 1) {
                    ForEach(Array(results.enumerated()), id: \.element.id) { index, row in
                        if index == 0 || results[index - 1].section != row.section {
                            sectionHeader(row.section)
                        }
                        PaletteRowView(row: row, isSelected: index == selection) {
                            let location = NSEvent.mouseLocation
                            guard location != lastHoverLocation else { return }
                            lastHoverLocation = location
                            selection = index
                        } activate: {
                            activate(row)
                        }
                        .id(row.id)
                    }
                }
                .padding(8)
            }
            .onChange(of: scrollRequest) { _, _ in
                guard results.indices.contains(selection) else { return }
                proxy.scrollTo(results[selection].id, anchor: nil)
            }
        }
    }

    /// Hugs the results (rows ≈ 46 pt, headers ≈ 26 pt) up to 420 pt, then scrolls.
    private func listHeight(_ results: [PaletteRow]) -> CGFloat {
        let sections = Set(results.map(\.section)).count
        let estimated = CGFloat(results.count) * 46 + CGFloat(sections) * 26 + 16
        return min(420, estimated)
    }

    private func sectionHeader(_ section: PaletteRow.Section) -> some View {
        Text(section.title)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(Theme.textTertiary)
            .textCase(.uppercase)
            .padding(.horizontal, 10)
            .padding(.top, 8)
            .padding(.bottom, 4)
            .accessibilityAddTraits(.isHeader)
    }

    private var footer: some View {
        HStack(spacing: 14) {
            hint("↑↓", "navigate")
            hint("↩", "open")
            hint("esc", "close")
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .hairline(.top)
        .accessibilityHidden(true)
    }

    private func hint(_ key: String, _ label: String) -> some View {
        HStack(spacing: 5) {
            Text(key).kbd()
            Text(label)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
        }
    }

    // MARK: - Navigation

    private func move(_ delta: Int, count: Int) {
        guard count > 0 else { return }
        selection = ((selection + delta) % count + count) % count
        scrollRequest += 1
    }

    private func selected(in results: [PaletteRow]) -> PaletteRow? {
        results.indices.contains(selection) ? results[selection] : results.first
    }

    private func close() {
        appState.paletteVisible = false
        appState.composerFocusToken += 1
    }

    private func activate(_ row: PaletteRow?) {
        guard let row else { return }
        close()
        switch row.kind {
        case .chat(let projectId, let sessionId):
            appState.select(project: projectId, restoreSession: false)
            Task { await appState.select(session: sessionId) }
        case .action(let action):
            run(action)
        }
    }

    private func run(_ action: PaletteAction) {
        switch action {
        case .newChat:
            appState.newChat()
        case .newWorktree:
            appState.newWorktreeChat()
        case .openProject:
            Task { await appState.chooseProject() }
        case .switchModel:
            if !appState.modelPickerVisible { appState.toggleModelPicker() }
        case .rebuildIndex:
            Task { await appState.rebuildIndex() }
        case .toggleInspector:
            appState.inspectorVisible.toggle()
        case .runDoctor:
            if appState.selectedProject != nil { appState.sidebarItem = .project }
            Task { await appState.runDoctor(probe: false) }
        case .settings:
            appState.openSettings()
        }
    }

    // MARK: - Results

    private func results(for query: String) -> [PaletteRow] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        var rows: [PaletteRow] = []
        rows.append(contentsOf: chatRows(query: trimmed))
        rows.append(contentsOf: actionRows(query: trimmed))
        return rows
    }

    private func chatRows(query: String) -> [PaletteRow] {
        let multipleProjects = appState.projects.count > 1
        var scored: [(row: PaletteRow, score: Int, date: Date)] = []
        for project in appState.projects {
            let tasksBySession = Dictionary(grouping: project.tasks, by: { $0.sessionId ?? "" })
            for session in project.sessions {
                let tasks = tasksBySession[session.id] ?? []
                let subtitle = multipleProjects ? project.name : nil
                if query.isEmpty {
                    guard project.id == appState.selectedProjectId else { continue }
                    let snippet = tasks.first.flatMap { $0.summary ?? $0.objective }
                    scored.append((PaletteRow(id: "chat-\(project.id)-\(session.id)", section: .chats, title: session.title, snippet: snippet.map { ChatState.clip($0, 90) }, trailing: TranscriptFormat.relative(session.updatedDate), subtitle: subtitle, systemImage: Icons.chat, kind: .chat(projectId: project.id, sessionId: session.id)), 0, session.updatedDate))
                    continue
                }
                var best = ModelCatalog.fuzzyMatch(query, in: session.title)?.score ?? -1
                var snippet: String?
                for task in tasks {
                    for text in [task.objective, task.summary ?? ""] where !text.isEmpty {
                        if let match = ModelCatalog.fuzzyMatch(query, in: text) {
                            // Task text matches rank slightly below title matches.
                            let score = match.score - 50
                            if score > best || (snippet == nil && best < 0) {
                                best = max(best, score)
                                snippet = Self.snippet(text, around: query)
                            }
                        }
                    }
                }
                guard best >= 0 else { continue }
                if snippet == nil, let first = tasks.first {
                    snippet = ChatState.clip(first.summary ?? first.objective, 90)
                }
                scored.append((PaletteRow(id: "chat-\(project.id)-\(session.id)", section: .chats, title: session.title, snippet: snippet, trailing: TranscriptFormat.relative(session.updatedDate), subtitle: subtitle, systemImage: Icons.chat, kind: .chat(projectId: project.id, sessionId: session.id)), best, session.updatedDate))
            }
        }
        if query.isEmpty {
            return scored.sorted { $0.date > $1.date }.prefix(8).map(\.row)
        }
        return scored.sorted { a, b in
            if a.score != b.score { return a.score > b.score }
            return a.date > b.date
        }.prefix(12).map(\.row)
    }

    private func actionRows(query: String) -> [PaletteRow] {
        let hasProject = appState.selectedProject != nil
        let available = PaletteAction.allCases.filter { !$0.needsProject || hasProject }
        let rows = available.map { action in
            PaletteRow(id: "action-\(action.rawValue)", section: .actions, title: action.title, snippet: action.detail, trailing: action.shortcut, subtitle: nil, systemImage: action.systemImage, kind: .action(action))
        }
        guard !query.isEmpty else { return rows }
        return rows.compactMap { row -> (PaletteRow, Int)? in
            let text = row.title + " " + (row.snippet ?? "")
            guard let match = ModelCatalog.fuzzyMatch(query, in: text) else { return nil }
            return (row, match.score)
        }
        .sorted { $0.1 > $1.1 }
        .map(\.0)
    }

    /// A ~90 character window of `text` around the first occurrence of `query`.
    static func snippet(_ text: String, around query: String) -> String {
        let collapsed = ChatState.clip(text, 400)
        guard let range = collapsed.range(of: query, options: [.caseInsensitive, .diacriticInsensitive]) else {
            return ChatState.clip(collapsed, 90)
        }
        let lead = 30
        var start = range.lowerBound
        var steps = 0
        while start > collapsed.startIndex, steps < lead {
            start = collapsed.index(before: start)
            steps += 1
        }
        var window = String(collapsed[start...])
        if start > collapsed.startIndex { window = "…" + window }
        return ChatState.clip(window, 90)
    }
}

// MARK: - Rows

private enum PaletteAction: String, CaseIterable {
    case newChat, newWorktree, openProject, switchModel, rebuildIndex, toggleInspector, runDoctor, settings

    var title: String {
        switch self {
        case .newChat: return "New chat"
        case .newWorktree: return "New chat in a worktree"
        case .openProject: return "Open project…"
        case .switchModel: return "Switch model…"
        case .rebuildIndex: return "Rebuild index"
        case .toggleInspector: return "Toggle inspector"
        case .runDoctor: return "Run doctor"
        case .settings: return "Settings…"
        }
    }

    var detail: String {
        switch self {
        case .newChat: return "Start an empty chat in the selected project"
        case .newWorktree: return "A second checkout of the repository, so tasks run side by side"
        case .openProject: return "Choose a repository folder"
        case .switchModel: return "Pick the project's model"
        case .rebuildIndex: return "Re-index the repository from scratch"
        case .toggleInspector: return "Show or hide the Run / Log pane"
        case .runDoctor: return "Check configuration, repository and provider"
        case .settings: return "General, Model, Permissions, Pricing, Appearance"
        }
    }

    var shortcut: String? {
        switch self {
        case .newChat: return "⌘N"
        case .newWorktree: return "⌘⇧N"
        case .openProject: return "⌘O"
        case .switchModel: return "⌘⇧M"
        case .rebuildIndex: return "⌘⇧R"
        case .toggleInspector: return "⌘⇧I"
        case .runDoctor: return nil
        case .settings: return "⌘,"
        }
    }

    var systemImage: String {
        switch self {
        case .newChat: return Icons.newChat
        case .newWorktree: return Icons.worktree
        case .openProject: return Icons.projects
        case .switchModel: return Icons.model
        case .rebuildIndex: return Icons.refresh
        case .toggleInspector: return Icons.inspector
        case .runDoctor: return Icons.doctor
        case .settings: return Icons.settings
        }
    }

    var needsProject: Bool {
        switch self {
        case .newChat, .newWorktree, .switchModel, .rebuildIndex, .runDoctor: return true
        case .openProject, .toggleInspector, .settings: return false
        }
    }
}

private struct PaletteRow: Identifiable, Equatable {
    enum Section: Hashable {
        case chats, actions
        var title: String { self == .chats ? "Chats" : "Actions" }
    }

    enum Kind: Equatable {
        case chat(projectId: String, sessionId: String)
        case action(PaletteAction)
    }

    let id: String
    let section: Section
    let title: String
    let snippet: String?
    let trailing: String?
    let subtitle: String?
    let systemImage: String
    let kind: Kind
}

private struct PaletteRowView: View {
    var row: PaletteRow
    var isSelected: Bool
    var hover: () -> Void
    var activate: () -> Void

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Button(action: activate) {
            HStack(spacing: 10) {
                Image(systemName: row.systemImage)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(isSelected ? Theme.textPrimary : Theme.textSecondary)
                    .frame(width: 24, height: 24)
                    .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(verbatim: row.title)
                            .font(.system(size: 13.5, weight: .medium))
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                        if let subtitle = row.subtitle {
                            Chip(subtitle, systemImage: Icons.folder)
                        }
                    }
                    if let snippet = row.snippet {
                        Text(verbatim: snippet)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 8)
                if let trailing = row.trailing {
                    if case .action = row.kind {
                        Text(trailing).kbd()
                    } else {
                        Text(verbatim: trailing)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(isSelected ? Theme.surfaceRaised : Color.clear, in: shape)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            if inside { hover() }
        }
        .accessibilityLabel(row.snippet.map { "\(row.title). \($0)" } ?? row.title)
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - Preview

#Preview("Command palette") {
    let state = MainWindowPreview.appState()
    state.paletteVisible = true
    return ZStack {
        Theme.canvas
        CommandPalette()
    }
    .frame(width: 1000, height: 700)
    .environment(state)
}
