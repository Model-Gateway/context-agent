// Port of apps/macos/ContextAgent/Models/ComposerAttachment.swift and
// Models/AttachmentUpload.swift: a file or folder attached to a composer
// draft (dropped on the composer, picked from the + menu or pasted). It
// travels as a path reference appended to the prompt (`attachmentPrompt`,
// the "Attached files:" list) and, where the model can take it, as content
// too (`prepareAttachmentPrompt`): images as `images`, and UTF-8 text files
// outside the project as `files`. `loadAttachmentPreview` is what the chip's
// hover card shows. `PastedFiles` saves pasted images under
// `<app data>/Pasted Images` (`fs_write_temp_image`).
//
// What the webview cannot do the way AppKit does: PDFs and Word / RTF /
// OpenDocument files cannot have their text extracted, so they go as path
// references only (a pasted PDF is referenced too, since it would otherwise
// be lost); images the webview cannot decode (HEIC on WebView2) are not
// sent, as Swift skips images CoreGraphics cannot decode; converted copies of
// large images are written to `Pasted Images` (there is no `Converted`
// directory command).

import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { appDataDir, join } from '@tauri-apps/api/path'
import type { FileUpload } from '../backend/types'
import { standardizePath } from './recentProject'
import type { OutgoingPrompt } from './transcript'

// MARK: - Tauri commands

/** `fs_file_info`'s result. */
interface FileInfo {
  size: number
  is_dir: boolean
  modified: string | null
  mime?: string
}

/** `fs_read_text`'s result. */
interface TextFile {
  content: string
  truncated: boolean
  size: number
}

/** `fs_list_dir`'s rows. */
interface DirEntry {
  name: string
  path: string
  is_dir: boolean
  size: number
}

async function fileInfo(path: string): Promise<FileInfo | null> {
  try {
    return await invoke<FileInfo>('fs_file_info', { path })
  } catch {
    return null
  }
}

async function readText(path: string, maxBytes: number): Promise<TextFile | null> {
  try {
    return await invoke<TextFile>('fs_read_text', { path, max_bytes: maxBytes })
  } catch {
    return null
  }
}

// MARK: - Paths

/** The last path component (either separator). */
export function pathName(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? path
}

/** The parent directory (either separator); the path itself at the root. */
function parentPath(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return cut > 0 ? path.slice(0, cut) : path
}

/** `URL.pathExtension`, lowercased: "png" for "shot.PNG", "" for none. */
export function pathExtension(path: string): string {
  const name = pathName(path)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
}

function isWindowsPath(path: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(path) || path.startsWith('\\\\')
}

/// `AppState.relativePath(_:root:)`: repository-relative inside `root`,
/// unchanged outside it. Windows roots compare case-insensitively and accept
/// either separator.
export function relativePath(absolute: string, root: string): string {
  if (root.length === 0) return absolute
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const normalizedRoot = root.endsWith('/') || root.endsWith('\\') ? root : root + separator
  if (absolute.startsWith(normalizedRoot)) return absolute.slice(normalizedRoot.length)
  if (isWindowsPath(root)) {
    const fold = (text: string) => text.replace(/\//g, '\\').toLowerCase()
    if (fold(absolute).startsWith(fold(normalizedRoot))) return absolute.slice(normalizedRoot.length)
  }
  return absolute
}

/// `AttachmentUpload.isInside(_:root:)`. Swift resolves symlinks on both
/// sides (the backend's root is canonical, `/private/tmp/…`, a picked path
/// may not be, `/tmp/…`); the webview cannot, so the macOS `/private`
/// aliases are folded instead.
function isInside(path: string, root: string): boolean {
  if (root.length === 0) return false
  let base = standardizePath(root)
  let candidate = standardizePath(path)
  if (isWindowsPath(base)) {
    base = base.replace(/\//g, '\\').toLowerCase()
    candidate = candidate.replace(/\//g, '\\').toLowerCase()
    const prefix = base.endsWith('\\') ? base : base + '\\'
    return candidate.startsWith(prefix)
  }
  const unalias = (text: string) => (/^\/private\/(tmp|var|etc)(\/|$)/.test(text) ? text.slice('/private'.length) : text)
  base = unalias(base)
  candidate = unalias(candidate)
  const prefix = base.endsWith('/') ? base : base + '/'
  return candidate.startsWith(prefix)
}

// MARK: - Pasted files

/** `PastedFiles.directory`'s last component (`src-tauri/src/fs.rs` `PASTED_IMAGES_DIRECTORY`). */
const PASTED_DIRECTORY_NAME = 'Pasted Images'

let pastedDirectory: string | null = null
const pastedDirectoryReady: Promise<string | null> = (async () => {
  try {
    pastedDirectory = standardizePath(await join(await appDataDir(), PASTED_DIRECTORY_NAME))
  } catch {
    pastedDirectory = null
  }
  return pastedDirectory
})()

/** `PastedFiles.directory`: `<app data>/Pasted Images` (null outside Tauri). */
export function pastedImagesDirectory(): Promise<string | null> {
  return pastedDirectoryReady
}

/// `PastedFiles.contains(_:)`: the file sits directly in the pasted-files
/// folder. Until the app data path is known, any folder named "Pasted
/// Images" counts.
export function isPastedPath(path: string): boolean {
  const parent = parentPath(standardizePath(path))
  if (pastedDirectory !== null) {
    return isWindowsPath(parent) ? parent.toLowerCase() === pastedDirectory.toLowerCase() : parent === pastedDirectory
  }
  return pathName(parent) === PASTED_DIRECTORY_NAME
}

/// `PastedFiles.saveImage(_:)`: saves pasted image data, downscaled to
/// `AttachmentUpload.maxPixelSize` (PNG, or JPEG when the PNG would be over
/// 4 MB), and returns its path.
export async function savePastedImage(blob: Blob): Promise<string> {
  const encoded = await downscaledImage(blob)
  if (!encoded) throw new Error('The pasted image could not be read.')
  return writePastedFile(encoded.blob, encoded.extension)
}

/** `PastedFiles.savePDF(_:)`: saves a pasted PDF (pages copied from a viewer) and returns its path. */
export async function savePastedPDF(blob: Blob): Promise<string> {
  const head = new Uint8Array(await blob.slice(0, 5).arrayBuffer())
  if (String.fromCharCode(...head) !== '%PDF-') throw new Error('The pasted document is not a PDF.')
  return writePastedFile(blob, 'pdf')
}

async function writePastedFile(blob: Blob, extension: string): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer())
  return invoke<string>('fs_write_temp_image', { bytes_base64: base64(bytes), extension })
}

function base64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

// MARK: - ComposerAttachment

/// `ComposerAttachment`: a file or folder attached to a composer draft
/// (`ChatState.attachments`, `AppState.pendingAttachments`).
export interface ComposerAttachment {
  /** `id`: the standardized path. */
  id: string
  path: string
  isDirectory: boolean
  /// An image or PDF pasted from the clipboard (saved by `PastedFiles`): it
  /// goes to the model as content only, never as a path reference.
  isPasted: boolean
}

/** `ComposerAttachment(url:isPasted:)` when the caller already knows whether it is a folder. */
export function makeComposerAttachment(path: string, options: { isDirectory?: boolean; isPasted?: boolean } = {}): ComposerAttachment {
  const standardized = standardizePath(path)
  return {
    id: standardized,
    path: standardized,
    isDirectory: options.isDirectory ?? false,
    isPasted: options.isPasted === true || isPastedPath(standardized),
  }
}

/** `ComposerAttachment(url:isPasted:)`: asks the file system whether `path` is a folder. */
export async function loadComposerAttachment(path: string, isPasted = false): Promise<ComposerAttachment> {
  const info = await fileInfo(standardizePath(path))
  return makeComposerAttachment(path, { isDirectory: info?.is_dir ?? false, isPasted })
}

/** `ComposerAttachment.appending(_:to:)`: appends `paths`, skipping ones already attached. */
export async function appendingAttachments(paths: readonly string[], list: readonly ComposerAttachment[]): Promise<ComposerAttachment[]> {
  const result = [...list]
  for (const path of paths) {
    if (path.length === 0) continue
    const attachment = await loadComposerAttachment(path)
    if (!result.some((entry) => entry.id === attachment.id)) result.push(attachment)
  }
  return result
}

/** Extensions macOS reports as `public.image` (what `isImage` checks). */
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'jpe', 'gif', 'webp', 'bmp', 'tif', 'tiff', 'heic', 'heif', 'ico', 'icns', 'svg', 'avif'])

/// `ComposerAttachment.isImage`: sent to the model as pixels (converted when
/// the endpoints do not take the format as is).
export function attachmentIsImage(attachment: ComposerAttachment): boolean {
  return !attachment.isDirectory && IMAGE_EXTENSIONS.has(pathExtension(attachment.path))
}

/** `ComposerAttachment.name`: "Pasted image" / "Pasted document" for pasted files, else the file name. */
export function attachmentName(attachment: ComposerAttachment): string {
  if (attachment.isPasted) return attachmentIsImage(attachment) ? 'Pasted image' : 'Pasted document'
  return pathName(attachment.path)
}

/** `ComposerAttachment.reference(root:)`: repository-relative inside `root`, absolute outside it; quoted when it holds whitespace. */
export function attachmentReference(attachment: ComposerAttachment, root: string): string {
  const relative = relativePath(attachment.path, root)
  return /\s/.test(relative) ? `"${relative}"` : relative
}

/** Pasted files the webview can send as content (images); a pasted PDF stays a reference. */
function isReferenced(attachment: ComposerAttachment): boolean {
  return !attachment.isPasted || !attachmentIsImage(attachment)
}

/** `ComposerAttachment.prompt(_:attachments:root:)`: `text` followed by an "Attached files:" list of references. */
export function attachmentPrompt(text: string, attachments: readonly ComposerAttachment[], root: string): string {
  const referenced = attachments.filter(isReferenced)
  if (referenced.length === 0) return text
  const list = referenced.map((attachment) => '- ' + attachmentReference(attachment, root) + (attachment.isDirectory ? '/' : '')).join('\n')
  const block = 'Attached files:\n' + list
  return text.length === 0 ? block : text + '\n\n' + block
}

// MARK: - AttachmentUpload

/** `AttachmentUpload`'s limits. */
export const AttachmentUpload = {
  /// The longest side of a sent image, in pixels. Larger images cost more
  /// tokens and are downscaled by the endpoints anyway.
  maxPixelSize: 2048,
  /** Files of the passthrough types up to this size and `maxPixelSize` are sent as they are. */
  maxPassthroughBytes: 4 << 20,
  /** `passthroughTypes`: PNG, JPEG, GIF, WebP. */
  passthroughExtensions: ['png', 'jpg', 'jpeg', 'jpe', 'gif', 'webp'] as readonly string[],
} as const

/** PDFs: no text extraction in the webview, so references only. */
const PDF_EXTENSIONS = new Set(['pdf'])
/** `documentTypes` (RTF, RTFD, Word, OpenDocument text): references only here. */
const DOCUMENT_EXTENSIONS = new Set(['rtf', 'rtfd', 'doc', 'docx', 'odt'])
/** Audiovisual, archive and executable types `isText` rejects without reading. */
const BINARY_EXTENSIONS = new Set([
  'mp3', 'm4a', 'aac', 'wav', 'aif', 'aiff', 'flac', 'ogg', 'opus', 'mp4', 'm4v', 'mov', 'avi', 'mkv', 'webm', 'wmv',
  'zip', 'gz', 'tgz', 'bz2', 'xz', 'zst', '7z', 'rar', 'tar', 'jar', 'dmg', 'pkg', 'iso',
  'exe', 'dll', 'msi', 'app', 'bin', 'so', 'dylib', 'o', 'a', 'class', 'wasm',
])

/// `AttachmentUpload.prompt(text:attachments:root:)`: the prompt for `text`
/// and `attachments`: the "Attached files:" references in the text, plus the
/// images and text files `task/run` takes.
export async function prepareAttachmentPrompt(text: string, attachments: readonly ComposerAttachment[], root: string): Promise<OutgoingPrompt> {
  const promptText = attachmentPrompt(text, attachments, root)
  if (attachments.length === 0) return { text: promptText, images: [], files: [] }
  const { images, files } = await prepareAttachments(attachments, root)
  return { text: promptText, images, files }
}

/// `AttachmentUpload.prepare(_:root:)`: images as pixels (as they are, or a
/// downscaled copy); other UTF-8 text files outside the project as `files`;
/// folders, in-project text files, PDFs, documents and other binaries stay
/// references only.
export async function prepareAttachments(
  attachments: readonly ComposerAttachment[],
  root: string,
): Promise<{ images: string[]; files: FileUpload[] }> {
  const images: string[] = []
  const files: FileUpload[] = []
  for (const attachment of attachments) {
    if (attachment.isDirectory) continue
    const path = attachment.path
    if (attachmentIsImage(attachment)) {
      const prepared = await preparedImage(path)
      if (prepared !== null) images.push(prepared)
      continue
    }
    const extension = pathExtension(path)
    if (PDF_EXTENSIONS.has(extension) || DOCUMENT_EXTENSIONS.has(extension)) continue
    if (!isInside(path, root) && (await isText(path))) files.push({ path, name: attachmentName(attachment) })
  }
  return { images, files }
}

/// `AttachmentUpload.image(_:)`: the file to send for an image: the file
/// itself when the endpoints accept it as is, else a downscaled copy; null
/// when it cannot be decoded.
async function preparedImage(path: string): Promise<string | null> {
  const blob = await readImageFile(path)
  if (!blob) return null
  const decoded = await decodeImage(blob)
  if (!decoded) return null
  try {
    if (
      AttachmentUpload.passthroughExtensions.includes(pathExtension(path)) &&
      blob.size <= AttachmentUpload.maxPassthroughBytes &&
      Math.max(decoded.width, decoded.height) <= AttachmentUpload.maxPixelSize
    ) {
      return path
    }
    const encoded = await encodeDownscaled(decoded.image, decoded.width, decoded.height)
    if (!encoded) return null
    return await writePastedFile(encoded.blob, encoded.extension)
  } catch {
    return null
  } finally {
    decoded.release()
  }
}

/** The image file's bytes through the asset protocol. */
async function readImageFile(path: string): Promise<Blob | null> {
  try {
    const response = await fetch(convertFileSrc(path))
    if (!response.ok) return null
    return await response.blob()
  } catch {
    return null
  }
}

interface DecodedImage {
  image: HTMLImageElement
  width: number
  height: number
  release: () => void
}

/** Decodes image bytes in the webview (what `CGImageSource` does); null when the engine cannot. */
async function decodeImage(blob: Blob): Promise<DecodedImage | null> {
  const url = URL.createObjectURL(blob)
  const image = new Image()
  image.src = url
  try {
    await image.decode()
  } catch {
    URL.revokeObjectURL(url)
    return null
  }
  const width = image.naturalWidth
  const height = image.naturalHeight
  if (width === 0 || height === 0) {
    URL.revokeObjectURL(url)
    return null
  }
  return { image, width, height, release: () => URL.revokeObjectURL(url) }
}

/// `AttachmentUpload.downscaled` + `writeImage`: longest side at most
/// `maxPixelSize`, PNG, or JPEG (quality 0.85) when the PNG would be over
/// `maxPassthroughBytes` (photos).
async function encodeDownscaled(image: CanvasImageSource, width: number, height: number): Promise<{ blob: Blob; extension: string } | null> {
  const scale = Math.min(1, AttachmentUpload.maxPixelSize / Math.max(width, height))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(width * scale))
  canvas.height = Math.max(1, Math.round(height * scale))
  const context = canvas.getContext('2d')
  if (!context) return null
  context.drawImage(image, 0, 0, canvas.width, canvas.height)
  const png = await canvasBlob(canvas, 'image/png')
  if (!png) return null
  if (png.size <= AttachmentUpload.maxPassthroughBytes) return { blob: png, extension: 'png' }
  const jpeg = await canvasBlob(canvas, 'image/jpeg', 0.85)
  return jpeg ? { blob: jpeg, extension: 'jpg' } : { blob: png, extension: 'png' }
}

function canvasBlob(canvas: HTMLCanvasElement, type: string, quality?: number): Promise<Blob | null> {
  return new Promise((resolve) => canvas.toBlob(resolve, type, quality))
}

/** Pasted image data → the downscaled encoding `savePastedImage` writes. */
async function downscaledImage(blob: Blob): Promise<{ blob: Blob; extension: string } | null> {
  const decoded = await decodeImage(blob)
  if (!decoded) return null
  try {
    return await encodeDownscaled(decoded.image, decoded.width, decoded.height)
  } finally {
    decoded.release()
  }
}

/// `AttachmentUpload.isText(_:type:)`: UTF-8 without NUL bytes in its first
/// 8 KB (what `task/run` accepts). `fs_read_text` decodes lossily, so an
/// invalid sequence shows as U+FFFD.
async function isText(path: string): Promise<boolean> {
  if (BINARY_EXTENSIONS.has(pathExtension(path))) return false
  const head = await readText(path, 8192)
  if (!head || head.content.length === 0) return false
  return !head.content.includes('\u0000') && !head.content.includes('�')
}

// MARK: - Preview

/** `AttachmentPreview.Body`. */
export type AttachmentPreviewBody =
  /** The first lines of a text file. */
  | { kind: 'text'; title: string | null; text: string; monospaced: boolean }
  /** An image (an asset URL for `<img>`). */
  | { kind: 'image'; src: string }
  /** A folder's first entries. */
  | { kind: 'entries'; entries: { name: string; isDirectory: boolean }[]; more: number }
  | { kind: 'none' }

/// `AttachmentPreview`: what the hover card shows for an attachment: a meta
/// line ("PNG · 2.3 MB · 1024 × 768") and the start of its content.
export interface AttachmentPreview {
  meta: string[]
  body: AttachmentPreviewBody
  /** Shown on the chip instead of the file icon (image files): an asset URL. */
  thumbnail: string | null
}

/** Text is read up to this many bytes; line counts stop past it. */
const PREVIEW_TEXT_BYTE_LIMIT = 4 << 20
const PREVIEW_LINES = 24
/** Plain-text types shown in the proportional font (everything else readable is monospaced). */
const PROPORTIONAL_EXTENSIONS = new Set(['txt', 'text', 'md', 'markdown', 'mdown', 'log'])

/// `AttachmentPreview.load(_:)`: loaded with size caps, so a huge file never
/// stalls the composer. PDFs show their meta line only (no page text or
/// thumbnail in the webview).
export async function loadAttachmentPreview(attachment: ComposerAttachment): Promise<AttachmentPreview> {
  const path = attachment.path
  if (attachment.isDirectory) return folderPreview(path)
  const info = await fileInfo(path)
  const meta = [previewKind(path)]
  if (info) meta.push(byteCount(info.size))

  if (PDF_EXTENSIONS.has(pathExtension(path))) return { meta, body: { kind: 'none' }, thumbnail: null }

  if (attachmentIsImage(attachment)) {
    const src = convertFileSrc(path)
    const size = await imagePixelSize(src)
    if (size) {
      meta.push(`${size.width} × ${size.height}`)
      return { meta, body: { kind: 'image', src }, thumbnail: src }
    }
  }

  const text = await previewText(path)
  if (text) {
    meta.push(count(text.lines, 'line') + (text.truncated ? '+' : ''))
    const monospaced = !PROPORTIONAL_EXTENSIONS.has(pathExtension(path))
    return { meta, body: { kind: 'text', title: null, text: clipLines(text.head), monospaced }, thumbnail: null }
  }

  return { meta, body: { kind: 'none' }, thumbnail: null }
}

async function folderPreview(path: string): Promise<AttachmentPreview> {
  let contents: DirEntry[] = []
  try {
    contents = (await invoke<DirEntry[]>('fs_list_dir', { path })).filter((entry) => !entry.name.startsWith('.'))
  } catch {
    contents = []
  }
  // `fs_list_dir` already sorts folders first, then names in Finder order.
  const shown = contents.slice(0, 8).map((entry) => ({ name: entry.name, isDirectory: entry.is_dir }))
  return {
    meta: ['Folder', count(contents.length, 'item')],
    body: shown.length === 0 ? { kind: 'none' } : { kind: 'entries', entries: shown, more: contents.length - shown.length },
    thumbnail: null,
  }
}

/** `AttachmentPreview.kind(url:type:)`: "PDF", "PNG", "SWIFT", … from the extension, else "File". */
function previewKind(path: string): string {
  const extension = pathExtension(path)
  if (extension.length > 0 && Array.from(extension).length <= 5) return extension.toUpperCase()
  return 'File'
}

/** `AttachmentPreview.count(_:_:)`: "1,204 lines". */
function count(n: number, noun: string): string {
  return `${n.toLocaleString('en-US')} ${noun}${n === 1 ? '' : 's'}`
}

/** `ByteCountFormatter.string(fromByteCount:countStyle: .file)`: "Zero KB", "12 bytes", "48 KB", "2.3 MB", "1.25 GB". */
export function byteCount(bytes: number): string {
  if (bytes === 0) return 'Zero KB'
  if (bytes < 1000) return `${bytes} byte${bytes === 1 ? '' : 's'}`
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`
  if (bytes < 1_000_000_000) return `${trimZero((bytes / 1_000_000).toFixed(1))} MB`
  return `${trimZero((bytes / 1_000_000_000).toFixed(2))} GB`
}

function trimZero(text: string): string {
  return text.replace(/\.0+$/, '')
}

/** `AttachmentPreview.clipLines(_:)`: the first 24 lines. */
function clipLines(text: string): string {
  return text.split('\n').slice(0, PREVIEW_LINES).join('\n')
}

/// `AttachmentPreview.readText(_:type:)`: UTF-8 text without NUL bytes (so
/// binaries without a known type are skipped): the line count over the first
/// `PREVIEW_TEXT_BYTE_LIMIT` bytes and the head.
async function previewText(path: string): Promise<{ head: string; lines: number; truncated: boolean } | null> {
  if (BINARY_EXTENSIONS.has(pathExtension(path))) return null
  const file = await readText(path, PREVIEW_TEXT_BYTE_LIMIT)
  if (!file) return null
  const content = file.content
  if (content.slice(0, 8192).includes('\u0000')) return null
  let lines = 0
  for (let i = 0; i < content.length; i += 1) if (content.charCodeAt(i) === 0x0a) lines += 1
  if (content.length > 0 && !content.endsWith('\n')) lines += 1
  return { head: Array.from(content.slice(0, 12_000)).slice(0, 6000).join(''), lines, truncated: file.truncated }
}

/** `AttachmentPreview.imagePixelSize(_:)`: the decoded size, or null when the webview cannot decode it. */
function imagePixelSize(src: string): Promise<{ width: number; height: number } | null> {
  return new Promise((resolve) => {
    const image = new Image()
    image.onload = () => resolve(image.naturalWidth > 0 ? { width: image.naturalWidth, height: image.naturalHeight } : null)
    image.onerror = () => resolve(null)
    image.src = src
  })
}

/** Paths of an outgoing prompt that are pasted files still on disk (edits and undo put them back as chips). */
export async function pastedAttachments(prompt: OutgoingPrompt): Promise<ComposerAttachment[]> {
  const out: ComposerAttachment[] = []
  for (const path of [...prompt.images, ...prompt.files.map((file) => file.path)]) {
    if (!isPastedPath(path)) continue
    const info = await fileInfo(path)
    if (info && !info.is_dir) out.push(makeComposerAttachment(path, { isPasted: true }))
  }
  return out
}
