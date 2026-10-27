import SwiftUI
import AppKit

/// The Project tab: a two-column grid of cards for the selected project —
/// Index (stats, progress, Rebuild), Git, Validation commands, Languages,
/// Doctor (Run / Run with probe), Memories and Recent tasks.
struct ProjectView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        Group {
            if let project = appState.selectedProject {
                ProjectContent(project: project)
            } else {
                EmptyPlaceholder(
                    systemImage: Icons.project,
                    title: "No project open",
                    subtitle: "Open a repository to see its index, git state, validation commands and memories.",
                    actionTitle: "Open a project…"
                ) {
                    Task { await appState.chooseProject() }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
    }
}

// MARK: - Content

private struct ProjectContent: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    /// Two columns when they fit, one otherwise (e.g. the 1180 pt window with
    /// the sidebar and inspector open). 340 pt keeps it at two at the 1032 pt
    /// content cap and fits one column in the narrowest pane
    /// (960 − 248 − 320 − 48 padding = 344).
    private static let columnMinimum: CGFloat = 340
    private static let spacing: CGFloat = 16
    private static let cardCount = 7

    @State private var twoColumns = true

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                ProjectHeader(project: project)
                cards
                    .onGeometryChange(for: Bool.self) { $0.size.width >= Self.columnMinimum * 2 + Self.spacing } action: { twoColumns = $0 }
            }
            .padding(24)
            .frame(maxWidth: 1080, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .task(id: project.id) { await refresh() }
    }

    /// A plain (not lazy) grid: with a lazy one the cards' heights were only
    /// known once scrolled into view, so the content height kept changing
    /// near the bottom and scrolling stalled before the end.
    private var cards: some View {
        Grid(alignment: .topLeading, horizontalSpacing: Self.spacing, verticalSpacing: Self.spacing) {
            if twoColumns {
                ForEach(Array(stride(from: 0, to: Self.cardCount, by: 2)), id: \.self) { index in
                    GridRow(alignment: .top) {
                        card(index)
                        if index + 1 < Self.cardCount {
                            card(index + 1)
                        } else {
                            Color.clear.frame(height: 0)
                        }
                    }
                }
            } else {
                ForEach(0..<Self.cardCount, id: \.self) { index in
                    GridRow { card(index) }
                }
            }
        }
    }

    @ViewBuilder
    private func card(_ index: Int) -> some View {
        switch index {
        case 0: IndexCard(project: project)
        case 1: GitCard(project: project)
        case 2: ValidationCard(project: project)
        case 3: LanguagesCard(project: project)
        case 4: DoctorCard(project: project)
        case 5: MemoriesCard(project: project)
        default: RecentTasksCard(project: project)
        }
    }

    private func refresh() async {
        await project.refreshStatus(backend: appState.backend)
        if project.tasks.isEmpty, project.memories.isEmpty, !project.extrasLoading {
            await appState.loadProjectExtras(project: project)
        }
    }
}

private struct ProjectHeader: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    /// Chips beside the title when the name and ~200 pt of path fit next to
    /// them; otherwise under it (narrow pane), losing the optional chips if
    /// even that row is too wide.
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 16) {
                titleBlock
                Spacer(minLength: 8)
                chips(sandbox: true, approval: true)
            }
            VStack(alignment: .leading, spacing: 12) {
                titleBlock
                ViewThatFits(in: .horizontal) {
                    chips(sandbox: true, approval: true)
                    chips(sandbox: false, approval: true)
                    chips(sandbox: false, approval: false)
                }
            }
        }
    }

    private var titleBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: project.name)
                .font(Theme.Font.title)
                .foregroundStyle(Theme.textPrimary)
            HStack(spacing: 4) {
                Text(verbatim: project.root)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .selectableText()
                CopyButton(text: project.root, label: "Copy path")
                IconButton(Icons.folder, label: "Reveal in Finder", size: 13) {
                    NSWorkspace.shared.activateFileViewerSelecting([project.rootURL])
                }
            }
            // A long root shouldn't push the chips under the title on its own.
            .frame(minWidth: 0, idealWidth: 200, maxWidth: .infinity, alignment: .leading)
        }
    }

    private func chips(sandbox: Bool, approval: Bool) -> some View {
        HStack(spacing: 8) {
            Chip(appState.modelDisplayName(project.info.model), systemImage: Icons.model)
                .help("Project model")
            if approval {
                Chip(AppState.approvalModeLabel(project.info.permissionProfile), systemImage: Icons.approval)
                    .help("Approval mode")
            }
            if sandbox, project.info.sandbox != "none" {
                Chip("command wrapper: \(project.info.sandbox)")
                    .help("Developer-mode command wrapper from [permissions.sandbox]")
            }
            StatusPill(running: project.isRunning)
        }
    }
}

// MARK: - Card chrome

private struct ProjectCard<Trailing: View, Content: View>: View {
    let title: String
    let systemImage: String
    @ViewBuilder let trailing: () -> Trailing
    @ViewBuilder let content: () -> Content

    init(_ title: String, systemImage: String, @ViewBuilder trailing: @escaping () -> Trailing, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.systemImage = systemImage
        self.trailing = trailing
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: systemImage)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Theme.textSecondary)
                    .accessibilityHidden(true)
                Text(title)
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 8)
                trailing()
            }
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .card()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(title)
    }
}

extension ProjectCard where Trailing == EmptyView {
    init(_ title: String, systemImage: String, @ViewBuilder content: @escaping () -> Content) {
        self.init(title, systemImage: systemImage, trailing: { EmptyView() }, content: content)
    }
}

private struct EmptyCardText: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Theme.Font.secondary)
            .foregroundStyle(Theme.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.vertical, 4)
    }
}

private struct StatRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
            Spacer(minLength: 8)
            Text(verbatim: value)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.middle)
                .selectableText()
        }
    }
}

// MARK: - Index

private struct IndexCard: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    var body: some View {
        ProjectCard("Index", systemImage: Icons.context) {
            Button("Rebuild") {
                Task { await appState.rebuildIndex() }
            }
            .controlSize(.small)
            .disabled(rebuildDisabled)
            .help(rebuildHelp)
        } content: {
            status
            if let progress = project.indexProgress, progress.total > 0 {
                ProgressBar(fraction: Double(progress.done) / Double(progress.total))
            }
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 6) {
                GridRow {
                    StatRow(label: "Files", value: project.info.index.files.formatted())
                    StatRow(label: "Symbols", value: project.info.index.symbols.formatted())
                }
                GridRow {
                    StatRow(label: "Chunks", value: project.info.index.chunks.formatted())
                    StatRow(label: "Relationships", value: project.info.index.relationships.formatted())
                }
                GridRow {
                    StatRow(label: "Embeddings", value: project.info.index.embeddings.formatted())
                    StatRow(label: "Generation", value: String(project.info.index.generation))
                }
            }
            if let last = project.info.index.lastFullIndexAt.flatMap(RFC3339.parse) {
                Text("Last full index \(TranscriptFormat.relative(last))")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            if let ready = project.lastIndexReady, ready.error == nil {
                Text("Last run: \(ready.filesIndexed) indexed · \(ready.filesRemoved) removed · \(ready.filesUnchanged) unchanged · \(TranscriptFormat.durationMs(ready.durationMs))")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
        }
    }

    private var isIndexing: Bool { project.isIndexing }

    @ViewBuilder
    private var status: some View {
        if isIndexing {
            HStack(spacing: 8) {
                BrailleSpinner()
                Text(project.indexSummary)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
        } else if let error = project.lastIndexReady?.error {
            HStack(alignment: .top, spacing: 6) {
                Image(systemName: Icons.warning)
                    .font(.system(size: 12, weight: .medium))
                Text("Indexing failed: \(error) · retrieval falls back to text tools")
                    .font(Theme.Font.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .foregroundStyle(Theme.danger)
        } else {
            HStack(spacing: 6) {
                Image(systemName: Icons.check)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.success)
                Text(project.indexSummary)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
        }
    }

    private var rebuildDisabled: Bool { project.isRunning || isIndexing }

    private var rebuildHelp: String {
        if project.isRunning { return "Finishes after the running task" }
        if isIndexing { return "Indexing is already in progress" }
        return "Re-index the repository from scratch"
    }
}

/// A thin determinate bar (the app never shows a system ProgressView).
private struct ProgressBar: View {
    let fraction: Double

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.surfaceRaised)
                Capsule()
                    .fill(Theme.accent)
                    .frame(width: max(0, min(1, fraction)) * geometry.size.width)
            }
        }
        .frame(height: 6)
        .animation(.easeOut(duration: 0.2), value: fraction)
        .accessibilityElement()
        .accessibilityLabel("Indexing progress")
        .accessibilityValue("\(Int((max(0, min(1, fraction)) * 100).rounded())) percent")
    }
}

// MARK: - Git

private struct GitCard: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    var body: some View {
        ProjectCard("Git", systemImage: Icons.branch) {
            if project.isGitRepository {
                Button("Open Changes") {
                    appState.sidebarItem = .changes
                }
                .controlSize(.small)
            }
        } content: {
            if project.isGitRepository {
                HStack(spacing: 8) {
                    Text("Branch")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                    Spacer(minLength: 8)
                    if let branch = project.branch {
                        Chip(branch, systemImage: Icons.branch)
                    } else {
                        Text("detached")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                StatRow(label: "Head", value: head)
                StatRow(label: "Changed files", value: String(project.dirtyCount))
                if let error = project.gitError {
                    Text(error)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.danger)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } else {
                EmptyCardText(text: "Not a git repository.")
            }
        }
    }

    private var head: String {
        let full = project.gitStatus?.head ?? project.info.git?.head
        guard let full, !full.isEmpty else { return "—" }
        return String(full.prefix(8))
    }
}

// MARK: - Validation commands

/// The first `shown` commands, then "See all N" (expands in place) /
/// "Show fewer".
private struct ValidationCard: View {
    let project: ProjectState
    private static let shown = 5

    @State private var showAll = false

    var body: some View {
        ProjectCard("Validation commands", systemImage: Icons.validationPass) {
            let commands = project.info.validationCommands
            if commands.isEmpty {
                EmptyCardText(text: "None detected — add [validation] commands to .context-agent.toml")
            } else {
                let limited = commands.count > Self.shown
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(showAll || !limited ? commands : Array(commands.prefix(Self.shown))) { command in
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 8) {
                                Text(verbatim: command.command)
                                    .font(Theme.Font.mono)
                                    .foregroundStyle(Theme.textPrimary)
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                    .selectableText()
                                Spacer(minLength: 8)
                                Chip(command.kind)
                            }
                            Text(verbatim: detail(command))
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                                .lineLimit(1)
                        }
                    }
                    if limited {
                        Button(showAll ? "Show fewer" : "See all \(commands.count)") {
                            showAll.toggle()
                        }
                        .buttonStyle(.plain)
                        .font(.system(size: 12.5, weight: .medium))
                        .foregroundStyle(Theme.accent)
                        .padding(.top, 2)
                    }
                }
            }
        }
    }

    private func detail(_ command: ValidationCommand) -> String {
        var parts: [String] = []
        if command.cwd != ".", !command.cwd.isEmpty { parts.append("in \(command.cwd)") }
        if !command.source.isEmpty { parts.append("from \(command.source)") }
        return parts.isEmpty ? "repository root" : parts.joined(separator: " · ")
    }
}

// MARK: - Languages

private struct LanguagesCard: View {
    let project: ProjectState
    private static let shown = 8

    var body: some View {
        ProjectCard("Languages", systemImage: Icons.chart) {
            let languages = sorted
            if languages.isEmpty {
                EmptyCardText(text: "No files indexed yet.")
            } else {
                let maximum = max(1, languages.first?.files ?? 1)
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(languages.prefix(Self.shown)) { language in
                        HStack(spacing: 10) {
                            Text(verbatim: label(language.language))
                                .font(Theme.Font.secondary)
                                .foregroundStyle(Theme.textPrimary)
                                .frame(width: 96, alignment: .leading)
                                .lineLimit(1)
                            GeometryReader { geometry in
                                Capsule()
                                    .fill(Theme.accent.opacity(0.75))
                                    .frame(width: max(3, geometry.size.width * CGFloat(language.files) / CGFloat(maximum)))
                                    .frame(maxHeight: .infinity, alignment: .center)
                            }
                            .frame(height: 8)
                            .accessibilityHidden(true)
                            Text(verbatim: language.files.formatted())
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 48, alignment: .trailing)
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("\(label(language.language)): \(language.files) files")
                    }
                    if languages.count > Self.shown {
                        Text("and \(languages.count - Self.shown) more")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
            }
        }
    }

    private var sorted: [LanguageCount] {
        let source = project.info.languages.isEmpty ? project.info.index.languages : project.info.languages
        return source.filter { $0.files > 0 }.sorted { $0.files != $1.files ? $0.files > $1.files : $0.language < $1.language }
    }

    private func label(_ language: String) -> String {
        language.prefix(1).uppercased() + language.dropFirst()
    }
}

// MARK: - Doctor

private struct DoctorCard: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    var body: some View {
        ProjectCard("Doctor", systemImage: Icons.doctor) {
            HStack(spacing: 6) {
                if project.doctorRunning {
                    BrailleSpinner()
                }
                Button("Run") {
                    Task { await appState.runDoctor(probe: false) }
                }
                .controlSize(.small)
                .disabled(disabled)
                .help(help(probe: false))
                Button("Run with probe") {
                    Task { await appState.runDoctor(probe: true) }
                }
                .controlSize(.small)
                .disabled(disabled)
                .help(help(probe: true))
            }
        } content: {
            if let error = project.doctorError {
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: Icons.warning)
                        .font(.system(size: 12, weight: .medium))
                    Text(error)
                        .font(Theme.Font.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
                .foregroundStyle(Theme.danger)
            }
            if let report = project.doctor {
                DoctorReportView(report: report)
            } else if project.doctorRunning {
                Text("Checking configuration, repository, validation commands, language servers and the model provider…")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else if project.doctorError == nil {
                EmptyCardText(text: "Run the doctor to check the configuration, repository, validation commands, language servers and the model provider. “Run with probe” sends a test request to the model endpoint.")
            }
        }
    }

    private var disabled: Bool { project.doctorRunning || project.isRunning }

    private func help(probe: Bool) -> String {
        if project.isRunning { return "Finishes after the running task" }
        if project.doctorRunning { return "The doctor is already running" }
        return probe ? "Also probe the model endpoint (sends real requests, up to 2 min)" : "Check the project without contacting the model"
    }
}

/// Sections of label/value rows with ✓/✗ marks.
private struct DoctorReportView: View {
    let report: DoctorReport

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Text(verbatim: report.problems == 0 ? "✓" : "✗")
                    .font(.system(size: 13, weight: .semibold, design: .monospaced))
                Text(report.problems == 0 ? "No problems found" : "\(report.problems) problem\(report.problems == 1 ? "" : "s") found")
                    .font(Theme.Font.secondary)
            }
            .foregroundStyle(report.problems == 0 ? Theme.success : Theme.danger)
            ForEach(report.sections) { section in
                VStack(alignment: .leading, spacing: 4) {
                    Text(verbatim: section.title.prefix(1).uppercased() + section.title.dropFirst())
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(.top, 2)
                    ForEach(Array(section.rows.enumerated()), id: \.offset) { _, row in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(verbatim: row.ok ? "✓" : "✗")
                                .font(.system(size: 12, weight: .semibold, design: .monospaced))
                                .foregroundStyle(row.ok ? Theme.success : Theme.danger)
                                .frame(width: 12)
                            Text(verbatim: row.label)
                                .font(Theme.Font.secondary)
                                .foregroundStyle(Theme.textSecondary)
                                .frame(width: 128, alignment: .leading)
                                .lineLimit(2)
                            Text(verbatim: row.value)
                                .font(Theme.Font.secondary)
                                .foregroundStyle(row.ok ? Theme.textPrimary : Theme.danger)
                                .lineLimit(3)
                                .truncationMode(.middle)
                                .fixedSize(horizontal: false, vertical: true)
                                .selectableText()
                        }
                        .accessibilityElement(children: .combine)
                        .accessibilityLabel("\(row.ok ? "OK" : "Problem"): \(row.label) \(row.value)")
                    }
                }
            }
        }
    }
}

// MARK: - Memories

private struct MemoriesCard: View {
    let project: ProjectState
    private static let shown = 10

    var body: some View {
        ProjectCard("Memories", systemImage: Icons.memory) {
            if !project.memories.isEmpty {
                Chip("\(project.memories.count)")
            }
        } content: {
            if project.memories.isEmpty {
                if project.extrasLoading {
                    HStack(spacing: 8) {
                        BrailleSpinner()
                        Text("Loading…")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                    }
                } else {
                    EmptyCardText(text: "Nothing remembered yet.")
                }
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(project.memories.prefix(Self.shown)) { memory in
                        MemoryRow(memory: memory)
                    }
                    if project.memories.count > Self.shown {
                        Text("and \(project.memories.count - Self.shown) more")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
            }
        }
    }
}

/// Collapsed to three lines; clicking a truncated row shows all of it.
/// Only the expanded text is selectable: AppKit's selection editor ignores
/// `lineLimit`, so selectable clipped text spilled over the rows below it.
private struct MemoryRow: View {
    let memory: MemoryEntry

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var expanded = false
    @State private var fullHeight: CGFloat = 0
    @State private var clippedHeight: CGFloat = 0

    private static let collapsedLines = 3

    /// Measured while collapsed; an expanded row can always collapse again.
    private var expandable: Bool { expanded || fullHeight > clippedHeight + 1 }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            header
            if expanded {
                content
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            } else {
                content
                    .lineLimit(Self.collapsedLines)
                    .fixedSize(horizontal: false, vertical: true)
                    .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { clippedHeight = $0 }
                    .background(alignment: .topLeading) {
                        content
                            .fixedSize(horizontal: false, vertical: true)
                            .hidden()
                            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { fullHeight = $0 }
                    }
                    .contentShape(Rectangle())
                    .onTapGesture { if expandable { toggle() } }
            }
            if !memory.evidencePaths.isEmpty {
                Text(verbatim: memory.evidencePaths.joined(separator: " · "))
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(expanded ? nil : 1)
                    .truncationMode(.middle)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.bottom, 6)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityValue(expandable ? (expanded ? "Expanded" : "Collapsed") : "")
    }

    private var header: some View {
        Button(action: toggle) {
            HStack(spacing: 6) {
                Chip(memory.category.replacingOccurrences(of: "_", with: " "))
                if memory.invalidated {
                    Chip("invalidated", style: .tinted(Theme.warning))
                } else if !memory.verification.isEmpty {
                    Text(verbatim: memory.verification.replacingOccurrences(of: "_", with: " "))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                Spacer(minLength: 4)
                if let date = RFC3339.parse(memory.createdAt) {
                    Text(TranscriptFormat.relative(date))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                if expandable {
                    Image(systemName: Icons.chevronDown)
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(Theme.textTertiary)
                        .rotationEffect(.degrees(expanded ? 0 : -90))
                        .accessibilityHidden(true)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!expandable)
        .help(expandable ? (expanded ? "Show less" : "Show the whole memory") : "")
    }

    private var content: some View {
        Text(verbatim: memory.content)
            .font(Theme.Font.secondary)
            .foregroundStyle(memory.invalidated ? Theme.textTertiary : Theme.textPrimary)
            .strikethrough(memory.invalidated)
    }

    private func toggle() {
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { expanded.toggle() }
    }
}

// MARK: - Recent tasks

private struct RecentTasksCard: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState
    private static let shown = 10

    var body: some View {
        ProjectCard("Recent tasks", systemImage: Icons.queued) {
            if let error = project.extrasError {
                Image(systemName: Icons.warning)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Theme.danger)
                    .help(error)
                    .accessibilityLabel("Could not load: \(error)")
            }
        } content: {
            let tasks = sortedTasks
            if tasks.isEmpty {
                if project.extrasLoading {
                    HStack(spacing: 8) {
                        BrailleSpinner()
                        Text("Loading…")
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.textSecondary)
                    }
                } else {
                    EmptyCardText(text: "No tasks yet.")
                }
            } else {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(tasks.prefix(Self.shown)) { task in
                        TaskRow(task: task, isRunning: task.sessionId.map(project.isRunning(session:)) == true && task.status == nil) {
                            open(task)
                        }
                    }
                    if tasks.count > Self.shown {
                        Text("and \(tasks.count - Self.shown) more")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                            .padding(.leading, 8)
                    }
                }
            }
        }
    }

    private var sortedTasks: [TaskSummary] {
        project.tasks.sorted { (RFC3339.parse($0.updatedAt) ?? .distantPast) > (RFC3339.parse($1.updatedAt) ?? .distantPast) }
    }

    private func open(_ task: TaskSummary) {
        guard let sessionId = task.sessionId else { return }
        Task { await appState.select(session: sessionId) }
    }
}

private struct TaskRow: View {
    let task: TaskSummary
    let isRunning: Bool
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            VStack(alignment: .leading, spacing: 3) {
                Text(verbatim: task.objective)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    if isRunning {
                        StatusPill(running: true)
                    } else if let status = task.status {
                        StatusPill(status)
                    } else {
                        StatusPill(text: task.state.label, tint: Theme.textSecondary)
                    }
                    if let date = RFC3339.parse(task.completedAt ?? task.updatedAt) {
                        Text(TranscriptFormat.relative(date))
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                    if !task.modifiedFiles.isEmpty {
                        Text("\(task.modifiedFiles.count) file\(task.modifiedFiles.count == 1 ? "" : "s")")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                    }
                    if !task.unresolved.isEmpty {
                        Text("\(task.unresolved.count) unresolved")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.warning)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        }
        .buttonStyle(.plain)
        .hoverHighlight()
        .disabled(task.sessionId == nil)
        .help(task.sessionId == nil ? "This task has no chat" : "Open the chat")
        .accessibilityLabel("Task: \(task.objective)")
    }
}

// MARK: - Preview

#Preview("Project") {
    let appState = AppState()
    let info = ProjectInfo(
        projectId: "preview-project",
        root: "/Users/me/Projects/context-agent",
        name: "context-agent",
        git: GitInfo(branch: "main", head: "862959a1f00c", dirtyFiles: 3),
        index: IndexStats(generation: 12, files: 310, symbols: 4102, chunks: 5110, relationships: 900, embeddings: 5110, lastFullIndexAt: "2026-10-20T09:12:00Z", languages: []),
        indexing: false,
        model: "gpt-4.1",
        permissionProfile: "approval_required",
        sandbox: "none",
        validationCommands: [
            ValidationCommand(command: "cargo test", kind: "test", cwd: ".", source: "Cargo.toml"),
            ValidationCommand(command: "cargo clippy --all-targets", kind: "lint", cwd: ".", source: "Cargo.toml"),
        ],
        languages: [
            LanguageCount(language: "rust", files: 120),
            LanguageCount(language: "swift", files: 48),
            LanguageCount(language: "markdown", files: 10),
            LanguageCount(language: "toml", files: 6),
        ],
        buildFiles: ["Cargo.toml"],
        fileCount: 310,
        runningTask: nil
    )
    let project = ProjectState(info: info, models: ModelCatalog(preferences: appState.preferences, baseURL: ""))
    project.doctor = DoctorReport(problems: 1, sections: [
        DoctorSection(title: "configuration", rows: [DoctorRow(label: "data dir", value: "/Users/me/.context-agent", ok: true)]),
        DoctorSection(title: "model provider", rows: [
            DoctorRow(label: "endpoint", value: "https://api.openai.com/v1", ok: true),
            DoctorRow(label: "api key", value: "OPENAI_API_KEY is not set", ok: false),
        ]),
    ])
    project.memories = [
        MemoryEntry(id: "m1", category: "project_knowledge", verification: "verified", content: "Validation runs through `cargo test`; integration tests live under crates/agent/tests.", evidencePaths: ["Cargo.toml", "crates/agent/tests/runtime.rs"], createdAt: "2026-10-19T10:00:00Z", invalidated: false),
        MemoryEntry(id: "m2", category: "episodic", verification: "unverified", content: "The TUI's approval names map onto permission profiles.", evidencePaths: [], createdAt: "2026-10-18T10:00:00Z", invalidated: true),
    ]
    project.tasks = [
        TaskSummary(id: "t1", sessionId: "s1", objective: "Fix spacing on cards", category: "localized_edit", state: .completed, status: .completed, summary: nil, createdAt: "2026-10-20T08:00:00Z", updatedAt: "2026-10-20T08:06:00Z", completedAt: "2026-10-20T08:06:00Z", modifiedFiles: ["src/cards.rs"], unresolved: []),
        TaskSummary(id: "t2", sessionId: "s2", objective: "Need better empty state for the sidebar when no project is open", category: "multi_file_change", state: .blocked, status: .blocked, summary: nil, createdAt: "2026-10-19T08:00:00Z", updatedAt: "2026-10-19T08:30:00Z", completedAt: nil, modifiedFiles: [], unresolved: ["Decide on copy"]),
    ]
    appState.projects.append(project)
    appState.selectedProjectId = project.id
    return ProjectView()
        .environment(appState)
        .frame(width: 980, height: 760)
}
