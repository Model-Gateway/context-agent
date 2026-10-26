import SwiftUI
import AppKit

/// The Editor tab (⌘4): the selected project's open documents as a tab
/// strip over the selected one's `CodeEditor`, with a status bar (language,
/// cursor, the symbol under the cursor, save state). Files arrive from the
/// Inspector Files tab and the "Open in Editor" items of every file menu;
/// images and PDFs get viewers instead of the text view.
struct EditorView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let project = appState.selectedProject {
            EditorPane(project: project, workspace: appState.editor(for: project))
                .id(project.id)
        } else {
            EmptyPlaceholder(systemImage: Icons.folder, title: "No project open", subtitle: "Open a project to edit its files.")
                .background(Theme.canvas)
        }
    }
}

private struct EditorPane: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var workspace: EditorWorkspace

    var body: some View {
        VStack(spacing: 0) {
            if workspace.documents.isEmpty {
                EmptyPlaceholder(
                    systemImage: Icons.editor,
                    title: "No file open",
                    subtitle: "Pick a file in the Inspector Files tab, or choose Open in Editor on a file in a chat or in Changes. Completions come from the project's index.",
                    actionTitle: "Show Files"
                ) {
                    appState.showFilesTab()
                }
            } else {
                EditorTabStrip(project: project, workspace: workspace)
                if let document = workspace.selected {
                    DocumentPane(project: project, document: document)
                        .id(document.path)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.canvas)
        // The agent (or a checkout) changed files: clean documents follow the disk.
        .onChange(of: project.gitStatus) { appState.refreshCleanDocuments(in: project) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Editor")
    }
}

// MARK: - Tab strip

private struct EditorTabStrip: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var workspace: EditorWorkspace

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                HStack(spacing: 2) {
                    ForEach(workspace.documents) { document in
                        let others = workspace.documents.filter { $0.path != document.path }
                        let toTheRight = Array(workspace.documents.drop { $0.path != document.path }.dropFirst())
                        EditorTabChip(
                            document: document,
                            selected: document.path == workspace.selectedPath,
                            select: { workspace.selectedPath = document.path },
                            close: { appState.closeDocument(document, in: project) },
                            closeOthers: others.isEmpty ? nil : { appState.closeDocuments(others, selecting: document, in: project) },
                            closeToTheRight: toTheRight.isEmpty ? nil : { appState.closeDocuments(toTheRight, selecting: document, in: project) }
                        )
                        .id(document.path)
                        .transition(.scale(scale: 0.9).combined(with: .opacity))
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
            }
            .scrollIndicators(.never)
            .onChange(of: workspace.selectedPath) { _, path in
                if let path { withAnimation(.easeOut(duration: 0.2)) { proxy.scrollTo(path) } }
            }
        }
        .background(Theme.surface)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Open files")
    }
}

/// File name with a dot while unsaved; the close button shows on hover,
/// the context menu closes it, the others or the tabs to its right.
private struct EditorTabChip: View {
    var document: EditorDocument
    var selected: Bool
    var select: () -> Void
    var close: () -> Void
    /// `nil` when there is nothing to close (the item is then disabled).
    var closeOthers: (() -> Void)?
    var closeToTheRight: (() -> Void)?

    @State private var hovering = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        HStack(spacing: 6) {
            Image(systemName: Icons.file)
                .font(.system(size: 11))
                .foregroundStyle(selected ? Theme.accent : Theme.textTertiary)
                .accessibilityHidden(true)
            Text(verbatim: document.name)
                .font(.system(size: 12.5, weight: selected ? .semibold : .medium))
                .foregroundStyle(selected ? Theme.textPrimary : Theme.textSecondary)
                .lineLimit(1)
            ZStack {
                if hovering {
                    Button(action: close) {
                        Image(systemName: Icons.close)
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(Theme.textSecondary)
                            .frame(width: 14, height: 14)
                            .background(Theme.textPrimary.opacity(0.08), in: Circle())
                    }
                    .buttonStyle(.plain)
                    .help(document.isDirty ? "Close (unsaved changes)" : "Close")
                    .accessibilityLabel("Close \(document.name)")
                } else if document.isDirty {
                    Circle()
                        .fill(Theme.warning)
                        .frame(width: 7, height: 7)
                        .help("Unsaved changes")
                }
            }
            .frame(width: 14, height: 14)
        }
        .padding(.leading, 10)
        .padding(.trailing, 6)
        .frame(height: 26)
        .background(selected ? Theme.surfaceRaised : (hovering ? Theme.textPrimary.opacity(0.04) : Color.clear), in: shape)
        .overlay(shape.strokeBorder(selected ? Theme.border : Color.clear, lineWidth: 1))
        .contentShape(shape)
        .onTapGesture(perform: select)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .contextMenu {
            Button("Close", action: close)
            Button("Close Others") { closeOthers?() }
                .disabled(closeOthers == nil)
            Button("Close Tabs to the Right") { closeToTheRight?() }
                .disabled(closeToTheRight == nil)
        }
        .help(document.path)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(document.name)\(document.isDirty ? ", unsaved" : "")")
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

// MARK: - Document

private struct DocumentPane: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var document: EditorDocument

    var body: some View {
        VStack(spacing: 0) {
            switch document.kind {
            case .image:
                ImageDocumentView(url: appState.fileURL(of: document, in: project))
            case .pdf:
                PDFDocumentView(url: appState.fileURL(of: document, in: project))
            case .binary:
                BinaryDocumentView(project: project, document: document)
            case .text:
                textContent
            }
        }
    }

    @ViewBuilder
    private var textContent: some View {
        Group {
            if let error = document.loadError {
                EmptyPlaceholder(systemImage: Icons.warning, title: "Could not open \(document.name)", subtitle: error, actionTitle: "Try again") {
                    Task { await appState.load(document, in: project) }
                }
            } else if document.loading {
                VStack(spacing: 10) {
                    BrailleSpinner(size: 14, tint: Theme.textTertiary)
                    Text("Opening \(document.name)…")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                banners
                CodeEditor(project: project, document: document)
                EditorStatusBar(project: project, document: document)
            }
        }
    }

    @ViewBuilder
    private var banners: some View {
        if document.conflict {
            EditorBanner(systemImage: Icons.warning, tint: Theme.warning, text: "\(document.name) changed on disk since you opened it. Reload it (your edits are lost) or overwrite it.") {
                Button("Reload") { Task { await appState.load(document, in: project) } }
                Button("Overwrite") { Task { await appState.save(document, in: project, overwrite: true) } }
            }
        }
        if document.truncated {
            EditorBanner(systemImage: Icons.info, tint: Theme.textSecondary, text: "Only the first \(ByteCountFormatter.string(fromByteCount: Int64(document.text.utf8.count), countStyle: .file)) of the file are shown; it is read-only here.") {
                Button("Open with Default Editor") { appState.reveal(path: document.path, in: project, action: .openWithEditor) }
            }
        }
    }
}

private struct EditorBanner<Actions: View>: View {
    var systemImage: String
    var tint: Color
    var text: String
    @ViewBuilder var actions: () -> Actions

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: systemImage)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(tint)
                .accessibilityHidden(true)
            Text(text)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 8)
            actions()
                .buttonStyle(.bordered)
                .controlSize(.small)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(tint.opacity(0.08))
        .hairline(.bottom)
    }
}

// MARK: - Status bar

/// Language (with the language server's name when one runs) · Ln/Col ·
/// diagnostics counts · the symbol under the cursor (the server's hover, or
/// the index's definition — click jumps to it) · save state.
private struct EditorStatusBar: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var document: EditorDocument

    var body: some View {
        HStack(spacing: 12) {
            Text(verbatim: languageLabel)
                .foregroundStyle(Theme.textSecondary)
                .help(document.lspServer.map { "Completion, hover and diagnostics from \($0)" } ?? "Completion from the project's index")
            Text(verbatim: "Ln \(document.cursorLine), Col \(document.cursorColumn)")
                .foregroundStyle(Theme.textSecondary)
                .monospacedDigit()
            if document.errorCount > 0 || document.warningCount > 0 {
                HStack(spacing: 6) {
                    if document.errorCount > 0 {
                        Label("\(document.errorCount)", systemImage: "xmark.circle.fill")
                            .foregroundStyle(Theme.danger)
                    }
                    if document.warningCount > 0 {
                        Label("\(document.warningCount)", systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(Theme.warning)
                    }
                }
                .labelStyle(.titleAndIcon)
                .help(diagnosticsSummary)
                .accessibilityLabel(diagnosticsSummary)
            }
            if let hover = document.cursorHover {
                HStack(spacing: 5) {
                    Image(systemName: "info.circle")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(Theme.accent)
                    Text(verbatim: hover)
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                .help(hover)
                .layoutPriority(-1)
            } else if let symbol = document.cursorSymbol {
                Button {
                    appState.openInEditor(path: symbol.path, line: symbol.line, in: project)
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: CompletionItem.systemImage(forSymbolKind: symbol.kind))
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(Theme.accent)
                        Text(verbatim: symbol.signature ?? "\(symbol.kind) \(symbol.name)")
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(verbatim: "\(symbol.path):\(symbol.line)")
                            .foregroundStyle(Theme.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.head)
                    }
                }
                .buttonStyle(.plain)
                .help("Go to the definition of \(symbol.name) (also ⌘-click it)")
                .layoutPriority(-1)
            }
            Spacer(minLength: 8)
            if document.saving {
                BrailleSpinner(size: 10, tint: Theme.textTertiary)
            } else if document.isDirty {
                Chip("unsaved", style: .tinted(Theme.warning))
                Button("Save") { Task { await appState.save(document, in: project) } }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .help("Save (⌘S)")
            } else {
                Text("Saved")
                    .foregroundStyle(Theme.textTertiary)
            }
        }
        .font(.system(size: 11.5, design: .monospaced))
        .padding(.horizontal, 12)
        .frame(height: 28)
        .background(Theme.surface)
        .hairline(.top)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Editor status")
    }

    private var languageLabel: String {
        let language: String
        if let name = document.languageName, !name.isEmpty {
            language = name.prefix(1).uppercased() + name.dropFirst()
        } else {
            language = document.language.displayName
        }
        if let server = document.lspServer {
            return "\(language) · \((server as NSString).lastPathComponent)"
        }
        return language
    }

    private var diagnosticsSummary: String {
        var parts: [String] = []
        if document.errorCount > 0 { parts.append("\(document.errorCount) error\(document.errorCount == 1 ? "" : "s")") }
        if document.warningCount > 0 { parts.append("\(document.warningCount) warning\(document.warningCount == 1 ? "" : "s")") }
        return parts.joined(separator: ", ")
    }
}

// MARK: - Previews

#Preview("Editor · empty") {
    let state = MainWindowPreview.appState()
    state.sidebarItem = .editor
    return EditorView()
        .frame(width: 900, height: 700)
        .environment(state)
}
