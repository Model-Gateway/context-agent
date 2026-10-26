import { AppWindow } from "@/components/mock/app/app-window";
import { GlassFrame, PhotoPanel } from "@/components/ui/photo";
import { Reveal } from "@/components/ui/reveal";
import { site } from "@/lib/site";

/**
 * The product shot under the hero: the macOS app floating in a glass frame
 * on the banner sky, cropped by the panel's bottom edge.
 */
export function Showcase() {
  return (
    <section aria-labelledby="showcase-title" className="relative bg-paper">
      <h2 id="showcase-title" className="sr-only">
        The Context app for macOS
      </h2>

      <PhotoPanel
        photo="pastel"
        position="50% 62%"
        tone="none"
        sizes="(min-width: 1248px) 1200px, 100vw"
        className="h-[500px] sm:h-[540px] md:h-[600px] lg:h-[640px] xl:h-[668px]"
      >
        <div className="absolute inset-x-4 top-4 flex items-center justify-between font-mono text-[11px] tracking-[0.08em] text-ink/80 sm:inset-x-6 md:inset-x-10 md:top-6 lg:inset-x-14 xl:inset-x-[92px] xl:top-8">
          <p>
            <span className="uppercase">Context for macOS</span> &middot; v{site.version}
          </p>
          <p className="hidden uppercase sm:block">macOS 26 &middot; Apple silicon</p>
        </div>

        <Reveal className="absolute inset-x-4 top-11 sm:inset-x-6 sm:top-14 md:inset-x-10 md:top-[72px] lg:inset-x-14 lg:top-[84px] xl:inset-x-[92px] xl:top-[92px]">
          <GlassFrame padding={10} radius={20}>
            <AppWindow className="h-[560px] sm:h-[620px] md:h-[660px] lg:h-[700px] xl:h-[720px]" />
          </GlassFrame>
        </Reveal>
      </PhotoPanel>
    </section>
  );
}
