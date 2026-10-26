import { CircleAlert, CircleCheck, FilePen } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

function Path({ children }: { children: ReactNode }) {
  return <span className="rounded-[4px] bg-surface-2 px-1 py-px font-mono text-[11px] text-ink-2">{children}</span>;
}

function Line({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <li className="grid grid-cols-[14px_minmax(0,1fr)] items-start gap-x-2.5 gap-y-1 py-2 @min-[300px]:grid-cols-[14px_5.6rem_minmax(0,1fr)]">
      <span className="mt-px">{icon}</span>
      <span className="text-[12.5px] font-medium text-ink">{label}</span>
      {/* Below the label in a narrow card, beside it otherwise. */}
      <span className="col-start-2 text-pretty text-[12.5px] leading-[1.5] text-muted @min-[300px]:col-start-3">
        {children}
      </span>
    </li>
  );
}

/**
 * The end of a finished run as the app reports it: what changed, what was
 * verified, what was not, and the green "Completed" status. Illustrative.
 */
export function ReportMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="A finished run's report: two files changed, npm test passed, and one thing not verified, how the spinner looks in Safari. Status: Completed."
      className={cn("select-none", className)}
    >
      <div aria-hidden className="@container rounded-[12px] bg-surface px-4 pb-3.5 pt-4 ring-1 ring-black/[0.06]">
        <p className="text-pretty text-[13px] leading-[1.55] text-ink">
          Added a spinner to <Path>LoginButton</Path> and disabled it while sign-in is pending.
        </p>

        <ul className="mt-2.5 divide-y divide-line-soft border-y border-line-soft">
          <Line icon={<FilePen size={14} strokeWidth={1.6} className="text-muted" />} label="Changed">
            <Path>LoginButton.tsx</Path> <Path>LoginButton.test.tsx</Path>
          </Line>
          <Line icon={<CircleCheck size={14} strokeWidth={1.7} className="text-green" />} label="Verified">
            <Path>npm test</Path> passed, 14 tests
          </Line>
          <Line icon={<CircleAlert size={14} strokeWidth={1.7} className="text-amber" />} label="Not verified">
            How the spinner looks in Safari; no browser was run.
          </Line>
        </ul>

        <div className="mt-3 flex items-center gap-2.5">
          <span className="inline-flex items-center gap-1.5 rounded-full bg-green-soft px-2 py-[3px] text-[11.5px] font-medium text-green">
            <span className="size-1.5 rounded-full bg-green" />
            Completed
          </span>
          <span className="text-[11.5px] text-muted">38.4k tokens &middot; 1m 52s</span>
        </div>
      </div>
    </div>
  );
}
