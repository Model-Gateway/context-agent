import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { CornerBrackets } from "@/components/ui/button";
import { cn } from "@/lib/cn";

/**
 * One pricing plan: a labelled header strip, the price, a one-line summary,
 * what is included, and the call to action pinned to the bottom so two
 * cards side by side line up. `featured` darkens the header strip.
 */
export function PlanCard({
  id,
  name,
  tag,
  price,
  priceNote,
  summary,
  features,
  featured = false,
  children,
}: {
  /** Id of the plan's heading, for `aria-labelledby`. */
  id: string;
  name: string;
  /** Small mono note at the right of the header strip. */
  tag?: ReactNode;
  price: string;
  /** Mono note set after the price, such as "forever". */
  priceNote?: string;
  summary: string;
  features: ReactNode[];
  featured?: boolean;
  /** The call to action. */
  children: ReactNode;
}) {
  return (
    <article aria-labelledby={id} className="relative flex h-full flex-col bg-surface ring-1 ring-inset ring-line">
      <CornerBrackets arm={11} className="z-10" />

      <header
        className={cn(
          "flex min-h-12 flex-wrap items-center justify-between gap-x-4 gap-y-1.5 px-5 py-3 sm:px-6 lg:px-8",
          featured ? "bg-ink text-white" : "border-b border-line-soft text-ink",
        )}
      >
        <h3 id={id} className="whitespace-nowrap font-mono text-[12px] uppercase leading-none tracking-[0.08em]">
          {name}
        </h3>
        {tag}
      </header>

      <div className="flex flex-1 flex-col px-5 pb-7 pt-8 sm:px-6 md:pb-8 md:pt-9 lg:px-8">
        <p className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className="text-[clamp(2.4rem,1.9rem+1.6vw,3.25rem)] font-medium leading-none tracking-[-0.045em] text-ink">
            {price}
          </span>
          {priceNote && <span className="font-mono text-[13px] tracking-[0.02em] text-muted">{priceNote}</span>}
        </p>
        {/* Two lines reserved, so the rules of two cards side by side line up. */}
        <p className="mt-4 max-w-[26rem] text-pretty text-[15px] leading-[1.6] text-muted md:min-h-[3.2em]">
          {summary}
        </p>

        <ul className="mt-7 space-y-3 border-t border-line-soft pt-7">
          {features.map((feature, i) => (
            <li key={i} className="flex gap-3 text-[15px] leading-[1.5] text-ink-2">
              <Check size={16} strokeWidth={1.75} className="mt-[3px] shrink-0 text-ink" aria-hidden />
              <span className="text-pretty">{feature}</span>
            </li>
          ))}
        </ul>

        <div className="mt-auto pt-10">{children}</div>
      </div>
    </article>
  );
}
