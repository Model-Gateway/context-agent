import SwiftUI
import AppKit
import WebKit

/// The Inspector Browser tab: the selected project's `BrowserWorkspace` — a
/// toolbar (back, forward, reload / stop, address field, open in the default
/// browser) over its embedded `WKWebView`. The agent drives the same page
/// through the `browser` tool; while it does, a status line says what it is
/// doing, but the Inspector never switches to this tab on its own.
struct BrowserTab: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let project = appState.selectedProject {
            BrowserPane(workspace: appState.browser(for: project))
                .id(project.id)
        } else {
            EmptyPlaceholder(systemImage: Icons.browser, title: "No project open", subtitle: "Open a project to browse alongside it.")
        }
    }
}

private struct BrowserPane: View {
    @Bindable var workspace: BrowserWorkspace

    var body: some View {
        VStack(spacing: 0) {
            BrowserToolbar(workspace: workspace)
            if workspace.isLoading {
                ProgressBar(fraction: workspace.progress)
            }
            if let activity = workspace.agentActivity ?? workspace.recentAgentAction {
                AgentActivityLine(text: activity, live: workspace.agentActivity != nil)
            }
            if let error = workspace.loadError {
                Text(verbatim: error)
                    .font(Theme.Font.caption)
                    .foregroundStyle(Theme.danger)
                    .lineLimit(2)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .hairline(.bottom)
            }
            ZStack {
                WebViewHost(webView: workspace.webView)
                if !workspace.hasPage && !workspace.isLoading {
                    EmptyPlaceholder(systemImage: Icons.browser, title: "Browser", subtitle: "Enter an address above, or ask the agent to open a page: it drives this tab without switching to it.")
                        .allowsHitTesting(false)
                }
            }
        }
        .background(Theme.canvas)
    }
}

// MARK: - Toolbar

private struct BrowserToolbar: View {
    @Bindable var workspace: BrowserWorkspace
    @FocusState private var addressFocused: Bool

    var body: some View {
        HStack(spacing: 4) {
            IconButton("chevron.left", label: "Back", size: 12) { workspace.goBack() }
                .disabled(!workspace.canGoBack)
                .opacity(workspace.canGoBack ? 1 : 0.4)
            IconButton("chevron.right", label: "Forward", size: 12) { workspace.goForward() }
                .disabled(!workspace.canGoForward)
                .opacity(workspace.canGoForward ? 1 : 0.4)
            if workspace.isLoading {
                IconButton(Icons.close, label: "Stop loading", size: 12) { workspace.stop() }
            } else {
                IconButton(Icons.refresh, label: "Reload", size: 12) { workspace.reload() }
                    .disabled(!workspace.hasPage)
                    .opacity(workspace.hasPage ? 1 : 0.4)
            }
            addressField
            if let url = workspace.url, workspace.hasPage {
                IconButton(Icons.externalLink, label: "Open in default browser", size: 11) {
                    NSWorkspace.shared.open(url)
                }
            }
        }
        .padding(.horizontal, 8)
        .frame(height: 36)
        .hairline(.bottom)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Browser toolbar")
    }

    private var addressField: some View {
        HStack(spacing: 6) {
            Image(systemName: workspace.url?.scheme == "https" ? "lock" : Icons.browser)
                .font(.system(size: 10, weight: .medium))
                .foregroundStyle(Theme.textTertiary)
                .accessibilityHidden(true)
            TextField("Address or search", text: $workspace.addressText)
                .textFieldStyle(.plain)
                .font(.system(size: 12))
                .focused($addressFocused)
                .onSubmit {
                    workspace.open(workspace.addressText)
                    addressFocused = false
                }
                .accessibilityLabel("Address")
        }
        .padding(.horizontal, 8)
        .frame(height: 24)
        .background(
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Theme.surfaceRaised)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .strokeBorder(addressFocused ? Theme.accent.opacity(0.6) : Color.clear, lineWidth: 1)
        )
        .help(workspace.title.isEmpty ? "Address" : workspace.title)
    }
}

/// Thin accent bar under the toolbar while a page loads.
private struct ProgressBar: View {
    var fraction: Double

    var body: some View {
        GeometryReader { geometry in
            Rectangle()
                .fill(Theme.accent)
                .frame(width: max(8, geometry.size.width * max(0.05, fraction)))
                .animation(.easeOut(duration: 0.2), value: fraction)
        }
        .frame(height: 2)
        .accessibilityHidden(true)
    }
}

/// "Opening example.com…" while the agent acts, then its last action for a moment.
private struct AgentActivityLine: View {
    var text: String
    var live: Bool

    var body: some View {
        HStack(spacing: 6) {
            if live {
                BrailleSpinner(size: 10, tint: Theme.accent)
            } else {
                Image(systemName: Icons.check)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(Theme.textTertiary)
                    .accessibilityHidden(true)
            }
            Text("Agent · \(text)")
                .font(Theme.Font.caption)
                .foregroundStyle(live ? Theme.textPrimary : Theme.textTertiary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 5)
        .background(live ? Theme.accentSoft.opacity(0.5) : Color.clear)
        .hairline(.bottom)
        .transition(.opacity)
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Web view host

/// Hosts the workspace's long-lived `WKWebView`, which keeps loading and
/// running script while the tab is not shown.
private struct WebViewHost: NSViewRepresentable {
    var webView: WKWebView

    func makeNSView(context: Context) -> NSView {
        NSView()
    }

    func updateNSView(_ container: NSView, context: Context) {
        guard webView.superview !== container else { return }
        container.subviews.forEach { $0.removeFromSuperview() }
        webView.removeFromSuperview()
        webView.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(webView)
        NSLayoutConstraint.activate([
            webView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            webView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            webView.topAnchor.constraint(equalTo: container.topAnchor),
            webView.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
    }

    static func dismantleNSView(_ container: NSView, coordinator: ()) {
        // The web view belongs to the workspace: detach it (keeping its
        // page) instead of letting it go with the host.
        for view in container.subviews {
            view.removeFromSuperview()
            view.translatesAutoresizingMaskIntoConstraints = true
            view.frame = NSRect(origin: .zero, size: BrowserWorkspace.detachedSize)
        }
    }
}
