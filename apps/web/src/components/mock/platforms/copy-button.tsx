"use client";

import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/cn";

/** Copies through a hidden textarea where the async clipboard API is unavailable. */
function copyWithSelection(text: string): boolean {
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }
  area.remove();
  return copied;
}

/**
 * A small button that copies `text` to the clipboard, shows a check for a
 * moment, and announces the result to screen readers.
 */
export function CopyButton({
  text,
  label,
  className,
}: {
  text: string;
  /** Accessible name; should start with "Copy" to match the visible text. */
  label: string;
  className?: string;
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  async function copy() {
    let ok = false;
    try {
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch {
      ok = copyWithSelection(text);
    }
    setState(ok ? "copied" : "failed");
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), ok ? 1500 : 2500);
  }

  const copied = state === "copied";

  return (
    <>
      <button
        type="button"
        onClick={copy}
        aria-label={label}
        className={cn(
          "inline-flex h-8 items-center gap-1.5 rounded-[6px] px-2.5 font-mono text-[12px] text-muted ring-1 ring-inset ring-line-soft transition-colors hover:bg-surface-2 hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ink",
          copied && "text-ink",
          className,
        )}
      >
        {copied ? (
          <Check size={14} strokeWidth={1.8} className="text-green" aria-hidden />
        ) : (
          <Copy size={14} strokeWidth={1.6} aria-hidden />
        )}
        <span>{copied ? "Copied" : "Copy"}</span>
      </button>
      <span role="status" aria-live="polite" className="sr-only">
        {state === "copied" ? "Copied to clipboard" : state === "failed" ? "Could not copy; select the commands instead" : ""}
      </span>
    </>
  );
}
