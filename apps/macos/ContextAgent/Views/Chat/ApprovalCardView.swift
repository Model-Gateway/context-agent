import SwiftUI

/// A pending approval: `warning` hairline card with the tool, a risk chip,
/// the one-line summary, the details in a code block (max 240 pt), the
/// policy's reason, and right-aligned *Deny* · *Approve for session* ·
/// *Approve*. While `approval/respond` is in flight the buttons give way to
/// a spinner; a failure renders inline. Once resolved the card collapses to
/// one muted line ("Approved · run: cargo test").
///
/// No keyboard shortcuts live here: `AppCommands` owns ⌘⏎ / ⌘⌫ and routes
/// them through `appState.primaryAction()` / `denyPendingApproval()`.
struct ApprovalCardView: View {
    var item: ApprovalItem
    var chat: ChatState

    @Environment(AppState.self) private var appState

    init(item: ApprovalItem, chat: ChatState) {
        self.item = item
        self.chat = chat
    }

    private var request: ApprovalRequest { item.request }
    private var meta: ToolMetadata { ToolMetadata.info(for: request.tool) }

    var body: some View {
        if item.isPending {
            pendingCard
        } else {
            resolvedLine
        }
    }

    // MARK: - Pending

    private var pendingCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: Icons.approval)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Theme.warning)
                    .accessibilityHidden(true)
                Text(request.isDiscountDrop ? "Lower discount" : "Approval needed")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 8)
                Chip(meta.label, systemImage: meta.icon)
                Chip(request.risk.label, style: .tinted(riskTint))
            }

            if !summary.isEmpty {
                Text(summary)
                    .font(Theme.Font.body)
                    .foregroundStyle(Theme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            }

            if !details.isEmpty {
                DetailsBlock(text: details, highlight: detailsHighlight)
            }

            if !reason.isEmpty {
                Text(reason)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            }

            if let error = chat.approvalError, !error.isEmpty {
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: Icons.warning)
                        .font(.system(size: 12, weight: .medium))
                        .accessibilityHidden(true)
                    Text("Could not send the decision: \(error)")
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.danger)
                .accessibilityLabel("Error: could not send the decision. \(error)")
            }

            buttons
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
                .strokeBorder(Theme.warning, lineWidth: 1)
        )
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Approval needed: \(summary)")
    }

    private var buttons: some View {
        HStack(spacing: 10) {
            Spacer(minLength: 0)
            if chat.approvalInFlight {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 12)
                    Text("Sending…")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                .padding(.trailing, 4)
            }
            if request.isDiscountDrop {
                Button("Cancel") { respond(.deny) }
                    .buttonStyle(.borderless)
                    .foregroundStyle(Theme.textSecondary)
                    .help("Stop this task")
                Button("Always continue") { respond(.approveForSession) }
                    .buttonStyle(.bordered)
                    .help("Continue, and stop asking (Settings → Spending)")
                Button("Continue") { respond(.approve) }
                    .buttonStyle(.borderedProminent)
                    .tint(Theme.accent)
                    .help("Use the less discounted providers for the rest of this task")
            } else {
                Button("Deny") { respond(.deny) }
                    .buttonStyle(.borderless)
                    .foregroundStyle(Theme.textSecondary)
                    .help("Do not run this action")
                Button("Approve for session") { respond(.approveForSession) }
                    .buttonStyle(.bordered)
                    .help("Approve this and identical actions for the rest of the session")
                Button("Approve") { respond(.approve) }
                    .buttonStyle(.borderedProminent)
                    .tint(Theme.accent)
                    .help("Run this action once")
            }
        }
        .controlSize(.regular)
        .disabled(chat.approvalInFlight)
    }

    private func respond(_ decision: ApprovalDecision) {
        guard !chat.approvalInFlight else { return }
        let requestId = request.id
        let target = chat
        Task { await appState.respond(approval: requestId, in: target, decision: decision) }
    }

    // MARK: - Resolved

    private var resolvedLine: some View {
        HStack(spacing: 8) {
            Image(systemName: item.decision == .deny ? Icons.failed : Icons.check)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(item.decision == .deny ? Theme.textTertiary : Theme.success)
                .frame(width: 24)
                .accessibilityHidden(true)
            Text(resolvedText)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .selectableText()
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .help(details.isEmpty ? resolvedText : details)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(resolvedText)
    }

    private var resolvedText: String {
        summary.isEmpty ? decisionLabel : "\(decisionLabel) · \(summary)"
    }

    private var decisionLabel: String {
        if request.isDiscountDrop {
            switch item.decision {
            case .approve?: return "Continued"
            case .approveForSession?: return "Always continue"
            case .deny?: return "Cancelled"
            case nil: return "Pending"
            }
        }
        switch item.decision {
        case .approve?: return "Approved"
        case .approveForSession?: return "Approved for session"
        case .deny?: return "Denied"
        case nil: return "Pending"
        }
    }

    // MARK: - Helpers

    private var summary: String { request.summary.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var details: String { request.details.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var reason: String { request.reason.trimmingCharacters(in: .whitespacesAndNewlines) }

    private var riskTint: Color {
        switch request.risk {
        case .readOnly: return Theme.textSecondary
        case .write: return Theme.warning
        case .execute: return Theme.warning
        case .network: return Theme.accent
        case .destructive: return Theme.danger
        }
    }

    private var detailsHighlight: SyntaxHighlighter.Language {
        switch request.tool {
        case "execute_command", "run_validation": return .shell
        case "apply_patch", "git_diff": return .diff
        default:
            let trimmed = details
            if trimmed.hasPrefix("diff --git") || trimmed.hasPrefix("@@") || trimmed.hasPrefix("--- ") { return .diff }
            if trimmed.hasPrefix("{") || trimmed.hasPrefix("[") { return .json }
            return .plain
        }
    }
}

// MARK: - Details block

/// The request's details (command text or patch preview) on `codeBackground`,
/// growing with the text up to 240 pt and scrolling beyond that.
private struct DetailsBlock: View {
    let text: String
    let highlight: SyntaxHighlighter.Language

    static let maxHeight: CGFloat = 240
    /// Longer blocks scroll inside the card; shorter ones sit inline so the
    /// transcript's wheel events are not captured by a nested scroll view.
    static let inlineLineLimit = 13

    var body: some View {
        let list = LineList(text: text, maxLines: 200, maxBytes: 16_384, highlight: highlight)
        let padded = list
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
        Group {
            if list.lineCount > Self.inlineLineLimit {
                ScrollView(.vertical) { padded }
                    .frame(height: Self.maxHeight)
            } else {
                padded
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
        .accessibilityLabel("Details")
    }
}

// MARK: - Previews

#Preview("Approval cards") {
    let summary = SessionSummary(id: "s1", model: "gpt-4.1", mode: "desktop", title: "Fix spacing on cards", createdAt: "2026-10-21T10:00:00Z", updatedAt: "2026-10-21T10:00:00Z", activeTaskId: nil, taskCount: 1, lastStatus: nil)
    let chat = ChatState(sessionId: "s1", projectId: "p1", summary: summary)
    let erroring = ChatState(sessionId: "s2", projectId: "p1", summary: summary)
    erroring.approvalError = "unknown request id approval_call_9"
    let busy = ChatState(sessionId: "s3", projectId: "p1", summary: summary)
    busy.approvalInFlight = true

    let command = ApprovalRequest(id: "approval_call_7", tool: "execute_command", risk: .execute, summary: "run: cargo test", details: "cargo test --workspace -- --nocapture", reason: "Shell commands need approval under the approval_required profile.")
    let patch = ApprovalRequest(id: "approval_call_8", tool: "apply_patch", risk: .write, summary: "edit src/app.rs (2 edits)", details: "--- src/app.rs\n+++ src/app.rs\n@@ -1,3 +1,3 @@\n-let x = 1;\n+let x = 2;\n let y = 3;", reason: "Edits outside the current task scope require approval.")
    let destructive = ApprovalRequest(id: "approval_call_9", tool: "execute_command", risk: .destructive, summary: "run: rm -rf target", details: "rm -rf target", reason: "Destructive commands always need approval.")

    return ScrollView {
        VStack(alignment: .leading, spacing: 20) {
            ApprovalCardView(item: ApprovalItem(request: command), chat: chat)
            ApprovalCardView(item: ApprovalItem(request: patch), chat: busy)
            ApprovalCardView(item: ApprovalItem(request: destructive), chat: erroring)
            ApprovalCardView(item: ApprovalItem(request: command, decision: .approve), chat: chat)
            ApprovalCardView(item: ApprovalItem(request: patch, decision: .approveForSession), chat: chat)
            ApprovalCardView(item: ApprovalItem(request: destructive, decision: .deny), chat: chat)
        }
        .padding(24)
        .frame(width: 640)
    }
    .frame(height: 820)
    .background(Theme.canvas)
    .environment(AppState())
}
