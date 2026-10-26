import SwiftUI

// The Changes tab's sheets: push (remote, branch, upstream, force-with-lease,
// git's output inline), a new branch, and committing one file.

/// Push: remote picker, branch, "Set as upstream" (on when the branch has
/// none), "Force with lease", git's output or error inline; Push (⏎) runs
/// `git/push` and closes on success.
struct PushSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    let project: ProjectState
    let status: GitStatus

    @State private var remote: String
    @State private var branch: String
    @State private var setUpstream: Bool
    @State private var force = false
    @State private var output: String?
    @State private var error: String?
    @State private var pushing = false

    init(project: ProjectState, status: GitStatus) {
        self.project = project
        self.status = status
        let remotes = status.remotes ?? []
        let upstreamRemote = status.upstream.flatMap { $0.split(separator: "/", maxSplits: 1).first.map(String.init) }
        _remote = State(initialValue: upstreamRemote ?? remotes.first?.name ?? "origin")
        _branch = State(initialValue: status.branch ?? "")
        _setUpstream = State(initialValue: status.upstream == nil)
    }

    private var remotes: [GitRemote] { status.remotes ?? [] }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: Icons.push)
                    .font(.system(size: 22, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: 32, height: 32)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Push")
                        .font(Theme.Font.heading)
                        .foregroundStyle(Theme.textPrimary)
                    Text(summary)
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 10) {
                GridRow {
                    Text("Remote")
                        .foregroundStyle(Theme.textSecondary)
                    if remotes.isEmpty {
                        TextField("origin", text: $remote)
                            .textFieldStyle(.roundedBorder)
                    } else {
                        Picker("Remote", selection: $remote) {
                            ForEach(remotes) { remote in
                                Text(verbatim: "\(remote.name)  ·  \(remote.url)")
                                    .tag(remote.name)
                            }
                        }
                        .labelsHidden()
                    }
                }
                GridRow {
                    Text("Branch")
                        .foregroundStyle(Theme.textSecondary)
                    TextField("branch", text: $branch)
                        .textFieldStyle(.roundedBorder)
                }
                GridRow {
                    Color.clear.frame(width: 1, height: 1)
                    VStack(alignment: .leading, spacing: 6) {
                        Toggle("Set as upstream (git push -u)", isOn: $setUpstream)
                        Toggle("Force with lease", isOn: $force)
                        if force {
                            Text("Overwrites the remote branch unless someone else pushed since you last fetched.")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.warning)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
            .font(Theme.Font.secondary)

            if remotes.isEmpty {
                Text("This repository has no remote yet; add one with `git remote add origin <url>` first, or type its name above.")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.warning)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if let error {
                outputBox(error, isError: true)
            } else if let output {
                outputBox(output, isError: false)
            }

            HStack(spacing: 8) {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                if pushing {
                    BrailleSpinner(size: 13, tint: Theme.textSecondary)
                        .padding(.leading, 4)
                }
                Spacer(minLength: 0)
                Button(force ? "Force Push" : "Push") { push() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(pushing || remote.trimmingCharacters(in: .whitespaces).isEmpty || branch.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 480)
        .background(Theme.surface)
    }

    private var summary: String {
        var parts: [String] = []
        if let ahead = status.ahead, ahead > 0 { parts.append("\(ahead) commit\(ahead == 1 ? "" : "s") to push") }
        if let behind = status.behind, behind > 0 { parts.append("\(behind) behind \(status.upstream ?? "upstream")") }
        if parts.isEmpty {
            return status.upstream.map { "Push \(status.branch ?? "HEAD") to \($0)." } ?? "Publish \(status.branch ?? "this branch") to a remote."
        }
        return parts.joined(separator: " · ") + "."
    }

    private func outputBox(_ text: String, isError: Bool) -> some View {
        ScrollView {
            Text(verbatim: text)
                .font(Theme.Font.monoSmall)
                .foregroundStyle(isError ? Theme.danger : Theme.textSecondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .selectableText()
                .padding(8)
        }
        .frame(maxHeight: 140)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
    }

    private func push() {
        pushing = true
        error = nil
        output = nil
        let remote = remote.trimmingCharacters(in: .whitespaces)
        let branch = branch.trimmingCharacters(in: .whitespaces)
        Task {
            defer { pushing = false }
            do {
                let text = try await appState.push(remote: remote, branch: branch, setUpstream: setUpstream, force: force, in: project)
                appState.toasts.show("Pushed \(branch) to \(remote).", kind: .success, details: text.isEmpty ? nil : text)
                dismiss()
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

/// New branch: a name, created from HEAD and switched to.
struct NewBranchSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    let project: ProjectState

    @State private var name = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("New Branch")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            Text("Created from the current commit and checked out. Uncommitted changes come along.")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            TextField("feature/name", text: $name)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit { create() }
            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer(minLength: 0)
                Button("Create and Switch") { create() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 400)
        .background(Theme.surface)
        .onAppear { focused = true }
    }

    private func create() {
        let branch = name.trimmingCharacters(in: .whitespaces)
        guard !branch.isEmpty else { return }
        dismiss()
        Task { await appState.checkout(branch, create: true, in: project) }
    }
}

/// Commit one file (staged first, whatever else is staged), with its own
/// message; the default message names the file.
struct CommitFileSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    let project: ProjectState
    let path: String

    @State private var message: String
    @State private var committing = false
    @FocusState private var focused: Bool

    init(project: ProjectState, path: String) {
        self.project = project
        self.path = path
        let name = (path as NSString).lastPathComponent
        let verb: String
        switch project.gitStatus?.files.first(where: { $0.path == path })?.code {
        case "A"?, "?"?: verb = "Add"
        case "D"?: verb = "Remove"
        default: verb = "Update"
        }
        _message = State(initialValue: "\(verb) \(name)")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Commit \((path as NSString).lastPathComponent)")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            Text(verbatim: path)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Text("Only this file is committed; other staged or changed files stay as they are.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            TextEditor(text: $message)
                .font(Theme.Font.secondary)
                .scrollContentBackground(.hidden)
                .focused($focused)
                .frame(minHeight: 60, maxHeight: 140)
                .padding(6)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                if committing {
                    BrailleSpinner(size: 13, tint: Theme.textSecondary)
                }
                Spacer(minLength: 0)
                Button("Commit") { commit() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(committing || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 440)
        .background(Theme.surface)
        .onAppear { focused = true }
    }

    private func commit() {
        committing = true
        let text = message.trimmingCharacters(in: .whitespacesAndNewlines)
        Task {
            defer { committing = false }
            if await appState.commit(message: text, paths: [path], in: project) != nil {
                dismiss()
            }
        }
    }
}

/// The Inspector's Changes card "Commit…": commits what is staged, or every
/// change when nothing is. Edits the Changes tab's draft (`commitMessage`),
/// so a message written in either place shows up in the other.
struct CommitChangesSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    let project: ProjectState

    @State private var committing = false
    @FocusState private var focused: Bool

    private var stagedCount: Int { project.gitStatus?.stagedFiles.count ?? 0 }
    private var fileCount: Int { project.gitStatus?.files.count ?? 0 }
    /// Nothing staged: stage everything first.
    private var commitAll: Bool { stagedCount == 0 }

    var body: some View {
        @Bindable var project = project
        VStack(alignment: .leading, spacing: 14) {
            Text("Commit Changes")
                .font(Theme.Font.heading)
                .foregroundStyle(Theme.textPrimary)
            Text(commitAll
                 ? "Nothing is staged: all \(fileCount) changed file\(fileCount == 1 ? "" : "s") are staged and committed on \(project.branch ?? "the current branch")."
                 : "The \(stagedCount) staged file\(stagedCount == 1 ? "" : "s") are committed on \(project.branch ?? "the current branch"); unstaged changes stay as they are.")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            TextEditor(text: $project.commitMessage)
                .font(Theme.Font.secondary)
                .scrollContentBackground(.hidden)
                .focused($focused)
                .frame(minHeight: 60, maxHeight: 140)
                .padding(6)
                .background(Theme.canvas, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                        .strokeBorder(Theme.border, lineWidth: 1)
                )
            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button {
                    Task { await appState.generateCommitMessage(in: project) }
                } label: {
                    Label("Generate", systemImage: "sparkles")
                }
                .disabled(project.generatingMessage || committing)
                .help("Write the message from the changes with the project's model")
                if committing || project.generatingMessage {
                    BrailleSpinner(size: 13, tint: Theme.textSecondary)
                }
                Spacer(minLength: 0)
                Button(commitAll ? "Commit All" : "Commit") { commit() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
                    .disabled(committing || project.generatingMessage || project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(20)
        .frame(width: 440)
        .background(Theme.surface)
        .onAppear { focused = true }
    }

    private func commit() {
        committing = true
        let text = project.commitMessage.trimmingCharacters(in: .whitespacesAndNewlines)
        Task {
            defer { committing = false }
            if await appState.commit(message: text, all: commitAll, in: project) != nil {
                project.commitMessage = ""
                dismiss()
            }
        }
    }
}
