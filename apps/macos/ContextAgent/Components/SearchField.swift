import SwiftUI

/// Rounded search field: magnifier, placeholder, a clear button while there
/// is text and an optional keyboard hint ("⌘K") while it is empty.
struct SearchField: View {
    var placeholder: String
    @Binding var text: String
    var kbdHint: String?
    var onSubmit: (() -> Void)?

    @FocusState private var focused: Bool

    init(_ placeholder: String, text: Binding<String>, kbdHint: String? = nil, onSubmit: (() -> Void)? = nil) {
        self.placeholder = placeholder
        self._text = text
        self.kbdHint = kbdHint
        self.onSubmit = onSubmit
    }

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: Icons.search)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)

            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textPrimary)
                .focused($focused)
                .onSubmit { onSubmit?() }
                .accessibilityLabel(placeholder)

            if !text.isEmpty {
                Button {
                    text = ""
                } label: {
                    Image(systemName: Icons.clear)
                        .font(.system(size: 12))
                        .foregroundStyle(Theme.textTertiary)
                }
                .buttonStyle(.plain)
                .help("Clear")
                .accessibilityLabel("Clear search")
            } else if let kbdHint {
                Text(kbdHint)
                    .kbd()
                    .accessibilityHidden(true)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 30)
        .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
                .strokeBorder(focused ? Theme.borderStrong : Theme.border, lineWidth: 1)
        )
        .animation(.easeOut(duration: 0.12), value: focused)
    }
}

#Preview("Search field") {
    struct Host: View {
        @State private var empty = ""
        @State private var filled = "sidebar"
        var body: some View {
            VStack(spacing: 12) {
                SearchField("Search chats…", text: $empty, kbdHint: "⌘K")
                SearchField("Filter", text: $filled)
                SearchField("Search models", text: $empty, onSubmit: {})
            }
            .frame(width: 320)
            .padding(24)
            .background(Theme.canvas)
        }
    }
    return Host()
}
