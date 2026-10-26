import Link from "next/link";
import { AppleIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { Crosshair, columnPadding } from "@/components/ui/frame";
import { Wordmark } from "@/components/ui/logo";
import { navLinks, site } from "@/lib/site";
import { cn } from "@/lib/cn";
import { MobileMenu } from "./nav-mobile-menu";

/** The top bar: wordmark, section links, and the two calls to action. */
export function Nav() {
  return (
    <header className="sticky top-0 z-50 border-b border-line bg-paper-2/85 backdrop-blur-md supports-[backdrop-filter]:bg-paper-2/75">
      <nav aria-label="Main" className={cn("flex h-16 items-center gap-10 md:h-[76px]", columnPadding)}>
        <Link href="/" aria-label="Context home" className="rounded-[4px] focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ink">
          <Wordmark />
        </Link>

        <ul className="hidden items-center gap-7 lg:flex">
          {navLinks.map((link) => (
            <li key={link.label}>
              <a
                href={link.href}
                className="text-[16px] text-muted transition-colors hover:text-ink"
                {...(link.href.startsWith("http") ? { target: "_blank", rel: "noopener noreferrer" } : {})}
              >
                {link.label}
              </a>
            </li>
          ))}
        </ul>

        <div className="ml-auto hidden items-center gap-4 md:flex">
          <Button variant="bracket" href={site.repo}>
            View on GitHub
          </Button>
          <Button variant="solid" href={site.downloadMac}>
            <AppleIcon size={15} className="-mt-0.5" />
            Download
          </Button>
        </div>

        <MobileMenu />
      </nav>
      <Crosshair x="left" y="bottom" />
      <Crosshair x="right" y="bottom" />
    </header>
  );
}
