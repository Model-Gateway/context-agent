import Foundation
import AppKit
import ImageIO
import PDFKit
import UniformTypeIdentifiers

/// A file or folder attached to a composer draft (dropped on the composer,
/// picked from the + menu or pasted). It travels as a path reference appended
/// to the prompt (`prompt(_:attachments:root:)`) and, where the model can
/// take it, as content too (`AttachmentUpload`: images as pixels, documents
/// and outside text files as text). `AttachmentPreview` is what the chip's
/// hover card shows.
struct ComposerAttachment: Identifiable, Hashable {
    let url: URL
    let isDirectory: Bool
    /// An image or PDF pasted from the clipboard (saved by `PastedFiles`): it
    /// goes to the model as content only, never as a path reference.
    let isPasted: Bool

    var id: String { url.path }
    var name: String { isPasted ? (isImage ? "Pasted image" : "Pasted document") : url.lastPathComponent }

    init(url: URL, isPasted: Bool = false) {
        let standardized = url.standardizedFileURL
        self.url = standardized
        self.isDirectory = (try? standardized.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory ?? false
        self.isPasted = isPasted || PastedFiles.contains(standardized)
    }

    /// Sent to the model as pixels: any image file macOS can decode
    /// (`AttachmentUpload.image` converts HEIC, TIFF and other formats).
    var isImage: Bool {
        !isDirectory && UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true
    }

    /// Repository-relative inside `root`, absolute outside it; quoted when it holds whitespace.
    func reference(root: String) -> String {
        let relative = AppState.relativePath(url.path, root: root)
        return relative.contains(where: \.isWhitespace) ? "\"\(relative)\"" : relative
    }

    /// `text` followed by an "Attached files:" list of references.
    static func prompt(_ text: String, attachments: [ComposerAttachment], root: String) -> String {
        let referenced = attachments.filter { !$0.isPasted }
        guard !referenced.isEmpty else { return text }
        let list = referenced
            .map { "- " + $0.reference(root: root) + ($0.isDirectory ? "/" : "") }
            .joined(separator: "\n")
        let block = "Attached files:\n" + list
        return text.isEmpty ? block : text + "\n\n" + block
    }

    /// Appends `urls` (file URLs only) to `list`, skipping ones already attached.
    static func appending(_ urls: [URL], to list: [ComposerAttachment]) -> [ComposerAttachment] {
        var result = list
        for url in urls where url.isFileURL {
            let attachment = ComposerAttachment(url: url)
            if !result.contains(where: { $0.id == attachment.id }) {
                result.append(attachment)
            }
        }
        return result
    }

    /// The Finder icon (a folder, the PDF document icon, …).
    var icon: NSImage {
        NSWorkspace.shared.icon(forFile: url.path)
    }
}

// MARK: - Preview

/// What the hover card shows for an attachment: a meta line ("PDF · 2.3 MB ·
/// 93 lines") and the start of its content. Loaded off the main actor with
/// size caps, so a huge file never stalls the composer.
struct AttachmentPreview {
    enum Body {
        /// The first lines of a text file or the first PDF page's text.
        case text(title: String?, String, monospaced: Bool)
        case image(NSImage)
        /// A folder's first entries (name, isDirectory).
        case entries([(String, Bool)], more: Int)
        case none
    }

    var meta: [String]
    var body: Body
    /// Shown on the chip instead of the Finder icon (image files).
    var thumbnail: NSImage?

    /// Text is read up to this many bytes; line counts stop past it.
    private static let textByteLimit = 4 << 20
    private static let previewLines = 24

    static func load(_ attachment: ComposerAttachment) async -> AttachmentPreview {
        await Task.detached(priority: .utility) {
            make(attachment)
        }.value
    }

    private static func make(_ attachment: ComposerAttachment) -> AttachmentPreview {
        let url = attachment.url
        if attachment.isDirectory {
            return folder(url)
        }
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentTypeKey])
        let type = values?.contentType ?? UTType(filenameExtension: url.pathExtension)
        var meta = [kind(url: url, type: type)]
        if let size = values?.fileSize {
            meta.append(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
        }

        if type?.conforms(to: .pdf) == true, let document = PDFDocument(url: url) {
            meta.append(count(document.pageCount, "page"))
            let title = (document.documentAttributes?[PDFDocumentAttribute.titleAttribute] as? String)
                .flatMap { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : $0 }
            let text = (document.page(at: 0)?.string ?? "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if text.isEmpty, let page = document.page(at: 0) {
                return AttachmentPreview(meta: meta, body: .image(page.thumbnail(of: NSSize(width: 560, height: 560), for: .mediaBox)))
            }
            return AttachmentPreview(meta: meta, body: .text(title: title, clipLines(text), monospaced: false))
        }

        if type?.conforms(to: .image) == true, let image = thumbnail(url) {
            if let size = imagePixelSize(url) {
                meta.append("\(Int(size.width)) × \(Int(size.height))")
            }
            return AttachmentPreview(meta: meta, body: .image(image), thumbnail: image)
        }

        if let text = readText(url, type: type) {
            meta.append(count(text.lines, "line") + (text.truncated ? "+" : ""))
            let monospaced = type?.conforms(to: .sourceCode) == true || type?.conforms(to: .json) == true
                || type?.conforms(to: .xml) == true || !(type?.conforms(to: .plainText) ?? false)
            return AttachmentPreview(meta: meta, body: .text(title: nil, clipLines(text.head), monospaced: monospaced))
        }

        return AttachmentPreview(meta: meta, body: .none)
    }

    private static func folder(_ url: URL) -> AttachmentPreview {
        let contents = (try? FileManager.default.contentsOfDirectory(
            at: url, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsHiddenFiles]
        )) ?? []
        let sorted = contents
            .map { ($0.lastPathComponent, (try? $0.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory ?? false) }
            .sorted { lhs, rhs in
                lhs.1 != rhs.1 ? lhs.1 : lhs.0.localizedStandardCompare(rhs.0) == .orderedAscending
            }
        let shown = Array(sorted.prefix(8))
        return AttachmentPreview(
            meta: ["Folder", count(contents.count, "item")],
            body: shown.isEmpty ? .none : .entries(shown, more: sorted.count - shown.count)
        )
    }

    /// "PDF", "PNG", "Swift", … from the extension, else the type's description.
    private static func kind(url: URL, type: UTType?) -> String {
        let ext = url.pathExtension
        if !ext.isEmpty, ext.count <= 5 { return ext.uppercased() }
        return type?.localizedDescription ?? "File"
    }

    private static func count(_ n: Int, _ noun: String) -> String {
        "\(n.formatted()) \(noun)\(n == 1 ? "" : "s")"
    }

    private static func clipLines(_ text: String) -> String {
        text.split(separator: "\n", maxSplits: previewLines, omittingEmptySubsequences: false)
            .prefix(previewLines)
            .joined(separator: "\n")
    }

    /// UTF-8 text without NUL bytes (so binaries without a known type are
    /// skipped): the line count over the first `textByteLimit` bytes and the head.
    private static func readText(_ url: URL, type: UTType?) -> (head: String, lines: Int, truncated: Bool)? {
        if let type, type.conforms(to: .audiovisualContent) || type.conforms(to: .archive) || type.conforms(to: .executable) {
            return nil
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: textByteLimit + 1) else { return nil }
        let truncated = data.count > textByteLimit
        let slice = data.prefix(textByteLimit)
        if slice.prefix(8192).contains(0) { return nil }
        guard let text = String(data: slice, encoding: .utf8) ?? String(data: slice.dropLast(3), encoding: .utf8) else { return nil }
        var lines = slice.reduce(0) { $1 == 0x0A ? $0 + 1 : $0 }
        if let last = slice.last, last != 0x0A { lines += 1 }
        return (String(text.prefix(6000)), lines, truncated)
    }

    private static func thumbnail(_ url: URL) -> NSImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: 640,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        return NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
    }

    private static func imagePixelSize(_ url: URL) -> CGSize? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int else { return nil }
        return CGSize(width: width, height: height)
    }
}
