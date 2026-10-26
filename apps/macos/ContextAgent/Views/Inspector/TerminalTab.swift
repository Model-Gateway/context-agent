import SwiftUI
import AppKit

/// The Inspector Terminal tab: the selected project's `TerminalWorkspace` —
/// a horizontally scrolling strip of shell tabs ending in a "+" button, over
/// the selected shell's embedded SwiftTerm view.
struct TerminalTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let project = appState.selectedProject {
            TerminalPane(workspace: appState.terminals(for: project))
                .id(project.id)
        } else {
            EmptyPlaceholder(systemImage: Icons.terminal, title: "No project open", subtitle: "Open a project to get a shell in its root.")
        }
    }
}

private struct TerminalPane: View {
    var workspace: TerminalWorkspace

    var body: some View {
        VStack(spacing: 0) {
            TerminalTabStrip(workspace: workspace)
            if let session = workspace.selected {
                TerminalHost(session: session)
                    .padding(.leading, 8)
                    .padding(.top, 4)
            } else {
                EmptyPlaceholder(systemImage: Icons.terminal, title: "No terminals", subtitle: "Open a shell in the project root.", actionTitle: "New terminal") {
                    workspace.newSession()
                }
            }
        }
        .background(Theme.canvas)
        .onAppear { workspace.startIfNeeded() }
    }
}

// MARK: - Tab strip

private struct TerminalTabStrip: View {
    var workspace: TerminalWorkspace

    private static let newButtonId = "terminal-new"

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                HStack(spacing: 2) {
                    ForEach(workspace.sessions) { session in
                        TerminalTabChip(
                            session: session,
                            selected: session.id == workspace.selected?.id,
                            select: { workspace.selectedId = session.id },
                            close: { withAnimation(.easeOut(duration: 0.15)) { workspace.close(session) } }
                        )
                        .id(session.id)
                        .transition(.scale(scale: 0.85).combined(with: .opacity))
                    }
                    IconButton(Icons.newChat, label: "New terminal", size: 12) {
                        withAnimation(.spring(response: 0.3, dampingFraction: 0.85)) {
                            workspace.newSession()
                        }
                        DispatchQueue.main.async {
                            withAnimation { proxy.scrollTo(Self.newButtonId, anchor: .trailing) }
                        }
                    }
                    .id(Self.newButtonId)
                }
                .padding(.horizontal, 8)
                .frame(maxHeight: .infinity)
            }
            .scrollIndicators(.never)
            .onChange(of: workspace.selectedId) { _, id in
                guard let id else { return }
                withAnimation { proxy.scrollTo(id) }
            }
        }
        .frame(height: 36)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Terminal tabs")
    }
}

private struct TerminalTabChip: View {
    var session: TerminalSession
    var selected: Bool
    var select: () -> Void
    var close: () -> Void

    @State private var hovering = false
    @State private var hoveringClose = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 6, style: .continuous)
        HStack(spacing: 5) {
            Image(systemName: Icons.terminal)
                .font(.system(size: 10, weight: .medium))
                .accessibilityHidden(true)
            Text(verbatim: session.displayTitle)
                .font(.system(size: 12, weight: selected ? .semibold : .medium))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 140, alignment: .leading)
            Button(action: close) {
                Image(systemName: Icons.close)
                    .font(.system(size: 8, weight: .bold))
                    .frame(width: 14, height: 14)
                    .background(Circle().fill(hoveringClose ? Theme.textPrimary.opacity(0.1) : Color.clear))
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .opacity(hovering || selected ? 1 : 0)
            .onHover { hoveringClose = $0 }
            .help("Close terminal")
            .accessibilityLabel("Close \(session.displayTitle)")
        }
        .foregroundStyle(selected || hovering ? Theme.textPrimary : Theme.textSecondary)
        .padding(.leading, 8)
        .padding(.trailing, 4)
        .frame(height: 24)
        .background(shape.fill(selected ? Theme.surfaceRaised : (hovering ? Theme.surfaceRaised.opacity(0.55) : Color.clear)))
        .contentShape(shape)
        .onTapGesture(perform: select)
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
        }
        .contextMenu {
            Button("Close Terminal", action: close)
        }
        .help(session.currentDirectory ?? session.displayTitle)
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : [.isButton])
    }
}

// MARK: - Terminal view

/// Hosts the selected session's long-lived `LocalProcessTerminalView`,
/// swapping it in when the selection changes and handing it the keyboard.
private struct TerminalHost: NSViewRepresentable {
    var session: TerminalSession

    func makeNSView(context: Context) -> NSView {
        NSView()
    }

    func updateNSView(_ container: NSView, context: Context) {
        let terminal = session.view
        guard terminal.superview !== container else { return }
        container.subviews.forEach { $0.removeFromSuperview() }
        terminal.removeFromSuperview()
        terminal.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(terminal)
        NSLayoutConstraint.activate([
            terminal.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            terminal.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            terminal.topAnchor.constraint(equalTo: container.topAnchor),
            terminal.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        DispatchQueue.main.async {
            terminal.window?.makeFirstResponder(terminal)
        }
    }
}
