import { ChartNoAxesColumn, Folder, GitFork, Globe, Hand, Settings, Split, SquareTerminal, X } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { site } from "@/lib/site";
import { CardHeading, Chip, RunningDots, StatRow, stroke } from "./parts";

/** One of the inspector's white cards. */
function Card({ children }: { children: ReactNode }) {
  return <div className="rounded-[10px] bg-white p-3 ring-1 ring-black/[0.08]">{children}</div>;
}

const tabs = [ChartNoAxesColumn, Folder, SquareTerminal, GitFork, Globe];

/**
 * The right-hand inspector on its status tab: the connection card, the live
 * task and how the prompt was assembled. Hidden below `lg`.
 */
export function Inspector() {
  return (
    <div className="hidden w-[232px] shrink-0 flex-col gap-2.5 border-l border-line-soft bg-[#fbfbfc] px-2.5 pt-2.5 text-[12px] leading-[1.45] lg:flex xl:w-[252px]">
      <div className="flex items-center">
        <div className="flex items-center gap-0.5 rounded-[9px] bg-black/[0.045] p-[3px]">
          {tabs.map((Icon, i) => (
            <span
              key={i}
              className={cn(
                "grid h-[24px] w-[30px] place-items-center rounded-[7px]",
                i === 0 ? "bg-white text-ink shadow-[0_1px_2px_rgba(0,0,0,0.08)]" : "text-muted",
              )}
            >
              <Icon size={14} strokeWidth={stroke} />
            </span>
          ))}
        </div>
        <X size={15} strokeWidth={stroke} className="ml-auto mr-1 text-muted" />
      </div>

      <Card>
        <div className="flex items-center gap-2">
          <span className="size-[7px] shrink-0 rounded-full bg-green" />
          <span className="truncate font-medium text-ink">ctx {site.version} &middot; connected</span>
          <Settings size={13} strokeWidth={stroke} className="ml-auto shrink-0 text-faint" />
        </div>
        <div className="mt-1.5 flex items-center gap-1.5 text-muted">
          Claude Sonnet 5.5
          <Chip>default</Chip>
        </div>
        <div className="mt-1.5 flex items-center gap-1.5 text-muted">
          <Hand size={12} strokeWidth={stroke} />
          Ask &middot; context
        </div>
      </Card>

      <Card>
        <CardHeading>Task</CardHeading>
        <div className="mt-2 flex items-center justify-between">
          <span className="inline-flex h-[22px] items-center gap-1.5 rounded-full bg-accent-soft pl-2 pr-2.5 font-medium text-accent">
            <RunningDots />
            Running
          </span>
          <span className="font-mono text-[11.5px] tabular-nums text-ink">1m 12s</span>
        </div>
        <div className="mt-2.5 space-y-1.5">
          <StatRow label="State" value="Investigating" />
          <StatRow label="Step" value="1" />
          <StatRow label="Session model" value="Claude Sonnet 5.5" />
        </div>
      </Card>

      <Card>
        <CardHeading>Context</CardHeading>
        <div className="mt-2.5 h-[5px] overflow-hidden rounded-full bg-black/[0.06]">
          {/* 28.7k of a 256k window. */}
          <div className="h-full w-[11.2%] rounded-full bg-accent" />
        </div>
        <div className="mt-2.5 space-y-1.5">
          <StatRow label="Prompt estimate" value="28.7k / 256k" />
          <StatRow label="Stable prefix" value="14.2k" />
          <StatRow label="Repository" value="40 snippets" />
          <StatRow label="Memory" value="0 records" />
          <StatRow label="Conversation" value="1 msg" />
          <StatRow label="Retrieval budget" value="24k" />
        </div>
      </Card>

      <Card>
        <CardHeading>Changes</CardHeading>
        <div className="mt-2 flex items-center justify-between">
          <span className="text-ink">3 files changed</span>
          <span className="font-mono text-[11.5px]">
            <span className="text-green">+385</span> <span className="text-red">&minus;57</span>
          </span>
        </div>
        <div className="mt-1.5 flex items-center gap-1 font-mono text-[11.5px] text-muted">
          <Split size={12} strokeWidth={stroke} />
          main
        </div>
      </Card>
    </div>
  );
}
