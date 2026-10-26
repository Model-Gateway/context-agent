import { Check, FileCode2, FileSearch, type LucideIcon } from "lucide-react";
import { GlassFrame } from "@/components/ui/photo";
import { IconTile, OutlineChip, PointerCursor } from "./parts";

const results: {
  file: string;
  source: string;
  detail: string;
  icon: LucideIcon;
  tone: "accent" | "violet";
}[] = [
  { file: "files.rs", source: "symbol", detail: "ApplyPatch", icon: FileCode2, tone: "accent" },
  { file: "execution.rs", source: "lexical", detail: "“stale patch”", icon: FileSearch, tone: "violet" },
];

/**
 * Two files retrieval picked for a task, named with the strategy that found
 * each one and marked as checked against the working tree, with the user's
 * cursor over the second.
 */
export function RetrievedContextMock() {
  return (
    <div
      role="img"
      aria-label="Retrieved context, 2 of 40: crates/tools/src/files.rs, found by the symbol strategy through ApplyPatch, and crates/tools/tests/execution.rs, found by lexical search for stale patch. Both are marked verified against the working tree, and a cursor labelled You points at the second."
      className="relative w-[296px]"
    >
      <div aria-hidden>
        <GlassFrame padding={7} radius={17}>
          <div className="rounded-[10px] bg-surface px-3 shadow-[0_1px_2px_rgba(11,11,12,0.05)]">
            <div className="flex h-[42px] items-center justify-between border-b border-line-soft">
              <span className="text-[13px] font-medium tracking-[-0.01em] text-ink">Retrieved context</span>
              <span className="font-mono text-[10.5px] tracking-[0.02em] text-faint">2 of 40</span>
            </div>
            <ul className="divide-y divide-line-soft">
              {results.map(({ file, source, detail, icon: Icon, tone }) => (
                <li key={file} className="flex items-center gap-2.5 py-[11px]">
                  <IconTile tone={tone}>
                    <Icon size={15} strokeWidth={1.6} />
                  </IconTile>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px] font-medium leading-[1.25] tracking-[-0.005em] text-ink">
                      {file}
                    </span>
                    <span className="mt-[3px] block truncate text-[11px] leading-[1.25] text-muted">
                      {source} &middot; {detail}
                    </span>
                  </span>
                  <OutlineChip className="h-[26px] gap-[5px] px-2 text-[11px]">
                    <Check size={12} strokeWidth={2} className="text-green" />
                    Verified
                  </OutlineChip>
                </li>
              ))}
            </ul>
          </div>
        </GlassFrame>
        <PointerCursor label="You" className="left-[222px] top-[calc(100%-20px)]" />
      </div>
    </div>
  );
}
