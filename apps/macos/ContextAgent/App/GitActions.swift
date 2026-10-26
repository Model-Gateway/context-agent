import SwiftUI
import AppKit

// The Changes tab's git actions: stage / unstage / discard, commit (box,
// per file, amend, undo), push / pull / fetch, branches, the log and commit
// message generation. Each action runs one backend `git/*` request under
// `project.gitBusy`, refreshes the status afterwards and reports failures
// as toasts; the views only decide when to ask for confirmation.

extension AppState {
    /// Runs one git action: refuses while another is in flight, shows the
    /// label meanwhile, refreshes the status afterwards (also on failure,
    /// since git may have done part of the work) and toasts errors.
    /// Returns whether `body` succeeded.
    @discardableResult
    private func gitAction(_ label: String, in project: ProjectState, refreshLog: Bool = false, _ body: () async throws -> Void) async -> Bool {
        if let busy = project.gitBusy {
            toasts.show("Wait for “\(busy)” to finish.", kind: .info)
            return false
        }
        project.gitBusy = label
        defer { project.gitBusy = nil }
        var ok = true
        do {
            try await body()
        } catch {
            ok = false
            toasts.show("\(label) failed: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
        await project.refreshGit(backend: backend)
        reconcileDiffMode(project)
        if refreshLog {
            await loadCommitLog(project)
            await loadBranches(project)
        }
        return ok
    }

    /// After staging / unstaging the selected file, show the diff that still
    /// exists for it (the file moved between the two sections).
    private func reconcileDiffMode(_ project: ProjectState) {
        guard let path = project.selectedChangePath, let file = project.gitStatus?.files.first(where: { $0.path == path }) else { return }
        let hasStaged = file.staged
        let hasUnstaged = !file.staged || file.status.count > 1
        if project.showStaged, !hasStaged, hasUnstaged {
            Task { await setShowStaged(false, in: project) }
        } else if !project.showStaged, hasStaged, !hasUnstaged {
            Task { await setShowStaged(true, in: project) }
        }
    }

    /// Row click in the Staged or Changes section: the diff follows the section.
    func selectChange(path: String, staged: Bool, in project: ProjectState) async {
        if project.showStaged != staged { project.showStaged = staged }
        await selectChange(path: path, in: project)
    }

    // MARK: Index

    /// Empty `paths` stages everything.
    func stage(_ paths: [String], in project: ProjectState) async {
        await gitAction(paths.isEmpty ? "Staging all changes" : "Staging", in: project) {
            try await backend.gitStage(projectId: project.id, paths: paths)
        }
    }

    func unstage(_ paths: [String], in project: ProjectState) async {
        await gitAction(paths.isEmpty ? "Unstaging all changes" : "Unstaging", in: project) {
            try await backend.gitUnstage(projectId: project.id, paths: paths)
        }
    }

    /// The views confirm first: tracked files go back to the index,
    /// untracked ones are deleted.
    func discard(_ paths: [String], in project: ProjectState) async {
        var result: GitDiscardResult?
        let ok = await gitAction(paths.isEmpty ? "Discarding all changes" : "Discarding changes", in: project) {
            result = try await backend.gitDiscard(projectId: project.id, paths: paths)
        }
        if ok, let result {
            var parts: [String] = []
            if result.restored > 0 { parts.append("restored \(result.restored) file\(result.restored == 1 ? "" : "s")") }
            if result.deleted > 0 { parts.append("deleted \(result.deleted) untracked file\(result.deleted == 1 ? "" : "s")") }
            toasts.show(parts.isEmpty ? "Nothing to discard." : "Discarded: " + parts.joined(separator: ", ") + ".", kind: .success)
        }
    }

    // MARK: Commits

    /// One commit; `paths` commits only those files (staged first), `all`
    /// stages everything first. Returns the commit on success.
    @discardableResult
    func commit(message: String, amend: Bool = false, all: Bool = false, paths: [String] = [], in project: ProjectState) async -> GitCommitResult? {
        var result: GitCommitResult?
        let ok = await gitAction(amend ? "Amending" : "Committing", in: project, refreshLog: true) {
            result = try await backend.gitCommit(projectId: project.id, message: message, amend: amend, all: all, paths: paths)
        }
        guard ok, let result else { return nil }
        toasts.show("\(amend ? "Amended" : "Committed") \(result.shortHash) · \(ChatState.clip(result.subject, 60))", kind: .success)
        return result
    }

    /// The commit box (button, ⌘⏎): nothing staged but changes present →
    /// asks "stage all and commit?" (the view shows it), unless `all`.
    func commitFromBox(in project: ProjectState, amend: Bool = false, all: Bool = false) async {
        let message = project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !message.isEmpty else {
            toasts.show("Enter a commit message first.", kind: .info)
            return
        }
        let status = project.gitStatus
        let nothingStaged = (status?.stagedFiles.isEmpty ?? true)
        if nothingStaged, !all, !amend {
            if status?.files.isEmpty ?? true {
                toasts.show("Nothing to commit: the working tree is clean.", kind: .info)
            } else {
                project.askCommitAll = true
            }
            return
        }
        if await commit(message: message, amend: amend, all: all, in: project) != nil {
            project.commitMessage = ""
        }
    }

    /// `git reset --soft HEAD~1`; the undone subject becomes the draft when the box is empty.
    func undoLastCommit(in project: ProjectState) async {
        var undone: GitCommitResult?
        let ok = await gitAction("Undoing last commit", in: project, refreshLog: true) {
            undone = try await backend.gitUndoCommit(projectId: project.id)
        }
        guard ok, let undone else { return }
        if project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            project.commitMessage = undone.subject
        }
        toasts.show("Undid \(undone.shortHash); its changes are staged again.", kind: .success)
    }

    /// Fills the commit box from the staged changes (or everything when
    /// nothing is staged) through the project's model; mock mode and a
    /// failed model call produce a heuristic message and say so.
    @discardableResult
    func generateCommitMessage(in project: ProjectState) async -> String? {
        guard !project.generatingMessage else { return nil }
        project.generatingMessage = true
        defer { project.generatingMessage = false }
        do {
            let generated = try await backend.gitCommitMessage(projectId: project.id)
            project.commitMessage = generated.message
            if let error = generated.error {
                toasts.show("The model could not write the message; using a summary of the files instead.", kind: .warning, details: error)
            } else if generated.source == "heuristic" {
                toasts.show("Mock mode: message summarized from the file list.", kind: .info)
            }
            return generated.message
        } catch {
            toasts.show("Could not generate a commit message: \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
            return nil
        }
    }

    // MARK: Remote

    /// Throws so the push sheet can show git's output inline.
    func push(remote: String?, branch: String?, setUpstream: Bool, force: Bool, in project: ProjectState) async throws -> String {
        if let busy = project.gitBusy {
            throw BackendError.rpc(code: -1, message: "wait for “\(busy)” to finish", kind: nil)
        }
        project.gitBusy = force ? "Force pushing" : "Pushing"
        defer { project.gitBusy = nil }
        do {
            let output = try await backend.gitPush(projectId: project.id, remote: remote, branch: branch, setUpstream: setUpstream, force: force)
            await project.refreshGit(backend: backend)
            await loadBranches(project)
            return output.output
        } catch {
            await project.refreshGit(backend: backend)
            throw error
        }
    }

    func pull(rebase: Bool, in project: ProjectState) async {
        var output = ""
        let ok = await gitAction(rebase ? "Pulling (rebase)" : "Pulling", in: project, refreshLog: true) {
            output = try await backend.gitPull(projectId: project.id, rebase: rebase).output
        }
        if ok {
            let line = output.split(separator: "\n").last.map(String.init) ?? "Pulled."
            toasts.show(ChatState.clip(line, 80), kind: .success, details: output.isEmpty ? nil : output)
        }
    }

    func fetch(in project: ProjectState) async {
        var output = ""
        let ok = await gitAction("Fetching", in: project, refreshLog: true) {
            output = try await backend.gitFetch(projectId: project.id).output
        }
        if ok {
            toasts.show("Fetched all remotes.", kind: .success, details: output.isEmpty ? nil : output)
        }
    }

    // MARK: Branches and log

    func loadBranches(_ project: ProjectState) async {
        guard project.isGitRepository else { return }
        if let branches = try? await backend.gitBranches(projectId: project.id) {
            project.branches = branches
        }
    }

    /// `git switch [-c]`; refused by the backend while a task runs.
    func checkout(_ branch: String, create: Bool, in project: ProjectState) async {
        let name = branch.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        var now: String?
        let ok = await gitAction(create ? "Creating branch" : "Switching branch", in: project, refreshLog: true) {
            now = try await backend.gitCheckout(projectId: project.id, branch: name, create: create)
        }
        if ok {
            toasts.show(create ? "Created and switched to \(now ?? name)." : "Switched to \(now ?? name).", kind: .success)
            await project.refreshStatus(backend: backend)
        }
    }

    func loadCommitLog(_ project: ProjectState) async {
        guard project.isGitRepository else { return }
        if let log = try? await backend.gitLog(projectId: project.id, limit: 20) {
            project.commitLog = log
        }
    }
}
