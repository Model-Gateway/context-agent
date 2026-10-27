// Port of apps/macos/ContextAgent/Views/Projects/ProjectsView.swift: the
// Projects page of the sidebar. The cards are the Welcome screen's
// `RecentCard` (views/Welcome/RecentCard.tsx), shared as in Swift.

import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react'
import { Button } from '../../components/Button'
import { EmptyPlaceholder } from '../../components/EmptyPlaceholder'
import { Menu, type MenuItem } from '../../components/Menu'
import { SearchField } from '../../components/SearchField'
import { Tooltip } from '../../components/Tooltip'
import { abbreviatedPath, recentProjectExists, type RecentProject } from '../../models/recentProject'
import { useApp } from '../../state/app'
import { useIsReady, useRecentProjectCards } from '../../state/selectors'
import { Icon } from '../../theme/icons'
import { RecentCard } from '../Welcome/RecentCard'
import styles from './ProjectsView.module.css'

/** `ProjectFilter`: which remembered projects the grid shows. */
type ProjectFilter = 'all' | 'pinned' | 'open' | 'missing'

const PROJECT_FILTERS: readonly ProjectFilter[] = ['all', 'pinned', 'open', 'missing']

function filterTitle(filter: ProjectFilter): string {
  switch (filter) {
    case 'all':
      return 'All projects'
    case 'pinned':
      return 'Pinned'
    case 'open':
      return 'Open'
    case 'missing':
      return 'Missing'
  }
}

/** `ProjectSort`. */
type ProjectSort = 'lastOpened' | 'name'

const PROJECT_SORTS: readonly ProjectSort[] = ['lastOpened', 'name']

function sortTitle(sort: ProjectSort): string {
  return sort === 'lastOpened' ? 'Last opened' : 'Name'
}

/** `localizedCaseInsensitiveContains`. */
function containsInsensitive(text: string, needle: string): boolean {
  return text.toLocaleLowerCase().includes(needle.toLocaleLowerCase())
}

/** `localizedStandardCompare` (Finder order: case-insensitive, numbers by value). */
const standardCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

/**
 * `ProjectsView` (Views/Projects/ProjectsView.swift): every remembered
 * project as the Welcome screen's recent cards, in a wider grid. The toolbar
 * has the search field at the left (name or path), the filter menu (All /
 * Pinned / Open / Missing, plus the sort order) beside it and "New project"
 * at the right.
 */
export function ProjectsView() {
  const all = useRecentProjectCards()
  const projects = useApp((state) => state.projects)
  const ready = useIsReady()
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<ProjectFilter>('all')
  const [sort, setSort] = useState<ProjectSort>('lastOpened')
  const exists = useExistence(all)

  const openRoots = useMemo(() => new Set(projects.map((project) => project.root)), [projects])
  const isOpen = (recent: RecentProject) => openRoots.has(recent.path)
  const existsAt = (recent: RecentProject) => exists.get(recent.path) ?? true

  // Search (name or path) and filter applied to the pinned-first list, then
  // the chosen sort; pinned stay first either way.
  const needle = query.trim()
  const matched = all.filter((recent) => {
    const matchesQuery = needle.length === 0 || containsInsensitive(recent.name, needle) || containsInsensitive(abbreviatedPath(recent.path), needle)
    if (!matchesQuery) return false
    switch (filter) {
      case 'all':
        return true
      case 'pinned':
        return recent.pinned
      case 'open':
        return isOpen(recent)
      case 'missing':
        return !existsAt(recent)
    }
  })
  const shown =
    sort === 'lastOpened'
      ? matched
      : [...matched].sort((a, b) => {
          if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
          return standardCollator.compare(a.name, b.name)
        })

  const noun = all.length === 1 ? 'project' : 'projects'
  const countLabel = shown.length === all.length ? `${all.length} ${noun}` : `${shown.length} of ${all.length} ${noun}`

  return (
    <div className={styles.root} role="region" aria-label="Projects">
      <div className={styles.content}>
        <div className={styles.toolbar}>
          <SearchField placeholder="Search projects" value={query} onChange={setQuery} className={styles.search} />
          <FilterMenu filter={filter} sort={sort} onFilter={setFilter} onSort={setSort} />
          <span className={styles.toolbarSpacer} />
          <Help text={ready ? 'Choose or create a folder for a new project' : 'The backend is not ready'}>
            <Button variant="prominent" disabled={!ready} onClick={() => void useApp.getState().chooseProject(true)}>
              <span className={styles.newProjectLabel}>
                <Icon name="newChat" size={12} strokeWidth={2.5} />
                New project
              </span>
            </Button>
          </Help>
        </div>
        {all.length === 0 ? (
          <div className={styles.empty}>
            <EmptyPlaceholder
              icon="folder"
              title="No projects yet"
              subtitle="Projects you open show up here."
              actionTitle="Open a project…"
              onAction={() => void useApp.getState().chooseProject()}
            />
          </div>
        ) : (
          <>
            <div className={styles.count}>{countLabel}</div>
            {shown.length === 0 ? (
              <div className={styles.noMatches}>
                <div className={styles.noMatchesTitle}>No projects match</div>
                <Button
                  variant="bordered"
                  onClick={() => {
                    setQuery('')
                    setFilter('all')
                  }}
                >
                  Clear search and filter
                </Button>
              </div>
            ) : (
              <div className={styles.grid}>
                {shown.map((recent) => (
                  <RecentCard key={recent.path} recent={recent} isOpen={isOpen(recent)} />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}

// MARK: - Toolbar

/// The filter menu: "Show" (All projects / Pinned / Open / Missing) and
/// "Sort by" (Last opened / Name) as checked sections; the button reads the
/// active filter in `accent`.
function FilterMenu({ filter, sort, onFilter, onSort }: { filter: ProjectFilter; sort: ProjectSort; onFilter: (filter: ProjectFilter) => void; onSort: (sort: ProjectSort) => void }) {
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement>(null)
  const active = filter !== 'all'
  const items: MenuItem[] = [
    { type: 'header', label: 'Show' },
    ...PROJECT_FILTERS.map((option): MenuItem => ({ id: `filter-${option}`, label: filterTitle(option), checked: option === filter, onSelect: () => onFilter(option) })),
    { type: 'separator' },
    { type: 'header', label: 'Sort by' },
    ...PROJECT_SORTS.map((option): MenuItem => ({ id: `sort-${option}`, label: sortTitle(option), checked: option === sort, onSelect: () => onSort(option) })),
  ]
  return (
    <>
      <Tooltip content="Filter and sort projects">
        <button
          ref={anchorRef}
          type="button"
          className={[styles.filterButton, active ? styles.filterActive : ''].filter(Boolean).join(' ')}
          aria-label={`Filter: ${filterTitle(filter)}, sorted by ${sortTitle(sort)}`}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <Icon name="filter" size={12} strokeWidth={2} />
          <span className={styles.filterText}>{active ? filterTitle(filter) : 'Filter'}</span>
          <Icon name="chevronDown" size={9} strokeWidth={2.5} className={styles.filterChevron} />
        </button>
      </Tooltip>
      <Menu open={open} onClose={() => setOpen(false)} items={items} anchorRef={anchorRef} aria-label="Filter and sort projects" />
    </>
  )
}

// MARK: - Helpers

/// `RecentProject.exists` for every listed folder (`fs_file_info`, async
/// here): a folder counts as present until the check says otherwise, so
/// nothing flashes "Missing" while it runs. Re-checked when the list changes
/// and when the window regains focus.
function useExistence(recents: RecentProject[]): Map<string, boolean> {
  const [exists, setExists] = useState<Map<string, boolean>>(() => new Map())
  const key = recents.map((recent) => recent.path).join('\n')
  useEffect(() => {
    let cancelled = false
    const check = async () => {
      const results = await Promise.all(recents.map(async (recent) => [recent.path, await recentProjectExists(recent)] as const))
      if (cancelled) return
      setExists((current) => {
        const next = new Map(results)
        if (next.size === current.size && results.every(([path, value]) => current.get(path) === value)) return current
        return next
      })
    }
    void check()
    const onFocus = () => void check()
    window.addEventListener('focus', onFocus)
    return () => {
      cancelled = true
      window.removeEventListener('focus', onFocus)
    }
    // `key` stands for the list of paths; the list object changes with every pin or open.
  }, [key])
  return exists
}

/// `.help(_:)` on a control that can be disabled: the tooltip hangs off a
/// wrapper, so it still shows while the button is disabled.
function Help({ text, children }: { text: string; children: ReactElement }) {
  return (
    <Tooltip content={text}>
      <span className={styles.help}>{children}</span>
    </Tooltip>
  )
}
