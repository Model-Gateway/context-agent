import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/** macOS window controls; grey when the window is in the background. */
export function TrafficLights({ active = true, className }: { active?: boolean; className?: string }) {
  const colors = active ? ["#ff5f57", "#febc2e", "#28c840"] : ["#dcdcdf", "#dcdcdf", "#dcdcdf"];
  return (
    <span aria-hidden className={cn("flex items-center gap-[7px]", className)}>
      {colors.map((color, i) => (
        <span
          key={i}
          className="size-[11px] rounded-full ring-1 ring-inset ring-black/10"
          style={{ backgroundColor: color }}
        />
      ))}
    </span>
  );
}

/**
 * A white card with an optional title bar, the base for product mock-ups.
 * Mock-ups are illustrations: wrap them in an element with an `aria-label`
 * describing what they show, and mark the inside `aria-hidden` if it is
 * only decoration.
 */
export function MockWindow({
  title,
  lights = false,
  toolbar,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  lights?: boolean;
  /** Extra content at the right of the title bar. */
  toolbar?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  const hasBar = title !== undefined || lights || toolbar !== undefined;
  return (
    <div className={cn("overflow-hidden rounded-[10px] bg-surface shadow-card ring-1 ring-black/5", className)}>
      {hasBar && (
        <div className="flex h-9 items-center gap-3 border-b border-line-soft px-3.5">
          {lights && <TrafficLights />}
          {title !== undefined && <div className="min-w-0 truncate text-[12.5px] text-muted">{title}</div>}
          {toolbar !== undefined && <div className="ml-auto flex items-center gap-2">{toolbar}</div>}
        </div>
      )}
      <div className={bodyClassName}>{children}</div>
    </div>
  );
}
