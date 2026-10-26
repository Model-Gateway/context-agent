import { Terminal } from "lucide-react";
import { cn } from "@/lib/cn";
import { mockSurface } from "./parts";

/**
 * A terminal running one headless `ctx run`, abridged from the CLI's own
 * output lines: the patch, the project's tests, the verdict, then the exit
 * code a CI script reads. Long lines run off the photo's right edge and the
 * bottom is cropped by the photo.
 */
export function HeadlessMock({ className }: { className?: string }) {
  return (
    <div
      role="img"
      aria-label="Terminal output of a headless ctx run with auto approval and a 600-second timeout: it patches crates/tools/src/files.rs, cargo test passes, the run ends COMPLETED and exits with code 0."
      className={cn(mockSurface, "min-h-[300px] min-w-[320px]", className)}
    >
      <div aria-hidden>
        <div className="flex h-10 items-center gap-2 border-b border-line-soft px-4 text-[12.5px] text-ink-2">
          <Terminal size={14} strokeWidth={1.7} className="text-faint" />
          ctx run
        </div>
        <div className="whitespace-pre px-4 pt-2.5 font-mono text-[12px] leading-[19px] text-muted">
          <p className="text-ink">
            <span className="text-faint">$ </span>ctx run --approval auto --timeout 600 &quot;Fix the failing tests&quot;
          </p>
          <p className="text-faint">&hellip;</p>
          <p>
            &rarr; <span className="text-ink-2">apply_patch</span> {'{"path":"crates/tools/src/files.rs","edits":[…]}'}
          </p>
          <p>
            {"  validation `cargo test`: "}
            <span className="text-green">PASSED</span>
          </p>
          <p className="text-ink">
            === <span className="text-green">COMPLETED</span> (14 steps, 15 model calls, 31 tool calls, 92.4s) ===
          </p>
          <p className="text-ink">
            <span className="text-faint">$ </span>echo $?
          </p>
          <p className="text-ink">0</p>
          <p className="text-ink">
            <span className="text-faint">$ </span>
            <span className="inline-block h-[14px] w-[7px] translate-y-[2px] bg-ink motion-safe:animate-blink" />
          </p>
        </div>
      </div>
    </div>
  );
}
