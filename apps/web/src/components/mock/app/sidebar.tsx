import { ChevronRight, CircleUserRound, Folder, FolderCog, LayoutGrid, Plus, Split } from "lucide-react";
import type { ReactNode } from "react";
import { RunningDots, stroke, WaveIcon } from "./parts";

function NavRow({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="flex h-[30px] items-center gap-2.5 rounded-[8px] px-2 text-ink">
      <span className="grid w-4 shrink-0 place-items-center text-ink-2">{icon}</span>
      {children}
    </div>
  );
}

function Heading({ children }: { children: ReactNode }) {
  return <div className="mt-4 px-2 pb-1 text-[11.5px] text-faint">{children}</div>;
}

function SelectedRow({ children }: { children: ReactNode }) {
  return <div className="flex h-[30px] items-center gap-2 rounded-[8px] bg-black/[0.05] px-2">{children}</div>;
}

/**
 * The app's left column: navigation, the open project with its branch, the
 * chat list with this chat's running indicator, and the optional
 * ModelGateway sign-in.
 */
export function Sidebar() {
  return (
    <div className="hidden w-[188px] shrink-0 flex-col border-r border-line-soft bg-[#fbfbfc] px-2 pb-2 pt-2.5 text-[12.5px] md:flex xl:w-[204px]">
      <NavRow icon={<WaveIcon size={16} />}>Welcome</NavRow>
      <NavRow icon={<Plus size={16} strokeWidth={stroke} />}>New chat</NavRow>
      <NavRow icon={<FolderCog size={16} strokeWidth={stroke} />}>Projects</NavRow>
      <NavRow icon={<LayoutGrid size={15} strokeWidth={stroke} />}>Apps</NavRow>

      <Heading>Projects</Heading>
      <SelectedRow>
        <Folder size={15} strokeWidth={stroke} className="shrink-0 text-accent" />
        <span className="text-ink">context</span>
        <span className="flex min-w-0 items-center gap-0.5 text-[11.5px] text-muted">
          <Split size={11} strokeWidth={stroke} className="shrink-0" />
          main
        </span>
      </SelectedRow>

      <Heading>Chats</Heading>
      <SelectedRow>
        <span className="min-w-0 truncate text-ink">How does Context guarantee an edit never overwrites a file</span>
        <RunningDots className="ml-auto mr-1 shrink-0 text-muted" />
      </SelectedRow>

      <div className="mt-auto flex items-center gap-2 rounded-[8px] px-1.5 py-1.5">
        <CircleUserRound size={20} strokeWidth={1.4} className="shrink-0 text-muted" />
        <span className="min-w-0 leading-tight">
          <span className="block truncate text-ink">Sign in with ModelGateway</span>
          <span className="block truncate text-[11px] text-muted">Optional &middot; usage and billing</span>
        </span>
        <ChevronRight size={13} strokeWidth={stroke} className="ml-auto shrink-0 text-faint" />
      </div>
    </div>
  );
}
