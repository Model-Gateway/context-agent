import { Brain, ChevronDown, ChevronRight, CircleCheck, GitFork, Hand, Plus, Search } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Chip, stroke } from "./parts";

/** Inline code in the answer, set like the app's Markdown renderer. */
function Code({ children }: { children: ReactNode }) {
  return (
    <code className="rounded-[4px] bg-black/[0.05] px-[3px] py-px font-mono text-[11.5px] text-ink-2 [overflow-wrap:anywhere]">
      {children}
    </code>
  );
}

/**
 * A collapsed disclosure line ("Run notes · 1 ›", "Thinking 0.3s ›").
 * `display` replaces the default `flex`, to hide it below a breakpoint.
 */
function Disclosure({ icon, children, display = "flex" }: { icon?: ReactNode; children: ReactNode; display?: string }) {
  return (
    <div className={cn("items-center gap-2 text-muted", display)}>
      {icon}
      <span className="flex items-center gap-1">
        {children}
        <ChevronRight size={13} strokeWidth={stroke} className="text-faint" />
      </span>
    </div>
  );
}

/** One finished explore agent in the group: status glyph, name, kind and totals. */
function AgentRow({ name, toolCalls, time }: { name: string; toolCalls: number; time: string }) {
  return (
    <div className="relative flex items-start gap-3 py-1.5">
      <span className="mt-0.5 grid size-[22px] shrink-0 place-items-center text-green">
        <CircleCheck size={16} strokeWidth={stroke} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="truncate text-ink">{name}</span>
          <Chip muted>
            <Search size={11} strokeWidth={1.8} />
            Explore
          </Chip>
        </div>
        <div className="mt-0.5 text-[12px] text-muted">Completed &middot; {toolCalls} tool calls</div>
      </div>
      {/* Phones keep the name and chip; the group header still shows the elapsed time. */}
      <span className="mt-1 hidden shrink-0 text-[12px] tabular-nums text-faint sm:block">{time}</span>
    </div>
  );
}

/** The composer while a task runs: a steering prompt and the stop button. */
function Composer() {
  return (
    <div className="shrink-0 px-4 pb-4 sm:px-6">
      <div className="mx-auto max-w-[560px] rounded-[14px] bg-white px-3 pb-2.5 pt-3 shadow-[0_2px_10px_-4px_rgba(0,0,0,0.12)] ring-1 ring-black/[0.09]">
        <div className="truncate px-1 text-faint">Steer the running task &middot; &#8984;Return to queue</div>
        <div className="mt-3.5 flex items-center gap-1.5 text-[12px]">
          <span className="grid size-7 shrink-0 place-items-center rounded-[8px] text-ink-2 ring-1 ring-inset ring-black/[0.1]">
            <Plus size={14} strokeWidth={stroke} />
          </span>
          <span className="flex h-7 items-center gap-1 rounded-[8px] px-2 text-ink-2 ring-1 ring-inset ring-black/[0.1]">
            <Hand size={12} strokeWidth={stroke} />
            Ask
            <ChevronDown size={12} strokeWidth={stroke} className="text-faint" />
          </span>
          <span className="ml-auto flex items-center gap-1 font-medium text-violet">
            <GitFork size={12} strokeWidth={1.8} />
            Ultra
            <ChevronDown size={12} strokeWidth={stroke} className="opacity-70" />
          </span>
          <span className="ml-2 hidden items-center gap-1 font-medium text-ink sm:flex">
            Claude Sonnet 5.5
            <ChevronDown size={12} strokeWidth={stroke} className="text-faint" />
          </span>
          <span className="ml-1.5 grid size-7 shrink-0 place-items-center rounded-full bg-accent-soft">
            <span className="size-[9px] rounded-[2px] bg-accent" />
          </span>
        </div>
      </div>
    </div>
  );
}

/**
 * The conversation: a question that asks for two explore agents, the run's
 * disclosure lines, the finished agent group, the answer streaming in from
 * their reports, and the composer pinned at the bottom.
 */
export function ChatPane() {
  return (
    <div className="flex min-w-0 flex-1 flex-col bg-white text-[13px] leading-[1.55]">
      <div className="min-h-0 flex-1 overflow-hidden px-4 pt-4 sm:px-6 md:pt-5">
        <div className="mx-auto max-w-[560px] text-pretty">
          {/* The question. */}
          <div className="flex flex-col items-end">
            <p className="max-w-[92%] rounded-[14px] bg-black/[0.045] px-3.5 py-2.5 text-ink sm:max-w-[88%]">
              How does Context guarantee an edit never overwrites a file that changed after the model read it? Deploy
              two explore agents in parallel, one on the Rust patch tool and one on the macOS app&rsquo;s editor save
              path, then answer in a few sentences.
            </p>
            <span className="mt-1 hidden text-[11px] text-faint sm:block">2:52 PM</span>
          </div>

          <div className="mt-3.5 space-y-3 sm:mt-2 md:mt-3">
            <Disclosure display="hidden sm:flex">Run notes &middot; 1</Disclosure>
            <Disclosure icon={<Brain size={15} strokeWidth={stroke} className="text-muted" />}>
              <span className="text-ink-2">Thinking</span>
              <span className="text-faint">0.3s</span>
            </Disclosure>

            {/* The tool call that deployed the agents. */}
            <div className="flex items-start gap-3">
              <span className="grid size-[22px] shrink-0 place-items-center rounded-[6px] bg-black/[0.045] text-muted">
                <GitFork size={13} strokeWidth={stroke} />
              </span>
              <span className="min-w-0 flex-1 pt-px text-ink">
                Deploying 2 agents: <span className="whitespace-nowrap">rust-patch-explorer,</span>{" "}
                <span className="whitespace-nowrap">macos-save-explorer</span>
              </span>
              <span className="mt-px hidden sm:flex">
                <Chip muted>agents</Chip>
              </span>
            </div>
          </div>

          {/* The agent group; both have reported back, so the lead can answer. */}
          <div className="mt-4">
            <div className="flex items-center gap-2.5">
              <GitFork size={15} strokeWidth={stroke} className="ml-[3px] shrink-0 text-ink-2" />
              <span className="text-ink">2 agents, done</span>
              <ChevronDown size={13} strokeWidth={stroke} className="text-faint" />
              <span className="ml-auto text-[12px] tabular-nums text-muted">58s</span>
            </div>
            <div className="relative mt-1">
              {/* The rule that joins the two status glyphs. */}
              <span className="absolute bottom-[34px] left-[10.5px] top-[30px] w-px bg-black/[0.08]" />
              <AgentRow name="rust-patch-explorer" toolCalls={41} time="58s" />
              <AgentRow name="macos-save-explorer" toolCalls={33} time="47s" />
            </div>
          </div>

          {/* The answer, streaming in. */}
          <div className="mt-4 text-ink">
            <p>
              I think Context enforces it as a read-gated refusal check{" "}
              <span className="whitespace-nowrap">&mdash; not a lock.</span>
            </p>
            <ul className="mt-1.5 space-y-1.5">
              <li className="relative pl-4 before:absolute before:left-1 before:top-[0.68em] before:size-[3.5px] before:rounded-full before:bg-ink-2">
                <strong className="font-semibold">Rust patch tool</strong> &mdash; <Code>crates/tools/src/files.rs</Code>{" "}
                re-reads the file at invoke and refuses through <Code>check_edit</Code> unless the session&rsquo;s last
                read hash matches the current content, then writes atomically.
              </li>
              <li className="relative pl-4 before:absolute before:left-1 before:top-[0.68em] before:size-[3.5px] before:rounded-full before:bg-ink-2">
                <strong className="font-semibold">macOS editor save</strong> &mdash; the app sends the read hash as{" "}
                <Code>expected_hash</Code> on <Code>file/write</Code>; on a mismatch the backend returns{" "}
                <Code>conflict</Code> and writes nothing.
              </li>
            </ul>
          </div>
        </div>
      </div>

      <Composer />
    </div>
  );
}
