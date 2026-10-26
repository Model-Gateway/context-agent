import SwiftUI
import AppKit
import UniformTypeIdentifiers

/// The chat composer (ARCHITECTURE.md "Composer"): a floating `surface` card
/// with the attachment chips (files dropped on the card or picked from the
/// + button), a growing `NSTextView`, the + button, the permissions chip (opens
/// `PermissionsPopover`), the reasoning-effort chip (`EffortPopover`), the model chip and the
/// send / stop / queue button. The context engine is always on unless
/// Settings → General turns it off. `ComposerView(chat:)` edits a live chat's draft and shows
/// its queue row; `ComposerView(draftFor:)` edits `appState.pendingDraft`
/// and `appState.pendingAttachments` for the empty state.
struct ComposerView: View {
    @Environment(AppState.self) private var appState

    private let chat: ChatState?
    private let draftProject: ProjectState?

    /// A live chat: binds to `chat.draft`, shows "Queued · N" and the stop button while it runs.
    init(chat: ChatState) {
        self.chat = chat
        self.draftProject = nil
    }

    /// The empty state: binds to `appState.pendingDraft[project.id]` and `pendingAttachments[project.id]`.
    init(draftFor project: ProjectState) {
        self.chat = nil
        self.draftProject = project
    }

    var body: some View {
        if let chat, let project = appState.project(for: chat.projectId) ?? appState.selectedProject {
            ChatComposer(chat: chat, project: project)
        } else if let draftProject {
            DraftComposer(project: draftProject)
        }
    }
}

// MARK: - Bindings

private struct ChatComposer: View {
    let chat: ChatState
    let project: ProjectState

    var body: some View {
        @Bindable var chat = chat
        VStack(alignment: .leading, spacing: 8) {
            if !chat.steering.isEmpty {
                SteeringRow(chat: chat)
            }
            if !chat.queued.isEmpty {
                QueueRow(chat: chat)
            }
            ComposerCard(project: project, chat: chat, text: $chat.draft, attachments: $chat.attachments)
        }
    }
}

private struct DraftComposer: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState

    var body: some View {
        let id = project.id
        ComposerCard(
            project: project,
            chat: nil,
            text: Binding(
                get: { appState.pendingDraft[id] ?? "" },
                set: { appState.pendingDraft[id] = $0 }
            ),
            attachments: Binding(
                get: { appState.pendingAttachments[id] ?? [] },
                set: { appState.pendingAttachments[id] = $0.isEmpty ? nil : $0 }
            )
        )
    }
}

// MARK: - The card

private struct ComposerCard: View {
    @Environment(AppState.self) private var appState
    let project: ProjectState
    let chat: ChatState?
    @Binding var text: String
    @Binding var attachments: [ComposerAttachment]

    @State private var measuredHeight: CGFloat = 0
    /// A file drag is over the card (SwiftUI drop area) or over the text view (`ComposerTextView`).
    @State private var cardDropTargeted = false
    @State private var textDropTargeted = false
    @State private var permissionsPresented = false
    @State private var effortSaving = false
    @State private var effortPresented = false
    @State private var mentions = MentionBridge()

    private enum PrimaryAction: Equatable {
        case send, stop, queue
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let hint = concurrentHint {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 10, tint: Theme.textTertiary)
                    Text(hint)
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                    if project.isGitRepository {
                        Text("·")
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textTertiary)
                        Button {
                            Task { await appState.sendDraftInNewWorktree() }
                        } label: {
                            Text(project.creatingWorktree ? "Creating a worktree…" : "Start in a new worktree instead")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.accent)
                                .lineLimit(1)
                        }
                        .buttonStyle(.plain)
                        .disabled(!canSubmit || project.creatingWorktree)
                        .help("Run this prompt in a second checkout of the repository, apart from the other chats (⇧⌘Return)")
                    }
                }
                .padding(.horizontal, 6)
            }
            card
        }
        .onChange(of: effortPresented) { _, shown in
            if !shown { appState.composerFocusToken += 1 }
        }
        .onChange(of: permissionsPresented) { _, shown in
            if !shown { appState.composerFocusToken += 1 }
        }
        .onChange(of: appState.modelPickerVisible) { _, shown in
            guard !shown else { return }
            // However the picker closed (choice, Esc, click outside, ⌘⇧M),
            // its hover card goes with it.
            HoverCardPanel.shared.hide()
            appState.composerFocusToken += 1
        }
    }

    private var card: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous)
        return VStack(alignment: .leading, spacing: 0) {
            if !attachments.isEmpty {
                if let visionWarning {
                    Label(visionWarning, systemImage: "exclamationmark.triangle")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.warning)
                        .padding(.horizontal, 14)
                        .padding(.top, 8)
                }
                AttachmentRow(attachments: attachments) { attachment in
                    attachments.removeAll { $0.id == attachment.id }
                }
                .padding(.horizontal, 12)
                .padding(.top, 10)
            }
            editor
            controls
        }
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .overlay {
            if cardDropTargeted || textDropTargeted {
                AttachmentDropHighlight()
            }
        }
        .floatingShadow()
        // The `@` app menu floats just above the card's top-left corner.
        .overlay(alignment: .topLeading) {
            MentionMenu(bridge: mentions, projectId: project.id)
                .alignmentGuide(.top) { d in d[.bottom] + 8 }
                .padding(.leading, 8)
        }
        .onChange(of: appState.mcpServers.map(\.name), initial: true) { _, names in
            mentions.installed = names
            if let textView = mentions.textView { mentions.highlight(in: textView) }
        }
        .task(id: appState.connection.isReady) {
            // The menu needs the apps' state and catalog titles and logos.
            guard appState.connection.isReady else { return }
            if appState.mcpServers.isEmpty { await appState.refreshMcpStatus() }
            await appState.loadMcpCatalog()
        }
        // Files dropped here attach to the draft; the window's own drop
        // target (open as project) only gets drops outside the composer.
        .onDrop(of: [UTType.fileURL], isTargeted: $cardDropTargeted) { providers in
            loadDroppedFiles(providers)
        }
        .animation(.easeOut(duration: 0.12), value: cardDropTargeted || textDropTargeted)
    }

    // MARK: Editor

    private var editor: some View {
        ZStack(alignment: .topLeading) {
            ComposerTextView(
                text: $text,
                measuredHeight: $measuredHeight,
                focusToken: appState.composerFocusToken,
                suggestion: suggestion,
                onSend: submit,
                onDropFiles: attach,
                onPaste: attachPasted,
                dropTargeted: $textDropTargeted,
                mentions: mentions
            )
            .frame(height: editorHeight)

            if text.isEmpty {
                // The suggested next message (→ accepts it), else the placeholder.
                HStack(spacing: 8) {
                    Text(verbatim: suggestion ?? (chat?.pendingQuestion != nil ? "Answer in your own words…" : chat?.isRunning == true ? "Steer the running task · ⌘Return to queue" : "Ask anything…"))
                        .font(Theme.Font.body)
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    if suggestion != nil {
                        Text("→")
                            .kbd()
                            .help("Press → to use this suggestion")
                    }
                }
                .padding(.top, ComposerTextView.verticalInset)
                .padding(.leading, ComposerTextView.horizontalPadding)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
        }
        .onChange(of: text) { _, value in
            // Typing (or accepting it) replaces the suggestion.
            if !value.isEmpty, chat?.suggestion != nil { chat?.suggestion = nil }
        }
        .padding(.horizontal, 14)
        .padding(.top, 10)
    }

    /// 1 → 8 lines: min(max(measured, line + 12), line * 8 + 12).
    private var editorHeight: CGFloat {
        let line = ComposerTextView.lineHeight
        let insets = ComposerTextView.verticalInset * 2
        return min(max(measuredHeight, line + insets), line * 8 + insets)
    }

    // MARK: Bottom row

    private var controls: some View {
        HStack(spacing: 6) {
            attachButton
            permissionsChip
            Spacer(minLength: 8)
            effortMenu
            modelChip
            primaryButton
        }
        .padding(.horizontal, 10)
        .padding(.top, 6)
        .padding(.bottom, 10)
    }

    private var attachButton: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return Button(action: chooseAttachments) {
            Image(systemName: Icons.attach)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Theme.textSecondary)
                .frame(width: 28, height: 28)
                .background(shape.strokeBorder(Theme.border, lineWidth: 1))
                .contentShape(shape)
        }
        .buttonStyle(.plain)
        .fixedSize()
        .help("Attach files · you can also drop or paste files and images on the composer")
        .accessibilityLabel("Attach files")
    }

    /// Opens `PermissionsPopover`; Bypass is tinted as a warning.
    private var permissionsChip: some View {
        let profile = project.info.permissionProfile
        let label = AppState.approvalModeLabel(profile)
        let tint = profile == "bypass_permissions" ? Theme.warning : Theme.textSecondary
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return Button {
            permissionsPresented = true
        } label: {
            HStack(spacing: 5) {
                Image(systemName: PermissionProfileOption.systemImage(for: profile))
                    .font(.system(size: 11, weight: .medium))
                Text(label)
                    .font(.system(size: 12.5, weight: .medium))
                    .lineLimit(1)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
            }
            .foregroundStyle(tint)
            .padding(.horizontal, 9)
            .frame(height: 28)
            .background(shape.strokeBorder(Theme.border, lineWidth: 1))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .hoverHighlight(radius: Theme.Metrics.radiusSmall)
        .fixedSize()
        .help("Permissions: \(label) · click to change")
        .accessibilityLabel("Permissions \(label)")
        .popover(isPresented: $permissionsPresented, arrowEdge: .top) {
            PermissionsPopover()
        }
    }

    /// Reasoning effort (`model.reasoning_effort`, all projects; idle
    /// projects pick it up at once, a running task keeps its own). Opens
    /// `EffortPopover`; Ultra (which also deploys sub-agents) shows in
    /// `Theme.ultra` purple.
    private var effortMenu: some View {
        let effort = appState.config?.model.reasoningEffort
        let level = EffortLevel.level(for: effort)
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        return Button {
            effortPresented = true
        } label: {
            HStack(spacing: 5) {
                if effortSaving {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                } else {
                    Image(systemName: level.isUltra ? Icons.agents : Icons.effort)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(level.isUltra ? Theme.ultra : Theme.textSecondary)
                }
                Text(level.title)
                    .font(.system(size: 13, weight: level.isUltra ? .semibold : .medium))
                    .foregroundStyle(level.tint)
                    .lineLimit(1)
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
            }
            .padding(.horizontal, 8)
            .frame(height: 28)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .fixedSize()
        .hoverHighlight(radius: Theme.Metrics.radiusSmall)
        .disabled(!appState.connection.isReady || effortSaving)
        .help("Reasoning effort for every project · Ultra also lets the model deploy sub-agents that explore and implement in parallel")
        .accessibilityLabel("Reasoning effort \(level.title)")
        .popover(isPresented: $effortPresented, arrowEdge: .top) {
            EffortPopover(current: effort, offered: EffortLevel.levels(for: appState.modelInfo(project.info.model)?.effortLevels)) { value in
                setEffort(value)
            }
        }
    }

    /// Presented through `appState.modelPickerVisible`, so ⌘⇧M and the
    /// self-test driver open the same popover as a click on the chip.
    private var modelChip: some View {
        let switching = project.switchingModel != nil
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        let presented = Binding(
            get: { appState.modelPickerVisible },
            set: { shown in if !shown { appState.modelPickerVisible = false } }
        )
        return Button {
            appState.modelPickerVisible = true
        } label: {
            HStack(spacing: 5) {
                if switching {
                    BrailleSpinner(size: 11, tint: Theme.textSecondary)
                } else {
                    ModelLogo(modelId: project.info.model, size: 13)
                }
                Text(verbatim: switching ? "Switching…" : appState.modelDisplayName(project.info.model))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(1)
                if !switching, let savings = appState.modelStat(project.info.model, in: project)?.savingsLabel {
                    Text(verbatim: savings)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Theme.success)
                        .lineLimit(1)
                        .fixedSize()
                }
                Image(systemName: Icons.chevronDown)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
            }
            .padding(.horizontal, 8)
            .frame(height: 28)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .hoverHighlight(radius: Theme.Metrics.radiusSmall)
        .help("Project model")
        .accessibilityLabel("Model \(appState.modelDisplayName(project.info.model))" + (appState.modelStat(project.info.model, in: project)?.savingsLabel.map { ", \($0)" } ?? ""))
        .popover(isPresented: presented, arrowEdge: .top) {
            ModelPickerPopover()
        }
    }

    private var primaryButton: some View {
        let action = primaryAction
        let cancelling = chat?.cancelRequested == true
        let disabled = action == .stop ? cancelling : !canSubmit
        let label = primaryLabel(for: action, cancelling: cancelling)
        return Button {
            switch action {
            case .stop:
                Task { await appState.cancelRunningTask() }
            case .send, .queue:
                submit()
            }
        } label: {
            ZStack {
                Circle().fill(Theme.accentSoft)
                if action == .stop, cancelling {
                    BrailleSpinner(size: 12, tint: Theme.accent)
                } else {
                    // The circle stays solid (reference); only the glyph dims while inert.
                    Image(systemName: icon(for: action))
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(Theme.accent.opacity(disabled ? 0.45 : 1))
                }
            }
            .frame(width: 32, height: 32)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .disabled(disabled)
        .help(label)
        .accessibilityLabel(label)
    }

    // MARK: Derived

    private var primaryAction: PrimaryAction {
        if let chat, chat.isRunning { return .stop }
        // Other chats of the project run alongside; only this chat's own
        // run (still settling) makes a prompt wait.
        if let chat, project.isRunning(session: chat.sessionId) { return .queue }
        return .send
    }

    /// The chat's suggested next message while nothing is typed or running.
    private var suggestion: String? {
        guard let chat, !chat.isRunning, text.isEmpty else { return nil }
        return chat.suggestion
    }

    private var trimmedText: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var canSubmit: Bool {
        (!trimmedText.isEmpty || !attachments.isEmpty) && project.switchingModel == nil
    }

    /// "“<title>” is also working in this checkout" while other chats of
    /// this project run (their agents are told about each other's files).
    private var concurrentHint: String? {
        let others = project.runningSessionIds.subtracting(chat.map { [$0.sessionId] } ?? []).sorted()
        guard let first = others.first else { return nil }
        let title = project.chatTitle(first) ?? "another chat"
        let more = others.count > 1 ? " and \(others.count - 1) more" : ""
        return "“\(title)”\(more) \(others.count > 1 ? "are" : "is") also working in this checkout"
    }

    private func icon(for action: PrimaryAction) -> String {
        switch action {
        case .send: return Icons.send
        case .stop: return Icons.stop
        case .queue: return Icons.queued
        }
    }

    private func primaryLabel(for action: PrimaryAction, cancelling: Bool) -> String {
        switch action {
        case .send:
            return project.switchingModel != nil ? "Send (unavailable while switching the model)" : "Send (Return)"
        case .stop:
            return cancelling ? "Cancelling…" : "Stop the running task (⌘.)"
        case .queue:
            return "Queue (this chat is still finishing its run)"
        }
    }

    // MARK: Actions

    /// Return / send button: `AppState.send` creates the session when needed
    /// and, while this chat runs, sends the prompt to the running turn
    /// (⌘Return, `AppState.primaryAction`, queues it instead). Attachments go along as
    /// an "Attached files:" list of path references and, where the model can
    /// take them, as images and text (`AttachmentUpload`).
    private func submit() {
        guard canSubmit else { return }
        let (text, attachments, root) = (trimmedText, attachments, project.root)
        Task { await appState.send(await AttachmentUpload.prompt(text: text, attachments: attachments, root: root)) }
    }

    private func setEffort(_ value: String) {
        guard value != (appState.config?.model.reasoningEffort ?? ""), !effortSaving else { return }
        effortSaving = true
        Task {
            defer { effortSaving = false }
            do {
                try await appState.patchConfig(["model.reasoning_effort": value.isEmpty ? .null : .string(value)])
            } catch {
                appState.toasts.show("Could not change the reasoning effort: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    private func clearDraft() {
        text = ""
        attachments = []
        appState.composerFocusToken += 1
    }

    private func attach(_ urls: [URL]) {
        attachments = ComposerAttachment.appending(urls, to: attachments)
    }

    /// ⌘V of an image (a screenshot, "Copy Image") or PDF pages: saved by
    /// `PastedFiles`, then a chip.
    private func attachPasted(_ content: PastedContent) {
        Task {
            do {
                let url = try await Task.detached(priority: .userInitiated) {
                    switch content {
                    case .image(let data): try PastedFiles.saveImage(data)
                    case .pdf(let data): try PastedFiles.savePDF(data)
                    }
                }.value
                attachments = ComposerAttachment.appending([url], to: attachments)
            } catch {
                appState.toasts.show("Could not paste: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    /// Shown over the chips when an image is attached and models.dev says the
    /// current model takes text only.
    private var visionWarning: String? {
        guard attachments.contains(where: \.isImage), let info = appState.modelInfo(project.info.model), !info.vision else { return nil }
        return "\(info.name) may not accept images"
    }

    /// File URLs from a drop on the card; false (drop refused) without any.
    private func loadDroppedFiles(_ providers: [NSItemProvider]) -> Bool {
        let candidates = providers.filter { $0.canLoadObject(ofClass: URL.self) }
        guard !candidates.isEmpty else { return false }
        for provider in candidates {
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url, url.isFileURL else { return }
                Task { @MainActor in attach([url]) }
            }
        }
        appState.composerFocusToken += 1
        return true
    }

    /// NSOpenPanel → attachment chips.
    private func chooseAttachments() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = true
        panel.canCreateDirectories = false
        panel.directoryURL = project.rootURL
        panel.prompt = "Attach"
        panel.message = "Choose files to attach to the prompt"
        Task { @MainActor in
            let response = await panel.begin()
            if response == .OK {
                attach(panel.urls)
            }
            appState.composerFocusToken += 1
        }
    }
}

// MARK: - Small controls

/// "Steering · N": prompts sent to the running turn that the model reads
/// at its next step; they cannot be taken back.
private struct SteeringRow: View {
    let chat: ChatState

    var body: some View {
        HStack(spacing: 8) {
            Text("Steering · \(chat.steering.count)")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textSecondary)
                .fixedSize()
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(Array(chat.steering.enumerated()), id: \.offset) { _, item in
                        QueuedItem(text: item.text, attachmentCount: item.images.count + item.files.count)
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .padding(.horizontal, 4)
        .help("The agent reads these at its next step")
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Messages for the running task")
    }
}

/// "Queued · N" with one chip per queued prompt: click edits (moves the
/// text back into the composer), × removes.
private struct QueueRow: View {
    @Environment(AppState.self) private var appState
    let chat: ChatState

    var body: some View {
        HStack(spacing: 8) {
            Text("Queued · \(chat.queued.count)")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textSecondary)
                .fixedSize()
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(Array(chat.queued.enumerated()), id: \.offset) { index, item in
                        QueuedItem(
                            text: item.text,
                            attachmentCount: item.images.count + item.files.count,
                            edit: { appState.editQueued(at: index, in: chat) },
                            remove: { appState.removeQueued(at: index, in: chat) }
                        )
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .padding(.horizontal, 4)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Queued prompts")
    }
}

/// A queued prompt (edit, remove) or, without actions, a steering one.
private struct QueuedItem: View {
    let text: String
    var attachmentCount = 0
    var edit: (() -> Void)? = nil
    var remove: (() -> Void)? = nil

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        HStack(spacing: 2) {
            Button { edit?() } label: {
                HStack(spacing: 4) {
                    if attachmentCount > 0 {
                        Image(systemName: Icons.attach)
                            .font(.system(size: 10))
                            .foregroundStyle(Theme.textSecondary)
                            .accessibilityLabel("\(attachmentCount) attachment\(attachmentCount == 1 ? "" : "s")")
                    }
                    Text(ChatState.clip(text, 48))
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                }
                .padding(.leading, 8)
                .padding(.trailing, remove == nil ? 8 : 0)
                .padding(.vertical, 4)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(edit == nil)
            .help(edit == nil ? "" : "Click to edit")
            .accessibilityLabel(edit == nil ? text : "Queued: \(text). Click to edit")

            if let remove {
                IconButton(Icons.close, label: "Remove from queue", size: 9, action: remove)
            }
        }
        .padding(.trailing, remove == nil ? 0 : 2)
        .background(shape.fill(hovering && edit != nil ? Theme.surfaceRaised : Theme.surface))
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
    }
}

// MARK: - ComposerTextView

/// The composer's `NSTextView`: Return sends, Shift-Return inserts a newline,
/// IME composition is left alone, the height is measured after every change
/// (`measuredHeight`), and a change of `focusToken` makes it first responder.
/// Files dragged onto it go to `onDropFiles` instead of being pasted as paths;
/// pasted files, images and PDFs become attachments too (`FileDropTextView.paste`).
struct ComposerTextView: NSViewRepresentable {
    @Binding var text: String
    @Binding var measuredHeight: CGFloat
    var focusToken: Int
    /// Ghost text shown while the field is empty; → inserts it.
    var suggestion: String? = nil
    var onSend: () -> Void
    var onDropFiles: ([URL]) -> Void = { _ in }
    var onPaste: (PastedContent) -> Void = { _ in }
    @Binding var dropTargeted: Bool
    /// `@` app mentions: the word under the cursor, keys while the menu is open.
    var mentions: MentionBridge? = nil

    static let font = NSFont.systemFont(ofSize: 14)
    /// `textContainerInset.height`.
    static let verticalInset: CGFloat = 6
    /// `NSTextContainer.lineFragmentPadding` (default 5); the placeholder overlay matches it.
    static let horizontalPadding: CGFloat = 5
    static let lineHeight: CGFloat = {
        let layout = NSLayoutManager()
        return ceil(layout.defaultLineHeight(for: font))
    }()

    func makeCoordinator() -> Coordinator {
        Coordinator(parent: self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = FileDropTextView.scrollableTextView()
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.focusRingType = .none

        if let textView = scrollView.documentView as? FileDropTextView {
            let coordinator = context.coordinator
            textView.onDropFiles = { [weak coordinator] urls in coordinator?.parent.onDropFiles(urls) }
            textView.onPaste = { [weak coordinator] content in coordinator?.parent.onPaste(content) }
            textView.onDropTargeted = { [weak coordinator] targeted in
                guard let coordinator, coordinator.parent.dropTargeted != targeted else { return }
                coordinator.parent.dropTargeted = targeted
            }
            textView.delegate = context.coordinator
            textView.drawsBackground = false
            textView.isRichText = false
            textView.allowsUndo = true
            textView.font = Self.font
            textView.textColor = Theme.nsTextPrimary
            textView.textContainerInset = NSSize(width: 0, height: Self.verticalInset)
            textView.isAutomaticQuoteSubstitutionEnabled = false
            textView.isAutomaticDashSubstitutionEnabled = false
            textView.isAutomaticTextReplacementEnabled = false
            textView.isAutomaticSpellingCorrectionEnabled = false
            textView.isAutomaticLinkDetectionEnabled = false
            textView.isAutomaticDataDetectionEnabled = false
            textView.focusRingType = .none
            textView.setAccessibilityLabel("Message")
            textView.string = text
            context.coordinator.attach(textView)
            mentions?.textView = textView
            mentions?.highlight(in: textView)
        }
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        let coordinator = context.coordinator
        coordinator.parent = self
        guard let textView = coordinator.textView else { return }

        if textView.string != text {
            coordinator.isSyncing = true
            textView.string = text
            textView.font = Self.font
            textView.setSelectedRange(NSRange(location: (text as NSString).length, length: 0))
            coordinator.isSyncing = false
            coordinator.measure(deferred: true)
            mentions?.highlight(in: textView)
        }
        mentions?.textView = textView

        if coordinator.lastFocusToken != focusToken {
            coordinator.lastFocusToken = focusToken
            coordinator.focus()
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerTextView
        weak var textView: NSTextView?
        /// nil until the first update, so the token the parent bumped before
        /// this view appeared (new chat, selection) still focuses it.
        var lastFocusToken: Int?
        var isSyncing = false

        private var frameObserver: NSObjectProtocol?

        init(parent: ComposerTextView) {
            self.parent = parent
        }

        deinit {
            if let frameObserver {
                NotificationCenter.default.removeObserver(frameObserver)
            }
        }

        func attach(_ textView: NSTextView) {
            self.textView = textView
            textView.postsFrameChangedNotifications = true
            // Width changes (window resize) change the wrapped line count.
            frameObserver = NotificationCenter.default.addObserver(
                forName: NSView.frameDidChangeNotification, object: textView, queue: .main
            ) { [weak self] _ in
                self?.measure(deferred: true)
            }
            measure(deferred: true)
        }

        /// Content height incl. insets → `measuredHeight` (deferred when called
        /// during a SwiftUI update, so state is never modified mid-render).
        func measure(deferred: Bool) {
            guard let textView,
                  let layoutManager = textView.layoutManager,
                  let container = textView.textContainer else { return }
            layoutManager.ensureLayout(for: container)
            let used = layoutManager.usedRect(for: container)
            let height = ceil(used.height) + textView.textContainerInset.height * 2
            guard abs(parent.measuredHeight - height) > 0.5 else { return }
            if deferred {
                DispatchQueue.main.async { [weak self] in
                    guard let self, abs(self.parent.measuredHeight - height) > 0.5 else { return }
                    self.parent.measuredHeight = height
                }
            } else {
                parent.measuredHeight = height
            }
        }

        /// `makeFirstResponder` once the view is in a window (retried briefly
        /// right after creation, when `window` is still nil).
        func focus(attempt: Int = 0) {
            DispatchQueue.main.async { [weak self] in
                guard let self, let textView = self.textView else { return }
                if let window = textView.window {
                    window.makeFirstResponder(textView)
                } else if attempt < 3 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
                        self?.focus(attempt: attempt + 1)
                    }
                }
            }
        }

        // MARK: NSTextViewDelegate

        func textDidChange(_ notification: Notification) {
            guard !isSyncing, let textView else { return }
            parent.text = textView.string
            measure(deferred: false)
            parent.mentions?.highlight(in: textView)
            parent.mentions?.update(from: textView)
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard !isSyncing, let textView else { return }
            parent.mentions?.update(from: textView)
        }

        func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            // The `@` menu owns ↑/↓, Return, Tab and Esc while it is open.
            if let mentions = parent.mentions, mentions.isOpen, !textView.hasMarkedText() {
                switch commandSelector {
                case #selector(NSResponder.moveUp(_:)):
                    mentions.move(-1)
                    return true
                case #selector(NSResponder.moveDown(_:)):
                    mentions.move(1)
                    return true
                case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.insertTab(_:)):
                    mentions.acceptSelected()
                    return true
                case #selector(NSResponder.cancelOperation(_:)):
                    mentions.dismiss()
                    return true
                default:
                    break
                }
            }
            // Esc (once the palette and the picker had their chance in
            // AppCommands' monitor): collapse the selection instead of
            // NSTextView's completion list. Esc never answers an approval.
            if commandSelector == #selector(NSResponder.cancelOperation(_:)) || commandSelector == #selector(NSResponder.complete(_:)) {
                if textView.hasMarkedText() { return false }
                let end = textView.selectedRange().upperBound
                textView.setSelectedRange(NSRange(location: end, length: 0))
                return true
            }
            // → in the empty field takes the suggested next message (undoable).
            if commandSelector == #selector(NSResponder.moveRight(_:)), textView.string.isEmpty, !textView.hasMarkedText(),
               let suggestion = parent.suggestion, !suggestion.isEmpty {
                textView.insertText(suggestion, replacementRange: NSRange(location: 0, length: 0))
                return true
            }
            guard commandSelector == #selector(NSResponder.insertNewline(_:)) else { return false }
            // An IME composition commits with Return: let the input method have it.
            if textView.hasMarkedText() { return false }
            if NSApp?.currentEvent?.modifierFlags.contains(.shift) == true {
                textView.insertNewlineIgnoringFieldEditor(nil)
                return true
            }
            parent.onSend()
            return true
        }
    }
}

// MARK: - FileDropTextView

/// Non-text pasteboard content the composer turns into an attachment.
enum PastedContent: Sendable {
    case image(Data)
    case pdf(Data)
}

/// The composer's text view. A drag carrying file URLs is taken whole and
/// handed to `onDropFiles` (the composer turns them into attachment chips);
/// every other drag (text) behaves as in any `NSTextView`. A text view does
/// not pass refused drags up to the SwiftUI drop target behind it, so it
/// handles files itself and reports hovering through `onDropTargeted`.
/// Pasting works the same way: files copied in Finder go to `onDropFiles`
/// and an image or PDF without text (a screenshot, "Copy Image", pages
/// copied from Preview) to `onPaste`.
final class FileDropTextView: NSTextView {
    var onDropFiles: (([URL]) -> Void)?
    var onPaste: ((PastedContent) -> Void)?
    var onDropTargeted: ((Bool) -> Void)?

    override func paste(_ sender: Any?) {
        if pasteAttachments(from: .general) { return }
        super.paste(sender)
    }

    /// Text wins over an image when both are there (cells copied from a
    /// spreadsheet carry a picture of themselves too).
    private func pasteAttachments(from pasteboard: NSPasteboard) -> Bool {
        let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        if !urls.isEmpty, let onDropFiles {
            onDropFiles(urls)
            return true
        }
        guard pasteboard.string(forType: .string) == nil, let onPaste else { return false }
        if let data = PastedFiles.imageData(from: pasteboard) {
            onPaste(.image(data))
        } else if let data = PastedFiles.pdfData(from: pasteboard) {
            onPaste(.pdf(data))
        } else {
            return false
        }
        return true
    }

    /// A plain-text view disables Paste when the pasteboard holds no text;
    /// an image (or a copied file) is pasteable here.
    private func canPasteAttachment(_ action: Selector?) -> Bool {
        guard action == #selector(paste(_:)), isEditable, onPaste != nil else { return false }
        return PastedFiles.hasContent(.general) || NSPasteboard.general.availableType(from: [.fileURL]) != nil
    }

    override func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        canPasteAttachment(item.action) || super.validateUserInterfaceItem(item)
    }

    override func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        canPasteAttachment(menuItem.action) || super.validateMenuItem(menuItem)
    }

    private static func fileURLs(_ info: NSDraggingInfo) -> [URL] {
        let urls = info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
        return urls ?? []
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard !Self.fileURLs(sender).isEmpty else { return super.draggingEntered(sender) }
        onDropTargeted?(true)
        return .copy
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard !Self.fileURLs(sender).isEmpty else { return super.draggingUpdated(sender) }
        return .copy
    }

    override func draggingExited(_ sender: NSDraggingInfo?) {
        onDropTargeted?(false)
        super.draggingExited(sender)
    }

    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool {
        Self.fileURLs(sender).isEmpty ? super.prepareForDragOperation(sender) : true
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let urls = Self.fileURLs(sender)
        guard !urls.isEmpty else { return super.performDragOperation(sender) }
        onDropTargeted?(false)
        onDropFiles?(urls)
        window?.makeFirstResponder(self)
        return true
    }

    override func concludeDragOperation(_ sender: NSDraggingInfo?) {
        if let sender, !Self.fileURLs(sender).isEmpty { return }
        super.concludeDragOperation(sender)
    }

    override func draggingEnded(_ sender: NSDraggingInfo) {
        onDropTargeted?(false)
        super.draggingEnded(sender)
    }
}

// MARK: - Previews

@MainActor
private enum ComposerPreview {
    static func info(running sessionId: String? = nil) -> ProjectInfo {
        ProjectInfo(
            projectId: "a1b2c3d4e5f6a1b2c3d4e5f6",
            root: "/Users/me/Projects/context-agent",
            name: "context-agent",
            git: GitInfo(branch: "main", head: "862959a1c0ffee", dirtyFiles: 3),
            index: IndexStats(generation: 12, files: 310, symbols: 4102, chunks: 5110, relationships: 900, embeddings: 5110, lastFullIndexAt: nil, languages: []),
            indexing: false,
            model: "gpt-4.1",
            permissionProfile: "approval_required",
            sandbox: "none",
            validationCommands: [],
            languages: [],
            buildFiles: ["Cargo.toml"],
            fileCount: 310,
            runningTask: sessionId.map { RunningTask(sessionId: $0, taskId: "task_1", startedAt: "2025-10-21T10:00:00Z") }
        )
    }

    static func session(_ id: String, title: String) -> SessionSummary {
        SessionSummary(id: id, model: "gpt-4.1", mode: "desktop", title: title, createdAt: "2025-10-21T10:00:00Z", updatedAt: "2025-10-21T10:05:00Z", activeTaskId: nil, taskCount: 1, lastStatus: nil)
    }

    static func make(running sessionId: String? = nil) -> (app: AppState, project: ProjectState) {
        let defaults = UserDefaults(suiteName: "dev.contextagent.desktop.preview") ?? .standard
        let preferences = Preferences(defaults: defaults)
        let app = AppState(preferences: preferences, backend: BackendClient(), recents: RecentProjectStore(), toasts: ToastCenter(), notifier: TaskNotifier())
        let project = ProjectState(info: info(running: sessionId), models: ModelCatalog(preferences: preferences, baseURL: "https://api.openai.com/v1"))
        project.sessions = [session("s1", title: "Fix spacing on cards"), session("s2", title: "Need better empty state")]
        app.projects = [project]
        app.selectedProjectId = project.id
        return (app, project)
    }
}

#Preview("Empty state draft") {
    let fixture = ComposerPreview.make()
    return VStack {
        ComposerView(draftFor: fixture.project)
    }
    .padding(32)
    .frame(width: 760, height: 220)
    .background(Theme.canvas)
    .environment(fixture.app)
}

#Preview("Live chat, queued, another chat running") {
    let fixture = ComposerPreview.make(running: "s1")
    let chat = fixture.project.chat(for: "s2")
    chat.draft = "Explain how the retrieval budget is computed\nand where it is enforced."
    chat.queued = [OutgoingPrompt(text: "Then run cargo test"), OutgoingPrompt(text: "Summarize the diff in three bullets")]
    fixture.app.selectedSessionId = "s2"
    return VStack {
        ComposerView(chat: chat)
    }
    .padding(32)
    .frame(width: 760, height: 260)
    .background(Theme.canvas)
    .environment(fixture.app)
}

#Preview("This chat running (stop)") {
    let fixture = ComposerPreview.make(running: "s1")
    let chat = fixture.project.chat(for: "s1")
    chat.status = .running(RunProgress(state: .investigating, step: 3, startedAt: Date().addingTimeInterval(-42)))
    fixture.app.selectedSessionId = "s1"
    return VStack {
        ComposerView(chat: chat)
    }
    .padding(32)
    .frame(width: 760, height: 200)
    .background(Theme.canvas)
    .environment(fixture.app)
}
