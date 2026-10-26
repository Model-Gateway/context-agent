import Image, { type StaticImageData } from "next/image";
import type { CSSProperties, ReactNode } from "react";
import { cn } from "@/lib/cn";

import dusk from "@/assets/photos/banner-app-dusk.jpg";
import ember from "@/assets/photos/banner-app-ember.jpg";
import forest from "@/assets/photos/banner-app-forest.jpg";
import pastel from "@/assets/photos/banner-app-pastel.jpg";
import marketplace from "@/assets/photos/banner-marketplace.jpg";

/**
 * The app's own banner art: grainy painted skies.
 *
 * - `pastel`: pale blue sky with peach and cream clouds (soft, light)
 * - `dusk`: deep blue sky with lit orange-red clouds (warm, rich)
 * - `ember`: saturated orange cloud wash
 * - `forest`: near-black green sky with an orange glow at the bottom (dark)
 * - `marketplace`: cream top, red-orange swirl, indigo bottom
 */
export const photos = { pastel, dusk, ember, forest, marketplace } satisfies Record<string, StaticImageData>;
export type PhotoName = keyof typeof photos;

/**
 * Overlays that calm a photo so white UI reads on it:
 * `none` leaves it as is, `soft` lifts it slightly, `mist` washes it out
 * towards the paper colour, `shade` darkens it for light text.
 */
export type PhotoTone = "none" | "soft" | "mist" | "shade";

const toneClass: Record<PhotoTone, string> = {
  none: "",
  soft: "bg-white/15",
  mist: "bg-[linear-gradient(180deg,rgba(255,255,255,0.45),rgba(255,255,255,0.2))]",
  shade: "bg-black/25",
};

/**
 * A panel filled with one of the banner photos plus film grain. Children are
 * laid over it; the panel clips them, so UI can bleed off its edges.
 */
export function PhotoPanel({
  photo,
  position = "50% 50%",
  tone = "none",
  sizes = "(min-width: 1200px) 1100px, 100vw",
  preload = false,
  className,
  style,
  children,
}: {
  photo: PhotoName;
  /** CSS object-position, to show a different part of the same photo. */
  position?: string;
  tone?: PhotoTone;
  sizes?: string;
  /** Preload the image; only for the first photo above the fold. */
  preload?: boolean;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
}) {
  return (
    <div className={cn("relative isolate overflow-hidden bg-paper", className)} style={style}>
      <Image
        src={photos[photo]}
        alt=""
        fill
        sizes={sizes}
        preload={preload}
        placeholder="blur"
        className="-z-10 object-cover"
        style={{ objectPosition: position }}
      />
      {tone !== "none" && <div aria-hidden className={cn("absolute inset-0 -z-10", toneClass[tone])} />}
      <div aria-hidden className="bg-grain pointer-events-none absolute inset-0 -z-10 opacity-[0.18] mix-blend-overlay" />
      {children}
    </div>
  );
}

/**
 * The frosted border that holds a UI card on a photo: a translucent white
 * rim (padding) with a brighter hairline, blurred backdrop and a soft drop
 * shadow. Give the child its own radius of `radius - padding`.
 */
export function GlassFrame({
  children,
  className,
  padding = 8,
  radius = 18,
}: {
  children: ReactNode;
  className?: string;
  padding?: number;
  radius?: number;
}) {
  return (
    <div
      className={cn(
        "relative bg-white/30 shadow-[0_30px_60px_-20px_rgba(20,20,30,0.35)] ring-1 ring-inset ring-white/60 backdrop-blur-md",
        className,
      )}
      style={{ padding, borderRadius: radius }}
    >
      {children}
    </div>
  );
}
