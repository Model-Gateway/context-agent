import { AppleIcon, GitHubIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { columnPadding } from "@/components/ui/frame";
import { site } from "@/lib/site";
import { cn } from "@/lib/cn";

/** Headline, one-paragraph pitch and the two calls to action. */
export function Hero() {
  return (
    <section aria-labelledby="hero-title" className="relative overflow-hidden bg-paper">
      {/* Dots on the right half, dissolving into the copy. */}
      <div aria-hidden className="bg-dots mask-fade-left pointer-events-none absolute inset-y-0 right-0 w-[62%] opacity-80" />

      <div className={cn("relative py-20 md:py-28 lg:py-32", columnPadding)}>
        <h1
          id="hero-title"
          className="max-w-[15ch] text-[clamp(2.6rem,1.2rem+3.9vw,4.1rem)] font-medium leading-[1.02] tracking-[-0.045em] text-ink sm:max-w-[17ch]"
        >
          A coding agent that finishes the task and shows its work.
        </h1>
        <p className="mt-6 max-w-[34rem] text-[17px] leading-[1.65] tracking-[-0.01em] text-muted md:text-[18px]">
          Context indexes your repository, reads the current source before every edit, runs your project&rsquo;s own
          tests and tells you exactly what it changed. Local-first, with any OpenAI-compatible model.
        </p>

        <div className="mt-10 flex flex-col gap-3 sm:flex-row sm:items-center sm:gap-4">
          <Button variant="solid" size="lg" href={site.downloadMac}>
            <AppleIcon size={16} className="-mt-0.5" />
            Download for Mac
          </Button>
          <Button variant="bracket" size="lg" href={site.repo}>
            <GitHubIcon size={15} />
            View on GitHub
          </Button>
        </div>

        <p className="mt-6 font-mono text-[12px] leading-[1.7] tracking-[0.02em] text-muted">
          <span className="whitespace-nowrap">v{site.version}</span> &middot;{" "}
          <span className="whitespace-nowrap">Free and open source</span> &middot;{" "}
          <span className="whitespace-nowrap">macOS, Windows and the terminal</span>
        </p>
      </div>
    </section>
  );
}
