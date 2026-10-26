import { columnPadding, Crosshair } from "@/components/ui/frame";
import { cn } from "@/lib/cn";

/**
 * Examples of OpenAI-compatible endpoints: a hosted API, gateways and local
 * or self-hosted servers. They are not customers or partners.
 */
const endpoints = ["OpenAI", "ModelGateway", "OpenRouter", "Ollama", "vLLM", "LM Studio", "llama.cpp"] as const;

/**
 * The compact band under the showcase: kinds of endpoint Context can be
 * pointed at, set as plain wordmarks. One row beside its label on wide
 * screens; a wrapped block under it below that.
 */
export function ModelsStrip() {
  return (
    <section aria-labelledby="models-title" className={cn("relative bg-paper", columnPadding)}>
      <div className="flex flex-col xl:flex-row">
        <div className="relative pt-10 md:pt-12 xl:w-[13.5rem] xl:shrink-0 xl:border-r xl:border-line xl:py-12 xl:pr-8">
          <h2
            id="models-title"
            className="font-mono text-[11px] uppercase leading-[1.7] tracking-[0.08em] text-muted"
          >
            Point it at any <br className="hidden xl:inline" />
            OpenAI-compatible <br className="hidden xl:inline" />
            endpoint
          </h2>
          {/* Where the divider meets the bands above and below. */}
          <Crosshair x="right" y="top" className="hidden xl:block" />
          <Crosshair x="right" y="bottom" className="hidden xl:block" />
        </div>

        <ul className="flex flex-wrap items-center gap-x-8 gap-y-3 pb-10 pt-5 md:gap-x-10 md:pb-12 xl:flex-1 xl:flex-nowrap xl:justify-between xl:gap-x-0 xl:py-12 xl:pl-12">
          {endpoints.map((name) => (
            <li
              key={name}
              className="whitespace-nowrap text-[20px] font-medium leading-none tracking-[-0.03em] text-muted md:text-[22px] xl:text-[21px]"
            >
              {name}
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
