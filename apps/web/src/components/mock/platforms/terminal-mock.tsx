import { GlassFrame } from "@/components/ui/photo";

/** A dark terminal waiting at `$ ctx` with a blinking block cursor. */
export function TerminalMock() {
  return (
    <div role="img" aria-label="A terminal with the ctx command typed at the prompt">
      <GlassFrame padding={7} radius={15} className="w-[272px]">
        <div aria-hidden className="overflow-hidden rounded-[8px] bg-[#0e100f] ring-1 ring-white/10">
          <div className="relative flex h-7 items-center border-b border-white/[0.08] px-3">
            <span className="flex gap-[6px]">
              <span className="size-[9px] rounded-full bg-white/[0.16]" />
              <span className="size-[9px] rounded-full bg-white/[0.16]" />
              <span className="size-[9px] rounded-full bg-white/[0.16]" />
            </span>
            <span className="absolute inset-x-0 text-center font-mono text-[10.5px] text-white/45">context-agent</span>
          </div>
          <div className="px-3.5 pb-4 pt-3 font-mono text-[12.5px] leading-[1.75]">
            <div className="text-white/40">~/code/context-agent</div>
            <div className="flex items-center text-white/90">
              <span className="text-[#7fd6a0]">$</span>
              <span className="ml-2">ctx</span>
              <span className="animate-blink ml-[3px] inline-block h-[15px] w-[8px] bg-white/85" />
            </div>
          </div>
        </div>
      </GlassFrame>
    </div>
  );
}
