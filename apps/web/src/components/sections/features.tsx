import { CircleArrowDown, ScanSearch, ShieldCheck, Sparkles, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { FeaturePhoto, type PhotoCrop } from "@/components/mock/features/feature-photo";
import { ModelConfigMock } from "@/components/mock/features/model-config";
import { PatchNotificationsMock } from "@/components/mock/features/patch-notifications";
import { RetrievedContextMock } from "@/components/mock/features/retrieved-context";
import { AppleIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { Eyebrow } from "@/components/ui/eyebrow";
import { Section } from "@/components/ui/frame";
import { Reveal } from "@/components/ui/reveal";
import { site } from "@/lib/site";

type Feature = {
  title: string;
  icon: LucideIcon;
  body: ReactNode;
  /** Close crops of the same pale sky, so the three cards read as one set. */
  crop: PhotoCrop;
  mock: ReactNode;
};

const features: Feature[] = [
  {
    title: "Starts from the right code",
    icon: ScanSearch,
    body: "A Tree-sitter index, lexical search, a symbol graph, embeddings and memory are fused for every request and checked against the working tree, so each task starts from the files that matter.",
    crop: { photo: "pastel", zoom: 2.6, focus: [4, 2] },
    mock: <RetrievedContextMock />,
  },
  {
    title: "Patches that never overwrite yours",
    icon: ShieldCheck,
    body: "Every patch carries the hash of the text it was written against and is refused if the file changed underneath. Each file touched and command run is recorded.",
    crop: { photo: "pastel", zoom: 2.8, focus: [14, 6] },
    mock: <PatchNotificationsMock />,
  },
  {
    title: "Any model, checked first",
    icon: Sparkles,
    body: (
      <>
        Point it at OpenAI, a gateway, a self-hosted server or a local runtime.{" "}
        <code className="whitespace-nowrap font-mono text-[0.9em] text-ink-2">ctx doctor --probe</code> checks what
        the endpoint really supports, and Context relies only on that.
      </>
    ),
    crop: { photo: "pastel", zoom: 2.6, focus: [0, 12] },
    mock: <ModelConfigMock />,
  },
];

/** Heading row and three photo cards: retrieval, safe edits and bring-your-own model. */
export function Features() {
  return (
    <Section id="features" tone="surface" labelledBy="features-title">
      <div className="flex flex-col gap-8 xl:flex-row xl:items-end xl:justify-between xl:gap-10">
        <div>
          <Eyebrow>Features</Eyebrow>
          <h2
            id="features-title"
            className="mt-5 text-balance text-[clamp(2.1rem,1.3rem+2.4vw,3.25rem)] font-normal leading-[1.06] tracking-[-0.035em] text-ink"
          >
            Give your agent the context <br className="hidden sm:inline" />
            it was missing
          </h2>
        </div>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center xl:shrink-0 xl:pb-1">
          <Button variant="dark" href={site.downloadMac}>
            <AppleIcon size={15} className="-mt-0.5" />
            Download for Mac
          </Button>
          <Button variant="light" href="#how-it-works">
            See how it works
            <CircleArrowDown size={17} strokeWidth={1.6} className="-mr-0.5" aria-hidden />
          </Button>
        </div>
      </div>

      <ul className="mt-12 grid gap-14 md:mt-16 md:gap-12 lg:grid-cols-3 lg:gap-5">
        {features.map(({ title, icon: Icon, body, crop, mock }, i) => (
          <Reveal
            as="li"
            key={title}
            delay={i * 80}
            className="flex flex-col md:grid md:grid-cols-2 md:items-center md:gap-10 lg:flex lg:items-stretch lg:gap-0"
          >
            <FeaturePhoto crop={crop} className="aspect-[1/1.02] sm:aspect-[4/3] md:aspect-[1/1.02]">
              {mock}
            </FeaturePhoto>
            <div className="mt-6 md:mt-0 lg:mt-6">
              <h3 className="flex items-start gap-2.5 text-balance text-[17px] leading-[1.4] tracking-[-0.01em] text-ink md:text-[18px]">
                <Icon size={18} strokeWidth={1.5} className="mt-[3px] shrink-0" aria-hidden />
                {title}
              </h3>
              <p className="mt-3 max-w-[36ch] text-pretty text-[15px] leading-[1.6] text-muted">{body}</p>
            </div>
          </Reveal>
        ))}
      </ul>
    </Section>
  );
}
