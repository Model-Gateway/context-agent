import SwiftUI
import AppKit

// Parallel sessions through linked git worktrees (docs/desktop-protocol.md
// "Worktrees"). The backend runs one task per project root, so a second
// prompt in a busy project queues; `worktree/create` gives the repository a
// second checkout on its own `ctx/<name>` branch, opened as a project of its
// own (own run slot, index and chats) and nested under the parent in the
// sidebar. Nothing waits, and the two never edit the same files.

extension AppState {
    /// Creates a worktree of `project`'s repository, opens it and selects
    /// its empty chat. `name` becomes the folder and branch name.
    @discardableResult
    func newWorktree(in project: ProjectState, name: String? = nil, select: Bool = true) async -> ProjectState? {
        guard connection.isReady else {
            toasts.show("The backend is not running.", kind: .error)
            return nil
        }
        guard project.isGitRepository else {
            toasts.show("\(project.name) is not a Git repository, so it cannot have worktrees.", kind: .warning)
            return nil
        }
        if project.creatingWorktree { return nil }
        project.creatingWorktree = true
        defer { project.creatingWorktree = false }
        do {
            let info = try await backend.worktreeCreate(projectId: project.id, name: name)
            let worktree = register(info)
            await restorePermissionProfile(worktree)
            if select {
                self.select(project: worktree.id, restoreSession: false)
                newChat()
            }
            startLoads(for: worktree)
            // The parent's worktree list and the chip in the sidebar.
            Task { await project.refreshGit(backend: backend) }
            return worktree
        } catch {
            toasts.show("Could not create a worktree: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            return nil
        }
    }

    /// `newWorktree` for the selected project (⌘⇧N, the sidebar, the palette).
    func newWorktreeChat() {
        guard let project = selectedProject else {
            sidebarItem = .welcome
            return
        }
        Task { await newWorktree(in: project) }
    }

    /// Sends the composer's current draft from a new worktree instead of
    /// queueing it behind the running task: the draft (with its attachments)
    /// moves to the worktree's first chat and runs there at once.
    func sendDraftInNewWorktree() async {
        guard let project = selectedProject else { return }
        let text: String
        let attachments: [ComposerAttachment]
        if let chat = selectedChat {
            text = chat.draft
            attachments = chat.attachments
        } else {
            text = pendingDraft[project.id] ?? ""
            attachments = pendingAttachments[project.id] ?? []
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !attachments.isEmpty else { return }
        let prompt = await AttachmentUpload.prompt(text: trimmed, attachments: attachments, root: project.root)
        guard let worktree = await newWorktree(in: project, name: Self.worktreeName(for: trimmed)) else { return }
        // The draft left the busy project with the prompt.
        if let chat = selectedChatIn(project) {
            chat.draft = ""
            chat.attachments = []
        }
        pendingDraft[project.id] = nil
        pendingAttachments[project.id] = nil
        guard selectedProjectId == worktree.id else { return }
        await send(prompt)
    }

    /// Opens a worktree of the repository that is not open yet (`worktree/list`).
    func openWorktree(_ entry: WorktreeEntry) async {
        await openProject(url: URL(fileURLWithPath: entry.path), select: true, restoreSession: true)
    }

    /// Removes a worktree's checkout (uncommitted changes in it are lost) and,
    /// on request, its branch. Refused by the backend while a task runs there.
    func removeWorktree(_ project: ProjectState, deleteBranch: Bool) async {
        guard project.isWorktree else { return }
        let parent = parentProject(of: project)
        gitRefreshTasks[project.id]?.cancel()
        gitRefreshTasks[project.id] = nil
        terminals.removeValue(forKey: project.id)?.terminateAll()
        browsers.removeValue(forKey: project.id)?.tearDown()
        do {
            let result = try await backend.worktreeRemove(projectId: project.id, deleteBranch: deleteBranch, force: true)
            forget(project)
            if let parent {
                if selectedProjectId == nil { select(project: parent.id) }
                Task { await parent.refreshGit(backend: backend) }
            }
            let branch = result.branch.map { " (branch \($0)\(result.branchDeleted ? " deleted" : " kept"))" } ?? ""
            toasts.show("Removed worktree \(project.worktreeLabel)\(branch).", kind: .info)
        } catch {
            toasts.show("Could not remove the worktree: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    /// Merges the worktree's branch into its parent checkout (a merge commit,
    /// `--no-ff`) and, with `deleteAfter`, removes the worktree and its
    /// branch. The backend refuses while either project runs, with
    /// uncommitted changes in the worktree, and on a conflict (the merge is
    /// aborted); the toast carries git's reason.
    func mergeWorktree(_ project: ProjectState, deleteAfter: Bool) async {
        guard project.isWorktree else { return }
        let parent = parentProject(of: project)
        let target = parent?.branch ?? "the parent branch"
        do {
            let result = try await backend.worktreeMerge(projectId: project.id, deleteAfter: deleteAfter)
            if result.removed {
                gitRefreshTasks[project.id]?.cancel()
                gitRefreshTasks[project.id] = nil
                terminals.removeValue(forKey: project.id)?.terminateAll()
                browsers.removeValue(forKey: project.id)?.tearDown()
                forget(project)
                if selectedProjectId == nil, let parent { select(project: parent.id) }
            }
            if let parent {
                Task {
                    await parent.refreshGit(backend: backend)
                    await self.loadCommitLog(parent)
                    await parent.refreshStatus(backend: backend)
                }
            }
            let removed = result.removed ? " and removed the worktree" : ""
            toasts.show("Merged \(project.worktreeLabel) into \(target) (\(result.shortMerged))\(removed).", kind: .success, details: result.subject)
        } catch {
            toasts.show("Could not merge \(project.worktreeLabel) into \(target): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            if let parent { Task { await parent.refreshGit(backend: backend) } }
        }
    }

    /// Drops a project the backend has already closed.
    private func forget(_ project: ProjectState) {
        projects.removeAll { $0.id == project.id }
        preferences.forgetOpenProject(path: project.root)
        if selectedProjectId == project.id {
            selectedProjectId = nil
            selectedSessionId = nil
        }
        refreshAttention()
    }

    private func selectedChatIn(_ project: ProjectState) -> ChatState? {
        guard let selectedSessionId else { return nil }
        return project.existingChat(for: selectedSessionId)
    }

    /// The first few words of the prompt as the worktree's name; the backend
    /// slugifies it and picks a random pair of words when nothing is left.
    nonisolated static func worktreeName(for prompt: String) -> String? {
        let words = prompt.split(whereSeparator: { $0.isWhitespace || $0.isNewline })
            .map { $0.trimmingCharacters(in: .punctuationCharacters) }
            .filter { !$0.isEmpty && $0.rangeOfCharacter(from: .alphanumerics) != nil }
            .prefix(4)
        let name = words.joined(separator: " ")
        return name.isEmpty ? nil : name
    }
}
