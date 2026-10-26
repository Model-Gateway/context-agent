import { ArrowUpRight } from "lucide-react";
import { PlanCard } from "@/components/pricing/plan-card";
import { AppleIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { Eyebrow } from "@/components/ui/eyebrow";
import { Section } from "@/components/ui/frame";
import { Reveal } from "@/components/ui/reveal";
import { site } from "@/lib/site";

/** Button plus a quiet alternative; stacked until both cards have room for a row, so the buttons line up. */
const ctaRow = "flex flex-col items-start gap-3 xl:flex-row xl:items-center xl:gap-5";

const textLink =
  "font-mono text-[13px] text-muted underline decoration-line underline-offset-4 transition-colors hover:text-ink hover:decoration-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink";

/** Pricing: the free open-source app, and the optional ModelGateway sign-in. */
export function Pricing() {
  return (
    <Section id="pricing" tone="paper" labelledBy="pricing-title">
      <Eyebrow>Pricing</Eyebrow>
      <h2
        id="pricing-title"
        className="mt-5 text-[clamp(2.1rem,1.3rem+2.4vw,3.25rem)] font-normal leading-[1.06] tracking-[-0.035em] text-ink"
      >
        <span className="sm:block">Free and open source.</span> <span className="sm:block">Bring your own key.</span>
      </h2>
      <p className="mt-5 max-w-[36rem] text-pretty text-[17px] leading-[1.6] text-muted md:text-[19px]">
        Context itself is free. You pay your model provider directly, or sign in to ModelGateway from the app for one
        account across many models.
      </p>

      <div className="mt-12 grid gap-5 md:mt-16 md:grid-cols-2 lg:gap-6">
        <Reveal className="h-full">
          <PlanCard
            id="plan-open-source"
            name="Open source"
            tag={<span className="whitespace-nowrap font-mono text-[11px] uppercase tracking-[0.08em] text-muted">No account needed</span>}
            price="$0"
            priceNote="forever"
            summary="Apache-2.0 or MIT, at your option."
            features={[
              "Any OpenAI-compatible endpoint",
              "macOS app, Windows app, terminal UI and CLI",
              "Index, history and memory stay on your machine",
              "MCP apps from a catalog or your own",
              "Spending limits and per-model pricing in Settings",
            ]}
          >
            <div className={ctaRow}>
              <Button variant="solid" href={site.downloadMac}>
                <AppleIcon size={15} className="-mt-0.5" />
                Download for Mac
              </Button>
              <a href={`${site.repo}#from-source`} target="_blank" rel="noopener noreferrer" className={textLink}>
                or build from source
              </a>
            </div>
          </PlanCard>
        </Reveal>

        <Reveal delay={80} className="h-full">
          <PlanCard
            id="plan-modelgateway"
            name="With ModelGateway"
            featured
            tag={
              <span className="inline-flex h-6 items-center whitespace-nowrap rounded-[4px] bg-white/10 px-2 font-mono text-[11px] uppercase leading-none tracking-[0.06em] text-white ring-1 ring-inset ring-white/20">
                $10 credit
              </span>
            }
            price="Pay as you go"
            summary="Optional. One sign-in for many models, with usage and billing in the app."
            features={[
              "Sign in from the app, no key to paste",
              "Balance and usage by project in the app",
              <>
                A one-time $10 Context credit after your first deposit
                <span aria-hidden className="text-muted">
                  *
                </span>
              </>,
              "Switch back to your own key any time",
            ]}
          >
            <div className={ctaRow}>
              <Button variant="bracket" href={site.modelGateway}>
                Visit ModelGateway
                <ArrowUpRight size={15} strokeWidth={1.6} aria-hidden />
              </Button>
              <span className="font-mono text-[13px] text-muted">or sign in from the app</span>
            </div>
          </PlanCard>
        </Reveal>
      </div>

      <p className="mt-6 max-w-[46rem] text-pretty text-[13px] leading-[1.6] text-muted">
        <span aria-hidden>* </span>
        The $10 Context credit can be claimed once per ModelGateway account after a completed card or crypto deposit,
        and only Context&rsquo;s own requests can spend it.
      </p>
    </Section>
  );
}
