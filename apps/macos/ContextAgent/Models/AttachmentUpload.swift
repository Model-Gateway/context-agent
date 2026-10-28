import Foundation
import AppKit
import ImageIO
import PDFKit
import UniformTypeIdentifiers
import os

/// A text file for `task/run` `files`: `name` is what the model is told the
/// file is called (the original document's name for text extracted from it).
/// `content` carries the text itself for a backend on another machine,
/// which cannot read `path` (`RemoteAttachments`).
struct FileUpload: Codable, Equatable, Hashable {
    var path: String
    var name: String
    var content: String? = nil
}

/// Attachments for a project on a remote host: the backend there cannot
/// open the app's files, so images go as their base64 bytes and text files
/// as their content, read here just before `task/run` / `task/steer`. The
/// prompt keeps its local paths for the transcript's thumbnails and chips.
enum RemoteAttachments {
    struct Prepared: Equatable {
        var inlineImages: [InlineImage] = []
        var files: [FileUpload] = []
        /// Attachments that could not be read (gone since they were attached).
        var skipped: [String] = []
    }

    /// Read off the main actor: a pasted screenshot is a few megabytes.
    static func prepare(images: [String], files: [FileUpload]) async -> Prepared {
        await Task.detached(priority: .userInitiated) { inline(images: images, files: files) }.value
    }

    nonisolated static func inline(images: [String], files: [FileUpload]) -> Prepared {
        var prepared = Prepared()
        for path in images {
            // A name from history (a remote run's image) has no bytes here.
            guard path.hasPrefix("/"), let data = try? Data(contentsOf: URL(fileURLWithPath: path)), !data.isEmpty else {
                prepared.skipped.append(path)
                continue
            }
            prepared.inlineImages.append(InlineImage(name: (path as NSString).lastPathComponent, data: data.base64EncodedString()))
        }
        for file in files {
            if file.content != nil {
                prepared.files.append(file)
                continue
            }
            guard file.path.hasPrefix("/"), let data = try? Data(contentsOf: URL(fileURLWithPath: file.path)), let text = String(data: data, encoding: .utf8) else {
                prepared.skipped.append(file.path)
                continue
            }
            prepared.files.append(FileUpload(path: file.path, name: file.name, content: text))
        }
        return prepared
    }
}

/// What `task/run` gets for a draft's attachments, besides the "Attached
/// files:" references in the text:
/// - images go as pixels (`images`), downscaled or converted when needed;
/// - PDFs and Word, RTF and OpenDocument files go as their text (`files`,
///   named after the original); a PDF without a text layer (a scan) goes as
///   images of its first pages;
/// - other UTF-8 text files go as their content when they are outside the
///   project (the agent reads files inside it with its own tools);
/// - folders and other binaries (archives, spreadsheets) go as references only.
enum AttachmentUpload {
    /// The longest side of a sent image, in pixels. Larger images cost more
    /// tokens and are downscaled by the endpoints anyway.
    static let maxPixelSize = 2048
    /// Files of these types up to this size and `maxPixelSize` are sent as they are.
    static let maxPassthroughBytes = 4 << 20
    static let passthroughTypes: [UTType] = [.png, .jpeg, .gif, .webP]
    /// Pages of a scanned PDF sent as images.
    static let maxScannedPages = 5
    /// Read with `NSAttributedString` (HTML is not: its importer needs the
    /// main thread, and HTML is text anyway).
    static let documentTypes: [UTType] = [.rtf, .rtfd] + [
        "com.microsoft.word.doc",
        "org.openxmlformats.wordprocessingml.document",
        "org.oasis-open.opendocument.text",
    ].compactMap { UTType($0) }

    static let convertedDirectory = AttachmentStore.directory("Converted")

    /// The prompt for `text` and `attachments`, prepared off the main actor.
    static func prompt(text: String, attachments: [ComposerAttachment], root: String) async -> OutgoingPrompt {
        let text = ComposerAttachment.prompt(text, attachments: attachments, root: root)
        guard !attachments.isEmpty else { return OutgoingPrompt(text: text) }
        let (images, files) = await Task.detached(priority: .userInitiated) {
            prepare(attachments, root: root)
        }.value
        return OutgoingPrompt(text: text, images: images, files: files)
    }

    static func prepare(_ attachments: [ComposerAttachment], root: String) -> (images: [String], files: [FileUpload]) {
        var images: [String] = []
        var files: [FileUpload] = []
        for attachment in attachments where !attachment.isDirectory {
            let url = attachment.url
            if attachment.isImage {
                if let prepared = image(url) { images.append(prepared.path) }
                continue
            }
            let type = (try? url.resourceValues(forKeys: [.contentTypeKey]))?.contentType ?? UTType(filenameExtension: url.pathExtension)
            if type?.conforms(to: .pdf) == true {
                guard let document = PDFDocument(url: url) else { continue }
                let text = (document.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                if !text.isEmpty {
                    if let converted = try? writeText(text, name: url.deletingPathExtension().lastPathComponent) {
                        files.append(FileUpload(path: converted.path, name: attachment.name))
                    }
                } else {
                    images.append(contentsOf: pageImages(document, name: url.deletingPathExtension().lastPathComponent))
                }
                continue
            }
            if let type, documentTypes.contains(where: { type.conforms(to: $0) }) {
                if let text = try? NSAttributedString(url: url, options: [:], documentAttributes: nil).string,
                   !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                   let converted = try? writeText(text, name: url.deletingPathExtension().lastPathComponent) {
                    files.append(FileUpload(path: converted.path, name: attachment.name))
                }
                continue
            }
            if !isInside(url, root: root), isText(url, type: type) {
                files.append(FileUpload(path: url.path, name: attachment.name))
            }
        }
        return (images, files)
    }

    /// The file to send for an image: the file itself when the endpoints
    /// accept it as is, else a downscaled copy. nil when it cannot be decoded.
    static func image(_ url: URL) -> URL? {
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentTypeKey])
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        if let type = values?.contentType, passthroughTypes.contains(where: { type.conforms(to: $0) }),
           (values?.fileSize ?? .max) <= maxPassthroughBytes,
           let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
           let width = properties[kCGImagePropertyPixelWidth] as? Int,
           let height = properties[kCGImagePropertyPixelHeight] as? Int,
           max(width, height) <= maxPixelSize {
            return url
        }
        guard let image = downscaled(source) else { return nil }
        return try? writeImage(image, name: url.deletingPathExtension().lastPathComponent, in: convertedDirectory)
    }

    static func downscaled(_ source: CGImageSource) -> CGImage? {
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixelSize,
        ]
        return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
    }

    /// PNG, or JPEG when the PNG would be over `maxPassthroughBytes` (photos).
    static func writeImage(_ image: CGImage, name: String, in directory: URL) throws -> URL {
        var type = UTType.png
        var data = try encode(image, as: type)
        if data.count > maxPassthroughBytes {
            type = .jpeg
            data = try encode(image, as: type, quality: 0.85)
        }
        return try AttachmentStore.write(data, name: name, type: type, in: directory)
    }

    private static func encode(_ image: CGImage, as type: UTType, quality: Double? = nil) throws -> Data {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, type.identifier as CFString, 1, nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        let properties = quality.map { [kCGImageDestinationLossyCompressionQuality: $0] as CFDictionary }
        CGImageDestinationAddImage(destination, image, properties)
        guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
        return data as Data
    }

    private static func writeText(_ text: String, name: String) throws -> URL {
        try AttachmentStore.write(Data(text.utf8), name: name, type: .plainText, in: convertedDirectory)
    }

    private static func pageImages(_ document: PDFDocument, name: String) -> [String] {
        let size = NSSize(width: maxPixelSize, height: maxPixelSize)
        return (0..<min(document.pageCount, maxScannedPages)).compactMap { index in
            guard let page = document.page(at: index),
                  let image = page.thumbnail(of: size, for: .mediaBox).cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
            return try? writeImage(image, name: "\(name) page \(index + 1)", in: convertedDirectory).path
        }
    }

    /// Symlinks resolved on both sides: the backend's root is canonical
    /// (`/private/tmp/…`) while a picked URL may not be (`/tmp/…`).
    private static func isInside(_ url: URL, root: String) -> Bool {
        let base = URL(fileURLWithPath: root).resolvingSymlinksInPath().path
        let prefix = base.hasSuffix("/") ? base : base + "/"
        return url.resolvingSymlinksInPath().path.hasPrefix(prefix)
    }

    /// UTF-8 without NUL bytes in its first 8 KB (what `task/run` accepts).
    private static func isText(_ url: URL, type: UTType?) -> Bool {
        if let type, type.conforms(to: .audiovisualContent) || type.conforms(to: .archive) || type.conforms(to: .executable) {
            return false
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? handle.close() }
        guard let head = try? handle.read(upToCount: 8192), !head.isEmpty, !head.contains(0) else { return false }
        // A multi-byte character cut by the 8 KB read is not a failure.
        return (0...3).contains { String(data: head.dropLast($0), encoding: .utf8) != nil }
    }
}

// MARK: - Pasted files

/// Images and PDFs pasted into the composer, saved so `task/run` can hand
/// the backend a path and history can show them again later.
enum PastedFiles {
    static let directory = AttachmentStore.directory("Pasted Images")

    static func contains(_ url: URL) -> Bool {
        url.standardizedFileURL.deletingLastPathComponent().path == directory.standardizedFileURL.path
    }

    /// Image data on `pasteboard` (a screenshot, "Copy Image"), PNG preferred.
    static func imageData(from pasteboard: NSPasteboard) -> Data? {
        for type in [NSPasteboard.PasteboardType.png, .tiff] {
            if let data = pasteboard.data(forType: type) { return data }
        }
        return nil
    }

    /// A PDF on `pasteboard` (pages copied from Preview).
    static func pdfData(from pasteboard: NSPasteboard) -> Data? {
        pasteboard.data(forType: .pdf)
    }

    static func hasContent(_ pasteboard: NSPasteboard) -> Bool {
        pasteboard.availableType(from: [.png, .tiff, .pdf]) != nil
    }

    /// Saves pasted image data, downscaled to `AttachmentUpload.maxPixelSize`.
    static func saveImage(_ data: Data) throws -> URL {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil), let image = AttachmentUpload.downscaled(source) else {
            throw CocoaError(.fileReadCorruptFile)
        }
        return try AttachmentUpload.writeImage(image, name: "Pasted image", in: directory)
    }

    static func savePDF(_ data: Data) throws -> URL {
        guard PDFDocument(data: data) != nil else { throw CocoaError(.fileReadCorruptFile) }
        return try AttachmentStore.write(data, name: "Pasted document", type: .pdf, in: directory)
    }
}

// MARK: - Store

/// Files the app writes for the backend under Application Support
/// (`ContextAgent/<directory>`). Files older than `retention` are removed
/// the first time a directory is written to after launch.
enum AttachmentStore {
    static let retention: TimeInterval = 30 * 24 * 60 * 60

    private static let pruned = OSAllocatedUnfairLock(initialState: Set<String>())

    static func directory(_ name: String) -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("ContextAgent").appendingPathComponent(name, isDirectory: true)
    }

    /// Writes `data` as `<name> <8 hex>.<ext>` (unique, keeps the name readable).
    static func write(_ data: Data, name: String, type: UTType, in directory: URL) throws -> URL {
        if pruned.withLock({ $0.insert(directory.path).inserted }) {
            prune(directory)
        }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("\(name) \(UUID().uuidString.prefix(8))", conformingTo: type)
        try data.write(to: url, options: .atomic)
        return url
    }

    private static func prune(_ directory: URL) {
        let cutoff = Date().addingTimeInterval(-retention)
        let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        for file in files {
            if let date = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate, date < cutoff {
                try? FileManager.default.removeItem(at: file)
            }
        }
    }
}
