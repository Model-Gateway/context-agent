import { SquareTerminal } from "lucide-react";
import { Fragment, type ReactNode } from "react";
import { AppleIcon, WindowsIcon } from "@/components/ui/brand-icons";
import { InlineCode } from "./faq-item";

/*
 * The FAQ's questions and answers. Every claim here comes from the README
 * and docs/ (security.md, configuration.md, cli.md, windows.md) or the
 * macOS app's labels (Ask · Auto · Bypass); keep them in step.
 */

type Faq = { question: string; answer: ReactNode };

/** A row of a small two-column list inside an answer: label, then text. */
function Row({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <li className="grid gap-1 py-3 sm:grid-cols-[9.5rem_minmax(0,1fr)] sm:gap-4">
      <span className="text-[14px] text-ink">{label}</span>
      <span className="text-[14.5px] leading-[1.6]">{children}</span>
    </li>
  );
}

function Rows({ children }: { children: ReactNode }) {
  return <ul className="divide-y divide-line-soft border-y border-line-soft">{children}</ul>;
}

const exitCodes: ReadonlyArray<readonly [string, string]> = [
  ["0", "completed"],
  ["10", "blocked"],
  ["1", "failed"],
  ["130", "cancelled"],
  ["2", "configuration error"],
  ["3", "permission denied"],
  ["4", "provider error"],
  ["5", "timeout"],
];

export const faqs: ReadonlyArray<Faq> = [
  {
    question: "Does my code leave my machine?",
    answer: (
      <>
        <p>
          The index, history, memories and checkpoints live in <InlineCode>~/.context-agent</InlineCode> on your
          machine, shared by the app, the terminal UI and <InlineCode>ctx run</InlineCode>.
        </p>
        <p>
          The model endpoint you configure receives the prompts Context assembles for each request: the code the task
          needs, with credential files kept out and credential patterns redacted.{" "}
          <InlineCode>ctx context explain</InlineCode> shows exactly what the model saw and why, and with a local
          runtime those prompts never leave your machine. If the agent searches the web, only the query goes to a
          search provider, after credential redaction and, under the default Ask profile, your approval.
        </p>
      </>
    ),
  },
  {
    question: "Which models can I use?",
    answer: (
      <p>
        Any OpenAI-compatible endpoint: OpenAI, a gateway such as ModelGateway, a self-hosted server or a local
        runtime. <InlineCode>ctx doctor --probe</InlineCode> checks what the endpoint actually supports (tool calling,
        streaming, usage reporting and parallel calls), and Context only relies on what the probe verified.
      </p>
    ),
  },
  {
    question: "Is it safe to let it run commands?",
    answer: (
      <>
        <p>You decide how much it may do without asking. There are three permission profiles:</p>
        <Rows>
          <Row label="Ask (default)">Prompts for every edit and for any command that is not read-only.</Row>
          <Row label="Auto">
            Edits and build or test commands inside the workspace run freely; network, destructive and unrecognised
            commands still prompt.
          </Row>
          <Row label="Bypass">Never prompts. You pick it explicitly; cycling modes never lands on it.</Row>
        </Rows>
        <p>
          A conservative classifier looks through pipes, wrappers and command substitutions to decide what needs
          approval. Under every profile, denied commands are refused, and file tools follow symlinks and refuse
          credential files and paths outside the workspace. There is no OS-level sandbox, so pick the profile that
          matches how far you trust the task.
        </p>
      </>
    ),
  },
  {
    question: "Which platforms does it run on?",
    answer: (
      <Rows>
        <Row
          label={
            <span className="inline-flex items-center gap-2">
              <AppleIcon size={14} className="-mt-0.5" /> macOS
            </span>
          }
        >
          macOS 26 Tahoe or later on Apple silicon. A signed, notarized native app with <InlineCode>ctx</InlineCode>{" "}
          bundled.
        </Row>
        <Row
          label={
            <span className="inline-flex items-center gap-2">
              <WindowsIcon size={13} /> Windows
            </span>
          }
        >
          Windows 10 (1809 or later) or 11. The x64 installer bundles <InlineCode>ctx.exe</InlineCode> and runs
          natively, without WSL; Git for Windows is recommended.
        </Row>
        <Row
          label={
            <span className="inline-flex items-center gap-2">
              <SquareTerminal size={15} strokeWidth={1.6} /> Terminal
            </span>
          }
        >
          Both apps bundle <InlineCode>ctx</InlineCode>; to use the CLI and terminal UI on their own, build from
          source with stable Rust 1.85 or later.
        </Row>
      </Rows>
    ),
  },
  {
    question: "How does it stay up to date?",
    answer: (
      <p>
        The Mac app checks for a new release once a day, downloads it in the background and installs it,{" "}
        <InlineCode>ctx</InlineCode> included, the next time you quit. <span className="text-ink">Check for Updates…</span>{" "}
        in the Context menu checks right away, and{" "}
        <span className="text-ink">Settings → General → Updates</span> turns the automatic parts off.
      </p>
    ),
  },
  {
    question: "Can I run it in CI?",
    answer: (
      <>
        <p>
          Yes. <InlineCode>ctx run</InlineCode> works headless: JSON mode prints events on stderr and the outcome on
          stdout.
        </p>
        <pre className="whitespace-pre-wrap rounded-[10px] bg-ink px-4 py-3.5 font-mono text-[12px] leading-[1.65] text-white/90 sm:text-[12.5px]">
          <code>
            <span className="select-none text-white/45">$ </span>
            {/* Flags stay whole when the line wraps on narrow screens. */}
            {["ctx run", "--json", "--approval auto", "--timeout 600 \\"].map((part, i) => (
              <Fragment key={part}>
                {i > 0 && " "}
                <span className="whitespace-nowrap">{part}</span>
              </Fragment>
            ))}
            {"\n    "}
            <span className="text-[#ffd59e]">&quot;Fix the failing tests&quot;</span>
          </code>
        </pre>
        <p>The exit codes are stable:</p>
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[10px] bg-line-soft ring-1 ring-line-soft sm:grid-cols-4">
          {exitCodes.map(([code, meaning]) => (
            <div key={code} className="flex flex-col gap-1 bg-surface px-3.5 py-3">
              <dt className="font-mono text-[15px] leading-none tracking-[-0.02em] text-ink">{code}</dt>
              <dd className="text-[13px] leading-[1.35]">{meaning}</dd>
            </div>
          ))}
        </dl>
      </>
    ),
  },
  {
    question: "What does it cost?",
    answer: (
      <p>
        Nothing. Context is free and open source, licensed under Apache-2.0 or MIT at your option. You pay your model
        provider for what you use, or nothing at all with a local runtime. Signing in with ModelGateway from the app is
        optional and puts usage and billing in one place; after a first deposit, an account can claim a one-time $10
        Context credit.
      </p>
    ),
  },
];
