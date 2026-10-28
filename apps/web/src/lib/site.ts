/** Facts and links the landing page repeats; one place to update per release. */
export const site = {
  name: "Context",
  version: "0.1.4",
  tagline: "A local-first coding agent that finishes the task and shows its work.",
  description:
    "Context is a local-first coding agent written in Rust, with a native macOS app, a Windows app and a terminal UI. Bring any OpenAI-compatible model; your code, history and index stay on your machine.",
  repo: "https://github.com/Model-Gateway/context-agent",
  releases: "https://github.com/Model-Gateway/context-agent/releases",
  downloadMac: "https://github.com/Model-Gateway/context-agent/releases/latest",
  downloadWindows: "https://github.com/Model-Gateway/context-agent/releases/latest",
  docs: "https://github.com/Model-Gateway/context-agent/tree/main/docs",
  docsConfiguration: "https://github.com/Model-Gateway/context-agent/blob/main/docs/configuration.md",
  docsSecurity: "https://github.com/Model-Gateway/context-agent/blob/main/docs/security.md",
  docsCli: "https://github.com/Model-Gateway/context-agent/blob/main/docs/cli.md",
  docsArchitecture: "https://github.com/Model-Gateway/context-agent/blob/main/docs/architecture.md",
  docsWindows: "https://github.com/Model-Gateway/context-agent/blob/main/docs/windows.md",
  docsEvaluation: "https://github.com/Model-Gateway/context-agent/blob/main/docs/evaluation.md",
  issues: "https://github.com/Model-Gateway/context-agent/issues",
  license: "https://github.com/Model-Gateway/context-agent#license",
  /** The optional hosted gateway the app can sign in to for usage and billing. */
  modelGateway: "https://www.modelgateway.co",
} as const;

/** In-page anchors for the nav and footer. */
export const navLinks = [
  { label: "Features", href: "#features" },
  { label: "How it works", href: "#how-it-works" },
  { label: "Pricing", href: "#pricing" },
  { label: "Docs", href: site.docs },
] as const;
