import { CircleCheck, CircleX, Hand } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";
import { MockLabel, Segmented, mockSurface } from "./parts";

/** A program name in the classifier's reason. */
function Program({ children }: { children: ReactNode }) {
  return <span className="font-mono text-[11px] text-ink-2">{children}</span>;
}

/**
 * The app's approval card under the Ask profile: the command, its risk, the
 * classifier's reason (it looked through the `timeout` wrapper to `cargo`)
 * and the app's three answers, with earlier answers below. Labels follow
 * the macOS app; the bottom is cropped by the photo.
 */
export function ApprovalMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="An approval card under the Ask profile: run timeout 600 cargo test, an execute-risk command that the classifier read through the timeout wrapper, with Deny, Approve for session and Approve."
      className={cn(mockSurface, className)}
    >
      <div aria-hidden>
        <div className="flex h-12 items-center justify-between gap-3 border-b border-line-soft pl-4 pr-2.5">
          <MockLabel tone="muted">Approval</MockLabel>
          <Segmented items={["Ask", "Auto", "Bypass"]} active="Ask" />
        </div>

        <div className="px-4 pb-3.5 pt-3.5">
          <div className="flex items-center gap-2">
            <Hand size={14} strokeWidth={1.7} className="shrink-0 text-amber" />
            <span className="text-[13.5px] font-medium tracking-[-0.01em] text-ink">Approval needed</span>
            <span className="ml-auto rounded-[6px] bg-amber-soft px-1.5 py-[4px] text-[11px] leading-none text-amber ring-1 ring-inset ring-amber/15">
              execute
            </span>
          </div>

          <div className="mt-2.5 rounded-[7px] bg-surface-2 px-3 py-2 font-mono text-[12px] leading-[18px] text-ink ring-1 ring-inset ring-black/[0.05]">
            <span className="text-faint">$ </span>timeout 600 cargo test
          </div>

          <p className="mt-2 text-[11.5px] leading-[1.4] text-muted">
            <Program>timeout</Program> wraps <Program>cargo</Program>
          </p>

          <div className="mt-3 flex items-center justify-end gap-1.5 whitespace-nowrap text-[11.5px] leading-none">
            <span className="pr-1.5 text-muted">Deny</span>
            <span className="rounded-[7px] bg-surface px-2.5 py-[7px] text-ink-2 ring-1 ring-inset ring-line">
              Approve for session
            </span>
            <span className="rounded-[7px] bg-accent px-2.5 py-[7px] font-medium text-white shadow-[0_1px_2px_rgba(47,107,255,0.3)]">
              Approve
            </span>
          </div>
        </div>

        {/* Earlier answers in the same session, cropped on most screens. */}
        <ul className="space-y-2 border-t border-line-soft px-4 pb-4 pt-3 text-[11.5px] leading-none text-muted">
          <li className="flex items-center gap-2">
            <CircleCheck size={12} strokeWidth={1.9} className="shrink-0 text-green" />
            Approved &middot; <span className="font-mono text-[11px] text-ink-2">run: cargo fmt --check</span>
          </li>
          <li className="flex items-center gap-2">
            <CircleX size={12} strokeWidth={1.9} className="shrink-0 text-faint" />
            Denied &middot; <span className="font-mono text-[11px] text-ink-2">run: rm -rf target</span>
          </li>
        </ul>
      </div>
    </div>
  );
}
