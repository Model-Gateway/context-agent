import SwiftUI
import AppKit
import UniformTypeIdentifiers

/// Every menu command and keyboard shortcut (ARCHITECTURE.md "Keyboard
/// shortcuts"). This is the only owner of ⌘⏎, ⌘⌫ and Esc handling, and of
/// File → Open Recent. Actions guard their own preconditions (no project, no
/// chat) instead of disabling menu items, so a stale menu never swallows a
/// shortcut.
struct AppCommands: Commands {
    let appState: AppState

    var body: some Commands {
        // Only in builds that update themselves (Developer ID releases).
        CommandGroup(after: .appInfo) {
            if appState.updater.isAvailable {
                Button("Check for Updates…") {
                    appState.updater.checkForUpdates()
                }
            }
        }

        // The app has no Settings window: ⌘, opens the profile page's settings tabs.
        CommandGroup(replacing: .appSettings) {
            Button("Settings…") {
                appState.openSettings()
                AppDelegate.ensureMainWindow()
            }
            .keyboardShortcut(",", modifiers: .command)
        }

        CommandGroup(replacing: .newItem) {
            Button("New Chat") {
                appState.newChat()
            }
            .keyboardShortcut("n", modifiers: .command)

            Button("New Chat in a Worktree") {
                appState.newWorktreeChat()
            }
            .keyboardShortcut("n", modifiers: [.command, .shift])

            Button("Open Project…") {
                Task { await appState.chooseProject() }
            }
            .keyboardShortcut("o", modifiers: .command)

            Menu("Open Recent") {
                let recents = appState.recentProjects
                if recents.isEmpty {
                    Text("No Recent Projects")
                } else {
                    ForEach(recents) { recent in
                        Button {
                            Task { await appState.openProject(url: recent.url) }
                        } label: {
                            Text(verbatim: recent.pinned ? "\(recent.name)  (pinned)" : recent.name)
                        }
                    }
                }
                Divider()
                Button("Clear Menu") {
                    appState.recents.clearUnpinned()
                }
            }

            Divider()

            Button("Export Chat as Markdown…") {
                AppCommands.exportChatAsMarkdown(appState: appState)
            }
        }

        CommandGroup(replacing: .saveItem) {
            Button("Save") {
                appState.saveSelectedDocument()
            }
            .keyboardShortcut("s", modifiers: .command)

            Button("Close Editor Tab") {
                appState.closeSelectedDocument()
            }
            .keyboardShortcut("w", modifiers: [.command, .shift])

            Divider()

            // VS Code's File → Auto Save; the same switch as Settings → General.
            Toggle("Auto Save", isOn: Binding(get: { appState.preferences.editorAutoSave }, set: { appState.setEditorAutoSave($0) }))
        }

        CommandGroup(after: .pasteboard) {
            Divider()
            Button("Copy Transcript as Markdown") {
                AppCommands.copyTranscript(appState: appState)
            }
            .keyboardShortcut("c", modifiers: [.command, .shift])

            Button("Find in Chat") {
                AppCommands.findInChat(appState: appState)
            }
            .keyboardShortcut("f", modifiers: .command)
        }

        CommandMenu("Chat") {
            Button("Send or Queue · Approve") {
                appState.primaryAction()
            }
            .keyboardShortcut(.return, modifiers: .command)

            Button("Send in a New Worktree") {
                Task { await appState.sendDraftInNewWorktree() }
            }
            .keyboardShortcut(.return, modifiers: [.command, .shift])

            Button("Deny Approval") {
                appState.denyPendingApproval()
            }
            .keyboardShortcut(.delete, modifiers: .command)

            Button("Cancel Running Task") {
                Task { await appState.cancelRunningTask() }
            }
            .keyboardShortcut(".", modifiers: .command)

            Divider()

            Button("Undo Last Turn…") {
                appState.undoLastTurn()
            }
            .keyboardShortcut("z", modifiers: [.command, .option])

            Divider()

            Button("Switch Model…") {
                appState.toggleModelPicker()
            }
            .keyboardShortcut("m", modifiers: [.command, .shift])

            Button("Rebuild Index") {
                Task { await appState.rebuildIndex() }
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])
        }

        CommandMenu("Go") {
            Button("Chat") {
                AppCommands.showTab(.chat, appState: appState)
            }
            .keyboardShortcut("1", modifiers: .command)

            Button("Changes") {
                AppCommands.showTab(.changes, appState: appState)
            }
            .keyboardShortcut("2", modifiers: .command)

            Button("Project") {
                AppCommands.showTab(.project, appState: appState)
            }
            .keyboardShortcut("3", modifiers: .command)

            Button("Editor") {
                AppCommands.showTab(.editor, appState: appState)
            }
            .keyboardShortcut("4", modifiers: .command)

            Divider()

            Button("Welcome") {
                appState.sidebarItem = .welcome
            }

            Button("Projects") {
                appState.sidebarItem = .projects
            }

            Button("Apps") {
                appState.sidebarItem = .apps
            }

            Button("Command Palette") {
                appState.paletteVisible.toggle()
            }
            .keyboardShortcut("k", modifiers: .command)
        }

        CommandGroup(after: .sidebar) {
            Button("Toggle Sidebar") {
                appState.sidebarVisible.toggle()
            }
            .keyboardShortcut("s", modifiers: [.command, .control])

            Button("Toggle Inspector") {
                appState.inspectorVisible.toggle()
            }
            .keyboardShortcut("i", modifiers: [.command, .shift])
        }
    }
}

// MARK: - Actions

extension AppCommands {
    /// ⌘1 / ⌘2 / ⌘3: a tab needs an open project.
    @MainActor
    static func showTab(_ item: SidebarItem, appState: AppState) {
        guard appState.selectedProject != nil else {
            appState.sidebarItem = .welcome
            return
        }
        appState.sidebarItem = item
        if item == .chat { appState.composerFocusToken += 1 }
    }

    /// ⌘F: the visible `ChatView` listens for this notification.
    @MainActor
    static func findInChat(appState: AppState) {
        guard appState.selectedChat != nil else { return }
        if appState.sidebarItem != .chat { appState.sidebarItem = .chat }
        NotificationCenter.default.post(name: ChatView.findNotification, object: nil)
    }

    /// ⌘⇧C: the whole transcript as Markdown on the pasteboard.
    @MainActor
    static func copyTranscript(appState: AppState) {
        guard let chat = appState.selectedChat else {
            appState.toasts.show("Select a chat to copy its transcript.", kind: .info)
            return
        }
        appState.copyToPasteboard(chat.transcript.markdown(title: chat.title))
        appState.toasts.show("Transcript copied as Markdown", kind: .success)
    }

    /// File → Export Chat as Markdown…: `NSSavePanel` → `Transcript.markdown`.
    @MainActor
    static func exportChatAsMarkdown(appState: AppState) {
        guard let chat = appState.selectedChat else {
            appState.toasts.show("Select a chat to export it.", kind: .info)
            return
        }
        let panel = NSSavePanel()
        panel.title = "Export Chat as Markdown"
        panel.prompt = "Export"
        panel.canCreateDirectories = true
        panel.isExtensionHidden = false
        if let markdown = UTType(filenameExtension: "md") {
            panel.allowedContentTypes = [markdown, .plainText]
        } else {
            panel.allowedContentTypes = [.plainText]
        }
        panel.nameFieldStringValue = fileName(for: chat.title) + ".md"
        let markdown = chat.transcript.markdown(title: chat.title)
        Task { @MainActor in
            let response = await panel.begin()
            guard response == .OK, let url = panel.url else { return }
            do {
                try markdown.data(using: .utf8)?.write(to: url, options: [.atomic])
                appState.toasts.show("Exported “\(url.lastPathComponent)”", kind: .success)
            } catch {
                appState.toasts.show("Could not export the chat: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            }
        }
    }

    /// "Fix spacing on cards" → "Fix spacing on cards"; path separators and
    /// control characters are dropped, long titles clipped.
    static func fileName(for title: String) -> String {
        let forbidden = CharacterSet(charactersIn: "/:\\").union(.controlCharacters).union(.newlines)
        let cleaned = title.unicodeScalars.filter { !forbidden.contains($0) }
        var name = String(String.UnicodeScalarView(cleaned)).trimmingCharacters(in: .whitespaces)
        if name.count > 60 { name = String(name.prefix(60)).trimmingCharacters(in: .whitespaces) }
        return name.isEmpty ? "Chat" : name
    }
}

// MARK: - Esc handling

extension AppCommands {
    @MainActor private static var keyMonitor: Any?

    /// Installs the window-level Esc monitor once (called from
    /// `MainWindow.onAppear`). Precedence: command palette → model picker →
    /// an inline message edit; anything else (tools popover, the composer's
    /// own cancel) keeps the event. Esc never answers an approval.
    @MainActor
    static func installKeyMonitor(appState: AppState) {
        guard keyMonitor == nil else { return }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak appState] event in
            guard let appState, event.keyCode == 53, event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty else {
                return event
            }
            if appState.paletteVisible {
                appState.paletteVisible = false
                appState.composerFocusToken += 1
                return nil
            }
            if appState.modelPickerVisible {
                appState.modelPickerVisible = false
                appState.composerFocusToken += 1
                return nil
            }
            if appState.rewindRequest == nil, let chat = appState.selectedChat, chat.editingTurnId != nil {
                appState.cancelEdit(in: chat)
                return nil
            }
            return event
        }
    }
}
