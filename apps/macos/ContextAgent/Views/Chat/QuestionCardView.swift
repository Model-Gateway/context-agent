import SwiftUI

/// The agent's `ask_user` questions, above the composer while the run waits
/// for them: each question with its header chip and choices (radio rows, or
/// checkboxes for `multi_select`) plus an "Other" field for the user's own
/// words. A single single-choice question is answered by clicking a choice;
/// otherwise *Submit* sends every answer at once. *Skip* tells the agent to
/// decide by itself, and typing in the composer below answers in free text
/// (`AppState.send`). While `question/respond` is in flight the buttons give
/// way to a spinner; a failure renders inline.
struct QuestionCardView: View {
    var request: QuestionRequest
    var chat: ChatState

    @Environment(AppState.self) private var appState
    /// Chosen option labels per question index.
    @State private var selected: [Int: Set<String>] = [:]
    /// The "Other" text per question index.
    @State private var other: [Int: String] = [:]

    private var questions: [AgentQuestion] { request.questions }

    /// One question with one answer: a click on a choice is the answer.
    private var answersOnClick: Bool {
        questions.count == 1 && !questions[0].multiSelect && !questions[0].options.isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Image(systemName: "questionmark.bubble")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Theme.warning)
                    .accessibilityHidden(true)
                Text(questions.count == 1 ? "The agent has a question" : "The agent has \(questions.count) questions")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                Spacer(minLength: 8)
            }

            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(Array(questions.enumerated()), id: \.offset) { index, question in
                        questionView(question, index: index)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 360)
            .fixedSize(horizontal: false, vertical: true)

            if let error = chat.questionError, !error.isEmpty {
                HStack(alignment: .top, spacing: 6) {
                    Image(systemName: Icons.warning)
                        .font(.system(size: 12, weight: .medium))
                        .accessibilityHidden(true)
                    Text("Could not send the answer: \(error)")
                        .fixedSize(horizontal: false, vertical: true)
                        .selectableText()
                }
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.danger)
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
        .disabled(chat.questionInFlight)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("The agent asks: " + questions.map(\.question).joined(separator: " "))
    }

    // MARK: - One question

    private func questionView(_ question: AgentQuestion, index: Int) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if let header = question.header?.trimmingCharacters(in: .whitespacesAndNewlines), !header.isEmpty {
                    Chip(header)
                }
                Text(verbatim: question.question)
                    .font(Theme.Font.body.weight(.medium))
                    .foregroundStyle(Theme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            }
            if question.multiSelect {
                Text("Choose any that apply")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(question.options.enumerated()), id: \.offset) { optionIndex, option in
                    OptionRow(
                        number: optionIndex + 1,
                        option: option,
                        multiSelect: question.multiSelect,
                        isSelected: selected[index, default: []].contains(option.label)
                    ) {
                        toggle(option.label, question: question, index: index)
                    }
                }
                otherField(question, index: index)
            }
        }
    }

    private func otherField(_ question: AgentQuestion, index: Int) -> some View {
        let binding = Binding(
            get: { other[index] ?? "" },
            set: { value in
                other[index] = value
                // Typing an own answer to a single-choice question replaces the choice.
                if !question.multiSelect, !value.isEmpty { selected[index] = [] }
            }
        )
        return TextField(question.options.isEmpty ? "Your answer" : "Other…", text: binding, axis: .vertical)
            .textFieldStyle(.plain)
            .font(Theme.Font.secondary)
            .lineLimit(1...4)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Theme.surfaceRaised, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
            .onSubmit(submit)
            .accessibilityLabel("Your own answer to: \(question.question)")
    }

    private func toggle(_ label: String, question: AgentQuestion, index: Int) {
        var set = selected[index, default: []]
        if question.multiSelect {
            if set.contains(label) { set.remove(label) } else { set.insert(label) }
        } else {
            set = set.contains(label) ? [] : [label]
            other[index] = nil
        }
        selected[index] = set
        if answersOnClick, !set.isEmpty { submit() }
    }

    // MARK: - Buttons

    private var answers: [QuestionAnswer] {
        questions.indices.map { index in
            let chosen = questions[index].options.map(\.label).filter { selected[index, default: []].contains($0) }
            let text = other[index]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return QuestionAnswer(selected: chosen, other: text.isEmpty ? nil : text)
        }
    }

    private var canSubmit: Bool { answers.contains { !$0.isEmpty } }

    private var buttons: some View {
        HStack(spacing: 10) {
            Text("Or reply in the message box below")
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textTertiary)
            Spacer(minLength: 0)
            if chat.questionInFlight {
                HStack(spacing: 6) {
                    BrailleSpinner(size: 12)
                    Text("Sending…")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                .padding(.trailing, 4)
            }
            Button("Skip") { send(answers: nil) }
                .buttonStyle(.borderless)
                .foregroundStyle(Theme.textSecondary)
                .help("Let the agent decide by itself")
            if !answersOnClick || other[0].map({ !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) == true {
                Button("Submit", action: submit)
                    .buttonStyle(.borderedProminent)
                    .tint(Theme.accent)
                    .disabled(!canSubmit)
                    .help("Send your answers to the agent")
            }
        }
        .controlSize(.regular)
    }

    private func submit() {
        guard canSubmit else { return }
        send(answers: answers)
    }

    private func send(answers: [QuestionAnswer]?) {
        guard !chat.questionInFlight else { return }
        let requestId = request.requestId
        let target = chat
        Task { await appState.respond(question: requestId, in: target, answers: answers) }
    }
}

// MARK: - Option row

/// One choice: a radio or checkbox mark, the label and its description.
private struct OptionRow: View {
    let number: Int
    let option: AgentQuestion.Option
    let multiSelect: Bool
    let isSelected: Bool
    let action: () -> Void

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        Button(action: action) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: mark)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(isSelected ? Theme.accent : Theme.textTertiary)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: option.label)
                        .font(Theme.Font.secondary.weight(.medium))
                        .foregroundStyle(Theme.textPrimary)
                    if let description = option.description?.trimmingCharacters(in: .whitespacesAndNewlines), !description.isEmpty {
                        Text(verbatim: description)
                            .font(Theme.Font.caption)
                            .foregroundStyle(Theme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
                Text("\(number)")
                    .font(Theme.Font.caption.monospacedDigit())
                    .foregroundStyle(Theme.textTertiary)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(shape.fill(isSelected ? Theme.accentSoft : hovering ? Theme.surfaceRaised : Color.clear))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .accessibilityLabel(option.label + (option.description.map { ". " + $0 } ?? ""))
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }

    private var mark: String {
        if multiSelect { return isSelected ? "checkmark.square.fill" : "square" }
        return isSelected ? "largecircle.fill.circle" : "circle"
    }
}

// MARK: - Preview

#Preview("Questions") {
    let summary = SessionSummary(id: "s1", model: "gpt-4.1", mode: "desktop", title: "Add a cache", createdAt: "2026-10-21T10:00:00Z", updatedAt: "2026-10-21T10:00:00Z", activeTaskId: nil, taskCount: 1, lastStatus: nil)
    let chat = ChatState(sessionId: "s1", projectId: "p1", summary: summary)
    let request = QuestionRequest(projectId: "p1", sessionId: "s1", requestId: "question_0", questions: [
        AgentQuestion(question: "Where should the cache live?", header: "Storage", options: [
            .init(label: "In memory (Recommended)", description: "Fastest; lost on restart"),
            .init(label: "SQLite", description: "Survives restarts; one more table"),
        ]),
        AgentQuestion(question: "Which platforms matter?", options: [.init(label: "macOS"), .init(label: "Linux"), .init(label: "Windows")], multiSelect: true),
    ])
    return QuestionCardView(request: request, chat: chat)
        .padding(24)
        .frame(width: 640)
        .background(Theme.canvas)
        .environment(AppState())
}
