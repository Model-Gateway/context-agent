import { Capabilities } from "@/components/sections/capabilities";
import { Faq } from "@/components/sections/faq";
import { Features } from "@/components/sections/features";
import { FinalCta } from "@/components/sections/final-cta";
import { Footer } from "@/components/sections/footer";
import { Hero } from "@/components/sections/hero";
import { HowItWorks } from "@/components/sections/how-it-works";
import { ModelsStrip } from "@/components/sections/models";
import { Nav } from "@/components/sections/nav";
import { Platforms } from "@/components/sections/platforms";
import { Pricing } from "@/components/sections/pricing";
import { Showcase } from "@/components/sections/showcase";
import { HatchBand, PageFrame } from "@/components/ui/frame";

export default function Home() {
  return (
    <PageFrame>
      <Nav />
      <main>
        <HatchBand topRule={false} />
        <Hero />
        <HatchBand />
        <Showcase />
        <HatchBand />
        <ModelsStrip />
        <HatchBand />
        <Features />
        <HatchBand />
        <Capabilities />
        <HatchBand />
        <HowItWorks />
        <HatchBand />
        <Platforms />
        <HatchBand />
        <Pricing />
        <HatchBand />
        <Faq />
        <HatchBand />
        <FinalCta />
        <HatchBand />
      </main>
      <Footer />
    </PageFrame>
  );
}
