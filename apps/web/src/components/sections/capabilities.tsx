import type { ReactNode } from "react";
import { ApprovalMock } from "@/components/mock/capabilities/approval-mock";
import { ChangesMock } from "@/components/mock/capabilities/changes-mock";
import { ContextMock } from "@/components/mock/capabilities/context-mock";
import { HeadlessMock } from "@/components/mock/capabilities/headless-mock";
import { SubAgentsMock } from "@/components/mock/capabilities/agents-mock";
import { Section } from "@/components/ui/frame";
import { PhotoPanel, type PhotoName, type PhotoTone } from "@/components/ui/photo";
import { Reveal } from "@/components/ui/reveal";
import { cn } from "@/lib/cn";

type Photo = { photo: PhotoName; position?: string; tone?: PhotoTone };

/** A command named in running copy. */
function Cmd({ children }: { children: ReactNode }) {
  return (
    <code className="whitespace-nowrap rounded-[4px] bg-surface-2 px-1 py-px font-mono text-[0.88em] text-ink-2 ring-1 ring-inset ring-black/[0.05]">
      {children}
    </code>
  );
}

/**
 * One of the two large cards: a photo with app UI inset from its top-left
 * corner that runs off the right and bottom edges, then a title and a line
 * of copy. The card is a size container: the mock-ups size their first pane
 * against it so that pane always fits on phones.
 */
function FeatureCard({
  title,
  body,
  photo,
  delay,
  children,
}: {
  title: string;
  body: ReactNode;
  photo: Photo;
  delay: number;
  children: ReactNode;
}) {
  return (
    <Reveal as="article" delay={delay} className="@container">
      <PhotoPanel
        {...photo}
        sizes="(min-width: 1200px) 552px, (min-width: 1024px) 46vw, (min-width: 768px) 88vw, 94vw"
        className="aspect-square rounded-[3px] sm:aspect-[4/3] md:aspect-[16/10] lg:aspect-[4/3]"
      >
        <div className="absolute left-5 top-5 sm:left-8 sm:top-8">{children}</div>
      </PhotoPanel>
      <h3 className="mt-5 text-balance text-[20px] leading-[1.3] tracking-[-0.02em] text-ink md:mt-6 md:text-[22px]">{title}</h3>
      <p className="mt-2 max-w-[44ch] text-pretty text-[16px] leading-[1.6] text-muted md:text-[17px]">{body}</p>
    </Reveal>
  );
}

/**
 * One of the three smaller cards: a UI card inset from the top and sides
 * of its photo and cropped at the bottom. Three across from xl; two across
 * below that, with the last card spanning the row.
 */
function DetailCard({
  title,
  body,
  photo,
  delay,
  className,
  aspect = "aspect-[4/3] sm:aspect-[16/10] md:aspect-[4/3] lg:aspect-[16/10]",
  sizes = "(min-width: 1280px) 360px, (min-width: 768px) 46vw, 94vw",
  inset = "right-4 sm:right-6",
  children,
}: {
  title: string;
  body: ReactNode;
  photo: Photo;
  delay: number;
  className?: string;
  /** The photo's aspect-ratio classes, for every breakpoint. */
  aspect?: string;
  sizes?: string;
  /** Right offset (or width) of the UI card; a negative offset runs it off the edge. */
  inset?: string;
  children: ReactNode;
}) {
  return (
    <Reveal as="article" delay={delay} className={className}>
      <PhotoPanel
        {...photo}
        sizes={sizes}
        className={cn("rounded-[3px]", aspect)}
      >
        <div className={cn("absolute left-4 top-4 sm:left-6 sm:top-6", inset)}>{children}</div>
      </PhotoPanel>
      <h3 className="mt-4 text-balance text-[18px] leading-[1.3] tracking-[-0.015em] text-ink md:mt-5 md:text-[19px]">{title}</h3>
      <p className="mt-1.5 max-w-[46ch] text-pretty text-[15px] leading-[1.6] text-muted">{body}</p>
    </Reveal>
  );
}

/**
 * The capabilities bento: a large headline, two large cards whose UI bleeds
 * off the photo, then three smaller cards.
 */
export function Capabilities() {
  return (
    <Section id="capabilities" tone="surface" labelledBy="capabilities-title">
      <h2
        id="capabilities-title"
        className="text-[clamp(2.5rem,1rem+4.2vw,4.75rem)] font-medium leading-[1] tracking-[-0.045em] text-ink"
      >
        Everything an agent needs
        <br className="hidden md:inline" /> to finish the job.
      </h2>
      <p className="mt-6 max-w-[38rem] text-[17px] leading-[1.6] tracking-[-0.01em] text-muted md:mt-8 md:text-[20px] md:leading-[1.55]">
        Plan, edit, validate and recover in one tool loop, then review the result in a native app built for it.
      </p>

      <div className="mt-14 grid gap-x-4 gap-y-12 md:mt-20 lg:grid-cols-2">
        <FeatureCard
          title="Sub-agents that work in parallel"
          body="At the Ultra effort tier Context deploys bounded sub-agents — read-only explorers or full implementers under the same permissions — that work side by side and report back."
          photo={{ photo: "forest", position: "50% 20%" }}
          delay={0}
        >
          <SubAgentsMock />
        </FeatureCard>
        <FeatureCard
          title="Review every change before it lands"
          body="See every diff the agent made in one place. Stage, commit, branch or open a pull request without leaving the app."
          photo={{ photo: "pastel", position: "20% 30%", tone: "soft" }}
          delay={80}
        >
          <ChangesMock />
        </FeatureCard>
      </div>

      <div className="mt-12 grid gap-x-4 gap-y-10 md:grid-cols-2 lg:mt-16 xl:grid-cols-3">
        <DetailCard
          title="Context is assembled, not accumulated"
          body={
            <>
              Stable policy and tool schemas come first so provider caches hit; old turns are compacted into checkpoints.{" "}
              <Cmd>ctx context explain</Cmd> shows what the model saw and why.
            </>
          }
          photo={{ photo: "dusk", position: "15% 10%", tone: "mist" }}
          delay={0}
        >
          <ContextMock />
        </DetailCard>
        <DetailCard
          title="Permissions you can read"
          body="Three permission profiles, a command classifier that looks through pipes and wrappers, path policy with symlink checks, and credential redaction everywhere."
          photo={{ photo: "forest", position: "70% 0%" }}
          delay={80}
        >
          <ApprovalMock />
        </DetailCard>
        <DetailCard
          title="At home in the terminal"
          body={
            <>
              The same runtime runs as a full-screen terminal UI and as headless <Cmd>ctx run</Cmd> with stable exit codes
              for CI. Sessions and memories follow you between the app and the terminal.
            </>
          }
          photo={{ photo: "pastel", position: "85% 15%", tone: "mist" }}
          delay={160}
          className="md:col-span-2 xl:col-span-1"
          aspect="aspect-[4/3] sm:aspect-[16/10] md:aspect-[21/9] lg:aspect-[3/1] xl:aspect-[16/10]"
          sizes="(min-width: 1280px) 360px, (min-width: 768px) 92vw, 94vw"
          inset="-right-10 md:right-auto md:w-[560px] lg:w-[640px] xl:-right-10 xl:w-auto"
        >
          <HeadlessMock />
        </DetailCard>
      </div>
    </Section>
  );
}
