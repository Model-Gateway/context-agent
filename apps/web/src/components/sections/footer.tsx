import { ArrowUpRight } from "lucide-react";
import Link from "next/link";
import { Crosshair, columnPadding } from "@/components/ui/frame";
import { Wordmark } from "@/components/ui/logo";
import { site } from "@/lib/site";
import { cn } from "@/lib/cn";

type FooterLink = { label: string; href: string };

const columns: ReadonlyArray<{ title: string; links: ReadonlyArray<FooterLink> }> = [
  {
    title: "Product",
    links: [
      { label: "Features", href: "#features" },
      { label: "How it works", href: "#how-it-works" },
      { label: "Download", href: "#download" },
      { label: "Pricing", href: "#pricing" },
      { label: "FAQ", href: "#faq" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "Documentation", href: site.docs },
      { label: "Configuration", href: site.docsConfiguration },
      { label: "Security", href: site.docsSecurity },
      { label: "CLI reference", href: site.docsCli },
      { label: "Architecture", href: site.docsArchitecture },
      { label: "Releases", href: site.releases },
    ],
  },
  {
    title: "Project",
    links: [
      { label: "GitHub", href: site.repo },
      { label: "Issues", href: site.issues },
      { label: "License", href: site.license },
      { label: "ModelGateway", href: site.modelGateway },
    ],
  },
];

const linkClass =
  "group/link inline-flex items-center gap-1 rounded-[2px] text-[15px] text-ink-2 transition-colors hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink";

function FooterAnchor({ link }: { link: FooterLink }) {
  if (/^https?:\/\//.test(link.href)) {
    return (
      <a href={link.href} target="_blank" rel="noopener noreferrer" className={linkClass}>
        {link.label}
        <ArrowUpRight
          size={13}
          strokeWidth={1.6}
          aria-hidden
          className="text-faint transition-[color,transform] group-hover/link:-translate-y-px group-hover/link:translate-x-px group-hover/link:text-ink"
        />
      </a>
    );
  }
  return (
    <a href={link.href} className={linkClass}>
      {link.label}
    </a>
  );
}

/**
 * The page's last block: brand, three link columns, a giant hatched
 * "Context" sinking behind the bottom rule, and the copyright line. The
 * page puts a `HatchBand` above it, which draws its top rule and crosshairs.
 */
export function Footer() {
  return (
    <footer className="relative bg-paper">
      <div className={cn("grid gap-14 pb-12 pt-16 md:pb-16 md:pt-20 lg:grid-cols-12 lg:gap-10", columnPadding)}>
        <div className="lg:col-span-5">
          <Link
            href="/"
            aria-label="Context home"
            className="inline-flex rounded-[4px] focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink"
          >
            <Wordmark />
          </Link>
          <p className="mt-6 max-w-[21rem] text-[15px] leading-[1.6] text-muted">
            The local-first coding agent that finishes the task and shows its work.
          </p>
          <p className="mt-5 font-mono text-[12px] tracking-[0.02em] text-muted">
            v{site.version} &middot; Apache-2.0 or MIT
          </p>
        </div>

        <nav aria-label="Footer" className="grid grid-cols-2 gap-x-8 gap-y-12 sm:grid-cols-3 lg:col-span-7">
          {/* Labels rather than headings, so the page outline stays one h2 per section. */}
          {columns.map((column) => (
            <div key={column.title}>
              <p
                id={`footer-${column.title.toLowerCase()}`}
                className="font-mono text-[11px] uppercase leading-none tracking-[0.08em] text-muted"
              >
                {column.title}
              </p>
              <ul aria-labelledby={`footer-${column.title.toLowerCase()}`} className="mt-6 space-y-3.5">
                {column.links.map((link) => (
                  <li key={link.label}>
                    <FooterAnchor link={link} />
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>
      </div>

      {/*
       * The name in hatching, cropped by the rule below it. The outline is a
       * 2px stroke under a hatch-filled copy that hides its inner half, so
       * the glyphs' overlapping contours (the crossings in "t" and "x") never
       * show as stray lines.
       */}
      <div aria-hidden className={cn("pointer-events-none select-none", columnPadding)}>
        <div className="@container overflow-hidden">
          <div className="h-[0.6em] text-[26.9cqw] leading-none">
            <span className="grid whitespace-nowrap font-mono font-medium leading-[0.78] tracking-[-0.07em] text-transparent">
              <span className="[-webkit-text-stroke:2px_var(--line)] [grid-area:1/1]">Context</span>
              <span className="bg-hatch bg-clip-text [grid-area:1/1]">Context</span>
            </span>
          </div>
        </div>
      </div>

      <div className="relative border-t border-line">
        <Crosshair x="left" y="top" />
        <Crosshair x="right" y="top" />
        <div
          className={cn(
            "flex flex-col gap-2 py-6 sm:flex-row sm:items-center sm:justify-between md:py-7",
            columnPadding,
          )}
        >
          <p className="text-[13px] text-muted">&copy; 2026 Context contributors</p>
          <p className="font-mono text-[11.5px] tracking-[0.02em] text-muted">Built in Rust. Runs on your machine.</p>
        </div>
      </div>
    </footer>
  );
}
