// Port of apps/macos/ContextAgent/Views/Chat/ApprovalCardView.swift.

import { useMemo } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { capLines, LineList } from '../../components/LineList'
import { Tooltip } from '../../components/Tooltip'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { isDiscountDrop, riskLevelLabel, type ApprovalDecision, type ApprovalRequest, type RiskLevel } from '../../backend/types'
import { lineHighlighter, type Language } from '../../markdown/highlighter'
import { toolInfo } from '../../models/toolMetadata'
import { approvalIsPending, type ApprovalItem } from '../../models/transcript'
import { useApp, useChatValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import { Icon } from '../../theme/icons'
import styles from './ApprovalCardView.module.css'

export interface ApprovalCardViewProps {
  item: ApprovalItem
  chat: ChatState
}

/**
 * `ApprovalCardView` of the macOS app. A pending approval is a `warning`
 * hairline card with the tool, a risk chip, the one-line summary, the
 * details in a code block (max 240 px), the policy's reason and the
 * right-aligned *Deny* · *Approve for session* · *Approve* (a lower-discount
 * question offers *Cancel* · *Always continue* · *Continue*). While
 * `approval/respond` is in flight a spinner reads "Sending…"; a failure
 * renders inline. Once resolved the card collapses to one muted line
 * ("Approved · run: cargo test").
 *
 * No keyboard handling lives here: state/keys.ts owns mod+Enter / mod+Backspace
 * and routes them through `primaryAction()` / `denyPendingApproval()`.
 */
export function ApprovalCardView({ item, chat }: ApprovalCardViewProps) {
  return approvalIsPending(item) ? <PendingCard request={item.request} chat={chat} /> : <ResolvedLine item={item} />
}

// MARK: - Pending

function PendingCard({ request, chat }: { request: ApprovalRequest; chat: ChatState }) {
  const inFlight = useChatValue(chat, (c) => c.approvalInFlight, false)
  const error = useChatValue(chat, (c) => c.approvalError, null)
  const meta = toolInfo(request.tool)
  const discountDrop = isDiscountDrop(request)
  const summary = request.summary.trim()
  const details = request.details.trim()
  const reason = request.reason.trim()

  const respond = (decision: ApprovalDecision) => {
    if (chat.approvalInFlight) return
    void useApp.getState().respond(request.id, chat, decision)
  }

  return (
    <div className={styles.card} role="group" aria-label={`Approval needed: ${summary}`}>
      <div className={styles.header}>
        <Icon name="approval" size={14} strokeWidth={2} className={styles.headerIcon} />
        <span className={styles.title}>{discountDrop ? 'Lower discount' : 'Approval needed'}</span>
        <span className={styles.spacer} />
        <Chip icon={meta.icon}>{meta.label}</Chip>
        <Chip variant="tinted" tint={riskTint(request.risk)}>
          {riskLevelLabel(request.risk)}
        </Chip>
      </div>

      {summary.length > 0 ? <p className={`${styles.summary} selectable`}>{summary}</p> : null}

      {details.length > 0 ? <DetailsBlock text={details} language={detailsLanguage(request.tool, details)} /> : null}

      {reason.length > 0 ? <p className={`${styles.reason} selectable`}>{reason}</p> : null}

      {error !== null && error.length > 0 ? (
        <div className={styles.error} role="alert" aria-label={`Error: could not send the decision. ${error}`}>
          <Icon name="warning" size={12} strokeWidth={2} className={styles.errorIcon} />
          <span className="selectable">Could not send the decision: {error}</span>
        </div>
      ) : null}

      <div className={styles.buttons}>
        <span className={styles.spacer} />
        {inFlight ? (
          <span className={styles.sending}>
            <BrailleSpinner size={12} />
            <span className={styles.sendingLabel}>Sending…</span>
          </span>
        ) : null}
        {discountDrop ? (
          <>
            <Tooltip content="Stop this task" shortcut="mod+Backspace">
              <Button variant="plain" className={styles.borderless} disabled={inFlight} onClick={() => respond('deny')}>
                Cancel
              </Button>
            </Tooltip>
            <Tooltip content="Continue, and stop asking (Settings → Spending)">
              <Button variant="bordered" disabled={inFlight} onClick={() => respond('approve_for_session')}>
                Always continue
              </Button>
            </Tooltip>
            <Tooltip content="Use the less discounted providers for the rest of this task" shortcut="mod+Enter">
              <Button variant="prominent" disabled={inFlight} onClick={() => respond('approve')}>
                Continue
              </Button>
            </Tooltip>
          </>
        ) : (
          <>
            <Tooltip content="Do not run this action" shortcut="mod+Backspace">
              <Button variant="plain" className={styles.borderless} disabled={inFlight} onClick={() => respond('deny')}>
                Deny
              </Button>
            </Tooltip>
            <Tooltip content="Approve this and identical actions for the rest of the session">
              <Button variant="bordered" disabled={inFlight} onClick={() => respond('approve_for_session')}>
                Approve for session
              </Button>
            </Tooltip>
            <Tooltip content="Run this action once" shortcut="mod+Enter">
              <Button variant="prominent" disabled={inFlight} onClick={() => respond('approve')}>
                Approve
              </Button>
            </Tooltip>
          </>
        )}
      </div>
    </div>
  )
}

// MARK: - Resolved

function ResolvedLine({ item }: { item: ApprovalItem }) {
  const request = item.request
  const summary = request.summary.trim()
  const details = request.details.trim()
  const decision = decisionLabel(request, item.decision)
  const resolvedText = summary.length === 0 ? decision : `${decision} · ${summary}`
  const denied = item.decision === 'deny'
  return (
    <Tooltip content={details.length === 0 ? resolvedText : details}>
      <div className={styles.resolved} role="note" aria-label={resolvedText}>
        <span className={styles.resolvedIcon} style={{ color: denied ? 'var(--text-tertiary)' : 'var(--success)' }} aria-hidden="true">
          <Icon name={denied ? 'failed' : 'check'} size={13} strokeWidth={2} />
        </span>
        <MiddleTruncate text={resolvedText} className={`${styles.resolvedText} selectable`} />
      </div>
    </Tooltip>
  )
}

function decisionLabel(request: ApprovalRequest, decision: ApprovalDecision | null): string {
  if (isDiscountDrop(request)) {
    switch (decision) {
      case 'approve':
        return 'Continued'
      case 'approve_for_session':
        return 'Always continue'
      case 'deny':
        return 'Cancelled'
      case null:
        return 'Pending'
    }
  }
  switch (decision) {
    case 'approve':
      return 'Approved'
    case 'approve_for_session':
      return 'Approved for session'
    case 'deny':
      return 'Denied'
    case null:
      return 'Pending'
  }
}

// MARK: - Helpers

function riskTint(risk: RiskLevel): string {
  switch (risk) {
    case 'read_only':
      return 'var(--text-secondary)'
    case 'write':
      return 'var(--warning)'
    case 'execute':
      return 'var(--warning)'
    case 'network':
      return 'var(--accent)'
    case 'destructive':
      return 'var(--danger)'
  }
}

function detailsLanguage(tool: string, details: string): Language {
  switch (tool) {
    case 'execute_command':
    case 'run_validation':
      return 'shell'
    case 'apply_patch':
    case 'git_diff':
      return 'diff'
    default:
      if (details.startsWith('diff --git') || details.startsWith('@@') || details.startsWith('--- ')) return 'diff'
      if (details.startsWith('{') || details.startsWith('[')) return 'json'
      return 'plain'
  }
}

// MARK: - Details block

/** `DetailsBlock.maxHeight` and the line count above which the block scrolls inside the card. */
const DETAILS_MAX_LINES = 200
const DETAILS_MAX_BYTES = 16_384
const DETAILS_INLINE_LINE_LIMIT = 13

/**
 * The request's details (command text or patch preview) on
 * `codeBackground`, growing with the text up to 240 px and scrolling
 * beyond that (only then: a nested scroller captures the transcript's wheel).
 */
function DetailsBlock({ text, language }: { text: string; language: Language }) {
  const lineCount = useMemo(() => capLines(text, DETAILS_MAX_LINES, DETAILS_MAX_BYTES).lines.length, [text])
  const highlight = lineHighlighter(language)
  const list = (
    <LineList
      text={text}
      maxLines={DETAILS_MAX_LINES}
      maxBytes={DETAILS_MAX_BYTES}
      highlight={highlight}
      cacheKey={highlight ? language : undefined}
      className={styles.detailsList}
    />
  )
  return (
    <div className={[styles.details, lineCount > DETAILS_INLINE_LINE_LIMIT ? styles.detailsScrolling : ''].filter(Boolean).join(' ')} aria-label="Details">
      {list}
    </div>
  )
}
