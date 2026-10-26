import { ChevronRight, Search } from "lucide-react";
import { cn } from "@/lib/cn";
import { Chip, MockLabel, mockSurface } from "./parts";

const stats = [
  { label: "Prompt", value: "28.7k", note: "of 256k" },
  { label: "Stable prefix", value: "14.2k", note: "policy + tools" },
  { label: "Retrieval", value: "24k", note: "budget" },
] as const;

/** What the 28.7k-token prompt is made of; the first two are the cached prefix. */
const parts = [
  { label: "Policy", tokens: 3.1, color: "bg-ink" },
  { label: "Tools", tokens: 11.1, color: "bg-ink-2/45" },
  { label: "Repository", tokens: 12.4, color: "bg-accent" },
  { label: "Conversation", tokens: 2.1, color: "bg-violet" },
] as const;

/**
 * The context of one request, as `ctx context explain` and the app's
 * inspector report it: prompt size, the stable prefix (policy and tool
 * schemas) and the retrieval budget of an investigation, then what the
 * prompt is made of. It fits the card's width on most screens; the bottom
 * is cropped by the photo.
 */
export function ContextMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="The context of one request: a 28.7k-token prompt out of a 256k window, a 14.2k-token stable prefix of policy and tool schemas, and the 24k retrieval budget of an investigation task."
      className={cn(mockSurface, "min-w-[284px]", className)}
    >
      <div aria-hidden>
        <div className="flex h-12 items-center justify-between gap-3 border-b border-line-soft pl-3.5 pr-2.5">
          <MockLabel tone="muted">Context</MockLabel>
          <Chip>
            <Search size={11} strokeWidth={1.8} />
            investigation
          </Chip>
        </div>

        <div className="grid grid-cols-[1fr_1.3fr_1fr] divide-x divide-line-soft border-b border-line-soft">
          {stats.map((stat) => (
            <div key={stat.label} className="px-3 pb-3.5 pt-3 sm:px-3.5">
              <span className="block whitespace-nowrap font-mono text-[10px] uppercase leading-none tracking-[0.06em] text-faint">
                {stat.label}
              </span>
              <p className="mt-2.5 text-[26px] leading-none tracking-[-0.035em] text-ink tabular-nums">{stat.value}</p>
              <p className="mt-1.5 text-[11.5px] leading-none text-faint">{stat.note}</p>
            </div>
          ))}
        </div>

        <div className="px-3.5 pb-4 pt-3.5">
          <div className="flex h-1.5 gap-[2px] overflow-hidden rounded-full">
            {parts.map((part) => (
              <span key={part.label} className={part.color} style={{ flexGrow: part.tokens }} />
            ))}
          </div>
          <ul className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-[11.5px]">
            {parts.map((part) => (
              <li key={part.label} className="flex items-center gap-1.5">
                <span className={cn("size-1.5 shrink-0 rounded-full", part.color)} />
                <span className="truncate text-muted">{part.label}</span>
                <span className="ml-auto font-mono text-[11px] text-ink-2 tabular-nums">{part.tokens}k</span>
              </li>
            ))}
          </ul>
        </div>

        {/* The command that explains the budget, cropped on most screens. */}
        <div className="flex items-center gap-1.5 border-t border-line-soft px-3.5 py-3 font-mono text-[11px] text-muted">
          <ChevronRight size={12} strokeWidth={1.8} className="text-faint" />
          ctx context explain
          <span className="ml-auto font-sans text-[11.5px] text-faint">why each item is here</span>
        </div>
      </div>
    </div>
  );
}
