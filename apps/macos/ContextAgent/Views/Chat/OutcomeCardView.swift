import SwiftUI
import AppKit

/// The task outcome at the end of a run: status pill, markdown summary
/// (left out when it repeats the reply just above, `summaryRepeatsReply`),
/// "Modified files" chips (click → Editor tab; ⌘-click opens it in the
/// default editor; path context menu), "Unresolved" bullets in `warning`, and a footer
/// "42.1k tokens · 6m 12s" (hover shows the absolute finish time).
struct OutcomeCardView: View {
    var item: OutcomeItem
    var chat: ChatState

    @Environment(AppState.self) private var appState
    @State private var footerHovering = false

    init(item: OutcomeItem, chat: ChatState) {
        self.item = item
        self.chat = chat
    }

    private var outcome: TaskOutcome { item.outcome }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                StatusPill(outcome.status)
                Spacer(minLength: 0)
            }

            if !summary.isEmpty, !item.summaryRepeatsReply {
                MarkdownView(summary)
            }

            if !outcome.modifiedFiles.isEmpty {
                section("Modified files", count: outcome.modifiedFiles.count) {
                    FlowLayout(spacing: 6) {
                        ForEach(outcome.modifiedFiles, id: \.self) { path in
                            FileChip(path: path) { action in
                                reveal(path: path, action: action)
                            }
                        }
                    }
                }
            }

            if !outcome.unresolved.isEmpty {
                section("Unresolved", count: outcome.unresolved.count, tint: Theme.warning, systemImage: Icons.warning) {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(outcome.unresolved.enumerated()), id: \.offset) { _, line in
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text("•")
                                    .accessibilityHidden(true)
                                Text(line)
                                    .fixedSize(horizontal: false, vertical: true)
                                    .selectableText()
                            }
                            .font(Theme.Font.secondary)
                            .foregroundStyle(Theme.warning)
                        }
                    }
                    .padding(.leading, 4)
                }
            }

            footer
        }
        // No box: the outcome reads as part of the transcript, aligned with
        // the reply text above it.
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Task \(outcome.status.label.lowercased())")
    }

    // MARK: - Sections

    private func section<Content: View>(_ title: String, count: Int?, tint: Color = Theme.textTertiary, systemImage: String? = nil, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 11, weight: .semibold))
                        .accessibilityHidden(true)
                }
                Text(count.map { "\(title) · \($0)" } ?? title)
            }
            .font(.system(size: 12, weight: .semibold))
            .foregroundStyle(tint)
            .textCase(nil)
            content()
        }
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 8) {
            Text(footerText)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 8)
            if footerHovering {
                Text("Finished \(absoluteFinish)")
                    .lineLimit(1)
                    .transition(.opacity)
            }
        }
        .font(Theme.Font.caption)
        .foregroundStyle(Theme.textTertiary)
        .monospacedDigit()
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { footerHovering = inside }
        }
        .help("Finished \(absoluteFinish)")
        .accessibilityLabel("\(footerText). Finished \(absoluteFinish)")
    }

    /// "42.1k tokens · 6m 12s"
    private var footerText: String {
        [
            "\(TranscriptFormat.tokens(outcome.usage.total)) tokens",
            TranscriptFormat.durationMs(outcome.durationMs),
        ].joined(separator: " · ")
    }

    private var absoluteFinish: String {
        item.date.formatted(date: .abbreviated, time: .shortened)
    }

    private var summary: String {
        outcome.summary.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func reveal(path: String, action: PathAction) {
        guard let project = appState.project(for: chat.projectId) ?? appState.selectedProject else { return }
        appState.reveal(path: path, in: project, action: action)
    }
}

// MARK: - File chip

/// A modified-file chip: click opens the file in the Editor tab, ⌘-click
/// opens it in the default editor, and the context menu offers those plus
/// Show in Changes, Reveal in Finder and Copy Path.
private struct FileChip: View {
    let path: String
    let perform: (PathAction) -> Void

    var body: some View {
        Button {
            perform(NSEvent.modifierFlags.contains(.command) ? .openWithEditor : .openInEditor)
        } label: {
            Chip(Self.displayName(path), systemImage: Icons.file)
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button("Show in Changes") { perform(.showInChanges) }
            Button("Open in Editor") { perform(.openInEditor) }
            Button("Reveal in Finder") { perform(.revealInFinder) }
            Button("Open with Default Editor") { perform(.openWithEditor) }
            Divider()
            Button("Copy Path") { perform(.copyPath) }
        }
        .help("\(path) — click to open in the Editor, ⌘-click to open with the default editor")
        .accessibilityLabel("Modified file \(path)")
        .accessibilityHint("Opens the file in the Editor tab")
    }

    /// Long paths keep their tail: "…/agent/src/runtime.rs".
    static func displayName(_ path: String, limit: Int = 48) -> String {
        guard path.count > limit else { return path }
        let parts = path.split(separator: "/").map(String.init)
        var tail: [String] = []
        var length = 0
        for part in parts.reversed() {
            let next = length + part.count + (tail.isEmpty ? 0 : 1)
            if next > limit - 2, !tail.isEmpty { break }
            tail.insert(part, at: 0)
            length = next
        }
        let joined = tail.joined(separator: "/")
        if joined.count > limit {
            return "…" + String(joined.suffix(limit - 1))
        }
        return tail.count == parts.count ? joined : "…/" + joined
    }
}

// MARK: - Flow layout

/// Wraps its children into rows (file chips).
private struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        return arrange(width: width, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let placement = arrange(width: bounds.width, subviews: subviews)
        for (index, origin) in placement.origins.enumerated() where subviews.indices.contains(index) {
            subviews[index].place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var maxX: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0, x + size.width > width {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
            maxX = max(maxX, x - spacing)
        }
        let height = subviews.isEmpty ? 0 : y + rowHeight
        return (CGSize(width: width.isFinite ? min(width, max(maxX, 0)) : maxX, height: height), origins)
    }
}

// MARK: - Previews

#Preview("Outcome cards") {
    let summary = SessionSummary(id: "s1", model: "gpt-4.1", mode: "desktop", title: "Fix spacing on cards", createdAt: "2026-10-21T10:00:00Z", updatedAt: "2026-10-21T10:00:00Z", activeTaskId: nil, taskCount: 1, lastStatus: .completed)
    let chat = ChatState(sessionId: "s1", projectId: "p1", summary: summary)
    let usage = TokenUsage(promptTokens: 38_400, completionTokens: 3_700, cachedPromptTokens: 12_000, reasoningTokens: nil, source: .reported)

    let completed = TaskOutcome(
        taskId: "t1",
        status: .completed,
        summary: "Tightened the card spacing to **12 pt** and aligned the chips.\n\n- Updated `CardView.swift`\n- Added a regression test",
        modifiedFiles: ["apps/macos/ContextAgent/Views/Chat/CardView.swift", "crates/agent/src/runtime.rs", "README.md"],
        validation: [ValidationRecord(command: "cargo test", passed: true, exitCode: 0, durationMs: 2_140, summary: "42 passed; 0 failed")],
        unresolved: [],
        steps: 12, modelCalls: 6, toolCalls: 18,
        usage: usage, costUsd: 0.12, durationMs: 372_000
    )
    let blocked = TaskOutcome(
        taskId: "t2",
        status: .blocked,
        summary: "The migration needs a decision before it can continue.",
        modifiedFiles: ["migrations/0004_add_index.sql"],
        validation: [ValidationRecord(command: "cargo clippy", passed: false, exitCode: 101, durationMs: 9_800, summary: "error: unused import")],
        unresolved: ["Which database should the index target?", "The clippy warning in runtime.rs still fails the build"],
        steps: 4, modelCalls: 3, toolCalls: 5,
        usage: usage, costUsd: nil, durationMs: 48_000
    )
    let failed = TaskOutcome(
        taskId: "t3",
        status: .failed,
        summary: "runtime error: authentication failed",
        modifiedFiles: [], validation: [], unresolved: [],
        steps: 1, modelCalls: 1, toolCalls: 0,
        usage: .zero, costUsd: nil, durationMs: 800
    )

    return ScrollView {
        VStack(alignment: .leading, spacing: 20) {
            OutcomeCardView(item: OutcomeItem(outcome: completed, date: Date()), chat: chat)
            OutcomeCardView(item: OutcomeItem(outcome: blocked, date: Date().addingTimeInterval(-3_600)), chat: chat)
            OutcomeCardView(item: OutcomeItem(outcome: failed, date: Date().addingTimeInterval(-86_400)), chat: chat)
        }
        .padding(24)
        .frame(width: 640)
    }
    .frame(height: 900)
    .background(Theme.canvas)
    .environment(AppState())
}
