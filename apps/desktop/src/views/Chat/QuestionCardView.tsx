// Port of apps/macos/ContextAgent/Views/Chat/QuestionCardView.swift (and
// the `.transition(.opacity.combined(with: .move(edge: .bottom)))` it gets
// in ChatView's composer dock).

import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { Chip } from '../../components/Chip'
import { Tooltip } from '../../components/Tooltip'
import { questionAnswerIsEmpty, type AgentQuestion, type AgentQuestionOption, type QuestionAnswer, type QuestionRequest } from '../../backend/types'
import { useApp, useChatValue } from '../../state/app'
import type { ChatState } from '../../state/chat'
import { Icon } from '../../theme/icons'
import { Fade } from './ToolTimelineView'
import styles from './QuestionCardView.module.css'

export interface QuestionCardViewProps {
  chat: ChatState
}

/**
 * `QuestionCardView` of the macOS app: the agent's `ask_user` questions
 * (`chat.pendingQuestion`), shown above the composer while the run waits
 * for them; renders nothing when there are none. Each question has its
 * header chip and choices (radio rows, or checkboxes for `multi_select`)
 * plus an "Other" field for the user's own words. A single single-choice
 * question is answered by clicking a choice; otherwise *Submit* sends every
 * answer at once. *Skip* lets the agent decide by itself. While
 * `question/respond` is in flight the buttons give way to a spinner; a
 * failure renders inline.
 */
export function QuestionCardView({ chat }: QuestionCardViewProps) {
  const request = useChatValue(chat, (c) => c.pendingQuestion, null)
  // The last request stays on screen while the card fades out.
  const [shown, setShown] = useState<QuestionRequest | null>(request)
  if (request !== null && request !== shown) setShown(request)
  return (
    <Fade show={request !== null} variant="moveBottom">
      {shown ? (
        <div className={styles.dockSpacing}>
          <QuestionCard key={shown.request_id} request={shown} chat={chat} />
        </div>
      ) : null}
    </Fade>
  )
}

function QuestionCard({ request, chat }: { request: QuestionRequest; chat: ChatState }) {
  const inFlight = useChatValue(chat, (c) => c.questionInFlight, false)
  const error = useChatValue(chat, (c) => c.questionError, null)
  /** Chosen option labels per question index. */
  const [selected, setSelected] = useState<ReadonlyMap<number, ReadonlySet<string>>>(() => new Map())
  /** The "Other" text per question index. */
  const [other, setOther] = useState<ReadonlyMap<number, string>>(() => new Map())

  const questions = request.questions
  const first = questions[0]
  // One question with one answer: a click on a choice is the answer.
  const answersOnClick = questions.length === 1 && first !== undefined && !first.multi_select && first.options.length > 0

  const answersFrom = (chosen: ReadonlyMap<number, ReadonlySet<string>>, own: ReadonlyMap<number, string>): QuestionAnswer[] =>
    questions.map((question, index) => {
      const set = chosen.get(index) ?? new Set<string>()
      const picked = question.options.map((option) => option.label).filter((label) => set.has(label))
      const text = (own.get(index) ?? '').trim()
      return { selected: picked, other: text.length === 0 ? null : text }
    })

  const answers = answersFrom(selected, other)
  const canSubmit = answers.some((answer) => !questionAnswerIsEmpty(answer))

  const send = (payload: QuestionAnswer[] | null) => {
    if (chat.questionInFlight) return
    void useApp.getState().respondQuestion(request.request_id, chat, payload)
  }

  const submit = () => {
    if (!canSubmit) return
    send(answers)
  }

  const toggle = (label: string, question: AgentQuestion, index: number) => {
    let set = new Set(selected.get(index) ?? [])
    const nextOther = new Map(other)
    if (question.multi_select) {
      if (set.has(label)) set.delete(label)
      else set.add(label)
    } else {
      set = set.has(label) ? new Set() : new Set([label])
      nextOther.delete(index)
    }
    const nextSelected = new Map(selected)
    nextSelected.set(index, set)
    setSelected(nextSelected)
    setOther(nextOther)
    if (answersOnClick && set.size > 0) {
      const next = answersFrom(nextSelected, nextOther)
      if (next.some((answer) => !questionAnswerIsEmpty(answer))) send(next)
    }
  }

  const changeOther = (value: string, question: AgentQuestion, index: number) => {
    const nextOther = new Map(other)
    nextOther.set(index, value)
    setOther(nextOther)
    // Typing an own answer to a single-choice question replaces the choice.
    if (!question.multi_select && value.length > 0) {
      const nextSelected = new Map(selected)
      nextSelected.set(index, new Set())
      setSelected(nextSelected)
    }
  }

  const firstOther = (other.get(0) ?? '').trim()
  const showsSubmit = !answersOnClick || firstOther.length > 0

  return (
    <div
      className={styles.card}
      role="group"
      aria-label={'The agent asks: ' + questions.map((question) => question.question).join(' ')}
      aria-disabled={inFlight || undefined}
    >
      <div className={styles.header}>
        <Icon name="question" size={14} strokeWidth={2} className={styles.headerIcon} />
        <span className={styles.title}>{questions.length === 1 ? 'The agent has a question' : `The agent has ${questions.length} questions`}</span>
        <span className={styles.spacer} />
      </div>

      <div className={styles.questions}>
        {questions.map((question, index) => (
          <QuestionView
            key={index}
            question={question}
            chosen={selected.get(index) ?? EMPTY_SET}
            otherText={other.get(index) ?? ''}
            disabled={inFlight}
            onToggle={(label) => toggle(label, question, index)}
            onOtherChange={(value) => changeOther(value, question, index)}
            onSubmit={submit}
          />
        ))}
      </div>

      {error !== null && error.length > 0 ? (
        <div className={styles.error} role="alert">
          <Icon name="warning" size={12} strokeWidth={2} className={styles.errorIcon} />
          <span className="selectable">Could not send the answer: {error}</span>
        </div>
      ) : null}

      <div className={styles.buttons}>
        <span className={styles.hint}>Or reply in the message box below</span>
        <span className={styles.buttonSpacer} />
        {inFlight ? (
          <span className={styles.sending}>
            <BrailleSpinner size={12} />
            <span className={styles.sendingLabel}>Sending…</span>
          </span>
        ) : null}
        <Tooltip content="Let the agent decide by itself">
          <Button variant="plain" className={styles.borderless} disabled={inFlight} onClick={() => send(null)}>
            Skip
          </Button>
        </Tooltip>
        {showsSubmit ? (
          <Tooltip content="Send your answers to the agent">
            <Button variant="prominent" disabled={inFlight || !canSubmit} onClick={submit}>
              Submit
            </Button>
          </Tooltip>
        ) : null}
      </div>
    </div>
  )
}

const EMPTY_SET: ReadonlySet<string> = new Set()

// MARK: - One question

interface QuestionViewProps {
  question: AgentQuestion
  chosen: ReadonlySet<string>
  otherText: string
  disabled: boolean
  onToggle: (label: string) => void
  onOtherChange: (value: string) => void
  onSubmit: () => void
}

function QuestionView({ question, chosen, otherText, disabled, onToggle, onOtherChange, onSubmit }: QuestionViewProps) {
  const header = question.header?.trim() ?? ''
  return (
    <div className={styles.question}>
      <div className={styles.questionLine}>
        {header.length > 0 ? <Chip>{header}</Chip> : null}
        <span className={`${styles.questionText} selectable`}>{question.question}</span>
      </div>
      {question.multi_select ? <span className={styles.chooseAny}>Choose any that apply</span> : null}
      <div className={styles.options}>
        {question.options.map((option, optionIndex) => (
          <OptionRow
            key={optionIndex}
            number={optionIndex + 1}
            option={option}
            multiSelect={question.multi_select}
            isSelected={chosen.has(option.label)}
            disabled={disabled}
            onClick={() => onToggle(option.label)}
          />
        ))}
        <OtherField
          placeholder={question.options.length === 0 ? 'Your answer' : 'Other…'}
          value={otherText}
          disabled={disabled}
          label={`Your own answer to: ${question.question}`}
          onChange={onOtherChange}
          onSubmit={onSubmit}
        />
      </div>
    </div>
  )
}

/** The `TextField(axis: .vertical)` with `lineLimit(1...4)`: grows from one to four lines; Return submits. */
function OtherField({
  placeholder,
  value,
  disabled,
  label,
  onChange,
  onSubmit,
}: {
  placeholder: string
  value: string
  disabled: boolean
  label: string
  onChange: (value: string) => void
  onSubmit: () => void
}) {
  const ref = useRef<HTMLTextAreaElement>(null)

  // Grow with the text up to four lines, then scroll.
  useLayoutEffect(() => {
    const field = ref.current
    if (!field) return
    field.style.height = 'auto'
    const style = window.getComputedStyle(field)
    const lineHeight = parseFloat(style.lineHeight) || 17
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    const max = lineHeight * 4 + padding
    field.style.height = `${Math.min(field.scrollHeight, max)}px`
    field.style.overflowY = field.scrollHeight > max ? 'auto' : 'hidden'
  }, [value])

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== 'Enter' || event.nativeEvent.isComposing || event.shiftKey || event.altKey) return
    event.preventDefault()
    onSubmit()
  }

  return (
    <textarea
      ref={ref}
      rows={1}
      className={styles.other}
      placeholder={placeholder}
      value={value}
      disabled={disabled}
      aria-label={label}
      spellCheck={false}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={onKeyDown}
    />
  )
}

// MARK: - Option row

/** One choice: a radio or checkbox mark, the label and its description, its number at the right. */
function OptionRow({
  number,
  option,
  multiSelect,
  isSelected,
  disabled,
  onClick,
}: {
  number: number
  option: AgentQuestionOption
  multiSelect: boolean
  isSelected: boolean
  disabled: boolean
  onClick: () => void
}) {
  const description = option.description?.trim() ?? ''
  return (
    <button
      type="button"
      className={[styles.option, isSelected ? styles.optionSelected : ''].filter(Boolean).join(' ')}
      role={multiSelect ? 'checkbox' : 'radio'}
      aria-checked={isSelected}
      aria-label={option.label + (option.description ? '. ' + option.description : '')}
      disabled={disabled}
      onClick={onClick}
    >
      <span className={styles.mark} style={{ color: isSelected ? 'var(--accent)' : 'var(--text-tertiary)' }} aria-hidden="true">
        {multiSelect ? <CheckboxMark on={isSelected} /> : <RadioMark on={isSelected} />}
      </span>
      <span className={styles.optionText}>
        <span className={styles.optionLabel}>{option.label}</span>
        {description.length > 0 ? <span className={styles.optionDescription}>{description}</span> : null}
      </span>
      <span className={styles.optionSpacer} />
      <span className={styles.number} aria-hidden="true">
        {number}
      </span>
    </button>
  )
}

/// The option marks at the 13 pt symbol size: "circle" / "largecircle.fill.circle"
/// (a ring around a large dot, which lucide's CircleDot is not) and "square" /
/// "checkmark.square.fill" (the check knocked out of the fill, so it shows the
/// selected row's `accentSoft`).
function RadioMark({ on }: { on: boolean }) {
  return (
    <svg width={15} height={15} viewBox="0 0 16 16" aria-hidden="true">
      <circle cx={8} cy={8} r={6.6} fill="none" stroke="currentColor" strokeWidth={1.3} />
      {on ? <circle cx={8} cy={8} r={4} fill="currentColor" /> : null}
    </svg>
  )
}

function CheckboxMark({ on }: { on: boolean }) {
  return (
    <svg width={15} height={15} viewBox="0 0 16 16" aria-hidden="true">
      {on ? (
        <>
          <rect x={1.35} y={1.35} width={13.3} height={13.3} rx={3} fill="currentColor" />
          <path d="M4.8 8.2 7 10.4l4.3-4.8" fill="none" stroke="var(--accent-soft)" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" />
        </>
      ) : (
        <rect x={2} y={2} width={12} height={12} rx={2.6} fill="none" stroke="currentColor" strokeWidth={1.3} />
      )}
    </svg>
  )
}
