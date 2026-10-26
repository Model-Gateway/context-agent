import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/*
 * Small pieces shared by the three feature mock-ups. All decorative: the
 * mock-up that uses them carries the accessible description.
 */

const tileTones = {
  accent: "bg-accent-soft text-accent",
  violet: "bg-violet-soft text-violet",
  green: "bg-green-soft text-green",
  amber: "bg-amber-soft text-amber",
} as const;

/** A tinted rounded square holding a glyph, in place of an avatar. */
export function IconTile({
  tone,
  size = 30,
  className,
  children,
}: {
  tone: keyof typeof tileTones;
  size?: number;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={cn(
        "grid shrink-0 place-items-center rounded-[7px] ring-1 ring-inset ring-black/[0.04]",
        tileTones[tone],
        className,
      )}
      style={{ width: size, height: size }}
    >
      {children}
    </span>
  );
}

/** A grey timestamp pill, as on the right of a notification. */
export function TimeChip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex h-[24px] shrink-0 items-center rounded-[6px] bg-surface-2 px-2 text-[11px] tabular-nums tracking-[0.01em] text-muted ring-1 ring-inset ring-black/[0.035]",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** A white outlined pill that looks like a small button. */
export function OutlineChip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-[6px] bg-white text-ink-2 shadow-[0_1px_2px_rgba(11,11,12,0.06)] ring-1 ring-inset ring-black/[0.09]",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** A black arrow pointer with a name pill below it, like a collaborator's cursor. */
export function PointerCursor({ label, className }: { label: string; className?: string }) {
  return (
    <span className={cn("pointer-events-none absolute block", className)}>
      <svg
        width="19"
        height="20"
        viewBox="0 0 19 20"
        className="block drop-shadow-[0_2px_3px_rgba(11,11,12,0.25)]"
      >
        <path
          d="M2.2 1.6 17 8.3c.5.2.4.9-.1 1L10.7 10.9 8.3 17c-.2.5-.9.5-1 0L1.4 2.4c-.2-.5.3-1 .8-.8Z"
          fill="#0b0b0c"
          stroke="#fff"
          strokeWidth="1.4"
          strokeLinejoin="round"
        />
      </svg>
      <span className="ml-[9px] mt-[1px] inline-flex h-[23px] items-center rounded-full bg-ink px-[11px] text-[12.5px] font-medium tracking-[-0.01em] text-white shadow-[0_4px_10px_-2px_rgba(11,11,12,0.35)] ring-[1.5px] ring-white/90">
        {label}
      </span>
    </span>
  );
}

/** The I-beam text cursor. */
export function TextCursor({ className }: { className?: string }) {
  return (
    <svg width="8" height="15" viewBox="0 0 8 15" className={cn("inline-block", className)}>
      <path
        d="M1 1.2c1.4 0 2.3.4 3 1.2.7-.8 1.6-1.2 3-1.2M4 2.4v10.2M1 13.8c1.4 0 2.3-.4 3-1.2.7.8 1.6 1.2 3 1.2M2.4 7.5h3.2"
        fill="none"
        stroke="#0b0b0c"
        strokeWidth="1.25"
        strokeLinecap="round"
      />
    </svg>
  );
}
