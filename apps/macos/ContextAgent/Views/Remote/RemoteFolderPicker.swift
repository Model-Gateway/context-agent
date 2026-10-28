import SwiftUI
import AppKit

/// The remote folder picker (Welcome "Open on a remote host…", the sidebar
/// host rows, ⌘⇧O): the stand-in for `NSOpenPanel` when the folder is on
/// another machine. Picks a host, connects to it, and browses its
/// directories through `fs/list` — a path field (`~` works), the parent
/// folder, repositories marked with a branch glyph — until Open.
struct RemoteFolderPicker: View {
    let request: RemotePickerRequest

    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var host: SSHHost?
    @State private var pathField = ""
    @State private var listing: FsListing?
    @State private var selected: FsEntry?
    @State private var loading = false
    @State private var error: String?
    @State private var showHidden = false
    @State private var opening = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            content
            Divider()
            footer
        }
        .frame(width: 620, height: 520)
        .background(Theme.canvas)
        .onAppear {
            host = request.host
            if host != nil { Task { await load(nil) } }
        }
    }

    // MARK: - Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Image(systemName: Icons.remoteHost)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .accessibilityHidden(true)
                Text("Open a project on a remote host")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
                hostPicker
            }
            HStack(spacing: 6) {
                Button {
                    guard let parent = listing?.parent else { return }
                    Task { await load(parent) }
                } label: {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 11, weight: .semibold))
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(listing?.parent == nil || loading)
                .help("Parent folder")
                TextField("Path on the host (~ for home)", text: $pathField)
                    .textFieldStyle(.roundedBorder)
                    .font(Theme.Font.mono)
                    .onSubmit { Task { await load(pathField) } }
                    .disabled(host == nil)
                Button("Go") { Task { await load(pathField) } }
                    .controlSize(.small)
                    .disabled(host == nil || loading)
                Button {
                    guard let home = listing?.home else { return }
                    Task { await load(home) }
                } label: {
                    Image(systemName: "house")
                        .font(.system(size: 11, weight: .medium))
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(listing == nil || loading)
                .help("Home folder")
                Toggle(isOn: $showHidden) {
                    Image(systemName: "eye")
                        .font(.system(size: 11, weight: .medium))
                }
                .toggleStyle(.button)
                .controlSize(.small)
                .help(showHidden ? "Hide dotfolders" : "Show dotfolders")
            }
        }
        .padding(16)
    }

    @ViewBuilder
    private var hostPicker: some View {
        let hosts = appState.hosts.hosts
        Picker("Host", selection: Binding(
            get: { host?.id },
            set: { id in
                host = hosts.first { $0.id == id }
                listing = nil
                selected = nil
                error = nil
                if host != nil { Task { await load(nil) } }
            }
        )) {
            Text("Choose a host…").tag(UUID?.none)
            ForEach(hosts) { candidate in
                Text(verbatim: candidate.displayName).tag(UUID?.some(candidate.id))
            }
        }
        .labelsHidden()
        .frame(width: 200)
        .help("Hosts are configured in Settings → Remote hosts")
    }

    // MARK: - Content

    @ViewBuilder
    private var content: some View {
        if host == nil {
            EmptyPlaceholder(systemImage: Icons.remoteHost, title: "Choose a host", subtitle: "Pick one of the configured hosts, or add one in Settings → Remote hosts.", actionTitle: "Open Settings") {
                dismiss()
                appState.openSettings(.remoteHosts)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let error {
            EmptyPlaceholder(systemImage: Icons.warning, title: "Could not list the folder", subtitle: error, actionTitle: "Try again") {
                Task { await load(listing?.path ?? pathField) }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let listing {
            folderList(listing)
        } else {
            VStack(spacing: 10) {
                BrailleSpinner(size: 14, tint: Theme.textSecondary)
                Text(connectingLabel)
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textSecondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private var connectingLabel: String {
        guard let host else { return "" }
        if case .ready = appState.remotes[host.id]?.state { return "Listing…" }
        return "Connecting to \(host.displayName)…"
    }

    private func folderList(_ listing: FsListing) -> some View {
        let entries = listing.entries.filter { $0.isDir && (showHidden || !$0.isHidden) }
        return ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: 0) {
                if entries.isEmpty {
                    Text("No folders here.")
                        .font(Theme.Font.secondary)
                        .foregroundStyle(Theme.textTertiary)
                        .padding(14)
                }
                ForEach(entries) { entry in
                    FolderRow(entry: entry, selected: selected == entry) {
                        selected = entry
                    } open: {
                        Task { await load(entry.path) }
                    }
                }
            }
            .padding(.vertical, 4)
        }
        .overlay {
            if loading {
                Color.clear
                    .overlay(alignment: .top) {
                        ProgressView().controlSize(.small).padding(8)
                    }
            }
        }
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 8) {
            if let listing {
                Text(verbatim: openTarget(listing).path)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if openTarget(listing).isGit {
                    Chip("git", systemImage: Icons.branch)
                }
            }
            Spacer(minLength: 8)
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button {
                Task { await open() }
            } label: {
                HStack(spacing: 6) {
                    if opening { BrailleSpinner(size: 11, tint: nil) }
                    Text(openLabel)
                }
            }
            .buttonStyle(.borderedProminent)
            .tint(Theme.accent)
            .keyboardShortcut(.defaultAction)
            .disabled(listing == nil || opening)
        }
        .padding(14)
    }

    /// The selected folder, else the folder being browsed.
    private func openTarget(_ listing: FsListing) -> (path: String, isGit: Bool) {
        if let selected { return (selected.path, selected.isGit) }
        return (listing.path, listing.entries.contains { $0.name == ".git" } || false)
    }

    private var openLabel: String {
        guard let listing else { return "Open" }
        let name = (openTarget(listing).path as NSString).lastPathComponent
        return name.isEmpty ? "Open" : "Open \(name)"
    }

    // MARK: - Actions

    private func load(_ path: String?) async {
        guard let host else { return }
        loading = true
        error = nil
        defer { loading = false }
        do {
            let result = try await appState.listRemoteDirectory(path, on: host)
            listing = result
            selected = nil
            pathField = result.path
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func open() async {
        guard let host, let listing else { return }
        opening = true
        defer { opening = false }
        let path = openTarget(listing).path
        if await appState.openProject(path: path, host: host, select: true, restoreSession: false) != nil {
            dismiss()
        }
    }
}

private struct FolderRow: View {
    let entry: FsEntry
    let selected: Bool
    let select: () -> Void
    let open: () -> Void

    @State private var hovering = false

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: selected ? "folder.fill" : Icons.folder)
                .font(.system(size: 12))
                .foregroundStyle(entry.isHidden ? Theme.textTertiary : Theme.accent)
                .frame(width: 16)
            Text(verbatim: entry.name)
                .font(Theme.Font.secondary)
                .foregroundStyle(entry.isHidden ? Theme.textSecondary : Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.middle)
            if entry.isGit {
                Image(systemName: Icons.branch)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .help("A Git repository")
            }
            Spacer(minLength: 4)
            Image(systemName: Icons.chevronRight)
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(Theme.textTertiary)
                .opacity(hovering ? 1 : 0)
        }
        .padding(.horizontal, 14)
        .frame(height: 26)
        .background {
            if selected {
                Theme.accentSoft
            } else if hovering {
                Theme.textPrimary.opacity(0.05)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { open() }
        .onTapGesture { select() }
        .onHover { hovering = $0 }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Folder \(entry.name)\(entry.isGit ? ", Git repository" : "")")
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}
