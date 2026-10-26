import { ArrowUpRight } from "lucide-react";
import { FaqItem } from "@/components/faq/faq-item";
import { faqs } from "@/components/faq/questions";
import { Eyebrow } from "@/components/ui/eyebrow";
import { Section } from "@/components/ui/frame";
import { site } from "@/lib/site";

/** Seven common questions, answered from the README and docs. */
export function Faq() {
  return (
    <Section id="faq" tone="surface" labelledBy="faq-title">
      <div className="grid gap-12 lg:grid-cols-12 lg:gap-10">
        <div className="lg:col-span-5">
          <div className="lg:sticky lg:top-[124px]">
            <Eyebrow>FAQ</Eyebrow>
            <h2
              id="faq-title"
              className="mt-5 text-[clamp(2.1rem,1.3rem+2.4vw,3.25rem)] font-normal leading-[1.06] tracking-[-0.035em] text-ink"
            >
              Questions, answered
            </h2>
            <p className="mt-5 max-w-[24rem] text-[17px] leading-[1.6] text-muted">
              Something missing?{" "}
              <a
                href={site.issues}
                target="_blank"
                rel="noopener noreferrer"
                className="group/link inline-flex items-baseline gap-0.5 text-ink underline decoration-line underline-offset-[5px] transition-colors hover:decoration-ink focus-visible:rounded-[2px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
              >
                Open an issue on GitHub.
                <ArrowUpRight
                  size={15}
                  strokeWidth={1.6}
                  aria-hidden
                  className="translate-y-[2px] text-muted transition-transform group-hover/link:-translate-y-px group-hover/link:translate-x-px group-hover/link:text-ink"
                />
              </a>
            </p>
          </div>
        </div>

        <div className="border-t border-line lg:col-span-7">
          {faqs.map((faq, i) => (
            <FaqItem key={faq.question} index={i + 1} question={faq.question} defaultOpen={i === 0}>
              {faq.answer}
            </FaqItem>
          ))}
        </div>
      </div>
    </Section>
  );
}
