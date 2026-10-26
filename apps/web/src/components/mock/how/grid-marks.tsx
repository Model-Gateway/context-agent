import { cn } from "@/lib/cn";

/*
 * Crosshairs for hairline grids drawn the usual way: the grid owns its top
 * and left rules (`border-t border-l`) and every cell owns its right and
 * bottom rules (`border-r border-b`). Each cell then marks its bottom-right
 * vertex, plus the top and left vertices that no neighbour covers, so every
 * vertex of the grid is drawn exactly once at any column count.
 */

/** Column counts at the base, `md`, `lg` and `xl` breakpoints. */
export type GridColumns = readonly [base: number, md: number, lg: number, xl: number];

const visibility = [
  ["hidden", "block"],
  ["md:hidden", "md:block"],
  ["lg:hidden", "lg:block"],
  ["xl:hidden", "xl:block"],
] as const;

/** Responsive `hidden`/`block` classes for a mark shown when `test(cols)` holds. */
function showWhen(columns: GridColumns, test: (cols: number) => boolean) {
  return columns.map((cols, i) => visibility[i][test(cols) ? 1 : 0]).join(" ");
}

/**
 * A 17px plus, like the frame's `Crosshair`, centred on a point given
 * relative to the parent's padding box.
 */
function PlusMark({ left, top, className }: { left: string; top: string; className?: string }) {
  return (
    <span
      aria-hidden
      className={cn("pointer-events-none absolute z-10 size-[17px] -translate-x-1/2 -translate-y-1/2", className)}
      style={{ left, top }}
    >
      <span className="absolute left-1/2 top-0 h-full w-px -translate-x-1/2 bg-ink" />
      <span className="absolute left-0 top-1/2 h-px w-full -translate-y-1/2 bg-ink" />
    </span>
  );
}

/*
 * Rule centres relative to a cell's padding box: the neighbour's (or the
 * grid's) rule sits just outside the top and left edges, the cell's own rule
 * just outside the right and bottom edges.
 */
const near = "-0.5px";
const far = "calc(100% + 0.5px)";

/** The crosshairs one cell of a hairline grid is responsible for. */
export function GridCellMarks({ index, columns }: { index: number; columns: GridColumns }) {
  return (
    <>
      <PlusMark left={far} top={far} />
      <PlusMark left={far} top={near} className={showWhen(columns, (cols) => index < cols)} />
      <PlusMark left={near} top={far} className={showWhen(columns, (cols) => index % cols === 0)} />
      {index === 0 && <PlusMark left={near} top={near} />}
    </>
  );
}
