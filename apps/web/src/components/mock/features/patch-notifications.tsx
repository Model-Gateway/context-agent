import { FileCheck2, FileWarning } from "lucide-react";
import type { ReactNode } from "react";
import { GlassFrame } from "@/components/ui/photo";
import { IconTile, OutlineChip, TimeChip } from "./parts";

/** One "Label → value" line in the applied-patch card. */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex h-[26px] items-center gap-2">
      <span className="text-[12px] text-muted">
        {label} <span className="text-faint">&rarr;</span>
      </span>
      {children}
    </div>
  );
}

/**
 * Two notifications from the activity log: a patch refused because its file
 * changed on disk, then the re-read patch applied, hashed and tested.
 */
export function PatchNotificationsMock() {
  return (
    <div
      role="img"
      aria-label="Two notifications. 2:21 PM: patch refused, the file changed since it was last read. 2:23 PM: patch applied to src/auth/session.ts, new hash sha256:9f2c1e, npm test passed."
      className="w-[290px]"
    >
      <div aria-hidden className="flex flex-col gap-[14px]">
        <GlassFrame padding={6} radius={15}>
          <div className="flex items-center gap-2.5 rounded-[9px] bg-surface py-[9px] pl-3 pr-2.5 shadow-[0_1px_2px_rgba(11,11,12,0.05)]">
            <IconTile tone="amber" size={26}>
              <FileWarning size={14} strokeWidth={1.6} />
            </IconTile>
            <span className="min-w-0 flex-1">
              <span className="block text-[13px] font-medium leading-[1.25] tracking-[-0.01em] text-ink">
                Patch refused
              </span>
              <span className="mt-[2px] block text-[11px] leading-[1.25] text-muted">Changed since last read</span>
            </span>
            <TimeChip>2:21 PM</TimeChip>
          </div>
        </GlassFrame>

        <GlassFrame padding={6} radius={15}>
          <div className="rounded-[9px] bg-surface pb-3 pl-3 pr-2.5 shadow-[0_1px_2px_rgba(11,11,12,0.05)]">
            <div className="flex h-[46px] items-center gap-2.5 border-b border-line-soft">
              <IconTile tone="green" size={26}>
                <FileCheck2 size={14} strokeWidth={1.6} />
              </IconTile>
              <span className="flex-1 text-[13px] font-medium tracking-[-0.01em] text-ink">Patch applied</span>
              <TimeChip>2:23 PM</TimeChip>
            </div>
            <div className="mt-2.5 flex flex-col gap-[5px]">
              <Field label="File">
                <OutlineChip className="h-[24px] px-2 font-mono text-[11px]">src/auth/session.ts</OutlineChip>
              </Field>
              <Field label="Hash">
                <OutlineChip className="h-[24px] px-2 font-mono text-[11px]">sha256:9f2c1e&hellip;</OutlineChip>
              </Field>
              <Field label="Tests">
                <span className="inline-flex items-center gap-1.5 text-[12px] text-muted">
                  <span className="font-mono text-[11px] text-ink-2">npm test</span>
                  <span className="text-faint">&middot;</span>
                  <span className="inline-flex items-center gap-1 text-green">
                    <span className="size-[6px] rounded-full bg-green" />
                    passed
                  </span>
                </span>
              </Field>
            </div>
          </div>
        </GlassFrame>
      </div>
    </div>
  );
}
