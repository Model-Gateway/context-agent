import {
  Brain,
  ChevronDown,
  ChevronRight,
  CircleCheck,
  EllipsisVertical,
  FileText,
  GitFork,
  Hammer,
  LoaderCircle,
  Search,
} from "lucide-react";
import { Fragment, type ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Chip, mockSurface } from "./parts";

type AgentState = "completed" | "running";

const agents: {
  name: string;
  role: "Explore" | "Implement";
  state: AgentState;
  detail: string;
  elapsed: string;
}[] = [
  { name: "retrieval-explorer", role: "Explore", state: "completed", detail: "80 tool calls", elapsed: "48s" },
  { name: "patch-explorer", role: "Explore", state: "running", detail: "35 tool calls", elapsed: "1m 12s" },
  { name: "ui-implementer", role: "Implement", state: "running", detail: "12 tool calls", elapsed: "1m 12s" },
];

/** The latest tool calls of the running explorer, shown in its own pane. */
const toolCalls = [
  { tool: "search_code", arg: '"content_hash"' },
  { tool: "read_file", arg: "crates/tools/src/files.rs" },
  { tool: "find_symbol", arg: "apply_edits" },
  { tool: "find_references", arg: "atomic_write" },
  { tool: "read_file", arg: "crates/tools/src/activity.rs" },
  { tool: "search_code", arg: '"changed since read"' },
  { tool: "read_file", arg: "crates/tools/tests/execution.rs" },
  { tool: "find_symbol", arg: "check_edit" },
] as const;

const stateLabel: Record<AgentState, string> = {
  completed: "Completed",
  running: "Thinking",
};

function StatusIcon({ state }: { state: AgentState }) {
  const box = "relative z-10 grid size-[24px] shrink-0 place-items-center rounded-[7px] ring-1 ring-inset";
  if (state === "completed") {
    return (
      <span className={cn(box, "bg-green-soft ring-green/15")}>
        <CircleCheck size={14} strokeWidth={1.8} className="text-green" />
      </span>
    );
  }
  return (
    <span className={cn(box, "bg-accent-soft ring-accent/15")}>
      <LoaderCircle size={14} strokeWidth={1.8} className="text-accent motion-safe:animate-spin" />
    </span>
  );
}

function Code({ children }: { children: ReactNode }) {
  return (
    <code className="rounded-[4px] bg-surface-2 px-1 py-px font-mono text-[11.5px] text-ink-2 ring-1 ring-inset ring-black/[0.04]">
      {children}
    </code>
  );
}

/**
 * The chat pane while three sub-agents run at the Ultra effort tier, with
 * the running explorer's tool calls beside it. Wider than its photo on
 * purpose: the panel crops the second pane at the right edge. Sized against
 * the nearest `@container` (the card), so on phones the chat pane still fits
 * and only a sliver of the second pane shows.
 */
export function SubAgentsMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="The Context app running three sub-agents at once: retrieval-explorer has completed after 80 tool calls and reported back, while patch-explorer and ui-implementer are still working."
      className={cn(mockSurface, "h-[520px] w-[780px]", className)}
    >
      <div aria-hidden className="flex h-full">
        {/*
          Chat pane: the card's width less the mock's left inset (20px, 32px
          from sm) and a 14px sliver of the next pane. Where the cards sit two
          across but are still narrow (lg), it fills the card instead of
          leaving a few cut-off letters of the second pane.
        */}
        <div className="w-[min(360px,calc(100cqw-34px))] shrink-0 border-r border-line-soft px-4 pt-4 sm:px-6 sm:pt-5 lg:w-[calc(100cqw-46px)] xl:w-[360px]">
          <div className="flex items-center gap-1.5 text-[12.5px] text-muted">
            <Brain size={14} strokeWidth={1.6} className="text-faint" />
            <span>Thinking</span>
            <span className="text-faint">0.4s</span>
            <ChevronRight size={13} strokeWidth={1.6} className="text-faint" />
          </div>

          <div className="mt-4 flex items-start gap-2.5">
            <span className="grid size-[24px] shrink-0 place-items-center rounded-[7px] bg-surface-2 ring-1 ring-inset ring-black/[0.05]">
              <EllipsisVertical size={13} strokeWidth={1.8} className="text-faint" />
            </span>
            <p className="min-w-0 flex-1 pt-[3px] text-[13px] leading-[1.45] text-ink-2">
              Deploying 3 agents:{" "}
              {agents.map((agent, i) => (
                <Fragment key={agent.name}>
                  {i > 0 && ", "}
                  <span className="whitespace-nowrap">{agent.name}</span>
                </Fragment>
              ))}
            </p>
            <Chip className="mt-px">agents</Chip>
          </div>

          <div className="mt-5 flex items-center gap-2">
            <GitFork size={15} strokeWidth={1.7} className="text-accent" />
            <span className="text-[13.5px] font-medium tracking-[-0.01em] text-ink">3 agents, 2 running</span>
            <ChevronDown size={13} strokeWidth={1.6} className="text-faint" />
            <span className="ml-auto font-mono text-[11.5px] tabular-nums text-muted">1m 12s</span>
          </div>

          <ul className="relative mt-3.5 space-y-3">
            {/* The thread that joins the agents' status icons. */}
            <span className="absolute bottom-6 left-[11.5px] top-6 w-px bg-line" />
            {agents.map((agent) => (
              <li key={agent.name} className="flex items-start gap-2.5">
                <StatusIcon state={agent.state} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate text-[13px] text-ink">{agent.name}</span>
                    <Chip>
                      {agent.role === "Explore" ? (
                        <Search size={11} strokeWidth={1.8} />
                      ) : (
                        <Hammer size={11} strokeWidth={1.8} />
                      )}
                      {agent.role}
                    </Chip>
                  </div>
                  <p className="mt-1 text-[11.5px] leading-none text-muted">
                    <span className={cn(agent.state === "completed" && "text-green")}>{stateLabel[agent.state]}</span>
                    <span> &middot; {agent.detail}</span>
                  </p>
                </div>
                {/* Per-agent clocks need room; on phone-width cards the group clock is enough. */}
                <span className="pt-1 font-mono text-[11.5px] tabular-nums text-faint @max-[400px]:hidden">
                  {agent.elapsed}
                </span>
              </li>
            ))}
          </ul>

          <div className="mt-5 border-t border-line-soft pt-4">
            <p className="flex items-center gap-1.5 text-[11.5px] text-muted">
              <CircleCheck size={12} strokeWidth={1.8} className="text-green" />
              retrieval-explorer reported back
            </p>
            <p className="mt-2 text-[13px] leading-[1.6] text-ink-2">
              Candidates are fused with reciprocal-rank fusion in <Code>crates/retrieval/src/fusion.rs</Code>, then
              checked against the working tree before they are packed into the budget
              <span className="ml-0.5 inline-block h-[13px] w-[6px] translate-y-[2px] bg-ink motion-safe:animate-blink" />
            </p>
          </div>
        </div>

        {/* The running sub-agent's own transcript, cropped by the photo's right edge. */}
        <div className="min-w-0 flex-1 bg-[#fafafb] px-3.5 pt-4">
          <div className="flex gap-1.5 whitespace-nowrap">
            <span className="inline-flex h-[26px] items-center gap-1.5 rounded-[7px] bg-surface px-2 text-[12px] text-ink shadow-[0_1px_2px_rgba(10,10,20,0.08)] ring-1 ring-black/[0.06]">
              <LoaderCircle size={12} strokeWidth={1.9} className="text-accent motion-safe:animate-spin" />
              patch-explorer
            </span>
            <span className="inline-flex h-[26px] items-center gap-1.5 rounded-[7px] px-2 text-[12px] text-muted">
              <LoaderCircle size={12} strokeWidth={1.9} className="text-accent motion-safe:animate-spin" />
              ui-implementer
            </span>
            <span className="inline-flex h-[26px] items-center gap-1.5 rounded-[7px] px-2 text-[12px] text-muted">
              <CircleCheck size={12} strokeWidth={1.9} className="text-green" />
              retrieval-explorer
            </span>
          </div>

          <ul className="mt-3.5 space-y-[7px] whitespace-nowrap font-mono text-[11.5px] leading-[18px]">
            {toolCalls.map((call, i) => (
              <li key={i} className="flex items-center gap-2">
                {call.tool === "read_file" ? (
                  <FileText size={12} strokeWidth={1.7} className="shrink-0 text-faint" />
                ) : (
                  <Search size={12} strokeWidth={1.7} className="shrink-0 text-faint" />
                )}
                <span className="text-ink-2">{call.tool}</span>
                <span className="text-muted">{call.arg}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
}
