import type { ReactNode } from "react";
import { PhotoPanel, type PhotoName } from "@/components/ui/photo";
import { cn } from "@/lib/cn";

/** A close crop of one banner photo: how far to zoom in, and the point to keep in view. */
export type PhotoCrop = {
  photo: PhotoName;
  /** 1 shows the photo as `PhotoPanel` would; 2 shows a quarter of that area. */
  zoom: number;
  /** The point of the photo to keep in view, in percent from the top left. */
  focus: readonly [x: number, y: number];
};

/**
 * The photos are soft painted skies with grain laid over them in CSS, so a
 * zoomed crop still looks right from a smaller file than its box would ask for.
 */
const softness = 0.7;

/**
 * Rendered width of a feature card's photo at each breakpoint (see
 * features.tsx), times the zoom (and `softness`), so next/image picks a file
 * that is sharp enough without fetching the full original.
 */
function sizesFor(zoom: number) {
  const scale = (n: number, unit: "px" | "vw") => `${Math.round(n * zoom * softness)}${unit}`;
  return [
    `(min-width: 1280px) ${scale(360, "px")}`,
    `(min-width: 1024px) ${scale(29, "vw")}`,
    `(min-width: 768px) ${scale(44, "vw")}`,
    scale(87, "vw"),
  ].join(", ");
}

/**
 * Keeps a fixed-size mock-up the same share of its photo as the card
 * narrows or widens. The photo is a size container; `zoom` scales the mock's
 * layout box too, so nothing around it has to know.
 */
const mockScale =
  "[zoom:1] @min-[460px]:[zoom:1.12] @max-[380px]:[zoom:0.92] @max-[322px]:[zoom:0.85] @max-[296px]:[zoom:0.8]";

/**
 * The photo half of a feature card: a patch of painted sky with a floating
 * mock-up centred on it.
 *
 * `PhotoPanel` positions a photo but always shows most of it, and at a
 * square aspect these photos read as busy orange cloud. Here the panel is
 * drawn `zoom` times larger than the card and shifted so `focus` stays in
 * view, which gives the calm, single-colour sky the cards need.
 */
export function FeaturePhoto({
  crop,
  className,
  children,
}: {
  crop: PhotoCrop;
  className?: string;
  children: ReactNode;
}) {
  const { photo, zoom, focus } = crop;
  const [x, y] = focus;
  const spill = zoom - 1;
  return (
    <div className={cn("@container relative isolate overflow-hidden bg-paper", className)}>
      <PhotoPanel
        photo={photo}
        position={`${x}% ${y}%`}
        sizes={sizesFor(zoom)}
        style={{
          position: "absolute",
          width: `${zoom * 100}%`,
          height: `${zoom * 100}%`,
          left: `${-spill * x}%`,
          top: `${-spill * y}%`,
        }}
      />
      <div className="absolute inset-0 flex items-center justify-center">
        <div className={mockScale}>{children}</div>
      </div>
    </div>
  );
}
