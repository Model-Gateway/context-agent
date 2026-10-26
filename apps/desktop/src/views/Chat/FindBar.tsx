// The find bar of apps/macos/ContextAgent/Views/Chat/ChatView.swift
// (`FindBar`, a private struct there).

import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { Button } from '../../components/Button'
import { IconButton } from '../../components/IconButton'
import { Icon } from '../../theme/icons'
import styles from './FindBar.module.css'

export interface FindBarProps {
  query: string
  onQueryChange: (query: string) => void
  matchCount: number
  /** The current match (0-based; meaningful while `matchCount > 0`). */
  cursor: number
  onPrevious: () => void
  onNext: () => void
  onClose: () => void
}

/**
 * `FindBar` (ChatView.swift): the mod+F bar over the transcript: the "Find
 * in chat" field (Return → next match, Esc closes), "2 of 7" / "No
 * matches", previous / next buttons and Done.
 */
export function FindBar({ query, onQueryChange, matchCount, cursor, onPrevious, onNext, onClose }: FindBarProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [focused, setFocused] = useState(false)

  // `.onAppear`: focus the field once the bar is in the window.
  useEffect(() => {
    const timer = window.setTimeout(() => inputRef.current?.focus(), 40)
    return () => window.clearTimeout(timer)
  }, [])

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return
    if (event.key === 'Enter') {
      event.preventDefault()
      onNext()
    } else if (event.key === 'Escape') {
      // `.onExitCommand(perform: onClose)`.
      event.preventDefault()
      event.stopPropagation()
      onClose()
    }
  }

  const countLabel = query.trim().length === 0 ? '' : matchCount === 0 ? 'No matches' : `${cursor + 1} of ${matchCount}`

  return (
    <div className={`${styles.bar} hairline-bottom`}>
      <div className={[styles.field, focused ? styles.focused : ''].filter(Boolean).join(' ')}>
        <Icon name="search" size={12} strokeWidth={2} className={styles.searchIcon} />
        <input
          ref={inputRef}
          className={styles.input}
          type="text"
          value={query}
          placeholder="Find in chat"
          aria-label="Find in chat"
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={onKeyDown}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
        />
        {query.length > 0 ? (
          <button
            type="button"
            className={styles.clear}
            aria-label="Clear search"
            onClick={() => {
              onQueryChange('')
              inputRef.current?.focus()
            }}
          >
            <Icon name="clear" size={12} />
          </button>
        ) : null}
      </div>
      <span className={styles.count} aria-label={countLabel.length === 0 ? 'No search' : countLabel}>
        {countLabel}
      </span>
      <IconButton icon="chevronUp" label="Previous match" size={12} disabled={matchCount === 0} onClick={onPrevious} />
      <IconButton icon="chevronDown" label="Next match" size={12} disabled={matchCount === 0} onClick={onNext} />
      <span className={styles.spacer} />
      <Button size="small" onClick={onClose}>
        Done
      </Button>
    </div>
  )
}
