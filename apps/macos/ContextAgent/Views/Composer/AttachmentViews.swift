import SwiftUI
import AppKit

/// The composer's attachment row: one chip per `ComposerAttachment`, scrolling
/// sideways when they overflow. Hovering a chip floats `AttachmentHoverCard`
/// under it (`HoverCardPanel`); × removes it.
struct AttachmentRow: View {
    let attachments: [ComposerAttachment]
    let remove: (ComposerAttachment) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(attachments) { attachment in
                    AttachmentChip(attachment: attachment, remove: { remove(attachment) })
                }
            }
            .padding(.vertical, 1)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Attachments")
    }
}

private struct AttachmentChip: View {
    let attachment: ComposerAttachment
    let remove: () -> Void

    @State private var preview: AttachmentPreview?
    @State private var hovering = false
    @State private var anchor = HoverAnchor()
    @State private var showTask: Task<Void, Never>?

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusSmall, style: .continuous)
        HStack(spacing: 6) {
            icon
            Text(verbatim: attachment.name)
                .font(.system(size: 12.5, weight: .medium))
                .foregroundStyle(Theme.textPrimary)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 160, alignment: .leading)
                .fixedSize(horizontal: true, vertical: false)
            IconButton(Icons.close, label: "Remove \(attachment.name)", size: 9, action: remove)
        }
        .padding(.leading, 7)
        .padding(.trailing, 2)
        .frame(height: 28)
        .background(shape.fill(hovering ? Theme.surfaceRaised : Theme.surface))
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .contentShape(shape)
        .background(HoverAnchorView(anchor: anchor))
        .onHover { inside in
            withAnimation(.easeOut(duration: 0.12)) { hovering = inside }
            if inside {
                scheduleCard()
            } else {
                hideCard()
            }
        }
        .onTapGesture {
            NSWorkspace.shared.activateFileViewerSelecting([attachment.url])
        }
        .help(attachment.url.path)
        .task(id: attachment.id) {
            preview = await AttachmentPreview.load(attachment)
            // Loaded while the card was up: show the real content.
            if hovering, showTask == nil { presentCard() }
        }
        .onDisappear(perform: hideCard)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Attachment \(attachment.name)")
        .accessibilityAction(named: "Remove", remove)
    }

    @ViewBuilder
    private var icon: some View {
        if let thumbnail = preview?.thumbnail {
            Image(nsImage: thumbnail)
                .resizable()
                .scaledToFill()
                .frame(width: 16, height: 16)
                .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                .accessibilityHidden(true)
        } else {
            Image(nsImage: attachment.icon)
                .resizable()
                .frame(width: 16, height: 16)
                .accessibilityHidden(true)
        }
    }

    /// The card appears after a short rest, so sweeping across the row stays quiet.
    private func scheduleCard() {
        showTask?.cancel()
        showTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(350))
            guard !Task.isCancelled else { return }
            showTask = nil
            presentCard()
        }
    }

    private func presentCard() {
        guard let view = anchor.view else { return }
        HoverCardPanel.shared.show(AttachmentHoverCard(attachment: attachment, preview: preview), beside: view, placement: .below)
    }

    private func hideCard() {
        showTask?.cancel()
        showTask = nil
        HoverCardPanel.shared.hide(from: anchor.view)
    }
}

// MARK: - Hover card

/// File name, a monospaced meta line ("PDF · 2.3 MB · 12 PAGES") and an inset
/// box with the start of the content (text, image, PDF page or folder
/// entries) fading out at the bottom.
struct AttachmentHoverCard: View {
    let attachment: ComposerAttachment
    let preview: AttachmentPreview?

    static let width: CGFloat = 340

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radius, style: .continuous)
        VStack(alignment: .leading, spacing: 8) {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: attachment.name)
                    .font(.system(size: 13.5, weight: .semibold))
                    .foregroundStyle(Theme.textPrimary)
                    .lineLimit(2)
                    .truncationMode(.middle)
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 6) {
                    Image(nsImage: attachment.icon)
                        .resizable()
                        .frame(width: 12, height: 12)
                    Text(verbatim: metaLine)
                        .font(.system(size: 10.5, weight: .medium, design: .monospaced))
                        .foregroundStyle(Theme.textTertiary)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 4)
            content
        }
        .padding(10)
        .frame(width: Self.width, alignment: .leading)
        .background(Theme.surface, in: shape)
        .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
        .floatingShadow()
        // Room for the shadow inside the borderless panel.
        .padding(12)
    }

    private var metaLine: String {
        guard let preview else { return "LOADING…" }
        return preview.meta.joined(separator: "  ·  ").uppercased()
    }

    @ViewBuilder
    private var content: some View {
        switch preview?.body {
        case .text(let title, let text, let monospaced)?:
            inset(fades: true) {
                VStack(alignment: .leading, spacing: 6) {
                    if let title {
                        Text(verbatim: title)
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(Theme.textPrimary)
                            .lineLimit(2)
                    }
                    Text(verbatim: text)
                        .font(monospaced ? .system(size: 10.5, design: .monospaced) : .system(size: 11))
                        .foregroundStyle(Theme.textSecondary)
                        .lineSpacing(2)
                        // Overflows the box and is clipped under the fade, never "…".
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        case .image(let image)?:
            inset(fades: false) {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: 180)
                    .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            }
        case .entries(let entries, let more)?:
            inset(fades: false) {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(entries.enumerated()), id: \.offset) { _, entry in
                        HStack(spacing: 6) {
                            Image(systemName: entry.1 ? Icons.folder : "doc")
                                .font(.system(size: 10))
                                .foregroundStyle(Theme.textTertiary)
                                .frame(width: 12)
                            Text(verbatim: entry.0)
                                .font(.system(size: 11.5))
                                .foregroundStyle(Theme.textSecondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    if more > 0 {
                        Text("+\(more) more")
                            .font(.system(size: 11))
                            .foregroundStyle(Theme.textTertiary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        case .none?, nil:
            EmptyView()
        }
    }

    /// The bordered box under the header; text previews fade out at the bottom.
    private func inset<Content: View>(fades: Bool, @ViewBuilder _ content: () -> Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: 9, style: .continuous)
        return content()
            .padding(10)
            .frame(maxWidth: .infinity, maxHeight: fades ? 150 : nil, alignment: .topLeading)
            .clipped()
            .mask {
                if fades {
                    LinearGradient(stops: [.init(color: .black, location: 0.6), .init(color: .black.opacity(0), location: 1)], startPoint: .top, endPoint: .bottom)
                } else {
                    Rectangle()
                }
            }
            .background(Theme.surface, in: shape)
            .overlay(shape.strokeBorder(Theme.border, lineWidth: 1))
    }
}

/// Accent outline + caption over the composer card while files are dragged onto it.
struct AttachmentDropHighlight: View {
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: Theme.Metrics.radiusLarge, style: .continuous)
        shape
            .strokeBorder(Theme.accent, lineWidth: 1.5)
            .background(Theme.accent.opacity(0.05), in: shape)
            .overlay {
                Label("Drop to attach", systemImage: "paperclip")
                    .font(.system(size: 12.5, weight: .medium))
                    .foregroundStyle(Theme.accent)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(Theme.surface, in: Capsule())
                    .floatingShadow()
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

// MARK: - Previews

#Preview("Hover card · text") {
    AttachmentHoverCard(
        attachment: ComposerAttachment(url: URL(fileURLWithPath: "/Users/me/Projects/context-agent/docs/contract-agreement brief.md")),
        preview: AttachmentPreview(
            meta: ["MD", "2.3 KB", "93 lines"],
            body: .text(title: "Rico and Wolfgang Agreement", "This Service Agreement (\"Agreement\") is entered into as of Jan 27, by and between Rico and the Provider.\n\n1. Services\n\nProvider agrees to perform the services described in Exhibit A attached hereto, in accordance with the terms and conditions of this Agreement.", monospaced: false),
            thumbnail: nil
        )
    )
}

#Preview("Hover card · folder") {
    AttachmentHoverCard(
        attachment: ComposerAttachment(url: URL(fileURLWithPath: "/Users/me/Projects/context-agent/crates", isDirectory: true)),
        preview: AttachmentPreview(meta: ["Folder", "4 items"], body: .entries([("cli", true), ("core", true), ("README.md", false)], more: 1), thumbnail: nil)
    )
}
