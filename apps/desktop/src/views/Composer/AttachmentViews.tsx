// Port of apps/macos/ContextAgent/Views/Composer/AttachmentViews.swift: the
// composer's attachment row (one chip per `ComposerAttachment`), the hover
// card a chip floats under itself after a short rest, and the drop
// highlight over the composer card.

import { memo, useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { IconButton } from '../../components/IconButton'
import { Tooltip } from '../../components/Tooltip'
import { attachmentName, loadAttachmentPreview, type AttachmentPreview, type ComposerAttachment } from '../../models/attachments'
import { fileType, Icon, type IconName } from '../../theme/icons'
import { FloatingCard, MiddleText } from './ModelHoverCard'
import styles from './AttachmentViews.module.css'

/** The rest before a chip floats its card, so sweeping across the row stays quiet. */
const CARD_DELAY_MS = 350

/** The Finder icon's stand-in: a folder, else the file-type glyph. */
function attachmentIcon(attachment: ComposerAttachment): IconName {
  return attachment.isDirectory ? 'folder' : fileType(attachment.path)
}

function AttachmentGlyph({ attachment, size }: { attachment: ComposerAttachment; size: number }) {
  return (
    <span
      className={[styles.glyph, attachment.isDirectory ? styles.folderGlyph : ''].filter(Boolean).join(' ')}
      style={{ width: size, height: size, color: attachment.isDirectory ? 'var(--accent)' : 'var(--text-tertiary)' }}
      aria-hidden="true"
    >
      <Icon name={attachmentIcon(attachment)} size={size} strokeWidth={1.75} />
    </span>
  )
}

// MARK: - Row

export interface AttachmentRowProps {
  attachments: readonly ComposerAttachment[]
  /** × on a chip. */
  onRemove: (attachment: ComposerAttachment) => void
}

/// `AttachmentRow` (AttachmentViews.swift): one chip per attachment,
/// scrolling sideways when they overflow. Hovering a chip floats
/// `AttachmentHoverCard` under it; × removes it; a click reveals it in the
/// file manager.
export function AttachmentRow({ attachments, onRemove }: AttachmentRowProps) {
  return (
    <div className={styles.row} role="group" aria-label="Attachments">
      <div className={styles.rowContent}>
        {attachments.map((attachment) => (
          <AttachmentChip key={attachment.id} attachment={attachment} onRemove={onRemove} />
        ))}
      </div>
    </div>
  )
}

interface AttachmentChipProps {
  attachment: ComposerAttachment
  onRemove: (attachment: ComposerAttachment) => void
}

const AttachmentChip = memo(function AttachmentChip({ attachment, onRemove }: AttachmentChipProps) {
  const [preview, setPreview] = useState<AttachmentPreview | null>(null)
  const [hovering, setHovering] = useState(false)
  const [cardShown, setCardShown] = useState(false)
  const chipRef = useRef<HTMLDivElement>(null)
  const showTimer = useRef<number | null>(null)
  const hoveringRef = useRef(false)
  const name = attachmentName(attachment)

  const cancelTimer = () => {
    if (showTimer.current !== null) {
      window.clearTimeout(showTimer.current)
      showTimer.current = null
    }
  }

  useEffect(() => {
    let cancelled = false
    setPreview(null)
    void loadAttachmentPreview(attachment).then((loaded) => {
      if (cancelled) return
      setPreview(loaded)
      // Loaded while the card was up: it shows the real content (it re-renders with it).
      if (hoveringRef.current && showTimer.current === null) setCardShown(true)
    })
    return () => {
      cancelled = true
    }
    // `attachment.id` is the path: a new id is a new file.
  }, [attachment.id])

  useEffect(() => cancelTimer, [])

  const onEnter = () => {
    hoveringRef.current = true
    setHovering(true)
    cancelTimer()
    showTimer.current = window.setTimeout(() => {
      showTimer.current = null
      setCardShown(true)
    }, CARD_DELAY_MS)
  }

  const onLeave = () => {
    hoveringRef.current = false
    setHovering(false)
    cancelTimer()
    setCardShown(false)
  }

  const reveal = () => {
    void invoke<void>('reveal_in_file_manager', { path: attachment.path }).catch(() => undefined)
  }

  return (
    <>
      <Tooltip content={attachment.path}>
        <div
          ref={chipRef}
          className={[styles.chip, hovering ? styles.chipHover : ''].filter(Boolean).join(' ')}
          role="group"
          aria-label={`Attachment ${name}`}
          onPointerEnter={onEnter}
          onPointerLeave={onLeave}
          onClick={reveal}
        >
          {preview?.thumbnail ? (
            <img className={styles.thumbnail} src={preview.thumbnail} alt="" aria-hidden="true" draggable={false} />
          ) : (
            <AttachmentGlyph attachment={attachment} size={16} />
          )}
          <MiddleText className={styles.chipName} text={name} />
          <IconButton
            icon="close"
            label={`Remove ${name}`}
            size={9}
            onClick={(event) => {
              event.stopPropagation()
              onRemove(attachment)
            }}
          />
        </div>
      </Tooltip>
      {cardShown && chipRef.current ? (
        <FloatingCard anchor={chipRef.current} placement="below">
          <AttachmentHoverCard attachment={attachment} preview={preview} />
        </FloatingCard>
      ) : null}
    </>
  )
})

// MARK: - Hover card

export interface AttachmentHoverCardProps {
  attachment: ComposerAttachment
  /** Null while loading ("LOADING…"). */
  preview: AttachmentPreview | null
}

/// `AttachmentHoverCard` (AttachmentViews.swift): the file name, a
/// monospaced meta line ("PDF · 2.3 MB · 12 PAGES") and an inset box with the
/// start of the content (text, image or folder entries) fading out at the
/// bottom. 340 px.
export function AttachmentHoverCard({ attachment, preview }: AttachmentHoverCardProps) {
  const meta = preview ? preview.meta.join('  ·  ').toUpperCase() : 'LOADING…'
  const body = preview?.body ?? null
  return (
    <div className={styles.card}>
      <div className={styles.cardHeader}>
        <div className={styles.cardName}>{attachmentName(attachment)}</div>
        <div className={styles.cardMeta}>
          <AttachmentGlyph attachment={attachment} size={12} />
          <span className={styles.cardMetaText}>{meta}</span>
        </div>
      </div>
      {body?.kind === 'text' ? (
        <div className={`${styles.inset} ${styles.insetFades}`}>
          <div className={styles.textBody}>
            {body.title !== null ? <div className={styles.textTitle}>{body.title}</div> : null}
            {/* Overflows the box and is clipped under the fade, never "…". */}
            <div className={body.monospaced ? styles.textMono : styles.textPlain}>{body.text}</div>
          </div>
        </div>
      ) : body?.kind === 'image' ? (
        <div className={styles.inset}>
          <img className={styles.image} src={body.src} alt="" draggable={false} />
        </div>
      ) : body?.kind === 'entries' ? (
        <div className={styles.inset}>
          <div className={styles.entries}>
            {body.entries.map((entry, index) => (
              <div key={`${index}-${entry.name}`} className={styles.entry}>
                <span className={styles.entryIcon} aria-hidden="true">
                  <Icon name={entry.isDirectory ? 'folder' : 'file'} size={10} strokeWidth={2} />
                </span>
                <MiddleText className={styles.entryName} text={entry.name} />
              </div>
            ))}
            {body.more > 0 ? <div className={styles.more}>+{body.more} more</div> : null}
          </div>
        </div>
      ) : null}
    </div>
  )
}

// MARK: - Drop highlight

export interface AttachmentDropHighlightProps {
  /** Fades in and out (0.12 s) instead of popping; shown by default. */
  visible?: boolean
}

/// `AttachmentDropHighlight` (AttachmentViews.swift): accent outline and a
/// "Drop to attach" capsule over the composer card while files are dragged
/// onto it. Fills its positioned parent; never takes the pointer.
export function AttachmentDropHighlight({ visible = true }: AttachmentDropHighlightProps) {
  return (
    <div className={[styles.dropHighlight, visible ? styles.dropVisible : ''].filter(Boolean).join(' ')} aria-hidden="true">
      <span className={styles.dropLabel}>
        <Icon name="paperclip" size={12.5} strokeWidth={2} />
        Drop to attach
      </span>
    </div>
  )
}
