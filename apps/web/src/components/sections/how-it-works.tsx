import type { ReactNode } from "react";
import { ArrowUpRight } from "lucide-react";
import { Eyebrow } from "@/components/ui/eyebrow";
import { Section } from "@/components/ui/frame";
import { Reveal } from "@/components/ui/reveal";
import {
  ContextLayersArtifact,
  MemoryArtifact,
  RetrievalArtifact,
  TaskMatchArtifact,
  ToolCallsArtifact,
  ValidationLogArtifact,
} from "@/components/mock/how/artifacts";
import { GridCellMarks, type GridColumns } from "@/components/mock/how/grid-marks";
import { LoopDiagram } from "@/components/mock/how/loop-diagram";
import { site } from "@/lib/site";

type Stage = {
  title: string;
  body: string;
  /** The crate in this repository that does the work. */
  crate: string;
  artifact: ReactNode;
};

const stages: Stage[] = [
  {
    title: "Match the task",
    body: "The message is matched against active and recent tasks: continue, refine, start new, refer to earlier work or reverse it.",
    crate: "crates/agent",
    artifact: <TaskMatchArtifact />,
  },
  {
    title: "Retrieve what matters",
    body: "Path, lexical, symbol, semantic and memory strategies run in parallel, are fused by reciprocal rank, expanded along imports and tests, and re-verified against the working tree.",
    crate: "crates/retrieval",
    artifact: <RetrievalArtifact />,
  },
  {
    title: "Compile the context",
    body: "Stable policy and tool schemas first, then the repository summary, the untrusted repository payload inside explicit delimiters, the retained conversation and the request.",
    crate: "crates/context",
    artifact: <ContextLayersArtifact />,
  },
  {
    title: "Act through tools",
    body: "read_file, search_code, find_symbol, apply_patch, execute_command, run_validation and MCP tools. Every call is schema-checked, permission-gated and recorded.",
    crate: "crates/tools",
    artifact: <ToolCallsArtifact />,
  },
  {
    title: "Validate and recover",
    body: "Changes are checked with the project’s own validation commands. A failure starts a bounded recovery cycle with fresh retrieval, and the report never claims a test that no tool observed.",
    crate: "crates/agent",
    artifact: <ValidationLogArtifact />,
  },
  {
    title: "Remember the work",
    body: "Task state, checkpoints, the event log and episodic memory are saved, so later messages and later sessions build on the work instead of replaying it.",
    crate: "crates/memory",
    artifact: <MemoryArtifact />,
  },
];

const stats: { value: string; caption: ReactNode }[] = [
  {
    value: "10",
    caption: (
      <>
        languages indexed with <span className="whitespace-nowrap">Tree-sitter</span>
      </>
    ),
  },
  { value: "5", caption: "retrieval strategies fused per request" },
  { value: "3", caption: "permission profiles, from ask to bypass" },
  { value: "1", caption: "data directory shared by the app, TUI and CLI" },
];

const stageColumns: GridColumns = [1, 2, 2, 3];
const statColumns: GridColumns = [2, 2, 4, 4];

/** The mono caption above each drawing, like a figure label on a sheet. */
function FigureCaption({ figure, title, aside }: { figure: string; title: string; aside?: ReactNode }) {
  return (
    <div className="mb-4 flex items-end justify-between gap-6 font-mono text-[11px] uppercase leading-none tracking-[0.08em] text-muted">
      <p>
        <span className="text-ink-2">Fig. {figure}</span>
        <span aria-hidden className="mx-2 text-faint">
          &mdash;
        </span>
        {title}
      </p>
      {aside}
    </div>
  );
}

/**
 * The six-stage loop every message runs through, drawn as a hairline grid
 * with crosshairs at its vertices, followed by four plain facts.
 */
export function HowItWorks() {
  return (
    <Section id="how-it-works" tone="paper" labelledBy="how-it-works-title">
      <div className="flex flex-col gap-10 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <Eyebrow>How it works</Eyebrow>
          <h2
            id="how-it-works-title"
            className="mt-5 text-[clamp(2.1rem,1.3rem+2.4vw,3.25rem)] font-normal leading-[1.06] tracking-[-0.035em] text-ink"
          >
            How a task runs
          </h2>
          <p className="mt-5 max-w-[36rem] text-[17px] leading-[1.6] text-muted md:text-[19px]">
            Every message goes through the same loop: find the work, gather the code, act through checked tools,
            validate the result, and remember it for next time.
          </p>
        </div>
        <LoopDiagram className="hidden shrink-0 lg:mb-1.5 lg:block" />
      </div>

      <div className="mt-14 md:mt-16">
        <FigureCaption
          figure="1"
          title="The task loop"
          aside={
            <a
              href={site.docsArchitecture}
              target="_blank"
              rel="noopener noreferrer"
              className="group/link hidden items-center gap-1 rounded-[2px] normal-case tracking-[0.02em] text-muted transition-colors hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink sm:inline-flex"
            >
              docs/architecture.md
              <ArrowUpRight
                size={13}
                strokeWidth={1.6}
                className="transition-transform group-hover/link:-translate-y-px group-hover/link:translate-x-px"
              />
            </a>
          }
        />
        <ol className="relative grid grid-cols-1 border-l border-t border-line md:grid-cols-2 xl:grid-cols-3">
          {stages.map((stage, i) => (
            <li
              key={stage.title}
              className="group relative row-span-4 grid grid-rows-subgrid border-b border-r border-line bg-surface"
            >
              <GridCellMarks index={i} columns={stageColumns} />
              {/* A faint dot field that surfaces on hover, like grid paper under the drawing. */}
              <div
                aria-hidden
                className="bg-dots pointer-events-none absolute inset-0 opacity-0 transition-opacity duration-500 [mask-image:radial-gradient(120%_70%_at_100%_0%,#000,transparent_60%)] group-hover:opacity-70"
              />
              <Reveal
                delay={(i % 3) * 80}
                className="relative row-span-4 grid grid-rows-subgrid p-5 sm:p-6 md:p-7 lg:p-8"
              >
                <div className="flex items-center gap-3 font-mono text-[11px] leading-none tracking-[0.06em]">
                  <span className="text-muted transition-colors duration-300 group-hover:text-accent">
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  <span aria-hidden className="h-px flex-1 border-t border-dashed border-line" />
                  <span className="text-muted">{stage.crate}</span>
                </div>
                <h3 className="mt-7 text-[19px] leading-[1.25] tracking-[-0.015em] text-ink md:text-[20px]">
                  {stage.title}
                </h3>
                <p className="mt-3 text-[15px] leading-[1.6] text-muted">{stage.body}</p>
                <div className="flex flex-col pt-7">{stage.artifact}</div>
              </Reveal>
            </li>
          ))}
        </ol>
      </div>

      <div className="mt-16">
        <FigureCaption figure="2" title="At a glance" />
        <ul className="relative grid grid-cols-2 border-l border-t border-line lg:grid-cols-4">
          {stats.map((stat, i) => (
            <li key={stat.value} className="relative border-b border-r border-line bg-surface">
              <GridCellMarks index={i} columns={statColumns} />
              <Reveal delay={i * 80} className="flex h-full flex-col p-5 sm:p-6 md:p-8">
                <p className="text-[clamp(2.75rem,2.1rem+2.2vw,3.5rem)] font-light leading-none tracking-[-0.05em] text-ink tabular-nums">
                  {stat.value}
                </p>
                <p className="mt-5 max-w-[15rem] text-balance font-mono text-[11px] uppercase leading-[1.6] tracking-[0.06em] text-muted">
                  {stat.caption}
                </p>
              </Reveal>
            </li>
          ))}
        </ul>
      </div>
    </Section>
  );
}
