import { ComposerMock } from "@/components/mock/cta/composer-mock";
import { ReportMock } from "@/components/mock/cta/report-mock";
import { AppleIcon, GitHubIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { Eyebrow } from "@/components/ui/eyebrow";
import { GlassFrame, PhotoPanel } from "@/components/ui/photo";
import { site } from "@/lib/site";

/**
 * The closing call to action: a full-column dusk photo with the headline and
 * download buttons on its dark left side, and the app's composer (a task
 * about to be sent) above the report that comes back, on the right.
 */
export function FinalCta() {
  return (
    <section aria-labelledby="final-cta-title" className="relative">
      <PhotoPanel
        photo="dusk"
        position="var(--cta-photo-position)"
        sizes="(min-width: 1248px) 1200px, 100vw"
        className="[--cta-photo-position:22%_30%] lg:min-h-[640px] lg:[--cta-photo-position:50%_14%]"
      >
        {/* Darkens the sky behind the copy so white text keeps its contrast. */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-[linear-gradient(180deg,rgba(6,11,26,0.62)_0%,rgba(6,11,26,0.4)_38%,rgba(6,11,26,0)_70%)] lg:bg-[linear-gradient(90deg,rgba(6,11,26,0.66)_0%,rgba(6,11,26,0.42)_34%,rgba(6,11,26,0)_60%)]"
        />

        <div className="relative grid grid-cols-1 lg:min-h-[640px] lg:grid-cols-[minmax(0,1fr)_minmax(0,1.02fr)]">
          {/* Copy */}
          <div className="flex flex-col justify-center px-5 pb-12 pt-16 sm:px-8 sm:pb-14 sm:pt-20 lg:py-24 lg:pl-12 lg:pr-6">
            <Eyebrow onDark>Get started</Eyebrow>
            <h2
              id="final-cta-title"
              className="mt-6 max-w-[11ch] text-balance text-[clamp(2.5rem,1rem+4.2vw,4.75rem)] font-medium leading-[1] tracking-[-0.045em] text-white"
            >
              Give it a real task.
            </h2>
            <p className="mt-6 max-w-[29rem] text-[17px] leading-[1.6] text-white/85 md:text-[18px]">
              Download Context, point it at your repository and describe the change. It will tell you what it did
              &mdash; and what it couldn&rsquo;t verify.
            </p>

            <div className="mt-9 flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4">
              <Button
                variant="solid"
                size="lg"
                onDark
                href={site.downloadMac}
              >
                <AppleIcon size={16} className="-mt-0.5" />
                Download for Mac
              </Button>
              <Button
                variant="bracket"
                size="lg"
                onDark
                href={site.repo}
              >
                <GitHubIcon size={15} />
                View on GitHub
              </Button>
            </div>

            <p className="mt-6 font-mono text-[12px] leading-[1.6] tracking-[0.02em] text-white/70">
              Also for{" "}
              <a
                href={site.downloadWindows}
                target="_blank"
                rel="noopener noreferrer"
                className="text-white underline decoration-white/40 underline-offset-4 transition-colors hover:decoration-white focus-visible:rounded-[2px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white"
              >
                Windows x64
              </a>{" "}
              and <span className="whitespace-nowrap">the terminal &middot; v{site.version}</span>
            </p>
          </div>

          {/* The task going in, and the report coming back. */}
          <div className="relative flex flex-col px-5 sm:px-8 lg:justify-center lg:pl-4 lg:pr-12 lg:pt-6">
            <GlassFrame padding={6} radius={20} className="w-full max-w-[560px] lg:max-w-none">
              <ComposerMock />
            </GlassFrame>
            <GlassFrame
              padding={6}
              radius={18}
              className="mb-12 ml-6 mt-5 w-[min(calc(100%-1.5rem),430px)] sm:mb-16 sm:ml-16 lg:mb-0 lg:ml-auto lg:mr-[-72px] lg:w-[460px]"
            >
              <ReportMock />
            </GlassFrame>
          </div>
        </div>
      </PhotoPanel>
    </section>
  );
}
