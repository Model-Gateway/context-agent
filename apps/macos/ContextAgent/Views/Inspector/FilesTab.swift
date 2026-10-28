import SwiftUI
import AppKit

/// The Inspector Files tab: the selected project's tree, read from disk one
/// folder at a time as it is expanded (through `fs/list` on the project's
/// connection for a project on a remote host). Changed files carry their
/// git status letter and folders holding changes a dot; a click opens a
/// file in the Editor tab, the context menu has the usual path actions
/// (including Open with Default Editor; Finder actions only locally).
struct FilesTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let project = appState.selectedProject {
            FileTreePane(project: project, tree: project.fileTree)
                .id(project.id)
        } else {
            EmptyPlaceholder(systemImage: Icons.folder, title: "No project open", subtitle: "Open a project to browse its files.")
        }
    }
}

// MARK: - Model

struct FileNode: Identifiable, Equatable {
    /// Path relative to the project root.
    var path: String
    var name: String
    var isDirectory: Bool
    var id: String { path }
}

/// Lazily loaded directory listings of one project root: `FileManager`
/// for a local project, `fs/list` on `remote` for one on a host.
@MainActor
@Observable
final class FileTreeModel {
    let root: String
    /// The host's connection, nil for a local project.
    let remote: BackendClient?
    /// Relative directory path ("" for the root) → its sorted entries.
    private(set) var children: [String: [FileNode]] = [:]
    var expanded: Set<String> = []
    var selectedPath: String?
    /// Folders whose remote listing is in flight.
    private(set) var loading: Set<String> = []

    /// Never listed: git internals and Finder metadata.
    private static let hiddenNames: Set<String> = [".git", ".DS_Store"]

    init(root: String, remote: BackendClient? = nil) {
        self.root = root
        self.remote = remote
    }

    var isRemote: Bool { remote != nil }

    func toggle(_ node: FileNode) {
        guard node.isDirectory else { return }
        if expanded.contains(node.path) {
            expanded.remove(node.path)
        } else {
            if children[node.path] == nil { load(node.path) }
            expanded.insert(node.path)
        }
    }

    func collapseAll() {
        expanded.removeAll()
    }

    /// Re-reads the root and every folder listed so far (after git status
    /// changes or the refresh button); folders that vanished are dropped.
    func reload() {
        let listed = Set(children.keys).union([""])
        children.removeAll()
        for path in listed.sorted() where path.isEmpty || children[Self.parent(of: path)]?.contains(where: { $0.path == path }) == true {
            load(path)
        }
        expanded = expanded.filter { children[$0] != nil }
    }

    func loadRootIfNeeded() {
        if children[""] == nil { load("") }
    }

    /// Rows in display order: each folder's entries follow it while expanded.
    var visibleRows: [(node: FileNode, depth: Int)] {
        var rows: [(node: FileNode, depth: Int)] = []
        func walk(_ path: String, depth: Int) {
            for node in children[path] ?? [] {
                rows.append((node, depth))
                if node.isDirectory, expanded.contains(node.path) {
                    walk(node.path, depth: depth + 1)
                }
            }
        }
        walk("", depth: 0)
        return rows
    }

    private func load(_ path: String) {
        if let remote {
            loadRemote(path, through: remote)
            return
        }
        let url = path.isEmpty ? URL(fileURLWithPath: root) : URL(fileURLWithPath: root).appendingPathComponent(path)
        let keys: [URLResourceKey] = [.isDirectoryKey]
        guard let entries = try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: keys) else {
            children[path] = []
            return
        }
        children[path] = entries.compactMap { entry -> FileNode? in
            let name = entry.lastPathComponent
            guard !Self.hiddenNames.contains(name) else { return nil }
            let isDirectory = (try? entry.resourceValues(forKeys: Set(keys)))?.isDirectory ?? false
            return FileNode(path: path.isEmpty ? name : path + "/" + name, name: name, isDirectory: isDirectory)
        }
        .sorted { lhs, rhs in
            if lhs.isDirectory != rhs.isDirectory { return lhs.isDirectory }
            return lhs.name.localizedStandardCompare(rhs.name) == .orderedAscending
        }
    }

    /// `fs/list` of the folder on the host; the rows appear when it answers
    /// (an empty list meanwhile, so the chevron shows at once).
    private func loadRemote(_ path: String, through client: BackendClient) {
        guard !loading.contains(path) else { return }
        loading.insert(path)
        if children[path] == nil { children[path] = [] }
        let absolute = path.isEmpty ? root : root + "/" + path
        Task { [weak self] in
            let listing = try? await client.fsList(path: absolute)
            guard let self else { return }
            self.loading.remove(path)
            guard let listing else { return }
            self.children[path] = listing.entries
                .filter { !Self.hiddenNames.contains($0.name) }
                .map { FileNode(path: path.isEmpty ? $0.name : path + "/" + $0.name, name: $0.name, isDirectory: $0.isDir) }
        }
    }

    private static func parent(of path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return "" }
        return String(path[..<slash])
    }
}

// MARK: - Pane

private struct FileTreePane: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var tree: FileTreeModel

    /// Changed path → status letter, and every folder above a changed path.
    private var changes: (files: [String: String], folders: Set<String>) {
        var files: [String: String] = [:]
        var folders: Set<String> = []
        for file in project.gitStatus?.files ?? [] {
            let path = file.path.hasSuffix("/") ? String(file.path.dropLast()) : file.path
            files[path] = file.code
            var parent = path
            while let slash = parent.lastIndex(of: "/") {
                parent = String(parent[..<slash])
                folders.insert(parent)
            }
        }
        return (files, folders)
    }

    var body: some View {
        let changes = changes
        VStack(spacing: 0) {
            toolbar
            ScrollView(.vertical) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(tree.visibleRows, id: \.node.id) { row in
                        FileRow(
                            node: row.node,
                            depth: row.depth,
                            expanded: tree.expanded.contains(row.node.path),
                            selected: tree.selectedPath == row.node.path,
                            statusCode: changes.files[row.node.path],
                            containsChanges: changes.folders.contains(row.node.path),
                            localActions: !project.isRemote,
                            open: { open(row.node) },
                            perform: { action in appState.reveal(path: row.node.path, in: project, action: action) }
                        )
                        .equatable()
                    }
                }
                .padding(.vertical, 4)
            }
        }
        .background(Theme.canvas)
        .onAppear { tree.loadRootIfNeeded() }
        .onChange(of: project.gitStatus) { tree.reload() }
    }

    private var toolbar: some View {
        HStack(spacing: 6) {
            Image(systemName: project.isRemote ? Icons.remoteProject : Icons.folder)
                .font(.system(size: 11))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            Text(verbatim: project.isRemote ? project.locationLabel : project.name)
                .font(Theme.Font.caption.weight(.medium))
                .foregroundStyle(Theme.textSecondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .help(project.locationLabel)
            Spacer(minLength: 0)
            IconButton("arrow.down.right.and.arrow.up.left", label: "Collapse all folders", size: 11) {
                withAnimation(.easeOut(duration: 0.15)) { tree.collapseAll() }
            }
            .disabled(tree.expanded.isEmpty)
            IconButton(Icons.refresh, label: "Reload files", size: 11) {
                tree.reload()
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 32)
        .hairline(.bottom)
    }

    private func open(_ node: FileNode) {
        tree.selectedPath = node.path
        if node.isDirectory {
            withAnimation(.easeOut(duration: 0.15)) { tree.toggle(node) }
        } else {
            appState.reveal(path: node.path, in: project, action: .openInEditor)
        }
    }
}

// MARK: - Row

private struct FileRow: View, Equatable {
    var node: FileNode
    var depth: Int
    var expanded: Bool
    var selected: Bool
    var statusCode: String?
    var containsChanges: Bool
    /// Finder and the default editor can reach the file (a local project).
    var localActions: Bool = true
    var open: () -> Void
    var perform: (PathAction) -> Void

    @State private var hovering = false

    /// Rows are compared on what they draw, so toggling one folder or a git
    /// status change rebuilds only the rows it changed (the closures carry
    /// nothing but the node).
    static func == (lhs: FileRow, rhs: FileRow) -> Bool {
        lhs.node == rhs.node && lhs.depth == rhs.depth && lhs.expanded == rhs.expanded
            && lhs.selected == rhs.selected && lhs.statusCode == rhs.statusCode && lhs.containsChanges == rhs.containsChanges
            && lhs.localActions == rhs.localActions
    }

    var body: some View {
        Button(action: open) {
            HStack(spacing: 4) {
                Image(systemName: expanded ? Icons.chevronDown : Icons.chevronRight)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(Theme.textTertiary)
                    .frame(width: 12)
                    .opacity(node.isDirectory ? 1 : 0)
                Image(systemName: node.isDirectory ? (expanded ? "folder.fill" : Icons.folder) : Icons.file)
                    .font(.system(size: 12))
                    .foregroundStyle(node.isDirectory ? Theme.accent : Theme.textTertiary)
                    .frame(width: 16)
                Text(verbatim: node.name)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(statusCode.map { ChangesStatus.tint(for: $0) } ?? Theme.textPrimary)
                    .strikethrough(statusCode == "D")
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 4)
                if let statusCode {
                    Text(verbatim: statusCode == "?" ? "U" : statusCode)
                        .font(.system(size: 10.5, weight: .semibold, design: .monospaced))
                        .foregroundStyle(ChangesStatus.tint(for: statusCode))
                } else if containsChanges {
                    Circle()
                        .fill(Theme.warning)
                        .frame(width: 5, height: 5)
                        .padding(.trailing, 2)
                }
            }
            .padding(.leading, 8 + CGFloat(depth) * 12)
            .padding(.trailing, 12)
            .frame(height: 24)
            .background {
                if selected {
                    Theme.accentSoft
                } else if hovering {
                    Theme.textPrimary.opacity(0.05)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
        .contextMenu {
            if statusCode != nil {
                Button("Show in Changes") { perform(.showInChanges) }
            }
            if !node.isDirectory {
                Button("Open in Editor") { perform(.openInEditor) }
                if localActions {
                    Button("Open with Default Editor") { perform(.openWithEditor) }
                }
            }
            if localActions {
                Button("Reveal in Finder") { perform(.revealInFinder) }
            }
            Button("Copy Path") { perform(.copyPath) }
        }
        .accessibilityLabel(accessibilityText)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    private var accessibilityText: String {
        var text = node.isDirectory ? "Folder \(node.name), \(expanded ? "expanded" : "collapsed")" : node.name
        if let statusCode { text += ", \(ChangesStatus.label(for: statusCode))" }
        else if containsChanges { text += ", contains changes" }
        return text
    }
}
