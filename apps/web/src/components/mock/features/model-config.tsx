import { GlassFrame } from "@/components/ui/photo";
import { TextCursor } from "./parts";

type TokenKind = "comment" | "section" | "key" | "punct" | "string" | "number";

const tokenClass: Record<TokenKind, string> = {
  comment: "text-faint",
  section: "text-violet",
  key: "text-[#2d5fd8]",
  punct: "text-faint",
  string: "text-green",
  number: "text-[#d9731a]",
};

type Token = [kind: TokenKind, text: string];

/** `key = value` as tokens. */
const pair = (key: string, value: Token): Token[] => [["key", key], ["punct", " = "], value];

/** The snippet, line by line; `null` marks the line drawn with the tooltip. */
const lines: (Token[] | null)[] = [
  [["section", "[model]"]],
  null,
  pair("base_url", ["string", '"http://localhost:4000/v1"']),
  pair("api_key_env", ["string", '"GATEWAY_API_KEY"']),
  [...pair("context_window", ["number", "128000"]), ["comment", "  # tokens"]],
  [],
  [["section", "[permissions]"]],
  pair("profile", ["string", '"approval_required"']),
];

/**
 * A code window showing the model section of `config.toml`, pointed at a
 * local gateway, with the model name selected and a dark tooltip suggesting
 * another model the same gateway serves.
 */
export function ModelConfigMock() {
  return (
    <div
      role="img"
      aria-label="A code window with ~/.context-agent/config.toml: model gpt-4.1 through a gateway at http://localhost:4000/v1 with the key from GATEWAY_API_KEY and a 128,000-token context window, and the approval_required permission profile. The model name is selected and a tooltip offers claude-sonnet-5-5 in its place."
      className="w-[296px]"
    >
      <div aria-hidden>
        <GlassFrame padding={8} radius={18}>
          <div className="rounded-[10px] bg-surface px-4 pb-4 shadow-[0_1px_2px_rgba(11,11,12,0.05)]">
            <div className="flex h-[32px] items-center gap-[5px] border-b border-line-soft">
              <span className="size-[7px] rounded-full bg-[#ff5f57]" />
              <span className="size-[7px] rounded-full bg-[#febc2e]" />
              <span className="size-[7px] rounded-full bg-[#28c840]" />
              <span className="ml-auto font-mono text-[10.5px] tracking-[0.01em] text-faint">~/.context-agent/config.toml</span>
            </div>
            <div className="mt-3.5 whitespace-pre font-mono text-[11px] leading-[20px] text-ink-2">
              {lines.map((line, i) =>
                line === null ? (
                  <div key={i}>
                    <span className={tokenClass.key}>name</span>
                    <span className={tokenClass.punct}> = </span>
                    <span className="relative">
                      <span className="rounded-[3px] bg-green-soft py-[2px] text-green shadow-[0_0_0_1.5px_var(--green-soft)]">
                        &quot;gpt-4.1&quot;
                      </span>
                      <TextCursor className="ml-[3px] -mb-[3px] align-baseline" />
                      <span className="absolute bottom-[calc(100%+4px)] left-[26px] rounded-[8px] bg-ink px-2.5 py-[6px] text-[11.5px] leading-none text-[#a8ec9a] shadow-[0_6px_16px_-4px_rgba(11,11,12,0.45)] ring-1 ring-white/10">
                        &quot;claude-sonnet-5-5&quot;
                      </span>
                    </span>
                  </div>
                ) : (
                  <div key={i}>
                    {line.length === 0
                      ? " "
                      : line.map(([kind, text], j) => (
                          <span key={j} className={tokenClass[kind]}>
                            {text}
                          </span>
                        ))}
                  </div>
                ),
              )}
            </div>
          </div>
        </GlassFrame>
      </div>
    </div>
  );
}
