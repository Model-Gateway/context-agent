import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/** A six-spoke asterisk, the eyebrow's marker. */
export function Asterisk({ size = 12, className }: { size?: number; className?: string }) {
  return (
    <svg viewBox="0 0 12 12" width={size} height={size} className={className} aria-hidden>
      <g stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
        <path d="M6 1v10" />
        <path d="M1.67 3.5l8.66 5" />
        <path d="M1.67 8.5l8.66-5" />
      </g>
    </svg>
  );
}

/** The small mono label above a section heading: "✱ FEATURES". */
export function Eyebrow({
  children,
  className,
  onDark = false,
}: {
  children: ReactNode;
  className?: string;
  /** Light text and a pale blue asterisk, for use over dark photos. */
  onDark?: boolean;
}) {
  return (
    <p
      className={cn(
        "inline-flex items-center gap-2 font-mono text-[12px] uppercase leading-none tracking-[0.08em]",
        onDark ? "text-white/75" : "text-muted",
        className,
      )}
    >
      <Asterisk className={onDark ? "text-[#9cb6ff]" : "text-accent"} />
      {children}
    </p>
  );
}
