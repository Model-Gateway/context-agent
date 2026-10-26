import SwiftUI

/// An edit of a sent message or an undo of a turn, waiting on the user's
/// choice in `RewindSheet`: keep the files as they are now, or revert them
/// to the snapshot taken before that message.
struct RewindRequest: Identifiable {
    enum Mode: Equatable {
        /// Re-send `newText` in place of the message.
        case edit(newText: String)
        /// Remove the turn; the message goes back into the composer.
        case undo
    }

    let id = UUID()
    let chat: ChatState
    let turn: UserTurn
    let mode: Mode
    /// Messages the user typed after this one (they go too).
    let laterMessages: Int
    /// Files changed from this message on, as the transcript recorded them.
    let changedFiles: [String]

    @MainActor
    init(chat: ChatState, turn: UserTurn, at index: Int, mode: Mode) {
        self.chat = chat
        self.turn = turn
        self.mode = mode
        let scope = chat.transcript.rewindScope(from: index)
        laterMessages = scope.laterMessages
        changedFiles = scope.changedFiles
    }
}

/// The keep-or-revert modal: what the rewind removes, which files changed
/// since, and the two ways out (plus Cancel). Revert is the default when a
/// snapshot exists; without one only Keep is possible.
struct RewindSheet: View {
    let request: RewindRequest

    @Environment(AppState.self) private var appState

    private static let maxListedFiles = 8

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: Icons.undo)
                    .font(.system(size: 22, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .frame(width: 32, height: 32)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 4) {
                    Text(title)
                        .font(Theme.Font.heading)
                        .foregroundStyle(Theme.textPrimary)
                    Text(subtitle)
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Label(messagesLine, systemImage: Icons.chat)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                Label(filesLine, systemImage: Icons.file)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textPrimary)
                if !request.changedFiles.isEmpty {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(request.changedFiles.prefix(Self.maxListedFiles), id: \.self) { path in
                            Text(verbatim: path)
                                .font(Theme.Font.mono)
                                .foregroundStyle(Theme.textSecondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                        if request.changedFiles.count > Self.maxListedFiles {
                            Text("+ \(request.changedFiles.count - Self.maxListedFiles) more")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                        }
                    }
                    .padding(.leading, 24)
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))

            Text(choiceHint)
                .font(Theme.Font.caption)
                .foregroundStyle(request.turn.hasSnapshot ? Theme.textTertiary : Theme.warning)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 8) {
                Button("Cancel") {
                    appState.rewindRequest = nil
                }
                .keyboardShortcut(.cancelAction)
                if appState.rewindInFlight {
                    BrailleSpinner(size: 13, tint: Theme.textSecondary)
                        .padding(.leading, 4)
                }
                Spacer(minLength: 0)
                if request.turn.hasSnapshot {
                    Button("Keep files") { perform(restoreFiles: false) }
                    Button("Revert files") { perform(restoreFiles: true) }
                        .keyboardShortcut(.defaultAction)
                } else {
                    Button("Keep files") { perform(restoreFiles: false) }
                        .keyboardShortcut(.defaultAction)
                }
            }
            .disabled(appState.rewindInFlight)
        }
        .padding(20)
        .frame(width: 460)
        .background(Theme.surface)
    }

    private var title: String {
        switch request.mode {
        case .edit: return "Edit this message?"
        case .undo: return "Undo this turn?"
        }
    }

    private var subtitle: String {
        let quoted = ChatState.clip(request.turn.text, 72)
        switch request.mode {
        case .edit: return "The chat goes back to before “\(quoted)” and the edited message is sent in its place."
        case .undo: return "The chat goes back to before “\(quoted)”; the message returns to the composer."
        }
    }

    private var messagesLine: String {
        switch request.laterMessages {
        case 0: return "This message and its reply are removed."
        case 1: return "This message, its reply and 1 later message are removed."
        default: return "This message, its reply and \(request.laterMessages) later messages are removed."
        }
    }

    private var filesLine: String {
        switch request.changedFiles.count {
        case 0: return "No file changes were recorded after this message."
        case 1: return "1 file changed since then:"
        default: return "\(request.changedFiles.count) files changed since then:"
        }
    }

    private var choiceHint: String {
        if request.turn.hasSnapshot {
            return "Revert puts the project files back to how they were before this message (ignored files and commits are untouched). Keep leaves them as they are now."
        }
        return "No snapshot was taken before this message (the project had no Git repository), so the files can only be kept."
    }

    private func perform(restoreFiles: Bool) {
        let request = request
        Task { await appState.performRewind(request, restoreFiles: restoreFiles) }
    }
}
