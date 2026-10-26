import SwiftUI

/// The one-line status bar above the composer (ARCHITECTURE.md "Status
/// bar"). Running: spinner + state + step + elapsed (the composer's stop
/// button cancels). Always:
/// "ctx 12.4k/128k" (click → Inspector Run tab, "compacted" badge), "42.1k
/// tok", "$0.12" with pricing, "index gen 12" / "indexing 120/310". Idle it
/// shows model · approval mode instead of the run segment.
struct StatusBarView: View {
    @Environment(AppState.self) private var appState

    private let chat: ChatState?

    init(chat: ChatState?) {
        self.chat = chat
    }

    var body: some View {
        let project = appState.project(for: chat?.projectId ?? "") ?? appState.selectedProject
        HStack(spacing: 12) {
            leading(project)
            Spacer(minLength: 8)
            trailing(project)
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.textSecondary)
        .frame(height: 22)
        .padding(.horizontal, 6)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Status")
    }

    // MARK: - Left: run state or model · mode

    @ViewBuilder
    private func leading(_ project: ProjectState?) -> some View {
        if let chat, let progress = chat.status.progress {
            HStack(spacing: 6) {
                BrailleSpinner(size: 11, tint: Theme.textSecondary)
                Text(progress.state.label)
                    .foregroundStyle(Theme.textPrimary)
                if progress.step > 0 {
                    dot
                    Text("step \(progress.step)")
                }
                dot
                ElapsedText(since: progress.startedAt)
                    .monospacedDigit()
                if chat.cancelRequested {
                    dot
                    Text("Cancelling…")
                        .foregroundStyle(Theme.textTertiary)
                }
            }
            .lineLimit(1)
        } else if let chat, chat.status == .loadingHistory {
            HStack(spacing: 6) {
                BrailleSpinner(size: 11, tint: Theme.textSecondary)
                Text("Loading history…")
            }
        } else if let project {
            HStack(spacing: 6) {
                ModelLogo(modelId: project.info.model, size: 11, tint: Theme.textTertiary)
                Text(verbatim: appState.modelDisplayName(project.info.model))
                    .foregroundStyle(Theme.textPrimary)
                dot
                Text(AppState.approvalModeLabel(project.info.permissionProfile))
            }
            .lineLimit(1)
            .help("Project model · approval mode")
        }
    }

    // MARK: - Right: context, tokens, cost, index

    @ViewBuilder
    private func trailing(_ project: ProjectState?) -> some View {
        HStack(spacing: 10) {
            if let stats = chat?.contextStats {
                Button {
                    appState.inspectorVisible = true
                    appState.inspectorTab = .run
                } label: {
                    HStack(spacing: 5) {
                        Text("ctx \(TranscriptFormat.tokens(stats.estimatedPromptTokens))/\(TranscriptFormat.tokens(stats.contextWindow))")
                            .monospacedDigit()
                        if stats.compacted {
                            Chip("compacted", style: .tinted(Theme.warning))
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .hoverHighlight(radius: 5)
                .help("Context window usage · click to open the Inspector")
                .accessibilityLabel("Context \(TranscriptFormat.tokens(stats.estimatedPromptTokens)) of \(TranscriptFormat.tokens(stats.contextWindow)) tokens\(stats.compacted ? ", compacted" : "")")
            }

            if let usage = chat?.usage {
                let pricing = appState.config?.pricing != nil
                Text("\(TranscriptFormat.tokens(usage.total)) tok")
                    .monospacedDigit()
                    .help(pricing ? "Tokens used in this chat" : "Tokens used in this chat · set pricing in Settings → Pricing to see costs")
                if pricing, let cost = chat?.costUsd {
                    Text(TranscriptFormat.cost(cost))
                        .monospacedDigit()
                        .help("Estimated cost from Settings → Pricing")
                }
            }

            if let project {
                indexSegment(project)
            }
        }
        .lineLimit(1)
        .foregroundStyle(Theme.textSecondary)
    }

    @ViewBuilder
    private func indexSegment(_ project: ProjectState) -> some View {
        if let progress = project.indexProgress, progress.total > 0 {
            HStack(spacing: 5) {
                BrailleSpinner(size: 10, tint: Theme.textTertiary)
                Text("indexing \(progress.done)/\(progress.total)")
                    .monospacedDigit()
            }
            .help("Indexing the repository")
        } else if project.info.indexing, project.lastIndexReady == nil {
            HStack(spacing: 5) {
                BrailleSpinner(size: 10, tint: Theme.textTertiary)
                Text("indexing")
            }
            .help("Indexing the repository")
        } else if let error = project.lastIndexReady?.error {
            HStack(spacing: 4) {
                Image(systemName: Icons.warning)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(Theme.warning)
                    .accessibilityHidden(true)
                Text("index failed")
                    .foregroundStyle(Theme.warning)
            }
            .help("Indexing failed: \(error) · retrieval falls back to text tools")
        } else {
            Text("index gen \(project.info.index.generation)")
                .monospacedDigit()
                .help("Index ready · \(project.info.index.files) files · generation \(project.info.index.generation)")
        }
    }

    private var dot: some View {
        Text("·")
            .foregroundStyle(Theme.textTertiary)
            .accessibilityHidden(true)
    }
}

/// "6m 12s" updated every second.
private struct ElapsedText: View {
    let since: Date

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            Text(TranscriptFormat.duration(max(0, context.date.timeIntervalSince(since))))
        }
        .accessibilityLabel("Elapsed")
    }
}

// MARK: - Previews

@MainActor
private enum StatusBarPreview {
    static func make(indexing: Bool = false, pricing: Bool = false) -> (app: AppState, project: ProjectState) {
        let defaults = UserDefaults(suiteName: "dev.contextagent.desktop.preview") ?? .standard
        let preferences = Preferences(defaults: defaults)
        let app = AppState(preferences: preferences, backend: BackendClient(), recents: RecentProjectStore(), toasts: ToastCenter(), notifier: TaskNotifier())
        let info = ProjectInfo(
            projectId: "a1b2c3d4e5f6a1b2c3d4e5f6", root: "/Users/me/Projects/context-agent", name: "context-agent",
            git: nil,
            index: IndexStats(generation: 12, files: 310, symbols: 0, chunks: 0, relationships: 0, embeddings: 0, lastFullIndexAt: nil, languages: []),
            indexing: indexing, model: "gpt-4.1", permissionProfile: "approval_required", sandbox: "none",
            validationCommands: [], languages: [], buildFiles: [], fileCount: 310, runningTask: nil
        )
        let project = ProjectState(info: info, models: ModelCatalog(preferences: preferences, baseURL: ""))
        if indexing { project.indexProgress = IndexProgress(projectId: info.projectId, done: 120, total: 310) }
        app.projects = [project]
        app.selectedProjectId = project.id
        if pricing {
            app.config = InitializeResult(
                protocolVersion: 1, ctxVersion: "0.1.0", dataDir: "~/.context-agent", configFile: "~/.context-agent/config.toml", configSources: [], mock: false,
                model: ModelSettings(name: "gpt-4.1", baseUrl: "https://api.openai.com/v1", apiKeyEnv: "OPENAI_API_KEY", apiKeySet: true, contextWindow: 128_000, maxOutputTokens: 16_000, reasoningEffort: nil),
                permissions: PermissionSettings(profile: "approval_required", allowedCommands: [], deniedCommands: [], allowNetworkCommands: false),
                pricing: PricingSettings(inputPerMillion: 2, cachedInputPerMillion: 0.5, outputPerMillion: 8)
            )
        }
        return (app, project)
    }

    static func chat(in project: ProjectState, running: Bool) -> ChatState {
        let chat = project.chat(for: "s1")
        chat.usage = TokenUsage(promptTokens: 38_400, completionTokens: 3_700, cachedPromptTokens: 12_000, reasoningTokens: nil, source: .reported)
        chat.costUsd = 0.12
        chat.contextStats = ContextStats(estimatedPromptTokens: 12_400, stablePrefixTokens: 4_000, repositorySnippets: 18, repositoryTokens: 6_000, memoryRecords: 3, memoryTokens: 400, conversationMessages: 9, conversationTokens: 2_000, retrievalBudgetTokens: 8_000, contextWindow: 128_000, compacted: true)
        if running {
            chat.status = .running(RunProgress(state: .implementing, step: 3, startedAt: Date().addingTimeInterval(-372)))
        } else {
            chat.status = .finished(.completed)
        }
        return chat
    }
}

#Preview("Running with pricing") {
    let fixture = StatusBarPreview.make(pricing: true)
    let chat = StatusBarPreview.chat(in: fixture.project, running: true)
    return StatusBarView(chat: chat)
        .padding(24)
        .frame(width: 760)
        .background(Theme.canvas)
        .environment(fixture.app)
}

#Preview("Idle, indexing, no pricing") {
    let fixture = StatusBarPreview.make(indexing: true)
    let chat = StatusBarPreview.chat(in: fixture.project, running: false)
    return VStack(spacing: 12) {
        StatusBarView(chat: chat)
        StatusBarView(chat: nil)
    }
    .padding(24)
    .frame(width: 760)
    .background(Theme.canvas)
    .environment(fixture.app)
}
