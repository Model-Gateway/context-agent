import SwiftUI
import AppKit
import PDFKit

/// An image file in the Editor tab: fit to the pane or at actual size over
/// a checkerboard, with its format, pixel size and file size below.
struct ImageDocumentView: View {
    var url: URL

    @State private var image: NSImage?
    @State private var failed = false
    @State private var actualSize = false

    var body: some View {
        VStack(spacing: 0) {
            if let image {
                GeometryReader { geometry in
                    ScrollView([.horizontal, .vertical]) {
                        Image(nsImage: image)
                            .resizable()
                            .interpolation(.high)
                            .aspectRatio(contentMode: .fit)
                            .frame(width: displaySize(image, in: geometry.size).width, height: displaySize(image, in: geometry.size).height)
                            .frame(minWidth: geometry.size.width, minHeight: geometry.size.height)
                    }
                    .background(Checkerboard())
                }
                MediaStatusBar {
                    Text(verbatim: "\(url.pathExtension.uppercased()) · \(pixelSize(image)) · \(fileSize)")
                        .foregroundStyle(Theme.textSecondary)
                    Spacer(minLength: 8)
                    Picker("Zoom", selection: $actualSize) {
                        Text("Fit").tag(false)
                        Text("100%").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .controlSize(.small)
                    .frame(width: 110)
                }
            } else if failed {
                EmptyPlaceholder(systemImage: "photo", title: "Could not read \(url.lastPathComponent)", subtitle: "The file is not an image macOS can decode.", actionTitle: "Open with Default App") {
                    NSWorkspace.shared.open(url)
                }
            } else {
                Color.clear
            }
        }
        .task(id: url) {
            let loaded = await Task.detached(priority: .userInitiated) { NSImage(contentsOf: url) }.value
            image = loaded
            failed = loaded == nil
        }
        .accessibilityLabel("Image \(url.lastPathComponent)")
    }

    private func displaySize(_ image: NSImage, in available: CGSize) -> CGSize {
        let pixels = pixelDimensions(image)
        guard !actualSize else { return pixels }
        let padded = CGSize(width: max(1, available.width - 32), height: max(1, available.height - 32))
        let scale = min(1, padded.width / max(1, pixels.width), padded.height / max(1, pixels.height))
        return CGSize(width: pixels.width * scale, height: pixels.height * scale)
    }

    private func pixelDimensions(_ image: NSImage) -> CGSize {
        if let rep = image.representations.first, rep.pixelsWide > 0 {
            return CGSize(width: rep.pixelsWide, height: rep.pixelsHigh)
        }
        return image.size
    }

    private func pixelSize(_ image: NSImage) -> String {
        let size = pixelDimensions(image)
        return "\(Int(size.width))×\(Int(size.height))"
    }

    private var fileSize: String {
        let bytes = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
        return ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }
}

/// A PDF in the Editor tab (PDFKit, continuous pages, fit to width).
struct PDFDocumentView: View {
    var url: URL

    var body: some View {
        VStack(spacing: 0) {
            PDFKitView(url: url)
            MediaStatusBar {
                Text(verbatim: "PDF · \(pageCount) · \(fileSize)")
                    .foregroundStyle(Theme.textSecondary)
                Spacer(minLength: 8)
                Button("Open in Preview") { NSWorkspace.shared.open(url) }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .accessibilityLabel("PDF \(url.lastPathComponent)")
    }

    private var pageCount: String {
        let count = PDFDocument(url: url)?.pageCount ?? 0
        return "\(count) page\(count == 1 ? "" : "s")"
    }

    private var fileSize: String {
        let bytes = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
        return ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }
}

private struct PDFKitView: NSViewRepresentable {
    var url: URL

    func makeNSView(context: Context) -> PDFView {
        let view = PDFView()
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.displayDirection = .vertical
        view.backgroundColor = NSColor.adaptive(light: "#F2F2F4", dark: "#2B2B2E")
        view.document = PDFDocument(url: url)
        return view
    }

    func updateNSView(_ view: PDFView, context: Context) {
        if view.document?.documentURL != url {
            view.document = PDFDocument(url: url)
        }
    }
}

/// A file `file/read` refuses as binary: its size and the apps that can open it.
struct BinaryDocumentView: View {
    @Environment(AppState.self) private var appState
    var project: ProjectState
    var document: EditorDocument

    var body: some View {
        let url = appState.fileURL(of: document, in: project)
        let bytes = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
        EmptyPlaceholder(
            systemImage: "doc.zipper",
            title: document.name,
            subtitle: "A binary file (\(ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file))) the editor cannot show.",
            actionTitle: "Open with Default App"
        ) {
            NSWorkspace.shared.open(url)
        }
        .contextMenu {
            Button("Reveal in Finder") { appState.reveal(path: document.path, in: project, action: .revealInFinder) }
            Button("Copy Path") { appState.reveal(path: document.path, in: project, action: .copyPath) }
        }
    }
}

/// The 28 pt bar under an image or PDF.
private struct MediaStatusBar<Content: View>: View {
    @ViewBuilder var content: () -> Content

    var body: some View {
        HStack(spacing: 12) {
            content()
        }
        .font(.system(size: 11.5, design: .monospaced))
        .padding(.horizontal, 12)
        .frame(height: 28)
        .background(Theme.surface)
        .hairline(.top)
    }
}

/// Transparent-image backdrop.
private struct Checkerboard: View {
    var body: some View {
        Canvas { context, size in
            let cell: CGFloat = 10
            let light = Color.white.opacity(0.06)
            var y: CGFloat = 0
            var row = 0
            while y < size.height {
                var x: CGFloat = row % 2 == 0 ? 0 : cell
                while x < size.width {
                    context.fill(Path(CGRect(x: x, y: y, width: cell, height: cell)), with: .color(light))
                    x += cell * 2
                }
                y += cell
                row += 1
            }
        }
        .background(Theme.surfaceRaised)
        .accessibilityHidden(true)
    }
}
