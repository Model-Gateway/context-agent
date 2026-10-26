import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { platform } from '@tauri-apps/plugin-os'
import { errorText } from '../../backend/errors'
import { Button } from '../../components/Button'
import { ContextMenu } from '../../components/ContextMenu'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import type { MenuItem } from '../../components/Menu'
import { isMac, revealLabel } from '../../components/platform'
import { toasts } from '../../components/toasts'
import { byteCount } from '../../models/attachments'
import { useApp } from '../../state/app'
import { documentFilePath, type EditorDocument } from '../../state/editor'
import type { ProjectState } from '../../state/project'
import styles from './MediaViews.module.css'

// Port of apps/macos/ContextAgent/Views/Editor/MediaViews.swift: the image,
// PDF and binary-file views of the Editor tab. Files are read straight from
// disk: images and PDFs through the asset protocol (`convertFileSrc`), sizes
// through `fs_file_info`.

// MARK: - Image

type Zoom = 'fit' | 'actual'

const ZOOM_OPTIONS = [
  { value: 'fit' as const, label: 'Fit' },
  { value: 'actual' as const, label: '100%' },
]

interface LoadedImage {
  src: string
  width: number
  height: number
}

/**
 * `ImageDocumentView` (MediaViews.swift): an image file in the Editor tab,
 * fit to the pane or at actual size over a checkerboard, with its format,
 * pixel size and file size below.
 */
export function ImageDocumentView({ path }: { path: string }) {
  const [image, setImage] = useState<LoadedImage | null>(null)
  const [failed, setFailed] = useState(false)
  const [zoom, setZoom] = useState<Zoom>('fit')
  const [bytes, setBytes] = useState(0)
  const name = lastPathComponent(path)

  // `.task(id: url)`: decode off the main path, then show.
  useEffect(() => {
    let cancelled = false
    setImage(null)
    setFailed(false)
    const src = convertFileSrc(path)
    void Promise.all([decodeImage(src), fileSize(path)]).then(([decoded, size]) => {
      if (cancelled) return
      setBytes(size)
      if (decoded) setImage({ src, ...decoded })
      else setFailed(true)
    })
    return () => {
      cancelled = true
    }
  }, [path])

  if (image) {
    return (
      <div className={styles.media} role="group" aria-label={`Image ${name}`}>
        <ImageCanvas image={image} actualSize={zoom === 'actual'} />
        <MediaStatusBar>
          <span className={styles.meta}>{`${pathExtension(path).toUpperCase()} · ${image.width}×${image.height} · ${byteCount(bytes)}`}</span>
          <span className={styles.spacer} />
          <ZoomPicker value={zoom} onChange={setZoom} />
        </MediaStatusBar>
      </div>
    )
  }
  if (failed) {
    return (
      <div className={styles.media} role="group" aria-label={`Image ${name}`}>
        <EmptyPlaceholder
          icon="image"
          title={`Could not read ${name}`}
          subtitle={`The file is not an image ${systemName()} can decode.`}
          actionTitle="Open with Default App"
          onAction={() => openWithDefaultApp(path)}
        />
      </div>
    )
  }
  return <div className={styles.media} role="group" aria-label={`Image ${name}`} />
}

/** The scroll view: the image fit to the pane (never enlarged) or at its pixel size, centred. */
function ImageCanvas({ image, actualSize }: { image: LoadedImage; actualSize: boolean }) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [available, setAvailable] = useState({ width: 0, height: 0 })

  // GeometryReader.
  useLayoutEffect(() => {
    const element = scrollRef.current
    if (!element) return
    const measure = () => setAvailable({ width: element.clientWidth, height: element.clientHeight })
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  let width = image.width
  let height = image.height
  if (!actualSize) {
    const paddedWidth = Math.max(1, available.width - 32)
    const paddedHeight = Math.max(1, available.height - 32)
    const scale = Math.min(1, paddedWidth / Math.max(1, image.width), paddedHeight / Math.max(1, image.height))
    width = image.width * scale
    height = image.height * scale
  }

  return (
    <div ref={scrollRef} className={styles.imageScroll}>
      <div className={styles.imageFrame}>
        <img className={styles.image} src={image.src} alt="" draggable={false} style={{ width, height }} />
      </div>
    </div>
  )
}

/// `Picker("Zoom").pickerStyle(.segmented).controlSize(.small).frame(width:
/// 110)`: the native small segmented control (two equal segments on a grey
/// track, the selected one raised) in the system font, not the bar's
/// monospaced one. ← → switch while it has focus.
function ZoomPicker({ value, onChange }: { value: Zoom; onChange: (zoom: Zoom) => void }) {
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    onChange(event.key === 'ArrowLeft' ? 'fit' : 'actual')
  }
  return (
    <div className={styles.zoom} role="radiogroup" aria-label="Zoom" onKeyDown={onKeyDown}>
      {ZOOM_OPTIONS.map((option) => {
        const selected = option.value === value
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            className={[styles.zoomSegment, selected ? styles.zoomSelected : ''].filter(Boolean).join(' ')}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}

// MARK: - PDF

/**
 * `PDFDocumentView` (MediaViews.swift): a PDF in the Editor tab — the
 * webview's own PDF viewer in place of PDFKit — with its page count and
 * file size below.
 */
export function PDFDocumentView({ path }: { path: string }) {
  const [pages, setPages] = useState<number | null>(null)
  const [bytes, setBytes] = useState(0)
  const name = lastPathComponent(path)
  const src = convertFileSrc(path)

  useEffect(() => {
    let cancelled = false
    setPages(null)
    void fileSize(path).then((size) => {
      if (cancelled) return
      setBytes(size)
      void pdfPageCount(src, size).then((count) => {
        if (!cancelled) setPages(count)
      })
    })
    return () => {
      cancelled = true
    }
  }, [path, src])

  const parts = ['PDF']
  if (pages !== null) parts.push(`${pages} page${pages === 1 ? '' : 's'}`)
  parts.push(byteCount(bytes))

  return (
    <div className={styles.media} role="group" aria-label={`PDF ${name}`}>
      {/* PDFView: no toolbar or thumbnails, continuous pages fit to the width. */}
      <iframe className={styles.pdf} src={`${src}#toolbar=0&navpanes=0&view=FitH`} title={name} />
      <MediaStatusBar>
        <span className={styles.meta}>{parts.join(' · ')}</span>
        <span className={styles.spacer} />
        <Button variant="bordered" size="small" className={styles.barButton} onClick={() => openWithDefaultApp(path)}>
          {isMac ? 'Open in Preview' : 'Open with Default App'}
        </Button>
      </MediaStatusBar>
    </div>
  )
}

// MARK: - Binary

/** `BinaryDocumentView` (MediaViews.swift): a file `file/read` refuses as binary: its size and the default app. */
export function BinaryDocumentView({ project, document }: { project: ProjectState; document: EditorDocument }) {
  const path = documentFilePath(document, project)
  const [bytes, setBytes] = useState<number | null>(null)

  useEffect(() => {
    let cancelled = false
    void fileSize(path).then((size) => {
      if (!cancelled) setBytes(size)
    })
    return () => {
      cancelled = true
    }
  }, [path])

  const items = (): MenuItem[] => [
    { label: revealLabel, onSelect: () => useApp.getState().reveal(document.path, project, 'revealInFileManager') },
    { label: 'Copy Path', onSelect: () => useApp.getState().reveal(document.path, project, 'copyPath') },
  ]

  return (
    <ContextMenu items={items}>
      <div className={styles.media}>
        {bytes !== null ? (
          <EmptyPlaceholder
            icon="archive"
            title={document.name}
            subtitle={`A binary file (${byteCount(bytes)}) the editor cannot show.`}
            actionTitle="Open with Default App"
            onAction={() => openWithDefaultApp(path)}
          />
        ) : null}
      </div>
    </ContextMenu>
  )
}

// MARK: - Chrome

/** `MediaStatusBar`: the 28 px bar under an image or PDF. */
function MediaStatusBar({ children }: { children: ReactNode }) {
  return <div className={`${styles.statusBar} hairline-top`}>{children}</div>
}

// MARK: - Helpers

/** `NSImage(contentsOf:)`: the decoded pixel size, or null when the webview cannot decode the file. */
function decodeImage(src: string): Promise<{ width: number; height: number } | null> {
  return new Promise((resolve) => {
    const image = new Image()
    // An SVG without an intrinsic size reports 0 × 0: the CSS default object size stands in.
    image.onload = () => resolve(image.naturalWidth > 0 ? { width: image.naturalWidth, height: image.naturalHeight } : { width: 300, height: 150 })
    image.onerror = () => resolve(null)
    image.src = src
  })
}

/** `attributesOfItem(atPath:)[.size]`, 0 when unknown. */
async function fileSize(path: string): Promise<number> {
  try {
    const info = await invoke<{ size: number }>('fs_file_info', { path })
    return info.size
  } catch {
    return 0
  }
}

/** `NSWorkspace.shared.open(url)`. */
function openWithDefaultApp(path: string): void {
  void invoke<void>('open_with_default_app', { path }).catch((error: unknown) => {
    toasts.push({ message: `Could not open ${lastPathComponent(path)}: ${errorText(error)}`, level: 'error' })
  })
}

/** The system named in "not an image … can decode". */
function systemName(): string {
  if (isMac) return 'macOS'
  try {
    return platform() === 'windows' ? 'Windows' : 'Linux'
  } catch {
    return 'Windows'
  }
}

/** PDFs larger than this are not scanned for their page count. */
const PDF_SCAN_LIMIT = 64 * 1024 * 1024

/// `PDFDocument(url:)?.pageCount`: the root page tree's `/Count`, read from
/// the file and from its compressed object streams; null when it cannot be
/// found (the status bar then leaves the count out).
async function pdfPageCount(src: string, size: number): Promise<number | null> {
  if (size <= 0 || size > PDF_SCAN_LIMIT) return null
  try {
    const bytes = new Uint8Array(await (await fetch(src)).arrayBuffer())
    const text = latin1(bytes)
    const direct = pageTreeCount(text)
    if (direct !== null) return direct
    let best: number | null = null
    for (const stream of objectStreams(text, bytes)) {
      const inflated = await inflate(stream)
      if (inflated === null) continue
      const count = pageTreeCount(latin1(inflated))
      if (count !== null) best = Math.max(best ?? 0, count)
    }
    if (best !== null) return best
    const pages = text.match(/\/Type\s*\/Page(?![A-Za-z])/g)?.length ?? 0
    return pages > 0 ? pages : null
  } catch {
    return null
  }
}

function latin1(bytes: Uint8Array): string {
  return new TextDecoder('latin1').decode(bytes)
}

/** The largest `/Count` of a `/Type /Pages` dictionary (the root holds the total). */
function pageTreeCount(text: string): number | null {
  let best: number | null = null
  for (const match of text.matchAll(/\/Type\s*\/Pages(?![A-Za-z])/g)) {
    const start = text.lastIndexOf('<<', match.index)
    if (start < 0) continue
    let end = text.indexOf('endobj', match.index)
    if (end < 0 || end - match.index > 4000) end = Math.min(text.length, match.index + 1000)
    const count = /\/Count\s+(\d+)/.exec(text.slice(start, end))?.[1]
    if (count !== undefined) best = Math.max(best ?? 0, Number(count))
  }
  return best
}

/** The raw bytes of every Flate-compressed `/Type /ObjStm` stream. */
function objectStreams(text: string, bytes: Uint8Array): Uint8Array[] {
  const streams: Uint8Array[] = []
  for (const match of text.matchAll(/\/Type\s*\/ObjStm(?![A-Za-z])/g)) {
    const dictStart = text.lastIndexOf('<<', match.index)
    const keyword = text.indexOf('stream', match.index)
    if (dictStart < 0 || keyword < 0) continue
    const dictionary = text.slice(dictStart, keyword)
    if (!dictionary.includes('/FlateDecode')) continue
    let start = keyword + 'stream'.length
    if (text.charCodeAt(start) === 0x0d) start += 1
    if (text.charCodeAt(start) === 0x0a) start += 1
    const length = /\/Length\s+(\d+)(?!\s+\d+\s+R)/.exec(dictionary)?.[1]
    let end = length !== undefined ? start + Number(length) : text.indexOf('endstream', start)
    if (end < 0 || end > bytes.length) continue
    while (length === undefined && end > start && (bytes[end - 1] === 0x0a || bytes[end - 1] === 0x0d)) end -= 1
    streams.push(bytes.subarray(start, end))
  }
  return streams
}

async function inflate(data: Uint8Array): Promise<Uint8Array | null> {
  if (typeof DecompressionStream === 'undefined') return null
  try {
    const copy = new Uint8Array(data)
    const stream = new Blob([copy]).stream().pipeThrough(new DecompressionStream('deflate'))
    return new Uint8Array(await new Response(stream).arrayBuffer())
  } catch {
    return null
  }
}

function lastPathComponent(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? path
}

function pathExtension(path: string): string {
  const name = lastPathComponent(path)
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot + 1) : ''
}
