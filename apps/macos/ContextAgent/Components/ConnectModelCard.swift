import SwiftUI

/// The "Connect a model" card (ARCHITECTURE.md), shared by the Welcome
/// screen and the empty chat while `appState.needsAPIKey`: base URL and
/// model name from the configuration (read-only), an API key `SecureField`, Save (Keychain, then a backend restart when idle;
/// disabled with "Finishes after the running task" while any project runs)
/// and a link to Settings → Model.
struct ConnectModelCard: View {
    @Environment(AppState.self) private var appState
    @State private var key = ""
    @State private var saving = false
    @State private var failure: String?
    @State private var saved = false

    private var busy: Bool { appState.anyProjectRunning }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        let model = appState.config?.model
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: Icons.key)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Theme.warning)
                    .accessibilityHidden(true)
                Text("Connect a model")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
            }
            Text("No API key is set for the configured endpoint. Paste one below; it is stored in your Keychain.")
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 8) {
                GridRow {
                    label("Base URL")
                    Text(verbatim: model?.baseUrl ?? "—")
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .selectableText()
                }
                GridRow {
                    label("Model")
                    Text(verbatim: model?.name ?? "—")
                        .font(Theme.Font.mono)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .selectableText()
                }
                GridRow {
                    label("API key")
                    HStack(spacing: 8) {
                        SecureField("Paste your API key", text: $key)
                            .textFieldStyle(.roundedBorder)
                            .font(Theme.Font.mono)
                            .onSubmit(save)
                            .accessibilityLabel("API key for \(appState.apiKeyEnvName)")
                        Button {
                            save()
                        } label: {
                            HStack(spacing: 5) {
                                // No tint: the spinner inherits the prominent
                                // button's label colour (white when key, dark
                                // when the window is inactive).
                                if saving { BrailleSpinner(size: 11, tint: nil) }
                                Text(saved ? "Saved" : "Save")
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(Theme.accent)
                        .disabled(saving || busy || key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .help(busy ? "Finishes after the running task" : "Store the key in the Keychain and restart the backend")
                    }
                }
            }
            if busy {
                Text("Finishes after the running task")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
            if let failure {
                Text(verbatim: failure)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
                    .fixedSize(horizontal: false, vertical: true)
                    .selectableText()
            }
            Button {
                appState.openSettings(.model)
            } label: {
                HStack(spacing: 4) {
                    Text("Open Settings → Model")
                    Image(systemName: Icons.externalLink)
                        .font(.system(size: 10, weight: .semibold))
                }
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.accent)
            }
            .buttonStyle(.plain)
            .help("Change the base URL, model name or key variable")
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.warning.opacity(0.5), lineWidth: 1))
        .accessibilityElement(children: .contain)
    }

    private func label(_ text: String) -> some View {
        Text(verbatim: text)
            .font(Theme.Font.caption)
            .foregroundStyle(Theme.textTertiary)
            .lineLimit(1)
            .frame(minWidth: 72, alignment: .leading)
    }

    private func save() {
        let value = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty, !saving, !busy else { return }
        saving = true
        failure = nil
        Task {
            do {
                try await appState.saveAPIKey(value)
                key = ""
                saved = true
            } catch {
                failure = error.localizedDescription
            }
            saving = false
        }
    }
}

#Preview("Connect a model") {
    ConnectModelCard()
        .padding(24)
        .frame(width: 640)
        .background(Theme.canvas)
        .environment(MainWindowPreview.appState(needsAPIKey: true, withProjects: false))
}
