import { lazy, Suspense } from 'react'
import { Skeleton } from '../../components/Skeleton'
import { getPref } from '../../models/preferences'
import { useApp, type AppStore, type SidebarItem } from '../../state/app'
import { pendingProjectPaths, selectedChat, selectedProject } from '../../state/selectors'
import { ChangesView } from '../Changes/ChangesView'
import { ChatTabView } from '../Chat/ChatView'
import { ProjectView } from '../Project/ProjectView'
import { ProjectsView } from '../Projects/ProjectsView'
import { ProfileView } from '../Settings/SettingsView'
import { WelcomeView } from '../Welcome/WelcomeView'
import styles from './DetailView.module.css'

// The Editor (CodeMirror) and Apps load on first use (views/preload.ts warms
// them once the app is up), so startup does not wait for them.
const EditorView = lazy(() => import('../Editor/EditorView').then((module) => ({ default: module.EditorView })))
const AppsView = lazy(() => import('../Apps/AppsView').then((module) => ({ default: module.AppsView })))

/**
 * `DetailView` (Views/Detail/DetailView.swift): the centre pane under the
 * window-wide top bar: the screen for `sidebarItem` — Welcome, Projects,
 * Apps, the Chat tab, Changes, Project, Editor or the profile page (Account
 * and the settings tabs).
 */
export function DetailView() {
  const item = useApp((state) => state.sidebarItem)
  return (
    <div className={styles.detail}>
      <Suspense fallback={<div className={styles.loading} aria-busy="true" />}>{content(item)}</Suspense>
    </div>
  )
}

function content(item: SidebarItem) {
  switch (item) {
    case 'welcome':
      return <WelcomeView />
    case 'projects':
      return <ProjectsView />
    case 'apps':
      return <AppsView />
    case 'chat':
      return <ChatTab />
    case 'changes':
      return <ChangesView />
    case 'project':
      return <ProjectView />
    case 'editor':
      return <EditorView />
    case 'account':
      return <ProfileView />
  }
}

/** What the Chat tab shows while no chat view can (`DetailView.content`'s `.chat` case). */
type ChatTabState = 'tab' | 'loading' | 'emptySkeleton'

/// The remembered chat being looked up (`restoringSessionId`), or the
/// project about to be selected still opening: the shape of what it will
/// land on. Otherwise `ChatTabView` (the chat, the empty chat, or "No project
/// open").
function chatTabState(state: AppStore): ChatTabState {
  if (selectedChat(state)) return 'tab'
  if (selectedProject(state)) return state.restoringSessionId !== null ? 'loading' : 'tab'
  if (pendingProjectPaths(state).length > 0) return rememberedChatPending() ? 'loading' : 'emptySkeleton'
  return 'tab'
}

/** The last selected project had a chat open when the app quit. */
function rememberedChatPending(): boolean {
  const id = getPref('lastSelectedProjectId')
  if (id === null) return false
  return getPref('lastSelectedSession')[id] !== undefined
}

function ChatTab() {
  const state = useApp(chatTabState)
  switch (state) {
    case 'loading':
      return <Skeleton.ChatLoading />
    case 'emptySkeleton':
      return <Skeleton.EmptyChat />
    case 'tab':
      return <ChatTabView />
  }
}
