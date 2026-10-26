import { LogoMark } from "@/components/ui/logo";
import { GlassFrame } from "@/components/ui/photo";

/**
 * The macOS app icon as Finder shows it: the light squircle tile with the
 * black mark, held in a glass frame, with the selected file name below.
 */
export function MacAppMock() {
  return (
    <div role="img" aria-label="The Context app icon, selected in Finder as Context.app" className="flex flex-col items-center">
      <div aria-hidden className="flex flex-col items-center gap-3.5">
        <GlassFrame padding={10} radius={32}>
          <span
            className="relative grid size-[92px] place-items-center rounded-[22px] text-ink ring-1 ring-inset ring-black/[0.06]"
            style={{
              background: "linear-gradient(180deg, #ffffff 0%, #f4f4f6 55%, #e9e9ec 100%)",
              boxShadow:
                "inset 0 1px 0 rgba(255,255,255,0.9), 0 1px 2px rgba(11,11,12,0.12), 0 10px 22px -10px rgba(11,11,12,0.35)",
            }}
          >
            <LogoMark size={46} label="" />
          </span>
        </GlassFrame>
        <span className="rounded-[5px] bg-accent px-1.5 py-[2px] text-[12px] font-medium leading-[1.25] text-white shadow-[0_1px_2px_rgba(11,11,12,0.18)]">
          Context.app
        </span>
      </div>
    </div>
  );
}
