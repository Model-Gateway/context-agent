import Link from "next/link";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/cn";

/**
 * Buttons come in two families:
 *
 * - The frame family (`solid`, `bracket`) is square and set in mono, like the
 *   nav and hero: `solid` is a black block, `bracket` is a pale block marked
 *   only at its four corners.
 * - The card family (`dark`, `light`) is rounded and set in sans, for use on
 *   white sections next to product cards.
 *
 * `onDark` inverts the frame family for use over dark photos.
 */
export type ButtonVariant = "solid" | "bracket" | "dark" | "light";
export type ButtonSize = "sm" | "md" | "lg";

const base =
  "group/button relative inline-flex shrink-0 select-none items-center justify-center gap-2 whitespace-nowrap transition-[background-color,color,box-shadow,transform] [&>svg]:shrink-0 duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 active:translate-y-px";

/** Kept out of `base` so the on-dark variants can swap it without a class conflict. */
const inkOutline = "focus-visible:outline-ink";

const variants: Record<ButtonVariant, string> = {
  solid: "bg-ink font-mono text-white hover:bg-ink-2",
  bracket: "bg-white/70 font-mono text-ink ring-1 ring-inset ring-line-soft hover:bg-white",
  dark: "rounded-[10px] bg-ink font-medium text-white shadow-[0_1px_2px_rgba(0,0,0,0.2)] hover:bg-ink-2",
  light:
    "rounded-[10px] bg-white font-medium text-ink ring-1 ring-inset ring-line shadow-[0_1px_2px_rgba(0,0,0,0.05)] hover:bg-surface-2",
};

/** The frame family inverted for dark photographic backgrounds. */
const onDarkVariants: Partial<Record<ButtonVariant, string>> = {
  solid: "bg-white font-mono text-ink hover:bg-white/90 focus-visible:outline-white",
  bracket:
    "bg-white/10 font-mono text-white ring-1 ring-inset ring-white/20 backdrop-blur-md hover:bg-white/20 focus-visible:outline-white",
};

const sizes: Record<ButtonSize, string> = {
  sm: "h-9 px-3.5 text-[13px]",
  md: "h-11 px-5 text-[15px]",
  lg: "h-12 px-6 text-[15px]",
};

/** The four L-shaped marks of the `bracket` variant. */
export function CornerBrackets({
  className,
  arm = 7,
  color = "border-ink",
}: {
  className?: string;
  arm?: number;
  /** A Tailwind border colour class for the marks. */
  color?: string;
}) {
  const style = { width: arm, height: arm };
  const corner = cn("pointer-events-none absolute", color);
  return (
    <span aria-hidden className={cn("pointer-events-none absolute inset-0", className)}>
      <span className={cn(corner, "left-0 top-0 border-l border-t")} style={style} />
      <span className={cn(corner, "right-0 top-0 border-r border-t")} style={style} />
      <span className={cn(corner, "bottom-0 left-0 border-b border-l")} style={style} />
      <span className={cn(corner, "bottom-0 right-0 border-b border-r")} style={style} />
    </span>
  );
}

type CommonProps = {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Invert `solid` and `bracket` for use over a dark photo. */
  onDark?: boolean;
  className?: string;
  children: ReactNode;
};

type ButtonAsLink = CommonProps & { href: string } & Omit<ComponentProps<"a">, "href" | "className" | "children">;
type ButtonAsButton = CommonProps & { href?: undefined } & Omit<ComponentProps<"button">, "className" | "children">;

/**
 * Renders a link when given `href` (external URLs open in a new tab) and a
 * `<button>` otherwise.
 */
export function Button({
  variant = "solid",
  size = "md",
  onDark = false,
  className,
  children,
  ...props
}: ButtonAsLink | ButtonAsButton) {
  const variantClass = (onDark && onDarkVariants[variant]) || cn(variants[variant], inkOutline);
  const classes = cn(base, variantClass, sizes[size], className);
  const content = (
    <>
      {variant === "bracket" && <CornerBrackets color={onDark ? "border-white" : undefined} />}
      {children}
    </>
  );

  if (typeof props.href === "string") {
    const { href, ...rest } = props as Omit<ButtonAsLink, keyof CommonProps>;
    if (/^https?:\/\//.test(href)) {
      return (
        <a href={href} className={classes} target="_blank" rel="noopener noreferrer" {...rest}>
          {content}
        </a>
      );
    }
    return (
      <Link href={href} className={classes} {...rest}>
        {content}
      </Link>
    );
  }

  const { type = "button", ...rest } = props as Omit<ButtonAsButton, keyof CommonProps>;
  return (
    <button type={type} className={classes} {...rest}>
      {content}
    </button>
  );
}
