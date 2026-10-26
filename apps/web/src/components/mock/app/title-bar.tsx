import { Box, CodeXml, Folder, Hand, MessageSquare, PanelRight, Search, Settings, Split, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { TrafficLights } from "@/components/ui/window";
import { cn } from "@/lib/cn";
import { Chip, stroke } from "./parts";

/** A toolbar tab. `display` replaces the default `flex`, to hide it below a breakpoint. */
function Tab({
  icon: Icon,
  children,
  count,
  active = false,
  display = "flex",
}: {
  icon: LucideIcon;
  children: ReactNode;
  count?: number;
  active?: boolean;
  display?: string;
}) {
  return (
    <span
      className={cn(
        "h-7 items-center gap-1.5 rounded-[8px] px-2",
        display,
        active ? "bg-white font-medium text-ink shadow-[0_1px_2px_rgba(0,0,0,0.06)] ring-1 ring-black/[0.08]" : "text-muted",
      )}
    >
      <Icon size={14} strokeWidth={stroke} className={active ? "text-ink" : "text-muted"} />
      {children}
      {count !== undefined && <span className="ml-0.5 text-[11px] tabular-nums text-faint">{count}</span>}
    </span>
  );
}

/**
 * The unified toolbar: window controls, the Chat / Changes / Project /
 * Editor tabs, chat search in the middle and the project, branch and
 * permission mode on the right. Collapses to controls, Chat and a search
 * glyph on narrow screens.
 */
export function TitleBar() {
  return (
    <div className="flex h-11 shrink-0 items-center gap-3 border-b border-line-soft bg-[#fbfbfc] px-3.5 text-[12.5px]">
      {/* Equal-growth sides keep the search centred until the tabs need the room. */}
      <div className="flex flex-1 basis-0 items-center">
        <TrafficLights />
        <div className="ml-4 flex items-center gap-0.5 md:ml-5">
          <Tab icon={MessageSquare} active>
            Chat
          </Tab>
          <Tab icon={Split} count={3} display="hidden sm:flex">
            Changes
          </Tab>
          <Tab icon={Box} display="hidden lg:flex">
            Project
          </Tab>
          <Tab icon={CodeXml} display="hidden xl:flex">
            Editor
          </Tab>
        </div>
      </div>

      <div className="hidden h-7 w-[150px] shrink-0 items-center gap-1.5 rounded-[8px] bg-white px-2.5 text-faint shadow-[0_1px_1px_rgba(0,0,0,0.03)] ring-1 ring-black/[0.09] md:flex lg:w-[200px] xl:w-[232px]">
        <Search size={13} strokeWidth={stroke} className="shrink-0" />
        <span className="truncate">Search chats&hellip;</span>
        <span className="ml-auto rounded-[4px] bg-black/[0.05] px-1 font-mono text-[10px] leading-[16px] text-muted">⌘K</span>
      </div>

      <div className="flex flex-1 basis-0 items-center justify-end gap-1.5">
        <Search size={15} strokeWidth={stroke} className="text-muted md:hidden" />
        <span className="mr-1 hidden items-center gap-1.5 font-medium text-ink lg:flex">
          <Folder size={14} strokeWidth={stroke} className="text-muted" />
          context
        </span>
        <span className="hidden sm:flex">
          <Chip>
            <Split size={11} strokeWidth={stroke} className="text-muted" />
            main
          </Chip>
        </span>
        <span className="hidden lg:flex">
          <Chip>
            <Hand size={11} strokeWidth={stroke} className="text-muted" />
            Ask
          </Chip>
        </span>
        <span className="ml-1 hidden size-7 place-items-center rounded-[7px] bg-accent-soft text-accent lg:grid">
          <PanelRight size={15} strokeWidth={stroke} />
        </span>
        <span className="hidden size-7 place-items-center text-muted md:grid">
          <Settings size={15} strokeWidth={stroke} />
        </span>
      </div>
    </div>
  );
}
