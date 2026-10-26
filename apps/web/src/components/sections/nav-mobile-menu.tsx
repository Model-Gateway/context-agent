"use client";

import { Menu, X } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { AppleIcon, GitHubIcon } from "@/components/ui/brand-icons";
import { Button } from "@/components/ui/button";
import { navLinks, site } from "@/lib/site";

/** The menu button and drop-down panel used below the `lg` breakpoint. */
export function MobileMenu() {
  const [open, setOpen] = useState(false);
  const panelId = useId();

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  // From md the button follows the header's own buttons; -ml-6 turns the
  // nav's 40px gap into their 16px one.
  return (
    <div className="ml-auto md:-ml-6 lg:hidden">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        aria-label={open ? "Close menu" : "Open menu"}
        onClick={() => setOpen((value) => !value)}
        className="grid size-10 place-items-center text-ink ring-1 ring-inset ring-line transition-colors hover:bg-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink"
      >
        {open ? <X size={18} strokeWidth={1.6} /> : <Menu size={18} strokeWidth={1.6} />}
      </button>

      <div
        id={panelId}
        hidden={!open}
        className="absolute inset-x-0 top-full border-b border-line bg-paper-2 shadow-[0_24px_40px_-24px_rgba(0,0,0,0.25)]"
      >
        <ul className="flex flex-col px-5 py-3 sm:px-8">
          {navLinks.map((link) => (
            <li key={link.label}>
              <a
                href={link.href}
                onClick={() => setOpen(false)}
                className="flex h-12 items-center border-b border-line-soft text-[17px] text-ink-2"
                {...(link.href.startsWith("http") ? { target: "_blank", rel: "noopener noreferrer" } : {})}
              >
                {link.label}
              </a>
            </li>
          ))}
        </ul>
        {/* From md the header bar already shows both buttons. */}
        <div className="flex flex-col gap-3 px-5 pb-5 sm:flex-row sm:px-8 md:hidden">
          <Button variant="solid" href={site.downloadMac} className="w-full sm:w-auto">
            <AppleIcon size={15} className="-mt-0.5" />
            Download for Mac
          </Button>
          <Button variant="bracket" href={site.repo} className="w-full sm:w-auto">
            <GitHubIcon size={15} />
            View on GitHub
          </Button>
        </div>
      </div>
    </div>
  );
}
