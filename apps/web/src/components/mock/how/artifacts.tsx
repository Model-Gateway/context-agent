import type { ReactNode } from "react";
import { ArrowRight, Check, FolderClosed, RotateCcw, X } from "lucide-react";
import { cn } from "@/lib/cn";

/*
 * The small, concrete illustrations at the foot of each "How a task runs"
 * cell. They are pictures, not controls: each one is a single `role="img"`
 * with a spoken description and hidden contents.
 */

/** The shared tray every artifact sits in. */
function Tray({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div role="img" aria-label={label} className="flex flex-1 flex-col rounded-[8px] bg-paper-2 ring-1 ring-inset ring-line-soft">
      <div aria-hidden className="flex flex-1 flex-col justify-center px-3.5 py-3.5 md:min-h-[132px]">
        {children}
      </div>
    </div>
  );
}

/** A mono chip; `active` marks the one that was chosen. */
function Chip({ children, active = false }: { children: ReactNode; active?: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] items-center gap-1.5 rounded-[5px] px-[7px] font-mono text-[10.5px] leading-none tracking-[0.01em]",
        active ? "bg-ink text-white" : "bg-surface text-ink-2 ring-1 ring-inset ring-line-soft",
      )}
    >
      {children}
    </span>
  );
}

/** 01: a follow-up message matched as a refinement of the open task. */
export function TaskMatchArtifact() {
  return (
    <Tray label="A follow-up message, “also handle a missing config.toml”, matched as a refinement of the current task.">
      <p className="self-start rounded-[7px] bg-surface px-2.5 py-[5px] text-[12.5px] leading-[1.4] text-ink-2 ring-1 ring-inset ring-line-soft">
        Also handle a missing <span className="font-mono text-[11.5px]">config.toml</span>
      </p>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <Chip>continue</Chip>
        <Chip active>refine</Chip>
        <Chip>new</Chip>
        <Chip>reverse</Chip>
      </div>
    </Tray>
  );
}

const strategies = [
  { name: "path", score: 44 },
  { name: "lexical", score: 86 },
  { name: "symbol", score: 72 },
  { name: "semantic", score: 58 },
  { name: "memory", score: 31 },
] as const;

/** 02: five retrieval strategies bracketed into one fused ranking. */
export function RetrievalArtifact() {
  return (
    <Tray label="Five retrieval strategies, path, lexical, symbol, semantic and memory, fused into one ranking by reciprocal rank.">
      <div className="flex items-stretch gap-2.5">
        <ul className="flex flex-1 flex-col gap-[7px]">
          {strategies.map((s) => (
            <li key={s.name} className="flex items-center gap-2.5">
              <span className="w-[54px] shrink-0 font-mono text-[10.5px] leading-none text-muted">{s.name}</span>
              <span className="relative h-[5px] flex-1 overflow-hidden rounded-full bg-ink/[0.07]">
                <span className="absolute inset-y-0 left-0 rounded-full bg-ink/75" style={{ width: `${s.score}%` }} />
              </span>
            </li>
          ))}
        </ul>
        <span className="my-[3px] w-2 shrink-0 rounded-r-[3px] border-y border-r border-ink/30" />
        <span className="self-center font-mono text-[10.5px] leading-[1.35] text-ink-2">
          rrf
          <span className="block text-muted">fused</span>
        </span>
      </div>
    </Tray>
  );
}

const layers = [
  { name: "policy + tool schemas", note: "cached", className: "bg-ink text-white" },
  { name: "repository summary", note: "", className: "bg-ink/55 text-white" },
  {
    name: "<<< repository payload >>>",
    note: "fenced",
    className: "border border-dashed border-ink/40 bg-surface text-ink-2",
  },
  { name: "retained conversation", note: "", className: "bg-ink/[0.11] text-ink-2" },
  { name: "request", note: "", className: "bg-ink/[0.05] text-ink-2" },
] as const;

/** 03: the compiled prompt as a stack of layers, stable first. */
export function ContextLayersArtifact() {
  return (
    <Tray label="The compiled context as five layers: policy and tool schemas, repository summary, the fenced repository payload, retained conversation, and the request.">
      <ol className="flex flex-col gap-[3px]">
        {layers.map((layer) => (
          <li key={layer.name} className="flex items-center gap-2">
            <span
              className={cn(
                "flex h-[17px] flex-1 items-center truncate rounded-[4px] px-2 font-mono text-[10px] leading-none",
                layer.className,
              )}
            >
              {layer.name}
            </span>
            <span className="w-[38px] shrink-0 text-right font-mono text-[10px] leading-none text-muted">{layer.note}</span>
          </li>
        ))}
      </ol>
    </Tray>
  );
}

const tools = [
  "read_file",
  "search_code",
  "find_symbol",
  "apply_patch",
  "execute_command",
  "run_validation",
  "mcp__github__*",
] as const;

/** 04: the tool surface, with the checks every call passes through. */
export function ToolCallsArtifact() {
  return (
    <Tray label="Tool calls such as read_file, apply_patch and run_validation, each schema-checked, permission-gated and recorded.">
      <div className="flex flex-wrap gap-1.5">
        {tools.map((tool) => (
          <Chip key={tool} active={tool === "apply_patch"}>
            {tool}
          </Chip>
        ))}
      </div>
      <p className="mt-3 flex flex-wrap gap-x-3.5 gap-y-1 font-mono text-[10.5px] leading-none text-muted">
        {["schema", "permission", "recorded"].map((check) => (
          <span key={check} className="inline-flex items-center gap-1">
            <Check size={11} strokeWidth={2.2} className="text-green" />
            {check}
          </span>
        ))}
      </p>
    </Tray>
  );
}

/** 05: a validation run that fails, recovers and passes. */
export function ValidationLogArtifact() {
  return (
    <Tray label="Validation log: cargo check passed, cargo test failed with one failure, recovery cycle 1 of 3 then passed.">
      <ol className="flex flex-col gap-2 font-mono text-[11.5px] leading-none">
        <li className="flex items-center gap-2 text-ink-2">
          <Check size={13} strokeWidth={2} className="shrink-0 text-green" />
          cargo check
        </li>
        <li className="flex items-center gap-2 text-ink-2">
          <X size={13} strokeWidth={2} className="shrink-0 text-red" />
          cargo test
          <span className="text-red">(1 failed)</span>
        </li>
        <li className="flex items-center gap-2 text-ink-2">
          <RotateCcw size={12} strokeWidth={2} className="shrink-0 text-muted" />
          recovery 1/3
          <ArrowRight size={12} strokeWidth={1.8} className="shrink-0 text-faint" />
          <span className="inline-flex items-center gap-1 rounded-[4px] bg-green-soft px-1.5 py-[3px] text-green">
            <Check size={11} strokeWidth={2.2} />
            passed
          </span>
        </li>
      </ol>
    </Tray>
  );
}

/** 06: what is saved to the shared data directory after a task. */
export function MemoryArtifact() {
  return (
    <Tray label="Episodic memory, checkpoints and the event log saved to ~/.context-agent.">
      <p className="flex items-center gap-2 font-mono text-[11.5px] leading-none text-ink-2">
        <FolderClosed size={13} strokeWidth={1.6} className="shrink-0 text-muted" />
        ~/.context-agent
        <span className="ml-auto inline-flex items-center gap-1 text-[10.5px] text-green">
          <Check size={11} strokeWidth={2.2} />
          saved
        </span>
      </p>
      <div className="mt-3 flex flex-wrap gap-1.5">
        <Chip>episodic</Chip>
        <Chip>checkpoint</Chip>
        <Chip>event log</Chip>
      </div>
    </Tray>
  );
}
