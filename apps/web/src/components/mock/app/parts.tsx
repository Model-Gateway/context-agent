import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/*
 * Small pieces shared by the app-window mock: the grey chip, the running
 * indicator and the mono label rows the inspector uses. Everything here is
 * decorative and rendered inside an `aria-hidden` tree.
 */

/** Stroke width every icon in the window uses, close to SF Symbols' regular weight. */
export const stroke = 1.6;

/**
 * The app's flat grey chip ("agents", "default", "Explore", "main").
 * `muted` greys the label, as the app does for tool and agent kinds.
 */
export function Chip({ children, muted = false }: { children: ReactNode; muted?: boolean }) {
  return (
    <span
      className={cn(
        "inline-flex h-[20px] shrink-0 items-center gap-1 whitespace-nowrap rounded-[6px] bg-black/[0.055] px-1.5 text-[11.5px] leading-none",
        muted ? "text-muted" : "text-ink-2",
      )}
    >
      {children}
    </span>
  );
}

/**
 * The app's "working" glyph: three stacked dots that pulse in turn. CSS only;
 * the global reduced-motion rule (and `motion-reduce`) leaves them still.
 */
export function RunningDots({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex flex-col items-center gap-[2px]", className)}>
      {[0, 220, 440].map((delay) => (
        <span
          key={delay}
          className="size-[2.5px] animate-pulse-dot rounded-full bg-current motion-reduce:animate-none"
          style={{ animationDelay: `${delay}ms` }}
        />
      ))}
    </span>
  );
}

/** A label/value row; values are set in mono, like the app's inspector. */
export function StatRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="shrink-0 text-muted">{label}</span>
      <span className="min-w-0 truncate text-right font-mono text-[11.5px] text-ink">{value}</span>
    </div>
  );
}

/** Uppercase card heading in the inspector ("TASK", "CONTEXT"). */
export function CardHeading({ children }: { children: ReactNode }) {
  return <div className="text-[10.5px] font-medium uppercase tracking-[0.06em] text-faint">{children}</div>;
}

/**
 * A waving hand for the sidebar's Welcome row. Lucide has no wave, so this
 * tilts its raised hand and adds two motion strokes.
 */
export function WaveIcon({ size = 16, className }: { size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <g transform="rotate(-18 12 14)">
        <path d="M18 11V6a2 2 0 0 0-4 0" />
        <path d="M14 10V4a2 2 0 0 0-4 0v2" />
        <path d="M10 10.5V6a2 2 0 0 0-4 0v8" />
        <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
      </g>
      <path d="M3.5 6.5c.4-1.3 1.2-2.4 2.3-3.1" />
      <path d="M2 4.2c.3-.9.8-1.7 1.4-2.3" />
    </svg>
  );
}
