import SwiftUI

/// A fenced code block: language label top-left, copy button top-right on
/// hover, horizontally scrolling highlighted lines on `codeBackground`.
/// Shell blocks also get a play button that runs them in a new terminal tab
/// of the selected project.
struct CodeBlockView: View {
    var code: String
    var language: String?

    /// Optional so previews and other hosts without an AppState still render.
    @Environment(AppState.self) private var appState: AppState?
    @State private var hovering = false

    init(code: String, language: String?) {
        self.code = code
        self.language = language
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .center) {
                Text(label)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                // Invisible until hovered, but always reachable by VoiceOver.
                HStack(spacing: 2) {
                    if detected == .shell, let appState, appState.selectedProject != nil, !command.isEmpty {
                        RunButton {
                            appState.runInTerminal(command)
                        }
                    }
                    CopyButton(text: code, label: "Copy code")
                }
                .opacity(hovering ? 1 : 0)
            }
            .frame(height: 24)

            ScrollView(.horizontal, showsIndicators: false) {
                LineList(text: code, highlight: detected)
                    .padding(.trailing, 8)
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
                .strokeBorder(Theme.border, lineWidth: 1)
        )
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
    }

    private var detected: SyntaxHighlighter.Language {
        SyntaxHighlighter.Language.detect(language)
    }

    /// The block as commands: with `$ ` prompts, only the prompted lines
    /// (output lines in console transcripts are dropped).
    private var command: String {
        let lines = code.components(separatedBy: "\n")
        let prompted = lines.filter { $0.hasPrefix("$ ") }
        let commands = prompted.isEmpty ? lines : prompted.map { String($0.dropFirst(2)) }
        return commands.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var label: String {
        if detected != .plain { return detected.displayName }
        guard let language else { return "Code" }
        let trimmed = language.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "Code" }
        return trimmed.prefix(1).uppercased() + trimmed.dropFirst()
    }
}

/// The play button beside Copy, styled like `CopyButton`.
private struct RunButton: View {
    var action: () -> Void

    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            Image(systemName: Icons.run)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(hovering ? Theme.success : Theme.textSecondary)
                .scaleEffect(hovering ? 1.08 : 1)
                .frame(width: 28, height: 28)
                .background(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .fill(hovering ? Theme.surfaceRaised : Color.clear)
                )
                .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help("Run in a new terminal tab")
        .accessibilityLabel("Run in terminal")
    }
}

#Preview("Code blocks") {
    VStack(alignment: .leading, spacing: 16) {
        CodeBlockView(code: "{\n  \"taskId\": \"11\",\n  \"status\": \"completed\"\n}", language: "json")
        CodeBlockView(code: "fn main() {\n    let answer = 42; // the answer\n    println!(\"{}\", answer);\n}", language: "rust")
        CodeBlockView(code: "cargo build --release -p ctx-cli && ./target/release/ctx serve --mock --data-dir /tmp/ctx-data-with-a-rather-long-path", language: "sh")
        CodeBlockView(code: "plain text\nwithout a language", language: nil)
    }
    .padding(24)
    .frame(width: 560)
    .background(Theme.canvas)
}
