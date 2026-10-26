import SwiftUI
import AppKit

/// Copies `text` to the general pasteboard; icon only, the glyph turns into
/// a green checkmark for 1.2 s (no label, so the button never changes size).
struct CopyButton: View {
    var text: String
    var label: String

    @State private var copied = false
    @State private var hovering = false
    @State private var resetTask: Task<Void, Never>?

    init(text: String, label: String = "Copy") {
        self.text = text
        self.label = label
    }

    var body: some View {
        Button(action: copy) {
            Image(systemName: copied ? Icons.copied : Icons.copy)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(foreground)
                .frame(width: 28, height: 28)
            .background(
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(hovering || copied ? Theme.surfaceRaised : Color.clear)
            )
            .contentShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(label)
        .accessibilityLabel(copied ? "Copied" : label)
        .onDisappear { resetTask?.cancel() }
    }

    private var foreground: Color {
        if copied { return Theme.success }
        return hovering ? Theme.textPrimary : Theme.textSecondary
    }

    private func copy() {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        withAnimation(.easeOut(duration: 0.15)) { copied = true }
        resetTask?.cancel()
        resetTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(1.2))
            guard !Task.isCancelled else { return }
            withAnimation(.easeOut(duration: 0.15)) { copied = false }
        }
    }
}

#Preview("Copy button") {
    HStack(spacing: 12) {
        CopyButton(text: "cargo build --release -p ctx-cli")
        CopyButton(text: "# Title", label: "Copy as Markdown")
    }
    .padding(24)
    .background(Theme.canvas)
}
