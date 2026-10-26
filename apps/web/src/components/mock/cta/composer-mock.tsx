import { ArrowUp, ChevronDown, FileText, GitFork, Hand, Plus, X } from "lucide-react";
import { cn } from "@/lib/cn";

/** The model picker's provider glyph, drawn as the app shows it: a small "A\". */
function ProviderGlyph() {
  return (
    <svg viewBox="0 0 16 12" width={14} height={11} aria-hidden className="text-ink">
      <path d="M1 11.2 5.2.8h1.6L11 11.2H9.1L8.1 8.6H3.9L2.9 11.2H1Zm3.5-4.2h3L6 3.1 4.5 7Z" fill="currentColor" />
      <path d="M10.2.8h1.9L16 11.2h-1.9L10.2.8Z" fill="currentColor" />
    </svg>
  );
}

/**
 * The app's message composer with a task typed in and a file attached:
 * the "+" and approval-mode buttons on the left, the effort tier, the model
 * and the send button on the right. Purely illustrative.
 */
export function ComposerMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="The Context composer with LoginButton.tsx attached and the task “Add a loading indicator to the login button” typed in, set to Ask approvals, Ultra effort and Claude Sonnet 5.5."
      className={cn("@container select-none", className)}
    >
      <div
        aria-hidden
        className="rounded-[14px] bg-surface shadow-[0_1px_2px_rgba(11,11,12,0.06)] ring-1 ring-black/[0.06]"
      >
        {/* Attachment chip: icon, file name and a remove cross, as in the app. */}
        <div className="flex px-3 pt-3">
          <span className="inline-flex h-7 min-w-0 items-center gap-1.5 rounded-[7px] bg-surface pl-[7px] pr-2 ring-1 ring-inset ring-line">
            <FileText size={13} strokeWidth={1.6} className="shrink-0 text-muted" />
            <span className="truncate text-[12.5px] font-medium text-ink">LoginButton.tsx</span>
            <X size={11} strokeWidth={1.8} className="ml-0.5 shrink-0 text-faint" />
          </span>
        </div>

        {/* Typed task */}
        <p className="text-pretty px-4 pb-5 pt-3 text-[15px] leading-[1.45] tracking-[-0.005em] text-ink">
          Add a loading indicator to the login{" "}
          <span className="whitespace-nowrap">
            button
            <span className="ml-[2px] inline-block h-[18px] w-[1.5px] translate-y-[3.5px] animate-blink rounded-full bg-accent" />
          </span>
        </p>

        {/* Controls */}
        <div className="flex items-center gap-2 px-3 pb-3">
          <span className="grid size-[30px] shrink-0 place-items-center rounded-[9px] text-ink-2 ring-1 ring-inset ring-line">
            <Plus size={15} strokeWidth={1.6} />
          </span>
          <span className="inline-flex h-[30px] shrink-0 items-center gap-1.5 rounded-[9px] pl-2.5 pr-2 text-[13px] text-ink-2 ring-1 ring-inset ring-line">
            <Hand size={13} strokeWidth={1.7} />
            Ask
            <ChevronDown size={12} strokeWidth={1.8} className="text-faint" />
          </span>

          <span className="ml-auto inline-flex shrink-0 items-center gap-1 text-[13px] font-medium text-violet">
            <GitFork size={13} strokeWidth={1.8} />
            Ultra
            <ChevronDown size={12} strokeWidth={1.8} className="opacity-70" />
          </span>
          <span className="ml-2 hidden shrink-0 items-center gap-1.5 text-[13px] font-medium text-ink @min-[380px]:inline-flex">
            <ProviderGlyph />
            Claude Sonnet 5.5
            <ChevronDown size={12} strokeWidth={1.8} className="text-faint" />
          </span>
          {/* The app's send button is always this pale blue disc. */}
          <span className="ml-1.5 grid size-8 shrink-0 place-items-center rounded-full bg-accent-soft text-accent">
            <ArrowUp size={15} strokeWidth={2.2} />
          </span>
        </div>
      </div>
    </div>
  );
}
