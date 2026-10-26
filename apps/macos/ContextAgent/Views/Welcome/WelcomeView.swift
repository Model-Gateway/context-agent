import SwiftUI
import AppKit
import UniformTypeIdentifiers

/// The Welcome screen (ARCHITECTURE.md "Welcome"): hero, "Open a project…",
/// the recents grid, the backend failure panel / Connect-a-model card when
/// needed, a backend status footer and a folder drop target. It stays
/// usable while the backend is down: recents remain visible and the open
/// button says why it is disabled.
struct WelcomeView: View {
    @Environment(AppState.self) private var appState
    @State private var dropTargeted = false

    var body: some View {
        ScrollView(.vertical) {
            VStack(spacing: 28) {
                hero
                if let failure = appState.connection.failure {
                    BackendFailurePanel(failure: failure)
                }
                if appState.needsAPIKey, appState.connection.isReady {
                    ConnectModelCard()
                }
                RecentsGrid()
                footer
            }
            .frame(maxWidth: Theme.Metrics.contentMaxWidth)
            .padding(.horizontal, 32)
            .padding(.top, 56)
            .padding(.bottom, 40)
            .frame(maxWidth: .infinity)
        }
        .background(Theme.canvas)
        .overlay {
            if dropTargeted {
                DropHighlight(message: "Drop a folder to open it as a project")
            }
        }
        .onDrop(of: [UTType.fileURL], isTargeted: $dropTargeted) { providers in
            MainWindow.openDroppedFolders(providers, appState: appState)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Welcome")
    }

    // MARK: - Hero

    private var hero: some View {
        VStack(spacing: 14) {
            Icons.LogoView(size: 34)
            Text("Context")
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(Theme.textPrimary)
                .accessibilityAddTraits(.isHeader)
            openButton
                .padding(.top, 6)
        }
    }

    private var openButton: some View {
        VStack(spacing: 8) {
            Button {
                Task { await appState.chooseProject() }
            } label: {
                HStack(spacing: 8) {
                    if case .connecting = appState.connection {
                        // No tint: inherits the prominent button's label colour.
                        BrailleSpinner(size: 13, tint: nil)
                    } else {
                        Image(systemName: Icons.projects)
                            .font(.system(size: 14, weight: .semibold))
                    }
                    Text("Open a project…")
                        .font(.system(size: 15, weight: .semibold))
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 8)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .tint(Theme.accent)
            .disabled(!appState.connection.isReady)
            .help(disabledReason ?? "Choose a repository folder (⌘O)")
            if let disabledReason {
                Text(verbatim: disabledReason)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            } else {
                Text("or drop a folder here")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
            }
        }
    }

    private var disabledReason: String? {
        switch appState.connection {
        case .connecting: return "Starting the backend…"
        case .ready: return nil
        case .failed(let failure): return "\(failure.title) — fix the backend to open a project"
        }
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 8) {
            footerDot
            Text(appState.connectionLabel)
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
            if let dataDir = appState.config?.dataDir, !dataDir.isEmpty {
                Text("·")
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                Text(verbatim: (dataDir as NSString).abbreviatingWithTildeInPath)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help("Data directory: \(dataDir)")
            }
            Spacer(minLength: 0)
            Button {
                appState.openSettings()
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: Icons.settings)
                        .font(.system(size: 11, weight: .medium))
                    Text("Settings")
                }
                .font(Theme.Font.caption)
                .foregroundStyle(Theme.textSecondary)
            }
            .buttonStyle(.plain)
            .help("Open Settings (⌘,)")
        }
        .padding(.top, 8)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var footerDot: some View {
        switch appState.connection {
        case .connecting:
            BrailleSpinner(size: 10, tint: Theme.textTertiary)
        case .ready:
            Circle().fill(Theme.success).frame(width: 7, height: 7).accessibilityHidden(true)
        case .failed:
            Circle().fill(Theme.danger).frame(width: 7, height: 7).accessibilityHidden(true)
        }
    }
}

// MARK: - Recents grid

/// Recent projects as cards: name, abbreviated path, last opened; pinned
/// first; hover reveals Remove; context menu Pin/Unpin, Reveal in Finder,
/// Remove. A currently open project is marked "Open". "View all" opens the
/// Projects screen (search, filter, sort).
private struct RecentsGrid: View {
    @Environment(AppState.self) private var appState

    private let columns = [GridItem(.adaptive(minimum: 220, maximum: 360), spacing: 12, alignment: .top)]

    var body: some View {
        let recents = appState.recentProjects
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Recent projects")
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                if !recents.isEmpty {
                    Text("\(recents.count)")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                    Button("View all") {
                        appState.sidebarItem = .projects
                    }
                    .buttonStyle(.plain)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.accent)
                    .help("Search and filter all projects")
                }
            }
            if recents.isEmpty {
                Text("Projects you open show up here.")
                    .font(Theme.Font.secondary)
                    .foregroundStyle(Theme.textTertiary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 18)
                    .padding(.horizontal, 16)
                    .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
                            .strokeBorder(Theme.border, style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    )
            } else {
                LazyVGrid(columns: columns, alignment: .leading, spacing: 12) {
                    ForEach(recents) { recent in
                        RecentCard(recent: recent, isOpen: appState.projects.contains { $0.root == recent.path })
                    }
                }
            }
        }
    }
}

/// Shared with the Projects screen.
struct RecentCard: View {
    var recent: RecentProject
    var isOpen: Bool

    @Environment(AppState.self) private var appState
    @State private var hovering = false

    private var isOpening: Bool { appState.openingProjectPaths.contains(recent.path) }
    private var canOpen: Bool { appState.connection.isReady && recent.exists && !isOpening }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        Button {
            guard canOpen else { return }
            Task { await appState.openProject(url: recent.url) }
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: Icons.folder)
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(recent.exists ? Theme.textSecondary : Theme.textTertiary)
                        .frame(width: 20, height: 20)
                        .accessibilityHidden(true)
                    Text(verbatim: recent.name)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(Theme.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 4)
                    if recent.pinned {
                        Image(systemName: Icons.pinned)
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(Theme.textTertiary)
                            .accessibilityLabel("Pinned")
                    }
                    // Always laid out, shown on hover, and held to the row's
                    // 20 pt height, so hovering never resizes the card.
                    IconButton(Icons.close, label: "Remove from recents", size: 11) {
                        appState.recents.remove(path: recent.path)
                    }
                    .frame(width: 20, height: 20)
                    .opacity(hovering ? 1 : 0)
                    .allowsHitTesting(hovering)
                    .accessibilityHidden(!hovering)
                }
                Text(verbatim: recent.abbreviatedPath)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 6) {
                    Text("Opened \(TranscriptFormat.relative(recent.lastOpened))")
                        .font(Theme.Font.caption)
                        .foregroundStyle(Theme.textTertiary)
                    Spacer(minLength: 0)
                    if isOpening {
                        HStack(spacing: 5) {
                            BrailleSpinner(size: 10, tint: Theme.textTertiary)
                            Text("Opening…")
                                .font(Theme.Font.caption)
                                .foregroundStyle(Theme.textTertiary)
                        }
                    } else if isOpen {
                        Chip("Open", style: .tinted(Theme.success))
                    } else if !recent.exists {
                        Chip("Missing", style: .tinted(Theme.warning))
                            .help("The folder no longer exists at this path")
                    }
                }
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovering && canOpen ? Theme.surfaceRaised : Theme.surface, in: shape)
            .overlay(shape.strokeBorder(hovering && canOpen ? Theme.borderStrong : Theme.border, lineWidth: 1))
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .help(recent.exists ? recent.path : "Missing: \(recent.path)")
        .contextMenu {
            Button("Open") {
                Task { await appState.openProject(url: recent.url) }
            }
            .disabled(!canOpen)
            Button(recent.pinned ? "Unpin" : "Pin") {
                appState.recents.togglePin(path: recent.path)
            }
            Button("Reveal in Finder") {
                NSWorkspace.shared.activateFileViewerSelecting([recent.url])
            }
            .disabled(!recent.exists)
            Button("Copy Path") {
                appState.copyToPasteboard(recent.path)
            }
            Divider()
            Button("Remove from Recents", role: .destructive) {
                appState.recents.remove(path: recent.path)
            }
        }
        .accessibilityLabel("\(recent.name), \(recent.abbreviatedPath)\(recent.pinned ? ", pinned" : "")\(isOpening ? ", opening" : isOpen ? ", open" : "")")
    }
}

// MARK: - Backend failure panel

/// Shared by Welcome and the sidebar card: title ("ctx not found" / "ctx is
/// too old" / "Backend stopped"), the searched paths or the found path, the
/// stderr tail, and Choose binary… / Copy build command / Retry.
struct BackendFailurePanel: View {
    var failure: BackendFailure

    @Environment(AppState.self) private var appState
    @State private var copied = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: Icons.warning)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Theme.danger)
                    .accessibilityHidden(true)
                Text(verbatim: failure.title)
                    .font(Theme.Font.heading)
                    .foregroundStyle(Theme.textPrimary)
                    .accessibilityAddTraits(.isHeader)
                Spacer(minLength: 0)
            }
            Text(verbatim: Self.explanation(for: failure))
                .font(Theme.Font.secondary)
                .foregroundStyle(Theme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            details
            HStack(spacing: 8) {
                Button {
                    Task { await appState.restartBackend() }
                } label: {
                    HStack(spacing: 5) {
                        if appState.backendStarting {
                            BrailleSpinner(size: 11, tint: nil)
                        } else {
                            Image(systemName: Icons.refresh)
                        }
                        Text("Retry")
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(Theme.accent)
                .disabled(appState.backendStarting)
                Button("Choose binary…") {
                    Task { await appState.chooseBackendBinary() }
                }
                .buttonStyle(.bordered)
                Button {
                    appState.copyBuildCommand()
                    withAnimation(.easeOut(duration: 0.15)) { copied = true }
                    Task { @MainActor in
                        try? await Task.sleep(nanoseconds: 1_200_000_000)
                        withAnimation(.easeOut(duration: 0.15)) { copied = false }
                    }
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: copied ? Icons.copied : Icons.copy)
                        Text(copied ? "Copied" : "Copy build command")
                    }
                }
                .buttonStyle(.bordered)
                .help(BackendLocator.buildCommand)
                Spacer(minLength: 0)
            }
            .controlSize(.regular)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.danger.opacity(0.45), lineWidth: 1))
        .accessibilityElement(children: .contain)
    }

    @ViewBuilder
    private var details: some View {
        switch failure {
        case .notFound(let searched):
            if !searched.isEmpty {
                pathList(title: "Searched", paths: searched)
            }
        case .incompatible(let found, let detail):
            pathList(title: "Found", paths: [found])
            if !detail.isEmpty {
                codeBlock(detail)
            }
        case .exited(_, let tail):
            if !tail.isEmpty {
                codeBlock(tail)
            }
        case .transport(let message):
            if !message.isEmpty {
                codeBlock(message)
            }
        }
    }

    private func pathList(title: String, paths: [String]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
            ForEach(paths, id: \.self) { path in
                Text(verbatim: (path as NSString).abbreviatingWithTildeInPath)
                    .font(Theme.Font.mono)
                    .foregroundStyle(Theme.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .selectableText()
            }
        }
    }

    private func codeBlock(_ text: String) -> some View {
        ScrollView(.vertical) {
            Text(verbatim: text)
                .font(Theme.Font.mono)
                .foregroundStyle(Theme.textPrimary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .selectableText()
                .padding(10)
        }
        .frame(maxHeight: 160)
        .background(Theme.codeBackground, in: RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous))
    }

    /// One-paragraph explanation under the title.
    static func explanation(for failure: BackendFailure) -> String {
        switch failure {
        case .notFound:
            return "The ctx backend binary was not found. Build it with the command below, or point the app at an existing binary."
        case .incompatible:
            return "This ctx binary does not speak the desktop protocol. Rebuild it from this checkout or choose a newer binary."
        case .exited(let code, _):
            return "The backend process exited with code \(code). Retry, or check the log below for the cause."
        case .transport:
            return "The app could not talk to the backend process."
        }
    }

    /// Short form for the sidebar card: title plus the first useful line.
    static func summary(for failure: BackendFailure) -> String {
        switch failure {
        case .notFound(let searched):
            return searched.isEmpty ? "No ctx binary was found." : "No ctx binary in \(searched.count) searched location\(searched.count == 1 ? "" : "s")."
        case .incompatible(let found, _):
            return "Too old: \((found as NSString).abbreviatingWithTildeInPath)"
        case .exited(let code, let tail):
            let last = tail.split(whereSeparator: \.isNewline).last.map(String.init) ?? ""
            return last.isEmpty ? "ctx exited with code \(code)." : "Exited (\(code)): \(last)"
        case .transport(let message):
            return message.isEmpty ? "Backend unavailable." : message
        }
    }
}

// MARK: - Previews

#Preview("Welcome") {
    WelcomeView()
        .frame(width: 900, height: 760)
        .environment(MainWindowPreview.appState())
}

#Preview("Welcome · no API key") {
    WelcomeView()
        .frame(width: 900, height: 760)
        .environment(MainWindowPreview.appState(needsAPIKey: true, withProjects: false))
}

#Preview("Welcome · ctx not found") {
    WelcomeView()
        .frame(width: 900, height: 760)
        .environment(MainWindowPreview.appState(failure: .notFound(searched: ["/Users/me/.cargo/bin/ctx", "/opt/homebrew/bin/ctx", "/usr/local/bin/ctx"]), withProjects: false))
}
