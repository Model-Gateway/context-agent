import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/*
 * Small pieces shared by the capability mock-ups. They imitate the macOS
 * app's chrome: grey chips, mono section labels, hairline segmented
 * controls and the white card every mock sits in.
 */

/** The white surface a capability mock-up is drawn on. */
export const mockSurface =
  "overflow-hidden rounded-[10px] bg-surface text-ink ring-1 ring-black/[0.06] shadow-[0_1px_2px_rgba(10,10,20,0.06),0_28px_56px_-28px_rgba(10,10,20,0.45)]";

/** A grey label chip, like "Explore" or "agents" in the app. */
export function Chip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-[22px] shrink-0 items-center gap-1 rounded-[6px] bg-surface-2 px-1.5 text-[11.5px] leading-none text-muted ring-1 ring-inset ring-black/[0.05]",
        className,
      )}
    >
      {children}
    </span>
  );
}

/**
 * The app's uppercase mono section label ("CHANGES", "TASK"); `muted` is the
 * darker shade used for a card's own title.
 */
export function MockLabel({ children, tone = "faint" }: { children: ReactNode; tone?: "faint" | "muted" }) {
  return (
    <span
      className={cn(
        "font-mono text-[10.5px] uppercase leading-none tracking-[0.1em]",
        tone === "muted" ? "text-muted" : "text-faint",
      )}
    >
      {children}
    </span>
  );
}

/** A pill-shaped segmented control; `active` is the selected segment. */
export function Segmented({
  items,
  active,
  className,
}: {
  items: readonly string[];
  active: string;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-0.5 rounded-[8px] bg-surface-2 p-[3px] ring-1 ring-inset ring-black/[0.05]",
        className,
      )}
    >
      {items.map((item) => (
        <span
          key={item}
          className={cn(
            "rounded-[6px] px-2 py-[5px] text-[11.5px] leading-none",
            item === active
              ? "bg-surface font-medium text-ink shadow-[0_1px_2px_rgba(10,10,20,0.08)] ring-1 ring-black/[0.06]"
              : "text-muted",
          )}
        >
          {item}
        </span>
      ))}
    </span>
  );
}
