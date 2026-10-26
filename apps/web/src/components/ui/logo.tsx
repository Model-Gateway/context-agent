import { cn } from "@/lib/cn";

/** The mark's bounding square, as in the app's `LogoShape`. */
const UNIT = 885;

/** The tall arm (left half); the right half mirrors it about x = 442.5. */
const ARM: ReadonlyArray<readonly [number, number]> = [
  [420, 0],
  [261, 116],
  [261, 388.5],
  [34.5, 528],
  [8.5, 744.5],
  [330, 546.5],
  [331.5, 459],
  [420, 395],
];

/** The lower wedge (left half). */
const WEDGE: ReadonlyArray<readonly [number, number]> = [
  [420, 640],
  [348, 593],
  [34.5, 790],
  [241.5, 885],
  [420, 771.5],
];

function polygon(points: ReadonlyArray<readonly [number, number]>, mirrored: boolean): string {
  return points.map(([x, y]) => `${mirrored ? UNIT - x : x},${y}`).join(" ");
}

const LOGO_PATH = [ARM, WEDGE]
  .flatMap((shape) => [polygon(shape, false), polygon(shape, true)])
  .map((points) => `M${points.replace(/ /g, "L")}Z`)
  .join("");

/** The Context mark, filled with the current text colour. */
export function LogoMark({
  size = 22,
  className,
  label = "Context",
}: {
  size?: number;
  className?: string;
  /** Accessible name; pass "" when the mark is decorative. */
  label?: string;
}) {
  return (
    <svg
      viewBox={`0 0 ${UNIT} ${UNIT}`}
      width={size}
      height={size}
      className={className}
      fill="currentColor"
      role={label ? "img" : undefined}
      aria-label={label || undefined}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <path d={LOGO_PATH} />
    </svg>
  );
}

/** The mark in a black rounded tile, the way the nav and app icon show it. */
export function LogoBadge({ size = 34, className }: { size?: number; className?: string }) {
  return (
    <span
      className={cn("inline-grid shrink-0 place-items-center bg-ink text-white", className)}
      style={{ width: size, height: size, borderRadius: Math.round(size * 0.22) }}
    >
      <LogoMark size={Math.round(size * 0.56)} label="" />
    </span>
  );
}

/** Badge plus the mono wordmark. */
export function Wordmark({ className, size = 34 }: { className?: string; size?: number }) {
  return (
    <span className={cn("inline-flex items-center gap-2.5", className)}>
      <LogoBadge size={size} />
      <span className="font-mono text-[22px] leading-none tracking-[-0.02em] text-ink">Context</span>
    </span>
  );
}
