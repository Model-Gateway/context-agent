import { ArrowUpRight, Check, SquareTerminal } from "lucide-react";
import type { ReactNode } from "react";
import { CopyButton } from "@/components/mock/platforms/copy-button";
import { MacAppMock } from "@/components/mock/platforms/mac-app-mock";
import { TerminalMock } from "@/components/mock/platforms/terminal-mock";
import { WindowsAppMock } from "@/components/mock/platforms/windows-app-mock";
import { AppleIcon, WindowsIcon } from "@/components/ui/brand-icons";
import { Button, type ButtonVariant } from "@/components/ui/button";
import { Eyebrow } from "@/components/ui/eyebrow";
import { Section } from "@/components/ui/frame";
import { PhotoPanel, type PhotoName } from "@/components/ui/photo";
import { Reveal } from "@/components/ui/reveal";
import { site } from "@/lib/site";

type Platform = {
  id: string;
  name: string;
  icon: ReactNode;
  /** Shown on one line, joined by middots; wraps only between parts. */
  requirements: string[];
  facts: string[];
  photo: PhotoName;
  position: string;
  mock: ReactNode;
  cta: { label: string; href: string; variant: ButtonVariant; icon?: ReactNode };
};

const platforms: Platform[] = [
  {
    id: "platform-mac",
    name: "macOS app",
    icon: <AppleIcon size={18} className="-mt-0.5" />,
    requirements: ["macOS 26 Tahoe or later", "Apple silicon"],
    facts: [
      "Signed and notarized, with ctx built in",
      "Updates itself in the background",
      "Chat, inspector, Changes and editor",
    ],
    photo: "pastel",
    position: "50% 45%",
    mock: <MacAppMock />,
    cta: {
      label: "Download for Mac",
      href: site.downloadMac,
      variant: "dark",
      icon: <AppleIcon size={15} className="-mt-0.5 shrink-0" />,
    },
  },
  {
    id: "platform-windows",
    name: "Windows app",
    icon: <WindowsIcon size={16} />,
    requirements: ["Windows 10 or 11", "x64 .exe or .msi"],
    facts: [
      "Bundles ctx.exe, runs without WSL",
      "CLI in PowerShell or Git Bash, too",
      "Git for Windows recommended",
    ],
    photo: "dusk",
    position: "62% 38%",
    mock: <WindowsAppMock />,
    cta: {
      label: "Download for Windows",
      href: site.downloadWindows,
      variant: "dark",
      icon: <WindowsIcon size={14} className="shrink-0" />,
    },
  },
  {
    id: "platform-terminal",
    name: "Terminal UI and CLI",
    icon: <SquareTerminal size={18} strokeWidth={1.5} />,
    requirements: ["Builds from source", "stable Rust 1.85+"],
    facts: [
      "Headless ctx run with stable exit codes",
      "Shares ~/.context-agent with the apps",
      "Slash commands, @\u00a0mentions, approvals",
    ],
    photo: "forest",
    position: "50% 78%",
    mock: <TerminalMock />,
    cta: { label: "Build from source", href: `${site.repo}#from-source`, variant: "light" },
  },
];

const quickStart = [
  { command: "ctx init", comment: "writes ~/.context-agent/config.toml" },
  { command: "ctx doctor --probe", comment: "checks what your endpoint supports" },
  { command: 'ctx run "Add a loading indicator to the login button"', comment: "in your repo: one task, end to end" },
] as const;

/**
 * One platform: a photo header with a small mock, facts, and its download.
 * Photo beside the text from md, above it once three fit in a row (xl).
 */
function PlatformCard({ platform }: { platform: Platform }) {
  return (
    <article
      aria-labelledby={platform.id}
      className="grid h-full grid-cols-1 overflow-hidden bg-surface ring-1 ring-inset ring-line md:grid-cols-[minmax(0,5fr)_minmax(0,6fr)] xl:grid-cols-1 xl:grid-rows-[auto_1fr]"
    >
      <PhotoPanel
        photo={platform.photo}
        position={platform.position}
        sizes="(min-width: 1280px) 352px, (min-width: 768px) 42vw, 100vw"
        className="flex h-[232px] items-center justify-center md:h-full md:min-h-[272px] xl:h-[236px] xl:min-h-0"
      >
        {platform.mock}
      </PhotoPanel>

      <div className="flex flex-col px-6 pb-6 pt-6 md:px-7 md:pt-7 xl:px-6">
        <h3 id={platform.id} className="flex items-center gap-2.5 text-[17px] tracking-[-0.01em] text-ink md:text-[18px]">
          <span className="grid size-[18px] shrink-0 place-items-center">{platform.icon}</span>
          {platform.name}
        </h3>
        <p className="mt-2 font-mono text-[12px] leading-[1.6] tracking-[0.01em] text-muted">
          {platform.requirements.map((part, i) => (
            <span key={part}>
              {i > 0 && " · "}
              <span className="whitespace-nowrap">{part}</span>
            </span>
          ))}
        </p>

        <ul className="mt-5 space-y-2.5 border-t border-line-soft pt-5">
          {platform.facts.map((fact) => (
            <li key={fact} className="flex gap-2.5 text-[14.5px] leading-[1.5] text-muted">
              <Check size={15} strokeWidth={1.75} className="mt-[3px] shrink-0 text-ink" aria-hidden />
              <span className="text-pretty">{fact}</span>
            </li>
          ))}
        </ul>

        <div className="mt-auto pt-7">
          <Button variant={platform.cta.variant} href={platform.cta.href} className="w-full">
            {platform.cta.icon}
            {platform.cta.label}
          </Button>
        </div>
      </div>
    </article>
  );
}

/** The three commands to get going, with a copy button for the block. */
function QuickStart() {
  const text = quickStart.map((line) => line.command).join("\n");
  return (
    <div className="mt-6 bg-surface ring-1 ring-inset ring-line md:mt-8">
      <div className="flex h-12 items-center justify-between gap-4 border-b border-line-soft pl-5 pr-2.5 md:pl-6">
        <h3 className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.08em] text-muted">
          <SquareTerminal size={14} strokeWidth={1.5} aria-hidden className="text-ink" />
          Quick start
        </h3>
        <CopyButton text={text} label="Copy quick start commands" />
      </div>

      {/* 11px below sm keeps "~/.context-agent/config.toml" on one line at 360px. */}
      <div className="px-5 py-5 font-mono text-[11px] leading-[1.65] sm:text-[13px] md:px-6 lg:grid lg:grid-cols-[max-content_minmax(0,1fr)] lg:gap-x-12 lg:gap-y-2 lg:text-[13.5px]">
        {quickStart.map((line) => (
          <div key={line.command} className="flex flex-col pb-3 last:pb-0 lg:contents">
            <code className="flex gap-2.5 text-ink">
              <span aria-hidden className="select-none text-faint">
                $
              </span>
              <span className="whitespace-pre-wrap break-words">{line.command}</span>
            </code>
            <span className="order-first whitespace-pre-wrap text-muted lg:order-none"># {line.comment}</span>
          </div>
        ))}
      </div>

      <p className="flex flex-col gap-2 border-t border-line-soft px-5 py-3.5 text-[13px] leading-[1.55] text-muted sm:flex-row sm:items-center sm:justify-between md:px-6">
        <span className="text-pretty">
          Set your endpoint and key in config.toml, or export OPENAI_API_KEY, before probing.
        </span>
        <a
          href={site.docsCli}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex shrink-0 items-center gap-1 font-mono text-[12px] text-ink underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
        >
          CLI reference
          <ArrowUpRight size={13} strokeWidth={1.6} aria-hidden />
        </a>
      </p>
    </div>
  );
}

/** Download: the Mac app, the Windows app and the terminal, plus a quick start. */
export function Platforms() {
  return (
    <Section id="download" tone="surface" labelledBy="download-title">
      <div className="flex flex-col gap-8 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <Eyebrow>Download</Eyebrow>
          <h2
            id="download-title"
            className="mt-5 text-balance text-[clamp(2.1rem,1.3rem+2.4vw,3.25rem)] font-normal leading-[1.06] tracking-[-0.035em] text-ink"
          >
            Works where you work
          </h2>
          <p className="mt-5 max-w-[36rem] text-pretty text-[17px] leading-[1.6] text-muted md:text-[19px]">
            One Rust runtime behind a native Mac app, a Windows app and a terminal UI. Sessions, tasks, the index and
            memories follow you between them.
          </p>
        </div>
        <a
          href={site.releases}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex shrink-0 items-center gap-1.5 self-start font-mono text-[12.5px] tracking-[0.01em] text-muted transition-colors hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink lg:self-auto"
        >
          v{site.version} &middot; All releases
          <ArrowUpRight size={13} strokeWidth={1.6} aria-hidden />
        </a>
      </div>

      <ul role="list" className="mt-12 grid gap-5 md:mt-16 xl:grid-cols-3 xl:gap-6">
        {platforms.map((platform, i) => (
          <Reveal as="li" key={platform.id} delay={i * 80} className="h-full">
            <PlatformCard platform={platform} />
          </Reveal>
        ))}
      </ul>

      <Reveal delay={80}>
        <QuickStart />
      </Reveal>
    </Section>
  );
}
