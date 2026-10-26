import SwiftUI
import AppKit

// The Editor tab's state: one `EditorWorkspace` per project (documents open
// in its tab strip) and the `AppState` actions that load, save and close
// them through `file/read` / `file/write`, keep the project's language
// server in step with the buffer (`lsp/*`) and look up symbols in the index
// (`symbols/find`) for the editor's completion and go-to-definition.

/// One open file. `text` follows the text view; `savedText` / `hash` are
/// what the disk held at the last read or write.
@MainActor
@Observable
final class EditorDocument: Identifiable {
    /// What the Editor shows for the file: the text view, an image, a PDF
    /// or a placeholder for other binary files.
    enum Kind {
        case text, image, pdf, binary
    }

    /// Repository-relative.
    let path: String
    nonisolated var id: String { path }
    var name: String { (path as NSString).lastPathComponent }
    var kind: Kind

    var text = ""
    var savedText = ""
    var hash: String?
    var language: SyntaxHighlighter.Language
    /// The registry's name ("rust", "markdown", …) for the status bar.
    var languageName: String?
    var loading = true
    var loadError: String?
    /// `file/read` returned only the first part: the document is read-only.
    var truncated = false
    var saving = false
    /// The last save was refused because the file changed on disk.
    var conflict = false
    var cursorLine = 1
    var cursorColumn = 1
    /// Line to reveal once the text view exists (go to definition, open at line).
    var pendingLine: Int?
    /// Bumped whenever `text` was replaced from outside the text view (load,
    /// reload), so the view knows to take it over.
    var revision = 0
    var isDirty = false

    /// Symbol under the cursor from the index, for the status bar.
    var cursorSymbol: SymbolInfo?
    /// What the language server says about the identifier under the cursor
    /// (its first line: the signature or type), preferred over `cursorSymbol`.
    var cursorHover: String?

    /// The language server's executable once `lsp/open` found one.
    var lspServer: String?
    /// Characters after which the server wants a completion request.
    var lspTriggers: [String] = []
    /// Set by `lsp/diagnostics`; the text view underlines them.
    var diagnostics: [LspDiagnostic] = []
    /// The buffer changed since the server last saw it.
    @ObservationIgnored var lspDirty = false
    @ObservationIgnored var lspSync: Task<Void, Never>?
    /// The pending "after a delay" auto save.
    @ObservationIgnored var autoSave: Task<Void, Never>?

    var errorCount: Int { diagnostics.reduce(0) { $0 + ($1.isError ? 1 : 0) } }
    var warningCount: Int { diagnostics.reduce(0) { $0 + ($1.isWarning ? 1 : 0) } }

    private static let imageExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tiff", "tif", "bmp", "svg", "ico", "icns"]

    init(path: String) {
        self.path = path
        self.language = SyntaxHighlighter.Language.detect(path)
        let ext = (path as NSString).pathExtension.lowercased()
        if Self.imageExtensions.contains(ext) {
            kind = .image
        } else if ext == "pdf" {
            kind = .pdf
        } else {
            kind = .text
        }
    }

    /// The text view's edits.
    func edited(_ newText: String) {
        text = newText
        isDirty = newText != savedText
        lspDirty = true
    }

    /// Content from disk replaces the buffer.
    func load(_ content: FileContent) {
        text = content.content
        savedText = content.content
        hash = content.hash
        truncated = content.truncated
        languageName = content.language
        if let name = content.language, SyntaxHighlighter.Language.detect(path) == .plain {
            language = SyntaxHighlighter.Language.detect(name)
        }
        loading = false
        loadError = nil
        conflict = false
        isDirty = false
        lspDirty = true
        revision += 1
    }
}

/// The documents open in one project's Editor tab.
@MainActor
@Observable
final class EditorWorkspace {
    let projectId: String
    var documents: [EditorDocument] = []
    var selectedPath: String?

    init(projectId: String) {
        self.projectId = projectId
    }

    var selected: EditorDocument? {
        guard let selectedPath else { return nil }
        return documents.first { $0.path == selectedPath }
    }

    var unsavedCount: Int {
        documents.reduce(0) { $0 + ($1.isDirty ? 1 : 0) }
    }

    func document(for path: String) -> EditorDocument? {
        documents.first { $0.path == path }
    }

    /// Selects the document, adding it (after the selected one) when new.
    @discardableResult
    func open(_ path: String) -> (document: EditorDocument, isNew: Bool) {
        if let existing = document(for: path) {
            selectedPath = path
            return (existing, false)
        }
        let document = EditorDocument(path: path)
        if let selectedPath, let index = documents.firstIndex(where: { $0.path == selectedPath }) {
            documents.insert(document, at: index + 1)
        } else {
            documents.append(document)
        }
        selectedPath = path
        return (document, true)
    }

    /// Drops the document; a neighbour takes the selection.
    func close(_ document: EditorDocument) {
        guard let index = documents.firstIndex(where: { $0.path == document.path }) else { return }
        documents.remove(at: index)
        if selectedPath == document.path {
            selectedPath = documents.indices.contains(index) ? documents[index].path : documents.last?.path
        }
    }
}

// MARK: - Actions

extension AppState {
    /// Shows `path` in the Editor tab, loading it on first open, and reveals
    /// `line` (1-based) when given. Images and PDFs are shown straight from
    /// disk; everything else goes through `file/read`.
    func openInEditor(path: String, line: Int? = nil, in project: ProjectState) {
        let relative = path.hasPrefix("/") ? Self.relativePath(path, root: project.root) : path
        let workspace = editor(for: project)
        let (document, isNew) = workspace.open(relative)
        if let line { document.pendingLine = line }
        select(project: project.id, restoreSession: false)
        sidebarItem = .editor
        guard document.kind == .text else {
            document.loading = false
            return
        }
        if isNew || document.loadError != nil {
            Task { await load(document, in: project) }
        }
    }

    /// Absolute path of a document, for the image and PDF views.
    func fileURL(of document: EditorDocument, in project: ProjectState) -> URL {
        URL(fileURLWithPath: (project.root as NSString).appendingPathComponent(document.path))
    }

    func load(_ document: EditorDocument, in project: ProjectState) async {
        document.loading = true
        do {
            let content = try await backend.fileRead(projectId: project.id, path: document.path)
            document.load(content)
            connectLanguageServer(document, in: project)
        } catch {
            document.loading = false
            let message = error.localizedDescription
            if message.contains("binary file") {
                // Not text: the placeholder offers the default app instead.
                document.kind = .binary
                document.loadError = nil
            } else {
                document.loadError = message
            }
        }
    }

    /// Re-reads a clean document whose file changed on disk (the agent
    /// edited it, a git checkout); an edited one keeps its buffer.
    func refreshCleanDocuments(in project: ProjectState) {
        guard let workspace = editors[project.id] else { return }
        for document in workspace.documents where document.kind == .text && !document.isDirty && !document.loading && document.loadError == nil {
            Task {
                guard let content = try? await backend.fileRead(projectId: project.id, path: document.path) else { return }
                if content.hash != document.hash, !document.isDirty {
                    document.load(content)
                    scheduleLanguageServerSync(document, in: project)
                }
            }
        }
    }

    /// `file/write` with the hash of the last read, so an edit made meanwhile
    /// surfaces as `conflict` (the banner then offers Reload / Overwrite).
    func save(_ document: EditorDocument, in project: ProjectState, overwrite: Bool = false) async {
        guard !document.saving, !document.truncated, document.kind == .text else { return }
        document.saving = true
        defer { document.saving = false }
        let text = document.text
        do {
            let result = try await backend.fileWrite(projectId: project.id, path: document.path, content: text, expectedHash: overwrite ? nil : document.hash)
            document.hash = result.hash
            document.savedText = text
            document.isDirty = document.text != text
            document.conflict = false
            scheduleGitRefresh(project)
        } catch BackendError.rpc(_, _, let kind) where kind == "conflict" {
            document.conflict = true
        } catch {
            toasts.show("Could not save \(document.name): \(error.localizedDescription)", kind: .error, details: error.localizedDescription)
        }
    }

    /// ⌘S.
    func saveSelectedDocument() {
        guard let project = selectedProject, sidebarItem == .editor, let document = editors[project.id]?.selected, document.isDirty else { return }
        Task { await save(document, in: project) }
    }

    /// ⌘⇧W.
    func closeSelectedDocument() {
        guard let project = selectedProject, sidebarItem == .editor, let document = editors[project.id]?.selected else { return }
        closeDocument(document, in: project)
    }

    /// Removes the tab and lets go of its language server document.
    private func discard(_ document: EditorDocument, in project: ProjectState) {
        editor(for: project).close(document)
        document.lspSync?.cancel()
        document.autoSave?.cancel()
        if document.lspServer != nil {
            Task { try? await backend.lspClose(projectId: project.id, path: document.path) }
        }
    }

    /// Closes the tab; unsaved edits ask first (Save · Don't Save · Cancel).
    func closeDocument(_ document: EditorDocument, in project: ProjectState) {
        func drop() { discard(document, in: project) }
        guard document.isDirty else {
            drop()
            return
        }
        // Auto save on: save rather than ask; a failed or conflicting save keeps the tab.
        if preferences.editorAutoSave, !document.truncated {
            Task {
                await save(document, in: project)
                if !document.isDirty, !document.conflict { drop() }
            }
            return
        }
        let alert = NSAlert()
        alert.messageText = "Save changes to \(document.name)?"
        alert.informativeText = "Your edits will be lost if you don't save them."
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Don't Save")
        alert.addButton(withTitle: "Cancel")
        switch alert.runModal() {
        case .alertFirstButtonReturn:
            Task {
                await save(document, in: project)
                if !document.isDirty, !document.conflict { drop() }
            }
        case .alertSecondButtonReturn:
            drop()
        default:
            break
        }
    }

    /// Close Others · Close Tabs to the Right: `documents` go, `survivor`
    /// keeps (or takes) the selection. Unsaved ones ask once for all of them;
    /// one whose save fails or conflicts stays open.
    func closeDocuments(_ documents: [EditorDocument], selecting survivor: EditorDocument, in project: ProjectState) {
        guard !documents.isEmpty else { return }
        let workspace = editor(for: project)
        func drop(_ list: [EditorDocument]) {
            if list.contains(where: { $0.path == workspace.selectedPath }) { workspace.selectedPath = survivor.path }
            for document in list { discard(document, in: project) }
        }
        let dirty = documents.filter(\.isDirty)
        guard !dirty.isEmpty else {
            drop(documents)
            return
        }
        if preferences.editorAutoSave {
            Task {
                for document in dirty { await save(document, in: project) }
                drop(documents.filter { !$0.isDirty && !$0.conflict })
            }
            return
        }
        let alert = NSAlert()
        if dirty.count == 1 {
            alert.messageText = "Save changes to \(dirty[0].name)?"
            alert.informativeText = "Your edits will be lost if you don't save them."
        } else {
            alert.messageText = "Save changes to \(dirty.count) files?"
            alert.informativeText = dirty.map(\.name).joined(separator: ", ") + "\n\nYour edits will be lost if you don't save them."
        }
        alert.addButton(withTitle: dirty.count == 1 ? "Save" : "Save All")
        alert.addButton(withTitle: "Don't Save")
        alert.addButton(withTitle: "Cancel")
        switch alert.runModal() {
        case .alertFirstButtonReturn:
            Task {
                for document in dirty { await save(document, in: project) }
                drop(documents.filter { !$0.isDirty && !$0.conflict })
            }
        case .alertSecondButtonReturn:
            drop(documents)
        default:
            break
        }
    }

    // MARK: Auto save

    /// Turns auto save on or off; turning it on saves every edited document
    /// right away, as VS Code does.
    func setEditorAutoSave(_ enabled: Bool) {
        preferences.editorAutoSave = enabled
        guard enabled else {
            for workspace in editors.values {
                for document in workspace.documents { document.autoSave?.cancel() }
            }
            return
        }
        for (projectId, workspace) in editors {
            guard let project = projects.first(where: { $0.id == projectId }) else { continue }
            for document in workspace.documents where document.isDirty {
                Task { await autoSave(document, in: project) }
            }
        }
    }

    /// Saves a second after the last edit while auto save is on.
    func scheduleAutoSave(_ document: EditorDocument, in project: ProjectState) {
        guard preferences.editorAutoSave else { return }
        document.autoSave?.cancel()
        document.autoSave = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_000_000_000)
            guard !Task.isCancelled, let self else { return }
            await autoSave(document, in: project)
        }
    }

    /// Saves an edited document unless the user has to decide first (a
    /// conflict waits for Reload / Overwrite); waits out a save in flight so
    /// edits made during it are not left behind.
    private func autoSave(_ document: EditorDocument, in project: ProjectState) async {
        while document.saving { try? await Task.sleep(nanoseconds: 100_000_000) }
        guard document.isDirty, !document.conflict, !document.truncated, document.kind == .text else { return }
        guard editors[project.id]?.document(for: document.path) === document else { return }
        await save(document, in: project)
    }

    // MARK: Language server

    /// `lsp/open` with the buffer; the server (if any for the language) then
    /// pushes diagnostics and answers completion and hover for this document.
    func connectLanguageServer(_ document: EditorDocument, in project: ProjectState) {
        Task {
            do {
                let status = try await backend.lspOpen(projectId: project.id, path: document.path, text: document.text)
                document.lspServer = status.server
                document.lspTriggers = status.triggerCharacters ?? []
                document.lspDirty = false
            } catch {
                document.lspServer = nil
                BackendLog.debug("lsp/open \(document.path): \(error.localizedDescription)")
            }
        }
    }

    /// Debounced `lsp/change` after an edit.
    func scheduleLanguageServerSync(_ document: EditorDocument, in project: ProjectState) {
        guard document.lspServer != nil else { return }
        document.lspSync?.cancel()
        document.lspSync = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 200_000_000)
            guard !Task.isCancelled, let self else { return }
            await flushLanguageServerSync(document, in: project)
        }
    }

    /// Sends the buffer now when the server has not seen it yet.
    func flushLanguageServerSync(_ document: EditorDocument, in project: ProjectState) async {
        guard document.lspServer != nil, document.lspDirty else { return }
        document.lspSync?.cancel()
        document.lspDirty = false
        do {
            try await backend.lspChange(projectId: project.id, path: document.path, text: document.text)
        } catch {
            document.lspDirty = true
            BackendLog.debug("lsp/change \(document.path): \(error.localizedDescription)")
        }
    }

    /// Completion at a 0-based position, after the server has the buffer.
    func languageServerCompletions(for document: EditorDocument, line: Int, character: Int, in project: ProjectState) async -> [LspCompletionItem] {
        guard document.lspServer != nil else { return [] }
        await flushLanguageServerSync(document, in: project)
        return (try? await backend.lspCompletion(projectId: project.id, path: document.path, line: line, character: character))?.items ?? []
    }

    func languageServerHover(for document: EditorDocument, line: Int, character: Int, in project: ProjectState) async -> String? {
        guard document.lspServer != nil else { return nil }
        await flushLanguageServerSync(document, in: project)
        return (try? await backend.lspHover(projectId: project.id, path: document.path, line: line, character: character))?.contents
    }

    // MARK: Symbols

    /// Definitions of `name` from the index; the current file's first.
    func findSymbols(_ query: String, mode: SymbolsFindMode, limit: Int = 50, in project: ProjectState, preferring path: String? = nil) async -> [SymbolInfo] {
        guard let symbols = try? await backend.symbolsFind(projectId: project.id, query: query, mode: mode, limit: limit) else { return [] }
        guard let path else { return symbols }
        return symbols.sorted { lhs, rhs in
            let lhsLocal = lhs.path == path, rhsLocal = rhs.path == path
            return lhsLocal != rhsLocal ? lhsLocal : false
        }
    }

    /// ⌘-click on an identifier: the language server's definition when it
    /// has one, else the index's (the one in the same file first).
    func jumpToDefinition(of name: String, line: Int, character: Int, from document: EditorDocument, in project: ProjectState) {
        Task {
            if document.lspServer != nil {
                await flushLanguageServerSync(document, in: project)
                if let location = (try? await backend.lspDefinition(projectId: project.id, path: document.path, line: line, character: character))?.first {
                    openInEditor(path: location.path, line: location.line, in: project)
                    return
                }
            }
            let symbols = await findSymbols(name, mode: .exact, limit: 10, in: project, preferring: document.path)
            guard let symbol = symbols.first else {
                toasts.show("No definition of \(name) found", kind: .info)
                return
            }
            openInEditor(path: symbol.path, line: symbol.line, in: project)
        }
    }

    /// Opens the Inspector Files tab (the Editor's empty state).
    func showFilesTab() {
        inspectorVisible = true
        withAnimation(.spring(response: 0.34, dampingFraction: 0.82)) {
            inspectorTab = .files
        }
    }
}
