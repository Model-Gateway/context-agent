import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/*
 * The blueprint frame every section sits in: dotted gutters, one column
 * bounded by hairlines, hatched bands between sections, and crosshairs
 * centred on the points where the column's rules meet a band's edges.
 */

/** Horizontal padding inside the column; every section uses it. */
export const columnPadding = "px-5 sm:px-8 lg:px-12";

/** Dotted page background with the bounded central column. */
export function PageFrame({ children }: { children: ReactNode }) {
  return (
    <div className="relative isolate min-h-screen bg-paper">
      <div aria-hidden className="bg-dots pointer-events-none absolute inset-0 -z-10" />
      <div className="relative mx-3 border-x border-line bg-paper md:mx-6 xl:mx-auto xl:max-w-[1200px]">
        {children}
      </div>
    </div>
  );
}

/**
 * A 17px plus sign centred on a corner of its positioned parent. Absolute
 * offsets start inside the parent's border, so ±0.5px puts the centre on the
 * middle of a 1px rule just outside that edge: the parent's own top/bottom
 * border, or the column's border beside it.
 */
export function Crosshair({
  x,
  y,
  className,
}: {
  x: "left" | "right";
  y: "top" | "bottom";
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={cn("pointer-events-none absolute z-10 size-[17px] -translate-x-1/2 -translate-y-1/2", className)}
      style={{
        left: x === "left" ? "-0.5px" : "calc(100% + 0.5px)",
        top: y === "top" ? "-0.5px" : "calc(100% + 0.5px)",
      }}
    >
      <span className="absolute left-1/2 top-0 h-full w-px -translate-x-1/2 bg-ink" />
      <span className="absolute left-0 top-1/2 h-px w-full -translate-y-1/2 bg-ink" />
    </span>
  );
}

/** Crosshairs on all four corners of the parent. */
export function CornerCrosshairs() {
  return (
    <>
      <Crosshair x="left" y="top" />
      <Crosshair x="right" y="top" />
      <Crosshair x="left" y="bottom" />
      <Crosshair x="right" y="bottom" />
    </>
  );
}

/**
 * The hatched strip between two sections, marked at its four corners. Under
 * the sticky header, pass `topRule={false}`: the header draws that rule and
 * its crosshairs, which would otherwise slide under it.
 */
export function HatchBand({ className, topRule = true }: { className?: string; topRule?: boolean }) {
  return (
    <div
      aria-hidden
      className={cn("bg-hatch relative h-8 border-b border-line md:h-10", topRule && "border-t", className)}
    >
      {topRule ? (
        <CornerCrosshairs />
      ) : (
        <>
          <Crosshair x="left" y="bottom" />
          <Crosshair x="right" y="bottom" />
        </>
      )}
    </div>
  );
}

/** A full-column section with the standard padding. */
export function Section({
  id,
  children,
  className,
  tone = "surface",
  labelledBy,
}: {
  id?: string;
  children: ReactNode;
  className?: string;
  /** `surface` is white; `paper` matches the frame. */
  tone?: "surface" | "paper";
  labelledBy?: string;
}) {
  return (
    <section
      id={id}
      aria-labelledby={labelledBy}
      className={cn(
        "relative",
        tone === "surface" ? "bg-surface" : "bg-paper",
        columnPadding,
        "py-20 md:py-28",
        className,
      )}
    >
      {children}
    </section>
  );
}
