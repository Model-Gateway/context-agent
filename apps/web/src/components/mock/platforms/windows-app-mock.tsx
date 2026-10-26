import { LogoMark } from "@/components/ui/logo";
import { GlassFrame } from "@/components/ui/photo";

/** Windows caption glyphs: 10px with 1px lines, as in the desktop app's WindowControls. */
function CaptionGlyph({ kind }: { kind: "minimize" | "maximize" | "close" }) {
  return (
    <svg viewBox="0 0 10 10" width={10} height={10} fill="none" stroke="currentColor" strokeWidth={1} aria-hidden>
      {kind === "minimize" && <path d="M0 5.5h10" />}
      {kind === "maximize" && <rect x="0.5" y="0.5" width="9" height="9" rx="1" />}
      {kind === "close" && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />}
    </svg>
  );
}

/**
 * A small Windows 11 window of the desktop app: icon and title at the left
 * of the title bar, minimise, maximise and close at the right, and one
 * finished task in the body.
 */
export function WindowsAppMock() {
  return (
    <div role="img" aria-label="The Context desktop app in a Windows window, showing a completed task">
      <GlassFrame padding={7} radius={15} className="w-[272px]">
        <div aria-hidden className="overflow-hidden rounded-[8px] bg-surface ring-1 ring-black/5">
          <div className="flex h-8 items-center border-b border-line-soft bg-surface-2 pl-3">
            <LogoMark size={12} label="" className="text-ink" />
            <span className="ml-2 text-[11.5px] text-ink-2">Context</span>
            <span className="ml-auto flex h-full text-ink-2">
              <span className="grid h-full w-9 place-items-center">
                <CaptionGlyph kind="minimize" />
              </span>
              <span className="grid h-full w-9 place-items-center">
                <CaptionGlyph kind="maximize" />
              </span>
              <span className="grid h-full w-9 place-items-center">
                <CaptionGlyph kind="close" />
              </span>
            </span>
          </div>
          <div className="space-y-2.5 px-3 py-3">
            <div className="ml-auto w-fit rounded-[8px] bg-surface-2 px-2.5 py-1.5 text-[11.5px] leading-[1.35] text-ink-2 ring-1 ring-inset ring-black/[0.04]">
              Fix the failing tests
            </div>
            <div className="flex items-center gap-2 text-[11px] leading-none">
              <span className="inline-flex items-center gap-1 rounded-full bg-green-soft px-1.5 py-[3px] font-medium text-green">
                <span className="size-[5px] rounded-full bg-green" />
                Completed
              </span>
              <span className="truncate font-mono text-[10.5px] text-muted">crates/agent · 2 files</span>
            </div>
          </div>
        </div>
      </GlassFrame>
    </div>
  );
}
