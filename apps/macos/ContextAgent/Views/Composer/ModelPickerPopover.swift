import SwiftUI
import AppKit

/// The model picker (ARCHITECTURE.md "Model picker popover"): 320 × 400,
/// auto-focused search, a "Recent" section, prefix groups past 12 entries,
/// fuzzy matches with highlighted runs, ↑↓ Return Esc handled here, a
/// trailing "Use “<typed>” as the model name" row, loading / list-error /
/// switching / capability states and a read-only mode while the project
/// runs. Reads `appState.selectedProject`.
struct ModelPickerPopover: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var query = ""
    /// Index into the selectable rows (headers are skipped).
    @State private var selected = 0
    /// Bumped by ↑↓ and typing; only these scroll the selection into view
    /// (hover selects without scrolling, so the list never moves under the cursor).
    @State private var scrollRequest = 0
    /// Screen location of the last hover that changed the selection. A hover
    /// at the same location comes from rows scrolling under a still cursor
    /// and is ignored, so arrow keys keep their selection.
    @State private var lastHoverLocation: CGPoint?
    /// Set once a model is chosen: rows stop floating hover cards while the
    /// popover fades out under the cursor.
    @State private var closing = false
    @FocusState private var searchFocused: Bool

    private struct Row: Identifiable {
        enum Kind {
            case header(String)
            case model(ModelCatalog.Match, recent: Bool)
            case custom(String)
        }

        let id: String
        let kind: Kind

        var isSelectable: Bool {
            if case .header = kind { return false }
            return true
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            searchField
                .padding(10)
            Rectangle()
                .fill(Theme.border)
                .frame(height: 1)
                .accessibilityHidden(true)
            if let project = appState.selectedProject {
                content(project)
            } else {
                EmptyPlaceholder(systemImage: Icons.model, title: "No project selected", subtitle: "Open a project to choose its model.")
            }
        }
        .frame(width: 320, height: 400)
        .background(Theme.surface)
        .defaultFocus($searchFocused, true)
        .onExitCommand { dismiss() }
        .onKeyPress(.escape) {
            dismiss()
            return .handled
        }
        // On the container (an ancestor of the focused search field), so the
        // arrows move the selection instead of the caret.
        .onKeyPress(.upArrow) {
            move(-1)
            return .handled
        }
        .onKeyPress(.downArrow) {
            move(1)
            return .handled
        }
        .onChange(of: query) { _, _ in
            selected = 0
            scrollRequest += 1
        }
        .onDisappear { HoverCardPanel.shared.hide() }
        .task {
            // Popover content is attached a beat after creation; focusing then sticks.
            try? await Task.sleep(for: .milliseconds(60))
            searchFocused = true
            // Normally prefetched when the project opened; fetches only when missing or stale.
            await appState.refreshModels(force: false)
        }
    }

    // MARK: - Search

    private var searchField: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return HStack(spacing: 6) {
            Image(systemName: Icons.search)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            TextField("Search models…", text: $query)
                .textFieldStyle(.plain)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textPrimary)
                .focused($searchFocused)
                .onSubmit { activateSelection() }
                .accessibilityLabel("Search models")
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: Icons.clear)
                        .font(.system(size: 12))
                        .foregroundStyle(Theme.textTertiary)
                }
                .buttonStyle(.plain)
                .help("Clear")
                .accessibilityLabel("Clear search")
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 30)
        .background(Theme.canvas, in: shape)
        .overlay(shape.strokeBorder(searchFocused ? Theme.borderStrong : Theme.border, lineWidth: 1))
    }

    // MARK: - List

    @ViewBuilder
    private func content(_ project: ProjectState) -> some View {
        let rows = rows(for: project)
        let ids = selectableIds(rows)
        let selectedId = ids.isEmpty ? nil : ids[min(max(selected, 0), ids.count - 1)]
        // A running task keeps the model it started with; the switch
        // applies to the next turn, so the picker stays live while it runs.
        let interactive = project.switchingModel == nil

        ScrollViewReader { proxy in
            ScrollView {
                // Not lazy: headers and rows differ in height, and a lazy
                // stack re-measuring them mid-scroll made scrolling stall.
                VStack(alignment: .leading, spacing: 1) {
                    ForEach(rows) { row in
                        rowView(row, project: project, isSelected: row.id == selectedId, interactive: interactive) {
                            let location = NSEvent.mouseLocation
                            guard location != lastHoverLocation else { return }
                            lastHoverLocation = location
                            if let index = ids.firstIndex(of: row.id) { selected = index }
                        }
                        .id(row.id)
                    }
                    if rows.isEmpty {
                        Text(project.models.showsLoading ? "Loading models…" : "No models listed")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textTertiary)
                            .padding(12)
                    }
                }
                .padding(6)
            }
            .onChange(of: scrollRequest) { _, _ in
                // No anchor: scroll only as far as needed to show the row.
                if let selectedId { proxy.scrollTo(selectedId) }
            }
        }

        if hasStatus(project) {
            statusFooter(project)
        }
    }

    @ViewBuilder
    private func rowView(_ row: Row, project: ProjectState, isSelected: Bool, interactive: Bool, onHover: @escaping () -> Void) -> some View {
        switch row.kind {
        case .header(let title):
            Text(title)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .padding(.horizontal, 8)
                .padding(.top, 8)
                .padding(.bottom, 3)
                .accessibilityAddTraits(.isHeader)
        case .model(let match, let recent):
            let isCurrent = match.name == project.info.model
            ModelRow(
                title: Self.highlighted(match),
                logoModelId: match.name,
                systemImage: nil,
                // The current model keeps "current"; the others show the gateway's savings.
                trailing: isCurrent ? "current" : appState.modelStat(match.name, in: project)?.savingsLabel,
                trailingTint: isCurrent ? Theme.accent : Theme.success,
                isSelected: isSelected,
                interactive: interactive,
                card: closing ? nil : appState.modelCard(match.name),
                action: { choose(match.name, project: project) },
                onHover: onHover
            )
            .help(match.display == match.name ? "" : match.name)
            .contextMenu {
                if recent {
                    Button("Remove from Recent") { project.models.removeRecent(match.name) }
                }
            }
        case .custom(let typed):
            ModelRow(
                title: Text("Use “\(typed)” as the model name").foregroundStyle(Theme.textPrimary),
                logoModelId: nil,
                systemImage: Icons.addCustom,
                trailing: nil,
                isSelected: isSelected,
                interactive: interactive,
                card: nil,
                action: { choose(typed, project: project) },
                onHover: onHover
            )
        }
    }

    // MARK: - Footer (states)

    /// The footer only appears with something to report: a switch in
    /// progress or failed, a first load, a list error or a model the probe
    /// ruled out (no idle "N models · Refresh" line).
    private func hasStatus(_ project: ProjectState) -> Bool {
        let catalog = project.models
        let ruledOut = catalog.lastSwitch.map { $0.model == project.info.model && !$0.capability.agentCapable } ?? false
        return project.switchingModel != nil || catalog.switchError != nil
            || catalog.showsLoading || catalog.error != nil || ruledOut
    }

    @ViewBuilder
    private func statusFooter(_ project: ProjectState) -> some View {
        let catalog = project.models
        VStack(alignment: .leading, spacing: 5) {
            if project.switchingModel != nil {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                    Text("Switching… probing the endpoint can take up to 2 min")
                        .foregroundStyle(Theme.textSecondary)
                }
            }
            if let error = catalog.switchError {
                footerLine(systemImage: Icons.failed, tint: Theme.danger, error)
            }
            if catalog.showsLoading {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                    Text("Loading models…")
                        .foregroundStyle(Theme.textSecondary)
                }
            } else if let error = catalog.error {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    footerLine(systemImage: Icons.warning, tint: Theme.danger, error)
                    Spacer(minLength: 4)
                    refreshButton(catalog)
                }
            }
            // Capabilities live on the hover cards; only a probe that rules the
            // model out for agent runs is worth a footer line.
            if let last = catalog.lastSwitch, last.model == project.info.model, !last.capability.agentCapable {
                footerLine(systemImage: Icons.failed, tint: Theme.danger, Self.capabilityLine(last.capability))
            }
        }
        .font(Theme.Font.caption)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.canvas)
        .hairline(.top)
    }

    /// Retries the fetch after a list error (bypassing the backend's cache);
    /// a spinner while it runs.
    @ViewBuilder
    private func refreshButton(_ catalog: ModelCatalog) -> some View {
        if catalog.loading {
            BrailleSpinner(size: 11, tint: Theme.textSecondary)
                .accessibilityLabel("Refreshing models")
        } else {
            Button("Refresh") {
                Task { await appState.refreshModels(force: true) }
            }
            .buttonStyle(.plain)
            .foregroundStyle(Theme.accent)
            .help("Fetch the model list again")
        }
    }

    private func footerLine(systemImage: String, tint: Color, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 5) {
            Image(systemName: systemImage)
                .font(.system(size: 10, weight: .medium))
                .foregroundStyle(tint)
                .accessibilityHidden(true)
            Text(text)
                .foregroundStyle(tint)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    // MARK: - Rows and selection

    private func rows(for project: ProjectState) -> [Row] {
        let catalog = project.models
        var out: [Row] = []
        for section in catalog.sections(query: query, displayName: appState.modelDisplayName) where !section.items.isEmpty {
            out.append(Row(id: "header:\(section.id)", kind: .header(section.title)))
            for item in section.items {
                out.append(Row(id: "\(section.id):\(item.name)", kind: .model(item, recent: section.kind == .recent)))
            }
        }
        if catalog.offersCustomName(for: query) {
            out.append(Row(id: "custom", kind: .custom(query.trimmingCharacters(in: .whitespaces))))
        }
        return out
    }

    private func selectableIds(_ rows: [Row]) -> [String] {
        rows.filter(\.isSelectable).map(\.id)
    }

    private var selectableCount: Int {
        guard let project = appState.selectedProject else { return 0 }
        return selectableIds(rows(for: project)).count
    }

    private func move(_ delta: Int) {
        let count = selectableCount
        guard count > 0 else { return }
        selected = min(max(selected + delta, 0), count - 1)
        scrollRequest += 1
    }

    /// Return: the selected row (model or custom name).
    private func activateSelection() {
        guard let project = appState.selectedProject else { return }
        let rows = rows(for: project)
        let ids = selectableIds(rows)
        guard !ids.isEmpty else { return }
        let id = ids[min(max(selected, 0), ids.count - 1)]
        guard let row = rows.first(where: { $0.id == id }) else { return }
        switch row.kind {
        case .model(let match, _): choose(match.name, project: project)
        case .custom(let typed): choose(typed, project: project)
        case .header: break
        }
    }

    /// Closes the picker and runs `models/switch` in the background: the
    /// composer chip shows "Switching…", and a failure or a model the probe
    /// rules out for agent runs comes back as a toast. Choosing the current
    /// model again re-runs the switch unless it last passed the probe, so a
    /// model that failed validation gets probed again. Allowed while a task
    /// runs: that turn keeps its model, the next one uses the new model.
    private func choose(_ name: String, project: ProjectState) {
        guard project.switchingModel == nil else { return }
        // The chosen row never gets a hover exit, and the popover's
        // `onDisappear` can come late: hide its card now.
        closing = true
        HoverCardPanel.shared.hide()
        appState.modelPickerVisible = false
        dismiss()
        let passed = project.models.lastSwitch.map { $0.model == name && $0.capability.agentCapable } ?? false
        guard name != project.info.model || !passed else { return }
        let toasts = appState.toasts
        let display = appState.modelDisplayName(name)
        Task {
            do {
                let result = try await appState.switchModel(name)
                if !result.capability.agentCapable {
                    toasts.show("\(display): \(Self.capabilityLine(result.capability))", kind: .warning)
                }
            } catch {
                toasts.show("Could not switch to \(display): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    // MARK: - Formatting

    /// Matched runs in `accent` semibold.
    static func highlighted(_ match: ModelCatalog.Match) -> Text {
        match.segments.reduce(Text("")) { partial, segment in
            let run = Text(segment.text)
                .foregroundStyle(segment.matched ? Theme.accent : Theme.textPrimary)
                .fontWeight(segment.matched ? .semibold : .regular)
            return Text("\(partial)\(run)")
        }
    }

    /// The probe's blocking issue for a model that cannot run agent tasks.
    static func capabilityLine(_ capability: CapabilitySummary) -> String {
        "Not usable for agent runs" + (capability.issues.first.map { ": \($0)" } ?? "")
    }
}

// MARK: - Row

private struct ModelRow: View {
    let title: Text
    /// Draws the model's lab logo (models.dev, ✳ when unknown) in front of the title.
    let logoModelId: String?
    let systemImage: String?
    let trailing: String?
    var trailingTint: Color = Theme.accent
    let isSelected: Bool
    let interactive: Bool
    /// Card floated beside the picker while hovered; nil without models.dev or gateway data.
    let card: ModelHoverCard?
    let action: () -> Void
    let onHover: () -> Void

    @State private var anchor = HoverAnchor()

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Button(action: action) {
            HStack(spacing: 8) {
                if let logoModelId {
                    ModelLogo(modelId: logoModelId, size: 14, tint: Theme.textPrimary)
                } else if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(Theme.textTertiary)
                        .frame(width: 14)
                        .accessibilityHidden(true)
                }
                title
                    .font(Theme.Font.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 6)
                if let trailing {
                    Chip(trailing, style: .tinted(trailingTint))
                }
            }
            .padding(.horizontal, 8)
            .frame(height: 28)
            .background(shape.fill(isSelected ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .disabled(!interactive)
        .opacity(interactive ? 1 : 0.55)
        .background(HoverAnchorView(anchor: anchor))
        .onHover { inside in
            if inside, interactive { onHover() }
            if inside, let card, let view = anchor.view {
                HoverCardPanel.shared.show(card, beside: view)
            } else if !inside {
                HoverCardPanel.shared.hide(from: anchor.view)
            }
        }
        .onDisappear { HoverCardPanel.shared.hide(from: anchor.view) }
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

// MARK: - Previews

@MainActor
private enum ModelPickerPreview {
    static let names: [String] = [
        "gpt-4.1", "gpt-4.1-mini", "gpt-4.1-nano", "gpt-4o", "gpt-4o-mini", "o3", "o3-mini", "o4-mini",
        "claude-sonnet-4", "claude-opus-4", "claude-haiku-3.5", "meta/llama-3.3-70b", "meta/llama-3.1-8b",
        "mistral-large", "mistral-small", "qwen/qwen2.5-coder",
    ]

    static func make(listed: Bool = true, running: Bool = false, error: String? = nil) -> AppState {
        let defaults = UserDefaults(suiteName: "dev.contextagent.desktop.preview") ?? .standard
        let preferences = Preferences(defaults: defaults)
        let app = AppState(preferences: preferences, backend: BackendClient(), recents: RecentProjectStore(), toasts: ToastCenter(), notifier: TaskNotifier())
        let info = ProjectInfo(
            projectId: "a1b2c3d4e5f6a1b2c3d4e5f6", root: "/Users/me/Projects/context-agent", name: "context-agent",
            git: nil,
            index: IndexStats(generation: 12, files: 310, symbols: 0, chunks: 0, relationships: 0, embeddings: 0, lastFullIndexAt: nil, languages: []),
            indexing: false, model: "gpt-4.1", permissionProfile: "approval_required", sandbox: "none",
            validationCommands: [], languages: [], buildFiles: [], fileCount: 310,
            runningTask: running ? RunningTask(sessionId: "s1", taskId: nil, startedAt: "2025-10-21T10:00:00Z") : nil
        )
        let catalog = ModelCatalog(preferences: preferences, baseURL: "https://api.openai.com/v1")
        catalog.apply(ModelsList(models: listed ? names : [], current: "gpt-4.1", source: listed ? "endpoint" : "fallback", error: error))
        catalog.promote("gpt-4.1-mini")
        catalog.promote("o3")
        let project = ProjectState(info: info, models: catalog)
        app.projects = [project]
        app.selectedProjectId = project.id
        return app
    }
}

#Preview("Grouped list") {
    ModelPickerPopover()
        .environment(ModelPickerPreview.make())
}

#Preview("List error + running") {
    ModelPickerPopover()
        .environment(ModelPickerPreview.make(listed: false, running: true, error: "GET /models: 401 Unauthorized"))
}
