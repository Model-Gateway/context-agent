import {
  ChevronDown,
  CircleArrowDown,
  CircleArrowUp,
  FileText,
  GitBranch,
  Plus,
  RefreshCw,
  RotateCw,
  Sparkles,
  Undo2,
} from "lucide-react";
import { cn } from "@/lib/cn";
import { Chip, MockLabel, mockSurface } from "./parts";

const files: { name: string; dir?: string; status: "M" | "A"; added: number; removed?: number }[] = [
  { name: "README.md", status: "M", added: 182, removed: 57 },
  { name: "files.rs", dir: "crates/tools/src", status: "M", added: 24, removed: 6 },
  { name: "execution.rs", dir: "crates/tools/tests", status: "A", added: 41 },
];

/** The first lines of the README diff: the old heading out, the centred header in. */
const diff: { kind: "del" | "add"; old?: number; next?: number; text: string }[] = [
  { kind: "del", old: 1, text: "# Context" },
  { kind: "del", old: 2, text: "" },
  { kind: "del", old: 3, text: "Context is an independent, local-first coding agent written in Rust." },
  { kind: "del", old: 4, text: "" },
  { kind: "del", old: 5, text: "## What is in the box" },
  { kind: "add", next: 1, text: '<p align="center">' },
  {
    kind: "add",
    next: 2,
    text: '  <img src="apps/macos/ContextAgent/Resources/Assets.xcassets/AppIcon.appiconset/icon_256x256.png" width="96">',
  },
  { kind: "add", next: 3, text: "</p>" },
  { kind: "add", next: 4, text: "" },
  { kind: "add", next: 5, text: '<h1 align="center">Context</h1>' },
  { kind: "add", next: 6, text: "" },
  { kind: "add", next: 7, text: '<p align="center">' },
  { kind: "add", next: 8, text: "  A local-first coding agent written in Rust, with a native macOS app and a terminal UI.<br>" },
  { kind: "add", next: 9, text: "  Bring any OpenAI-compatible model. Keep your code, history and index on your machine." },
  { kind: "add", next: 10, text: "</p>" },
  { kind: "add", next: 11, text: "" },
  { kind: "add", next: 12, text: '<p align="center">' },
  { kind: "add", next: 13, text: '  <a href="#install">Install</a> ·' },
];

/**
 * The Changes view: git status, a commit box and the file list, with the
 * selected file's diff beside it. Wider than its photo on purpose: the panel
 * crops the diff at the right edge. Sized against the nearest `@container`
 * (the card), so on phones the status column still fits.
 */
export function ChangesMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="The Context app's Changes view on branch main: three changed files (README.md, crates/tools/src/files.rs and the new crates/tools/tests/execution.rs) with a commit message box and the README diff beside them."
      className={cn(mockSurface, "h-[520px] w-[800px]", className)}
    >
      <div aria-hidden className="flex h-full">
        {/* Git status column */}
        <div className="w-[min(292px,calc(100cqw-34px))] shrink-0 border-r border-line-soft">
          <div className="px-4 pt-4">
            <div className="flex items-center gap-2">
              <span className="text-[15px] font-semibold tracking-[-0.015em] text-ink">Changes</span>
              <span className="rounded-[5px] bg-surface-2 px-1.5 py-[3px] font-mono text-[10.5px] leading-none text-muted ring-1 ring-inset ring-black/[0.05]">
                3
              </span>
              <RotateCw size={14} strokeWidth={1.6} className="ml-auto text-faint" />
            </div>

            <div className="mt-3 flex items-center gap-1 text-[12.5px] text-ink-2">
              <GitBranch size={13} strokeWidth={1.7} className="text-muted" />
              <span>main</span>
              <ChevronDown size={12} strokeWidth={1.7} className="text-faint" />
              <span className="ml-auto flex items-center gap-2.5 text-faint">
                <RefreshCw size={14} strokeWidth={1.6} />
                <CircleArrowDown size={14} strokeWidth={1.6} />
                <CircleArrowUp size={14} strokeWidth={1.6} />
              </span>
            </div>

            <div className="mt-3 h-[64px] rounded-[8px] px-3 py-2.5 text-[12.5px] text-faint ring-1 ring-inset ring-line">
              Message (&#8984;&#9166; to commit on main)
            </div>

            <div className="mt-2.5 flex items-center justify-between">
              <Sparkles size={14} strokeWidth={1.6} className="text-faint" />
              <span className="flex items-center gap-1.5">
                <span className="rounded-[6px] bg-surface-2 px-2.5 py-[5px] text-[11.5px] leading-none text-faint ring-1 ring-inset ring-black/[0.05]">
                  Commit
                </span>
                <ChevronDown size={13} strokeWidth={1.6} className="text-faint" />
              </span>
            </div>
          </div>

          <div className="mt-4 border-t border-line-soft px-2 pt-3.5">
            <div className="flex items-center px-2">
              <MockLabel>Changes 3</MockLabel>
              <span className="ml-auto flex items-center gap-2.5 text-faint">
                <Undo2 size={13} strokeWidth={1.6} />
                <Plus size={14} strokeWidth={1.6} />
              </span>
            </div>
            <ul className="mt-2 space-y-0.5">
              {files.map((file, i) => (
                <li
                  key={file.name}
                  className={cn("flex h-8 items-center gap-2 rounded-[6px] px-2", i === 0 && "bg-surface-2")}
                >
                  <span
                    className={cn(
                      "grid size-[18px] shrink-0 place-items-center rounded-[4px] font-mono text-[10px] font-medium leading-none",
                      file.status === "M" ? "bg-amber-soft text-amber" : "bg-green-soft text-green",
                    )}
                  >
                    {file.status}
                  </span>
                  <span className="shrink-0 text-[12.5px] text-ink">{file.name}</span>
                  {file.dir && <span className="min-w-0 truncate text-[11.5px] text-faint">{file.dir}</span>}
                  <span className="ml-auto flex shrink-0 gap-1.5 font-mono text-[11px] tabular-nums">
                    <span className="text-green">+{file.added}</span>
                    {file.removed !== undefined && <span className="text-red">&minus;{file.removed}</span>}
                  </span>
                </li>
              ))}
            </ul>
          </div>

          <div className="mt-4 px-4">
            <MockLabel>Recent commits</MockLabel>
            <ul className="mt-2.5 space-y-2 text-[12px]">
              <li className="flex gap-2.5">
                <span className="font-mono text-[11px] text-faint">62d5688</span>
                <span className="text-ink-2">Import README</span>
              </li>
              <li className="flex gap-2.5">
                <span className="font-mono text-[11px] text-faint">400ac79</span>
                <span className="text-ink-2">Initial import</span>
              </li>
            </ul>
          </div>
        </div>

        {/* Diff of the selected file, cropped by the photo's right edge. */}
        <div className="min-w-0 flex-1">
          <div className="flex h-[52px] items-center gap-2 whitespace-nowrap border-b border-line-soft px-4">
            <FileText size={14} strokeWidth={1.6} className="shrink-0 text-muted" />
            <span className="font-mono text-[12px] text-ink">README.md</span>
            <span className="rounded-[5px] bg-amber-soft px-1.5 py-[3px] text-[11px] leading-none text-amber">Modified</span>
            <span className="font-mono text-[11px] text-green">+182</span>
            <span className="font-mono text-[11px] text-red">&minus;57</span>
            <Chip>working tree</Chip>
          </div>
          <div className="whitespace-pre font-mono text-[11.5px] leading-[20px]">
            <div className="bg-surface-2 pl-[78px] text-faint">@@ -1,104 +1,229 @@</div>
            {diff.map((line, i) => (
              <div
                key={i}
                className={cn("flex", line.kind === "del" ? "bg-red-soft text-red" : "bg-green-soft text-green")}
              >
                <span className="w-[30px] shrink-0 pr-1.5 text-right text-faint/80">{line.old ?? ""}</span>
                <span className="w-[30px] shrink-0 pr-1.5 text-right text-faint/80">{line.next ?? ""}</span>
                <span className="w-[18px] shrink-0 text-center">{line.kind === "del" ? "-" : "+"}</span>
                <span>{line.text}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
