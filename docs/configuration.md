# Configuration

Resolution order (later overrides earlier): built-in defaults → `~/.context-agent/config.toml` → `<repo>/.context-agent.toml` → `--config <file>` → environment (`CTX_MODEL_NAME`, `CTX_MODEL_BASE_URL`, `CTX_MODEL_API_KEY_ENV`, `CTX_MODEL_CONTEXT_WINDOW`, `CTX_MODEL_MAX_OUTPUT_TOKENS`, `CTX_MODEL_MAX_RETRIES`, `CTX_MODEL_FALLBACKS`, `CTX_MODEL_FALLBACK_COOLDOWN_SECS`, `CTX_PERMISSION_PROFILE`, `CTX_AGENT_MAX_STEPS`, `CTX_AGENT_MAX_MODEL_FAILURES`, `CTX_LOG_LEVEL`, `CTX_DATA_DIR`). `CTX_SHELL` picks the shell agent commands run in (default `sh`; on Windows Git for Windows' bash, else PowerShell; see [windows.md](windows.md)), and `CTX_ASCII=1|0` forces the terminal UI's fallback or Unicode glyphs.

The model picked with `/model` in the interactive session is remembered per workspace (in `<data_dir>/repositories/<id>/preferences.json`, never in the repository) and used by every `ctx` command run there, as long as `model.base_url` is unchanged. `--model` and `CTX_MODEL_NAME` override it without replacing it. The approval mode picked with `/approval` or Shift-Tab is remembered the same way (`--profile` and `CTX_PERMISSION_PROFILE` override it), except `bypass`, which is never remembered: the workspace reopens in the last other mode. The reasoning effort picked with `/effort` is remembered too (`--effort` overrides it); `/effort auto` remembers "let the model decide" and clears a configured level for that workspace.

A repository-level file may only set the `[index]`, `[context]`, `[validation]`, `[agent]` and `[embeddings]` tables; other tables (including `[web_search]`, `[web_fetch]` and `[mcp]`) in it are ignored and listed by `ctx doctor` (see docs/security.md).

`ctx init` writes a complete template. Sections:

- `[model]` — `name`, `base_url`, `api_key_env`, `transport = "chat_completions"`, `context_window`, `max_output_tokens`, `request_timeout_secs`, `max_retries`, `retry_base_delay_ms`, `retry_max_delay_ms`, `fallback_cooldown_secs`, `temperature`, `reasoning_effort` (one of the model's own effort values as models.dev lists them, e.g. `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`, sent as is, or `ultra`: the model's strongest value, plus sub-agents; sent automatically to models models.dev lists effort values for, to other models only with `reasoning_controls = true`, `ultra` then going out as `high`; a value the current model does not list goes out as its nearest listed one; an endpoint that rejects the field stops getting it), `extra_headers`, `[model.capabilities]` (`streaming`, `tool_calling`, `parallel_tool_calls`, `prompt_caching = "auto"|"enabled"|"disabled"`, `reasoning_controls`, `developer_role`) and `[[model.fallbacks]]` (see below). Configuration can disable a capability; a probe that proved a capability unsupported disables it regardless.
- `[permissions]` — `profile` (`approval_required` | `workspace_autonomous` | `bypass_permissions`), `allowed_commands` (prefixes that never prompt), `denied_commands`, `allow_network_commands`, `extra_workspace_roots`, and the developer-mode `[permissions.sandbox]` (`kind = "none"|"wrapper"`, `wrapper = ["docker", "run", ...]`), which wraps every shell command for tests and development without changing what is allowed.
- `[index]` — `exclude` globs, `max_file_bytes`, `watch`, `include_docs`, `index_hidden`, `credential_patterns` (never indexed, never readable by tools), `max_chunk_bytes`.
- `[embeddings]` — `backend = "hashing"|"none"|"onnx"`, `dimension`, `model_path`.
- `[context]` — budget ranges per task category (`localized_edit`, `multi_file_change`, `complex_feature`, `investigation`), `reserve_output_tokens`, `reserve_tool_tokens`, `compaction_threshold`, `max_tool_output_chars`, `keep_recent_turns`.
- `[agent]` — `max_steps` (0: no limit; a task runs until the model finishes, the user cancels, or the `stall_threshold` / `max_consecutive_tool_failures` guards stop it; `ctx run --max-steps N` and the `task/run` `max_steps` option set a cap for one run), `max_recovery_cycles`, `max_consecutive_tool_failures`, `command_timeout_secs`, `auto_validate` (false: when true, the runtime runs the project's tests itself if the model finishes with unvalidated edits), `stall_threshold`, `max_model_failures` (3; see "Retries and fallbacks"), and for the Ultra tier `max_parallel_agents` (4), `max_agents_per_call` (6) and `agent_max_steps` (0: no limit).
- `[validation]` — `commands` (explicit list wins), `auto_detect`, `timeout_secs`.
- `[telemetry]` — `retention_days`, `redact_secrets`, `log_level`, `store_full_tool_output`.
- `[web_search]` — `enabled` (true), `provider` (`auto` | `modelgateway` | `brave` | `tavily` | `duckduckgo`), `api_key_env`, `max_results` (8), `timeout_secs` (20). `auto` searches through the signed-in ModelGateway account (`POST /v1/web/search`, billed per search to that account, carrying `model.project_header` like model requests), else Brave when `BRAVE_SEARCH_API_KEY` is set, else Tavily when `TAVILY_API_KEY` is set, else DuckDuckGo's HTML results page, which needs no key but rate-limits automated queries. When the gateway does not offer search (an older deployment, or no search key on the server) `auto` carries on down that list. An explicit provider never falls back: `modelgateway` needs a sign-in, and `brave` or `tavily` read their key from `api_key_env` (default: the variable above). `enabled = false` removes the tool from the model's tool list. User configuration only.
- `[web_fetch]` — `enabled` (true), `timeout_secs` (30), `max_bytes` (5 MiB; longer responses are cut off). HTML comes back as text with headings, lists, code blocks and absolute links; long pages are paged by character `offset` within `context.max_tool_output_chars`. User configuration only.
- `[mcp]` — `enabled` (true), `startup_timeout_secs` (30), `call_timeout_secs` (120), the servers in `[mcp.servers.<name>]` and `uninstalled` (default apps the user removed); see "MCP servers and apps" below. User configuration only.
- `[pricing]` — `input_per_million`, `cached_input_per_million`, `output_per_million` (enables cost reporting).

## Retries and fallbacks

A model request goes through three layers before a task gives up on it:

1. **Retries on one model.** Rate limits (`429`, honouring `Retry-After`), timeouts, connection failures, interrupted streams and `5xx`/`408`/`409` answers are retried up to `model.max_retries` times (default 3) with exponential backoff and jitter, starting at `retry_base_delay_ms` (500) and never waiting longer than `retry_max_delay_ms` (20000) between attempts. Only the first attempt streams; a retry answers in one piece so the UI never shows the same text twice. Authentication errors, bad requests and context-length errors are not retried (the runtime compacts the conversation and resends on a context-length error).
2. **Fallback models.** When a model has spent its retries, or fails with anything other than a context-length error or a malformed tool call, the next entry of `model.fallbacks` is tried with the same request. A model that failed over is demoted for `fallback_cooldown_secs` (60): requests in that window go straight to the next healthy model instead of paying the retry budget again, and when the cooldown ends it is tried first again, so the chain returns to the configured order on its own. Every failover, every request served by a fallback and every recovery is reported as a warning in the session (and in `ctx run` output), and the model that actually answered is recorded per request. Fallbacks never apply to the capability probe or `/models`, which always address the primary model.
3. **Step retries.** When every model has failed on a transient condition (an outage, a rate limit that outlasted the retries), the runtime waits 5 s, then 10 s, then 20 s, and repeats the step, up to `agent.max_model_failures` times (3) per task, without charging the step budget. A rejected key or an unsupported request fails the task at once. Ctrl-C cancels a waiting retry.

Each `[[model.fallbacks]]` entry needs a `name`; `base_url`, `api_key_env`, `context_window`, `max_output_tokens`, `request_timeout_secs`, `max_retries`, `temperature`, `reasoning_effort`, `extra_headers` and `capabilities` are optional and inherit from `[model]`, so a bare `name` is another model on the same endpoint and a `base_url` plus `api_key_env` is a different provider altogether. An entry that repeats the primary model (for example after switching to it with `/model`) is skipped. Fallbacks have their own cached capability profiles when they were probed (`ctx doctor --probe` with that model as the primary); otherwise the configured capabilities apply.

```toml
[model]
name = "gpt-4.1"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
max_retries = 4
fallback_cooldown_secs = 120

[[model.fallbacks]]
name = "gpt-4.1-mini"

[[model.fallbacks]]
name = "claude-sonnet-5-5"
base_url = "https://api.anthropic.com/v1"
api_key_env = "ANTHROPIC_API_KEY"
context_window = 200000
```

`CTX_MODEL_FALLBACKS="gpt-4.1-mini,o4-mini"` sets same-endpoint fallbacks by name for one run and replaces the configured list (an empty value clears it). `ctx doctor` prints the chain and warns about a fallback whose key environment variable is not set.

## MCP servers and apps

Model Context Protocol servers add tools to the built-in set. The desktop app's **Apps** page and `ctx mcp catalog` offer a curated catalog of them (Computer Use, GitHub, Playwright, Context7, Fetch, Knowledge Graph, Sequential Thinking); installing one writes the `[mcp.servers.<id>]` entry below from its template plus the values you enter for its inputs (`ctx mcp install github --env GITHUB_PERSONAL_ACCESS_TOKEN=…`), with `app = "<id>"` recording where it came from. **Computer Use** is the one app every installation starts with: `Config::load` adds `[mcp.servers.computer-use]` (`transport = "builtin"`, `builtin = "computer-use"`) unless `mcp.uninstalled` lists it, which is what removing it (`ctx mcp uninstall computer-use`, the Apps page) writes. Built-in servers are served by the `ctx` binary itself (`ctx mcp-serve <name>`, run as a child over stdio). Each `[mcp.servers.<name>]` table describes one server; its tools reach the model as `mcp__<name>__<tool>` (characters outside `[A-Za-z0-9_-]` become `_`, the whole name is cut to 64 characters), with the server's own description prefixed by `[<name> MCP server]`. Server names use letters, digits, `_` and `-` (at most 32 characters).

```toml
[mcp]
enabled = true                 # false: no server is started, no MCP tools offered
startup_timeout_secs = 30      # start + initialize + tools/list
call_timeout_secs = 120        # one tool call

[mcp.servers.github]           # a stdio server: a child process
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}" }
# cwd = "/path"                # default: the repository (CLI) or the home folder (ctx serve)
# tools = ["search_issues"]    # offer only these tools (`*` wildcards allowed)
# exclude_tools = ["delete_*"] # never offer these, even when `tools` allows them
# bypass_permissions = true    # run every tool without asking (tools set to "ask" still ask)
# permissions = { create_issue = "allow", delete_file = "ask" }  # per-tool: "allow" never asks, "ask" always asks
# enabled = false              # keep the entry, start nothing

[mcp.servers.docs]             # a Streamable HTTP server
transport = "http"
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ${DOCS_TOKEN}" }
```

`${VAR}` anywhere in `command`, `args`, `env`, `cwd`, `url` or `headers` is replaced with that environment variable when the server starts (empty when unset), so keys stay out of the file. `${credential:NAME}` is replaced with the key stored under NAME in `<data_dir>/credentials.toml` (mode 0600); a server whose entry names a credential nobody signed in for fails with "not signed in" instead of starting with an empty token.

**Signing in instead of pasting a token.** Apps whose service supports it sign in through the browser. GitHub uses its OAuth device flow, the one `gh auth login` uses: `ctx mcp login github` (or Sign in on the app's connect page) shows a code, opens github.com, waits for approval, stores the token under the `github` credential with the account name, and installs the app with `GITHUB_PERSONAL_ACCESS_TOKEN = "${credential:github}"`. The token asks for the `repo`, `read:org` and `read:user` scopes. Uninstalling the app forgets the stored token (it stays valid on GitHub until revoked under Settings → Applications); `ctx mcp logout github` forgets it without uninstalling. The flow needs the client ID of Context's GitHub OAuth App with Device Flow enabled: `GITHUB_CLIENT_ID` in `crates/tools/src/mcp/oauth.rs`, or `CTX_GITHUB_CLIENT_ID` in the environment. Without one, the app falls back to the token field. Servers are connected once per process, in the background, when `ctx` or `ctx serve` starts; a run that begins while a server is still starting waits for it (bounded by `startup_timeout_secs`). A server that exits or fails to initialize is reported (`ctx mcp list`, `ctx doctor`, `/mcp`, the app's MCP settings) and the model simply does not get its tools. A server's `notifications/tools/list_changed` refreshes its tool list; `ctx serve` reconnects servers whose entry changes through `config/patch`. Stdio servers run with the user's `PATH` plus the usual Homebrew, Cargo, npm, Bun, Deno and Go directories, so `npx` and friends resolve from the desktop app too. Only the Streamable HTTP transport is supported over HTTP (the legacy HTTP+SSE pair of endpoints is not); sampling, prompts and resources are not used.

**`@` file mentions.** A message can also name repository files as `@path` (`@src/app.rs`, or `@"docs/My Notes.md"` with spaces), as in Claude Code: the same `@` list shows the project's files below the apps, fuzzy-matched (exact file names first, then names starting with what was typed, then paths containing it, then paths containing its characters in order) over an ignore-aware walk cached for ten seconds. When the message runs (or is sent to a running turn), each mentioned file is read through the same policy as `read_file` (nothing outside the workspace, nothing matching a credential pattern such as `.env`) and its current text follows the message as data, up to 10 files, 100 KB each and 300 KB in all; a truncated file says so. The files count as read for the stale-edit checks, so the model can edit them without reading them again. Mentioned paths that do not exist, are refused, are binary or did not fit are named in the turn's instruction. Trailing punctuation is ignored (`@src/a.rs,`), and the full-width `＠` that CJK input methods type works like `@`.

**`@` app mentions.** A message can name an installed app as `@<name>` (its `[mcp.servers.<name>]` key: `@github`, `@computer-use`), as in Claude Code and Codex. The interactive session and the desktop composer complete them: typing `@` lists the installed apps with their state; ↑/↓ choose, Tab or Return inserts `@name `, Esc hides the list. A mention counts at the start of the text or after whitespace or an opening bracket or quote (so `me@github.com` is not one) and matches a name case-insensitively. For the turn, the runtime adds a trusted instruction naming each mentioned app's enabled tools (up to twelve, then a count) and telling the model to prefer them where they fit; mentioned apps that are paused, still connecting, failed or have every tool switched off are listed as unavailable so the model tells the user instead of working around it. Mentions in messages sent to a running turn are added the same way. Mentions change no permissions: approvals follow the rules below.

Permissions follow the server's tool annotations: a tool marked `readOnlyHint` runs without asking under every profile; any other tool asks under `approval_required` and runs under `workspace_autonomous`; a tool marked `destructiveHint` asks under both, and only `bypass_permissions` runs it unprompted. The entry's own settings come first: `permissions.<tool> = "allow"` runs that tool without asking and `"ask"` asks every time (whatever its annotations), and `bypass_permissions = true` on the entry allows every tool without its own setting. The profile `bypass_permissions` still never asks. In the desktop app these are the app page's "Bypass permissions" switch and each tool's permission icon. Changing only these keys rebuilds the server's tools on its live connection instead of reconnecting it. Tool output is third-party content and is labelled as such for the model.

Computer Use (macOS only) gives the model `screenshot` (a PNG of a display scaled to points, which the runtime shows to the model in a follow-up message, since tool results are text only; the model therefore needs vision), `cursor_position`, `click`, `move_mouse`, `drag`, `scroll`, `type`, `key`, `wait` and `open`. Screenshots and waits are read-only and never ask; every other action asks under `approval_required`. macOS attributes its Screen Recording and Accessibility permissions to the responsible process: Context.app for the desktop app, the terminal for `ctx`; the server asks the system to prompt when one is missing and reports what to grant.

`ctx mcp add <name> -- <command> [args…]` and `ctx mcp add <name> --url <url>` write these tables (`--env KEY=VALUE`, `--header 'Name: value'`, `--cwd`, `--tool`, `--disabled`); `ctx mcp list` connects and reports every server, `ctx mcp tools [name]` lists the tools, `ctx mcp remove|enable|disable <name>` edit the entry.
