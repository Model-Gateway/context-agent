import { Plus } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/** A command, path or flag inside an answer. */
export function InlineCode({ children }: { children: ReactNode }) {
  return (
    <code className="rounded-[5px] bg-surface-2 px-[5px] py-px font-mono text-[0.87em] text-ink-2 ring-1 ring-inset ring-line-soft whitespace-nowrap">
      {children}
    </code>
  );
}

/**
 * One question of the FAQ, built on a native disclosure so it is keyboard
 * and screen-reader friendly and works without JavaScript. The plus turns
 * into a cross when open; where the browser supports `::details-content`
 * and `interpolate-size`, the answer also eases open.
 */
export function FaqItem({
  index,
  question,
  children,
  defaultOpen = false,
}: {
  /** 1-based position, shown as a small "01" label. */
  index: number;
  question: string;
  children: ReactNode;
  defaultOpen?: boolean;
}) {
  return (
    <details
      open={defaultOpen}
      className={cn(
        "group border-b border-line [interpolate-size:allow-keywords]",
        "details-content:h-0 details-content:overflow-clip details-content:opacity-0",
        "open:details-content:h-auto open:details-content:opacity-100",
        "motion-safe:details-content:transition-[height,opacity,content-visibility] motion-safe:details-content:duration-300 motion-safe:details-content:ease-[cubic-bezier(0.2,0.7,0.2,1)] motion-safe:details-content:[transition-behavior:allow-discrete]",
      )}
    >
      <summary
        className={cn(
          "flex cursor-pointer list-none items-start gap-4 py-6 md:gap-6 md:py-7 [&::-webkit-details-marker]:hidden",
          "rounded-[2px] outline-none focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink",
        )}
      >
        <span
          aria-hidden
          className="w-6 shrink-0 pt-[6px] font-mono text-[11px] leading-none tracking-[0.08em] text-faint transition-colors group-open:text-accent"
        >
          {String(index).padStart(2, "0")}
        </span>
        <span className="min-w-0 flex-1 text-pretty text-[17px] leading-[1.4] tracking-[-0.012em] text-ink md:text-[18px]">
          {question}
        </span>
        <span
          aria-hidden
          className={cn(
            "-mt-0.5 grid size-8 shrink-0 place-items-center rounded-full text-ink ring-1 ring-inset ring-line",
            "transition-[transform,background-color,color] duration-300 ease-[cubic-bezier(0.2,0.7,0.2,1)] motion-reduce:transition-none",
            "group-hover:bg-surface-2 group-open:rotate-45 group-open:bg-ink group-open:text-white group-open:ring-ink",
          )}
        >
          <Plus size={15} strokeWidth={1.6} />
        </span>
      </summary>

      <div className="pb-8 pl-10 pr-0 text-[15px] leading-[1.65] text-muted md:pl-12 md:pr-14 [&>*+*]:mt-4 [&>p]:max-w-[40rem]">
        {children}
      </div>
    </details>
  );
}
