import { useEffect, useRef, useState } from 'react'
import { describe, type BackendFailure } from '../../backend/errors'
import { BrailleSpinner } from '../../components/BrailleSpinner'
import { Button } from '../../components/Button'
import { MiddleTruncate } from '../../components/MiddleTruncate'
import { Tooltip } from '../../components/Tooltip'
import { abbreviatedPath } from '../../models/recentProject'
import { BUILD_COMMAND, useApp } from '../../state/app'
import { Icon } from '../../theme/icons'
import styles from './BackendFailurePanel.module.css'

export interface BackendFailurePanelProps {
  failure: BackendFailure
  /// The Inspector's Backend card form (InspectorView.swift `failureSection`):
  /// the short summary in `danger` with small Retry and Choose binary…
  /// buttons, instead of the full panel.
  compact?: boolean
}

/**
 * `BackendFailurePanel` (Views/Welcome/WelcomeView.swift), shared by Welcome
 * and the Inspector's Backend card: the title ("ctx not found" / "ctx is too
 * old" / "Backend stopped"), a one-paragraph explanation, the searched paths
 * or the found path, the stderr tail, and Retry / Choose binary… / Copy
 * build command. `compact` renders the Inspector card's summary form.
 */
export function BackendFailurePanel({ failure, compact = false }: BackendFailurePanelProps) {
  if (compact) return <CompactFailure failure={failure} />
  return <FullFailure failure={failure} />
}

/// `BackendFailurePanel.summary(for:)`: the title's short form for the
/// Inspector card ("No ctx binary in 3 searched locations.").
function failureSummary(failure: BackendFailure): string {
  if (failure.kind === 'incompatible') return `Too old: ${abbreviatedPath(failure.found)}`
  return describe(failure).summary
}

function FullFailure({ failure }: { failure: BackendFailure }) {
  const backendStarting = useApp((state) => state.backendStarting)
  const description = describe(failure)
  return (
    <section className={styles.panel} aria-label={description.title}>
      <div className={styles.header}>
        <Icon name="warning" size={14} strokeWidth={2.1} className={styles.warning} />
        <h2 className={styles.title}>{description.title}</h2>
      </div>
      <p className={styles.explanation}>{description.explanation}</p>
      <FailureDetails failure={failure} />
      <div className={styles.buttons}>
        <Button variant="prominent" icon="refresh" loading={backendStarting} onClick={() => void useApp.getState().restartBackend()}>
          Retry
        </Button>
        <Button variant="bordered" onClick={() => void useApp.getState().chooseBackendBinary()}>
          Choose binary…
        </Button>
        <CopyBuildCommandButton />
      </div>
    </section>
  )
}

/** The searched / found paths and the stderr tail of a failure. */
function FailureDetails({ failure }: { failure: BackendFailure }) {
  switch (failure.kind) {
    case 'notFound':
      return failure.searched.length > 0 ? <PathList title="Searched" paths={failure.searched} /> : null
    case 'incompatible':
      return (
        <>
          <PathList title="Found" paths={[failure.found]} />
          {failure.detail.length > 0 ? <CodeBlock text={failure.detail} /> : null}
        </>
      )
    case 'exited':
      return failure.tail.length > 0 ? <CodeBlock text={failure.tail} /> : null
    case 'transport':
      return failure.message.length > 0 ? <CodeBlock text={failure.message} /> : null
  }
}

function PathList({ title, paths }: { title: string; paths: string[] }) {
  return (
    <div className={styles.pathList}>
      <div className={styles.pathTitle}>{title}</div>
      {paths.map((path) => (
        <MiddleTruncate key={path} text={abbreviatedPath(path)} className={`${styles.path} selectable`} />
      ))}
    </div>
  )
}

/** The stderr tail / detail: monospace, selectable, scrolling past 160 px. */
function CodeBlock({ text }: { text: string }) {
  return <pre className={`${styles.code} selectable`}>{text}</pre>
}

/** "Copy build command" → a check and "Copied" for 1.2 s. */
function CopyBuildCommandButton() {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | null>(null)
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current)
    },
    [],
  )
  const copy = () => {
    useApp.getState().copyBuildCommand()
    setCopied(true)
    if (timer.current !== null) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      timer.current = null
      setCopied(false)
    }, 1200)
  }
  return (
    <Tooltip content={BUILD_COMMAND}>
      <Button variant="bordered" icon={copied ? 'copied' : 'copy'} onClick={copy}>
        {copied ? 'Copied' : 'Copy build command'}
      </Button>
    </Tooltip>
  )
}

/// InspectorView.swift `failureSection`: the summary (four lines at most)
/// and the small Retry (with a spinner while the backend starts) and Choose
/// binary… buttons.
function CompactFailure({ failure }: { failure: BackendFailure }) {
  const backendStarting = useApp((state) => state.backendStarting)
  return (
    <div className={styles.compact}>
      <p className={`${styles.summary} selectable`}>{failureSummary(failure)}</p>
      <div className={styles.compactButtons}>
        <Button variant="prominent" size="small" disabled={backendStarting} onClick={() => void useApp.getState().restartBackend()}>
          <span className={styles.spinnerLabel}>
            {backendStarting ? <BrailleSpinner size={10} tint="var(--text-secondary)" /> : null}
            Retry
          </span>
        </Button>
        <Button variant="bordered" size="small" onClick={() => void useApp.getState().chooseBackendBinary()}>
          Choose binary…
        </Button>
      </div>
    </div>
  )
}
