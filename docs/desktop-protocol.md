# Desktop protocol: `ctx serve`

Status: implemented in `crates/cli/src/serve/` (`mod.rs` request loop and
handlers, `runner.rs` per-run execution, `dto.rs` wire shapes, `support.rs`
helpers, `browser.rs` the desktop-only `browser` tool; registered in
`crates/cli/src/main.rs`); the server uses only public crate APIs and leaves
`crates/core`, `crates/agent`, `crates/storage` and `crates/tools` unchanged
(`crates/security` knows the `Browser` action kind the tool is judged by).

Configuration layers: a project's configuration is loaded exactly as `ctx run`
loads it — user file, then the repository's `.context-agent.toml` (only the
tables the core crate trusts from a repository: `index`, `context`,
`validation`, `agent`, `embeddings`; others are reported by `doctor/run`),
then an explicit `--config` file, environment overrides and CLI overrides.
The user-level configuration that `initialize`, `config/get` and
`config/patch` expose never includes the repository layer.

The macOS app (`apps/macos`) drives the same Rust backend as the TUI and
`ctx run`. It launches `ctx serve` as a child process and talks JSON-RPC 2.0
over the child's stdin/stdout, one JSON object per line (newline-delimited,
UTF-8, no embedded newlines). Logs go to the log file and stderr only;
**nothing but protocol frames may be written to stdout.**

```
ctx serve [--mock] [--data-dir DIR] [--config FILE] [-v]
```

`--mock` replaces the model provider with a scripted `MockProvider` (no API
key, no network, no capability-profile files written) built fresh for every
`task/run`: turn 1 streams "I'll take a quick look at the repository before
answering." and calls `list_files {"pattern":"*.md","limit":20}`; turn 2
calls `read_file {"path":"README.md","start_line":1,"end_line":40}`; turn 3
answers with text; the fallback text is "Nothing further to do; no files
were changed." The mock never edits files and emits no reasoning deltas.
`models/switch` in mock mode only records the name. It exists so the UI can
be exercised offline. `CTX_MOCK_PACE_MS=<ms>` in the server's environment
spaces the mock's streamed deltas (unset or 0 = instant); the macOS app sets
20 for its mock child so streaming is visible.

## Framing

Request: `{"jsonrpc":"2.0","id":<int|string>,"method":"<name>","params":{...}}`
Response: `{"jsonrpc":"2.0","id":<same>,"result":{...}}` or
`{"jsonrpc":"2.0","id":<same>,"error":{"code":<int>,"message":"...","data":{"kind":"<ctx error code>"}}}`
Notification (server → client, no id): `{"jsonrpc":"2.0","method":"<name>","params":{...}}`

`params`, when present, must be a JSON object. Requests are handled
concurrently (each in its own tokio task): a long `models/switch` never
blocks a `task/cancel`. Responses may therefore arrive out of order; the
client matches on `id`. All frames go through one writer task (one `mpsc`
channel → `tokio::io::stdout`, flushed per frame) so lines never interleave.
Frames are never dropped; when the client stops reading and more than 16 384
frames are queued, event forwarding pauses until the queue drains (which in
turn pauses the run through the runtime's bounded event sink), and
`index/progress` frames are skipped while the queue is congested. Blocking work (`ensure_indexed`, `full_index`,
`reconcile`, `git_info`, `profile()`, SQLite-heavy history assembly) runs in
`spawn_blocking`; `AgentRuntime::run` runs in `tokio::spawn` on the
multi-thread runtime.

Error codes: `-32700` parse error, `-32600` invalid request, `-32601` unknown
method, `-32602` invalid params, `-32000` backend error (`data.kind` carries
`CtxError::code()`: `config`, `provider_auth`, `not_found`, …), `-32001`
project not open, `-32002` task already running, `-32003` no task running.
A frame without an `id` is a notification and gets no response; an explicit
`"id": null` is answered with `"id": null`.

All timestamps are RFC 3339 strings. All ids are strings. Project ids are the
repository id (`RepositoryId`, 24 hex chars) of the opened root.

## Methods

### `initialize` → `InitializeResult`
```json
{"protocol_version":1,"ctx_version":"0.1.0","data_dir":"/Users/me/.context-agent",
 "config_file":"/Users/me/.context-agent/config.toml","config_sources":["…"],
 "mock":false,
 "model":{"name":"gpt-4.1","base_url":"https://api.openai.com/v1","api_key_env":"OPENAI_API_KEY","api_key_set":true,
          "context_window":128000,"max_output_tokens":16000,"reasoning_effort":null | "<the model's effort value>" | "ultra"},
 "permissions":{"profile":"approval_required","allowed_commands":[],"denied_commands":[],"allow_network_commands":false},
 "pricing":{"input_per_million":2.0 | null,"cached_input_per_million":0.5 | null,"output_per_million":8.0 | null,
            "models":{"gpt-4.1":{"input":2.0,"output":8.0,"cache_read":0.5}}} | null,
 "spend":{"period":"day" | "week" | "month","max_usd":50.0,"models":{"gpt-4.1":10.0}},
 "host":{"hostname":"devbox","user":"alice","home_dir":"/home/alice","os":"linux"}}
```
Must be the first call. `model`, `permissions`, `pricing`, `spend` describe the
user-level configuration (before any per-project override). `host` is the
machine the server runs on (`gethostname`, `$USER`, the home directory,
`std::env::consts::OS`): the desktop app shows it for a backend reached over
SSH and starts its remote folder picker in `home_dir` (see **Remote hosts**).

`pricing` holds the user's own prices (USD per million tokens): `models` prices one
model each (`config/patch` replaces the whole map with `"pricing.models": {…}`);
the flat prices are the fallback for models with no other price. A request is
priced by the model that answered it: its `pricing.models` entry, else models.dev's
catalogue price (resolved against `model.base_url`), else the flat prices; with no
price the request has no cost and `cost_usd` stays absent. Requests sent with the
ModelGateway credential are then re-priced with what the gateway billed (looked up
by the `X-Oneapi-Request-Id` of each reply when the run finishes, see
**ModelGateway**), so the outcome's `cost_usd` is the billed amount.

`spend` caps spending per period (local day, week from Monday, calendar month):
`max_usd` across every model, `models` per model id. Before each model request the
run compares what all repositories spent this period (`<data_dir>/spend.sqlite`)
with the caps on the project's model and on all models; at or over a cap the task
ends `blocked` with a summary that names the cap. Patches to `pricing.` and
`spend.` reach every open project at once.

`spend.confirm_discount_drop` (default `true`) guards against ModelGateway
falling back to a much less discounted provider. While it is on, each run
request signed in to ModelGateway (`model.credential = "modelgateway"`) carries
`X-ModelGateway-Max-Discount-Drop: 10`;
the gateway then refuses (HTTP 412, code `discount_drop`, with
`X-ModelGateway-Expected-Discount` / `X-ModelGateway-Available-Discount`)
instead of routing to a provider discounted 10 or more percentage points below
the model's best offer (the `savings_percent` of `models/list`). The run turns
that into an `approval_requested` with `tool: "discount_drop"` (summary
`Continue gpt-5 at 0% off instead of 98% off?`): `deny` ends the task
`cancelled`, `approve` repeats the step and sends the rest of the run's requests
without the header. The app answers *Always continue* with
`config/patch {"spend.confirm_discount_drop": false}` followed by
`approve_for_session`. Commit-message and suggestion requests never carry the
header. Headless runs (`--approval auto`) continue without asking.

`mcp` is the user-level `[mcp]` table as loaded (`enabled`, `startup_timeout_secs`,
`call_timeout_secs`, `uninstalled`, `servers` keyed by name with `enabled`,
`transport: "stdio"|"http"|"builtin"`, `command`, `args`, `env`, `cwd`, `url`, `headers`,
`tools`, `builtin`, `app`; empty lists and maps are omitted). `Config::load` adds the
default app `computer-use` (a `builtin` server) unless `uninstalled` names it. The
servers' live state is **MCP servers and apps** below.

### `config/get` → same shape as `initialize` (re-read from disk).

### `config/patch` `{ "values": { "model.name": "gpt-4.1-mini", "model.base_url": "…", "model.api_key_env": "…", "permissions.profile": "workspace_autonomous", "pricing.input_per_million": 2.0, "model.reasoning_effort": "<effort value, e.g. low, xhigh, max>" | "ultra" | null } }` → `config/get` result.
An effort value is one the model accepts (models.dev `reasoning_options`, which the app lists in its effort picker) and goes to the endpoint as is. `model.reasoning_effort = "ultra"` is the Ultra tier: the endpoint is sent the model's strongest value (`high` when models.dev does not list the model) and every run started afterwards (`task/run` reads the project's live configuration) hands the model the `spawn_agents` tool; see **Sub-agents** below.
`model.credential` (a name in `<data_dir>/credentials.toml`, or `null`) makes the model authenticate with that stored key instead of the variable named by `model.api_key_env` (`api_key_set` reflects whichever source resolves); `gateway/device/poll` stores the ModelGateway device key under `"modelgateway"`, see **ModelGateway**.
`model.project_header` (a header name, or `null`) makes every model request of a project carry that header with the project's name (its root folder, reduced to printable ASCII and 64 bytes); the provider is built per project (`project/open`, `models/switch`, idle rebuilds after a `model.*` patch), so the value is always the right repository's. The desktop app sets it to `X-ModelGateway-Project` while signed in to ModelGateway with per-project usage on, see **ModelGateway** below.
Dotted keys are merged into `~/.context-agent/config.toml` (tables created as
needed, `null` removes the key) with `ctx_core::config::merge_tables`; the
file is rewritten with `toml::to_string_pretty` (comments are dropped) and
validated by reloading (`Config::load`) before anything is written — an
invalid patch is `-32602` and leaves the file untouched; the result reflects
the reloaded configuration. Only these prefixes are accepted: `model.`,
`permissions.`, `pricing.`, `spend.`, `agent.`, `context.`, `validation.`,
`telemetry.`, `embeddings.`, `index.`, and every key must exist in the
configuration shape (`model.nmae` or `index.foo.bar` are `-32602`; the
children of free-form maps such as `model.extra_headers.*` are not checked).
The file is written atomically (temp file + rename) with mode 0600, patches
are serialized, and the work runs in `spawn_blocking`. Open projects keep
their current in-memory configuration until reopened, except `model.*`
changes, which are applied to idle projects immediately (provider rebuilt,
cached capability profile applied; no probe): a patch of `model.name` sets
every idle project's model and updates the workspace's remembered model,
while any other `model.*` key keeps each project's current model name (a
model picked with `models/switch` survives). `model.context_window` and
`model.max_output_tokens` are read by the agent runtime from the project's
own configuration, so they take effect when the project is reopened.

### `project/open` `{ "path": "/abs/dir", "exact": false, "init_git": false }` → `ProjectInfo`
`path` must be absolute (`-32602` otherwise). Reopening a root whose
project was closed while a task was running waits up to 30 s for that run's
`task/finished` before the new project is built.
`init_git` (the app's Settings → General "Initialize a Git repository in
projects that have none", on by default): when the folder is outside any
repository and not yet open, `git init -q` runs in it first, so the
working-tree snapshots `task/run` takes (see `session/rewind`) exist; the
result then carries `"git_initialized": true` (false on every other open).
It is never run for `/`, the home directory, its parent or any top-level
directory (a `log` warning says so), and a failed `git init` only logs.
Opens the repository containing `path` (git toplevel unless `exact`), loads
its configuration through the layers described above (so repository
validation commands, `agent.max_steps`, index excludes … apply), creates or
reuses its stores, and starts indexing in the background in `spawn_blocking`
(`index/progress` then `index/ready` notifications; `ProjectInfo.indexing`
stays `true` until `index/ready` has been sent, and `task/run` waits on the
same flag for at most 2 s). Reopening an already-open root returns the existing project;
reopening a root closed while its indexing was still running first waits for
that pass. The server checks `path.is_dir()` itself
(`RepositoryContext` tolerates missing dirs) and fails with `-32000` kind
`config` otherwise. The provider is built with `build_provider` and a cached
capability profile is applied when present (`load_profile`); **no probe runs
at open** — probing happens lazily in `task/run` / `models/switch` (120 s
timeout) and its notices (`app.rs` warnings) are sent as `log`
notifications. `permission_profile` is the project's live per-run profile
(`permissions/set`), which starts as `config.permissions.profile`.
`git.dirty_files` is a count (the library's `Vec<String>` length);
`index.languages` / `languages` are arrays of `{language, files}` objects;
`build_files` are path strings; `languages`, `build_files`, `file_count` and
detected `validation_commands` come from the published index, so they are
empty (`0`) until a project's first index completes (configured
`validation.commands` show at once), and the app asks `project/status`
again on `index/ready`; `validation_commands` carry only the four
fields shown — the server maps library structs into these DTOs explicitly
and never serializes them directly.
```json
{"project_id":"…","root":"/abs/dir","name":"dir","git":{"branch":"main","head":"862959a…","dirty_files":3,"worktree":null | {"parent_root":"/abs/main","managed":true}} | null,"git_initialized":false,
 "index":{"generation":12,"files":310,"symbols":4102,"chunks":5110,"relationships":900,"embeddings":5110,"last_full_index_at":"…"|null,"languages":[{"language":"rust","files":120},{"language":"markdown","files":10}]},
 "indexing":true,
 "model":"gpt-4.1","permission_profile":"approval_required","sandbox":"none",
 "validation_commands":[{"command":"cargo test","kind":"test","cwd":".","source":"Cargo.toml"}],
 "languages":[{"language":"rust","files":120}],"build_files":["Cargo.toml"],"file_count":310,
 "running_task":null | {"session_id":"…","task_id":"…"|null,"started_at":"…"},
 "running_tasks":[{"session_id":"…","task_id":"…"|null,"started_at":"…"}]}
```
`running_tasks` lists every session's running task, oldest first;
`running_task` is its first entry (for clients written when a project ran
one task at a time).

### `project/close` `{ "project_id" }` → `{}`
Cancels a running task, drops the context. An indexing pass that is still
running cannot be interrupted; it finishes silently (no further
`index/progress` / `index/ready` for the closed id) and a `project/open` of
the same root waits for it.

### `project/status` `{ "project_id" }` → `ProjectInfo` (fresh git + index stats).

### `index/rebuild` `{ "project_id" }` → `{}` (progress/ready notifications follow; `index/ready.rebuilt = true`). Refused with `-32002` while a task is running (`IndexStore::clear` would pull the index from under the run); the check and the start of the rebuild happen under the lock `task/run` reserves its slot with, so neither can slip in between the other's check and start.

### `sessions/list` `{ "project_id", "limit": 100 }` → `{ "sessions": [SessionSummary] }`
Sub-agent sessions (mode `agent`, see **Sub-agents**) are never listed.
```json
{"id":"…","model":"gpt-4.1","mode":"desktop","title":"Fix spacing on cards","created_at":"…","updated_at":"…",
 "active_task_id":"…"|null,"task_count":3,"last_status":"completed"|null}
```
`title` is `metadata.title` when set, else the first task objective truncated
to 80 chars, else `"New chat"`. Ordered by `updated_at` descending.

### `session/create` `{ "project_id", "title"?: "…" }` → `SessionSummary` (mode `desktop`, model = project's current model).
### `session/rename` `{ "project_id", "session_id", "title" }` → `SessionSummary` (stores `metadata.title`; an empty title clears it).
### `session/delete` `{ "project_id", "session_id" }` → `{}`. Refused (`-32002`) while a task of that session is running, and a `task/run` for a session whose delete is in flight is `-32602`. The session's sub-agent sessions (`metadata.parent_session`) are deleted first, the same way. Deletes, in one transaction through `SessionStore::database().with_tx`: metrics, validation runs, model requests, checkpoints and events of the session's tasks (and of the session), the conversation, the tasks and the session row; `memories.task_id` and other tasks' `parent_task_id` that pointed at deleted tasks are set to NULL (episodic memories are kept: they are repository knowledge). No table declares foreign keys, so nothing cascades on its own.

### `session/history` `{ "project_id", "session_id" }` → `SessionHistory`
Everything needed to render a past chat without replaying the run:
```json
{"session":SessionSummary,
 "turns":[{
   "task":{"id":"…","objective":"…","category":"localized_edit","state":"COMPLETED","status":"completed"|null,
           "summary":"…"|null,"created_at":"…","updated_at":"…","completed_at":"…"|null,"modified_files":["…"],"unresolved":["…"]},
   "messages":[
     {"id":41,"kind":"user","text":"…","images":["/abs/path.png"],"files":[{"path":"/abs/notes.txt","name":"notes.txt"}],"snapshot":true,"created_at":"…"},
     {"id":42,"kind":"assistant","text":"…"|null,"tool_calls":[{"id":"call_1","name":"read_file","arguments":{…}}],"created_at":"…"},
     {"id":43,"kind":"tool_result","call_id":"call_1","tool_name":"read_file","content":"…","is_error":false,"created_at":"…"}
   ],
   "events":[AgentEvent…],
   "outcome":TaskOutcome|null,
   "usage":{"prompt_tokens":0,"completion_tokens":0,"cached_tokens":0,"requests":0,"cost_usd":null}
 }]}
```
`messages` are the task's persisted conversation in order
(`messages_for_task(task, 0)`, i.e. including rows before any compaction;
only `user`, `assistant` and `tool_result` kinds; user messages whose text
starts with `[Context runtime]` are flagged `"runtime": true`, except the
note that resumes a response stream that broke off mid-way, which is left
out (`is_hidden_note`); `images`,
and `files`, omitted when empty, are the image and text files sent with the
message — the files may have been deleted since).
`events` are the persisted `AgentEvent`s for the task via
`EventLog::replay` (raw kinds such as `tool_output`, `context_explanation`,
`context_request` are filtered out; no text deltas, no top-level
`tool_call_output`). Sub-agent events are persisted wrapped in
`agent_progress`; for them the server additionally synthesizes an
`agent_progress{inner: tool_call_output}` right after each wrapped
`tool_call_finished` from the agent task's `tool_output` rows (paired
positionally per call id), because an agent's conversation lives in its own
session and is not part of `messages`. Its `content` is clipped to the first
1,000 bytes (on a char boundary) and its `full_len` is at least the stored
content's length, so the UI's truncation badge covers the clipped part: a long
session's agents make thousands of calls, and full outputs made one history
tens of megabytes. `tool_result.content`
is the conversation record's content — the (truncated, redacted) text the
model saw, with `is_error`; the `tool_output` row's `full_len` is reported
as `"full_len"` on the message when it exceeds the content length so the
UI can show a truncation badge (rows and results are paired positionally in
id order, because call ids repeat inside a continued task). `outcome` is the `task_completed` event's
outcome when the task finished. `usage` comes from
`SessionStore::usage_totals(Some(task_id), None)`; `cost_usd` is the sum of the
recorded request costs (absent when none had a price). A user message's `snapshot` says whether a
working-tree snapshot exists for the run it started (`session/rewind` can
then restore the files).

### `session/rewind` `{ "project_id", "session_id", "message_id": 41, "restore_files": false }` → `{ "removed_tasks": 1, "removed_messages": 7, "files": { "restored": 2, "deleted": 1, "skipped": ["src/b.rs"] } | null, "session": SessionSummary }`
Edit-a-sent-message and undo: removes user message `message_id` (a
`session/history` user message id, or `task/finished.message_id`) and
everything the session recorded after it, in one transaction — later tasks
entirely (with their sub-agent sessions, as `session/delete` would), and
the message's own task either entirely (when the message started it) or
from that message on (conversation rows from its id, events / model
requests / validation runs / metrics / checkpoints from its `created_at`,
sub-agent sessions created since), with the task's state, status, summary
and `completed_at` put back to its last remaining `task_completed` event
(IDLE / no status when none is left). The session's `active_task_id` becomes
the last remaining task and its snapshot list is cut back. Refused with
`-32002` while a task of that session is running; `task/run` for the
session is refused meanwhile (the `session/delete` guard). An unknown
message id, a message of another session, a non-user message or a runtime
message is `-32602`.

`restore_files: true` puts back the files **this session** changed since
the snapshot taken before that message, because other sessions may be
working in the same checkout. Every run records the files it (and its
sub-agents) wrote with the file tools, with the content hash it left each
at (`metadata.snapshots[].files`, added when the run ends). Of the files the
runs since the message wrote, those still as the session left them are
written from the snapshot, or deleted when the snapshot did not have them
(empty directories too); files changed since by anyone else are left alone
and listed in `files.skipped`. Changes the session made through shell
commands are not restored. Ignored files, the user's index and HEAD are
untouched; `files` reports the counts. A snapshot recorded before runs kept
that list restores the whole tree as it did before (files that differ are
written, files that did not exist then are deleted); that is refused with
`-32000` kind `conflict` while another session of the project is running.
Restoring files also drops what the session's agent has seen
(`ProjectActivity::forget`), so it reads files again before editing them. When no snapshot exists for the message (the project
had no repository, git failed) or it was pruned by `git gc`, the request
fails with `-32000` kind `not_found` **before anything is removed** — the
client retries without restoring. `files` is `null` when files were kept.

**Snapshots.** Before every run (after the index wait and the probe)
`task/run` writes the working tree as a git tree object through a scratch
index (`GIT_INDEX_FILE`, seeded from the real index so unchanged files are
not re-hashed; `git add -A` then `git write-tree`: untracked files included,
ignored files excluded) and records `{after_message_id, tree | null,
created_at}` in `sessions.metadata.snapshots` (at most 200 per session),
where `after_message_id` is the session's highest conversation id at that
moment — the run's own user message gets the next id, so the snapshot for a
message is the entry with the highest `after_message_id` below it. `tree` is
`null` when the root is no git work tree. The objects are unreferenced:
`git gc` prunes them after `gc.pruneExpire` (two weeks by default).

### `task/run` `{ "project_id", "session_id", "prompt", "images"?: ["/abs/path.png" | { "name"?: "shot.png", "data": "<base64>" }], "files"?: [{ "path": "/abs/notes.txt", "name"?: "report.pdf" } | { "name": "notes.txt", "path"?: "/client/notes.txt", "content": "…" }], "options"?: { "max_steps"?: 40, "proactive_retrieval"?: true, "memory_context"?: true, "stream"?: true, "debug_prompt"?: false } }` → `{ "accepted": true }`
Starts the agent loop for the prompt in that session. `images` (at most
20 absolute paths of PNG, JPEG, GIF or WebP files, each at most 20 MB; the
format is read from the file's signature) are read before the run starts —
a missing, unsupported or oversized file is `-32602` and starts nothing —
and go to the model with the prompt as `image_url` content parts (base64
data URLs). They are stored inline in the user message, so a continued task
replays them, and `session/history` lists their paths in the message's
`images`. A client on another machine (the desktop app over SSH, see
**Remote hosts**) sends an image's bytes instead: `{ "name", "data" }` with
`data` the file's base64 (same size limit and format check; bad base64 is
`-32602`), and `session/history` then lists its `name` where a path would
be. `files` (at most 20) are UTF-8 text files read the same way —
missing, relative or binary (a NUL byte or invalid UTF-8) is `-32602` — and
appended to the message text the model sees as
`<<<BEGIN ATTACHED FILE :: <name>>>> … <<<END ATTACHED FILE>>>` blocks;
`name` defaults to the file name (the desktop app sends the original name
for text it extracted from a PDF or Word document). Each file keeps its
first 200 KB (`FileInput::MAX_BYTES`, cut on a character boundary) and a
note of how many bytes were left out. They are stored inline too, and
`session/history` lists `{path, name}` in the message's `files` while its
`text` stays the prompt alone. A file sent inline (`content` with a `name`;
`-32602` without one) is cut the same way and listed with its `path` when
the client sent one, else its name. Exactly one task may
run per session (`-32002` otherwise); sessions of one project run at the same
time, in the same checkout (see "Concurrent sessions" below). Events stream as `event`
notifications; the run ends with a `task/finished` notification, which is
queued before the run slot is released (a `task/run` accepted afterwards is
always ordered after it on the wire). The
runtime decides whether the prompt continues the session's active task or
starts a new one (`task_started.continued`). Omitted (or `null`) options take
`RunOptions::default()`. `debug_prompt: true` (the app's developer mode)
makes the runtime emit a live-only `prompt_sent` event with the full prompt
of the run's first model request, right after its `model_request_started`;
sub-agents never emit it. The session's `updated_at` is bumped automatically
(`resolve_task` → `set_active_task`). Before spawning, the server waits up
to 2 s for the project's background index task if it is still running; a run
that starts while it still runs skips the runtime's own reconcile (so a fresh
repository is not indexed twice) and retrieves from what is indexed so far,
while edits and watcher events made meanwhile are queued and applied before
that pass publishes. It then probes the provider when no capability
profile is cached (`log` notifications report progress; an unusable endpoint
ends the run with `task/finished.error_kind = provider_unsupported`). The
probe and the index wait honour `task/cancel`: a run cancelled before it
reached the runtime ends with `task/finished{task_id:null,status:null,error_kind:"cancelled"}`
and creates no task. Each
run builds a fresh `AgentRuntime` with a fresh `CancellationToken`:
`AgentRuntime::new(ctx, provider, remembering).with_event_sink(tx).with_cancel(token).with_options(opts).with_permission_profile(profile).with_reasoning_effort(effort)`,
where `remembering` shares the project's `RememberingApprovals` answers (so
"approve for session" survives across runs) around the run's own
`SessionApprovals` (its requests filed under its session) and
`effort` is the project's current `model.reasoning_effort` (a `config/patch`
reaches the next run without reopening the project).

### `task/steer` `{ "project_id", "session_id", "prompt", "images"?, "files"? }` → `{}` (`images` and `files` as in `task/run`)
Hands a message to the session's running turn (attachments as in `task/run`). The runtime gives it to the model before its next step and reports it with a `steered` event; a turn that was about to finish keeps going to answer it. `-32003` when the session has no run or its run is ending (the message was not taken): the client sends it with `task/run` after `task/finished`. A message the run never read (it was cancelled, failed or ran out of steps) is dropped with the run; the client, which saw no `steered` event for it, sends it again as the next turn.

### `task/cancel` `{ "project_id", "session_id"? }` → `{}` (`-32003` when idle). With `session_id`, cancels that session's run; without it, every run of the project. Cancels cooperatively; `task/finished` follows. A run parked on a pending approval is unblocked by answering it with `deny`; a run still waiting for the background index or a capability probe is cancelled immediately (`task/finished` with `status: null`, `error_kind: "cancelled"`).

### `approval/respond` `{ "project_id", "session_id"?, "request_id", "decision": "approve"|"deny"|"approve_for_session" }` → `{}`
Answers an `approval_requested` event of the session the `event`
notification named. Request ids are only unique within a session (two
sessions' providers may both number calls `call_0`), so clients send
`session_id`; without it the server picks the session that has a request
with that id, preferring one whose run is waiting. Unknown ids are an error (`-32602`).
Because the runtime emits `approval_requested` **before** the run's
`SessionApprovals` files the reply channel, the server keeps per project,
keyed by session and request id, `pending: HashMap<key, oneshot::Sender>` and
`early: HashMap<key, ApprovalDecision>`: a decision that arrives
first is stored in `early` and consumed when the pair arrives (one lock
guards `pending`, `early` and the announced ids, so the two
check-then-insert sequences cannot interleave); the `early` entry and the
announced id are dropped on `approval_resolved`, while a `pending` reply
sender is dropped only on that session's `task/finished` or `task/cancel`
(request ids such as `approval_call_1` repeat across steps and runs; only
one approval is in flight per session at a time). A request answered from the remembered set
("approve for session") never produces a pair, but the runtime still emits
`approval_requested` immediately followed by `approval_resolved`; the client
should treat such a back-to-back pair as already answered. Not every request
is a tool call: `tool: "discount_drop"` asks whether to go on at a much lower
ModelGateway discount (see `spend.confirm_discount_drop` under `initialize`).

### `browser/respond` `{ "project_id", "session_id", "request_id", "ok": true, "content": "…", "url": "…"|null, "title": "…"|null, "error": "…"|null }` → `{}`
Answers a `browser/request` notification (see **The browser tool** below):
the page text, element list, script result or screenshot path the app
produced goes in `content`; `url` and `title` are the page the browser shows
afterwards (the tool remembers `url` for its policy checks). `ok: false`
with `error` makes the tool call fail with that message (the model sees it
and adjusts). An unknown or already answered `request_id` is `-32602`.
Unlike approvals the request id is minted by the server and filed before
the notification is sent, so there is no early-answer race; `task/cancel`
and `task/finished` drop a session's waiting requests (the tool reports
"cancelled").

### `question/respond` `{ "project_id", "session_id", "request_id", "answers": [{ "selected": ["label"], "other": "…"|null }]|null, "message": "…"|null }` → `{}`
Answers a `question/request` notification (see **The ask_user tool** below):
one entry of `answers` per question, in order (`selected` holds the chosen
option labels, `other` the user's own words), and/or a free-form `message`
(the user wrote in the composer instead of picking). Neither, or nothing
picked, tells the agent the user skipped the questions. An unknown or
already answered `request_id` is `-32602`; like browser requests the id is
minted and filed by the server before the notification goes out.

### `permissions/set` `{ "project_id", "profile": "approval_required"|"workspace_autonomous"|"bypass_permissions" }` → `ProjectInfo`
Changes the permission profile used for the project's **next** runs
(`AgentRuntime::with_permission_profile`); the configuration file is not
touched. Names are parsed with `PermissionProfile::parse` (so any variant
the core crate knows, including `bypass_permissions`, is accepted) and
reported with `as_str()`; unknown names, including the retired `read_only`
and `sandbox_autonomous`, are `-32602`. The TUI's `/approval` names map onto
these: ask → approval_required, auto → workspace_autonomous, bypass →
bypass_permissions. Allowed while a task is running (the running task keeps
its profile). `ProjectInfo.sandbox` (`"none"` | `"wrapper"`) only reports the
developer-mode command wrapper from `[permissions.sandbox]`; it is not a
profile.

### `models/list` `{ "project_id"? }` → `{ "models": ["…"], "current": "gpt-4.1", "source": "endpoint"|"cached"|"fallback", "error": "…"|null, "stats": { "<model>": ModelStat } }`
`models` is the endpoint's `/models` listing (sorted, deduped, with the
current model inserted first when missing). On failure the result still
succeeds with the current model only and `error` set, so a picker can always
open. Results are cached per base URL for 10 minutes; `{"refresh":true}`
bypasses the cache.

`stats` is filled only while signed in to ModelGateway (`model.credential =
"modelgateway"`), else `{}` (also for a `modelgateway.co` host used with your
own key). Each entry is
`{"savings_percent": 98, "error_rate_24h": 1.5}` (both percentages, each
omitted when unknown): the gateway's discount against the official output
price (`/api/pricing?view=summary`) and its share of failed requests over the
last 24 hours (`/api/perf-metrics/summary`). Both public endpoints are fetched
together and cached in `<data_dir>/model-stats.json` for an hour (a failed
fetch keeps the last values and retries after 5 minutes); `refresh` does not
bypass that cache. The TUI's `/model` picker reads the same cache.

### `models/switch` `{ "project_id", "model": "…" }` → `{ "model":"…", "capability": CapabilitySummary }`
Builds a provider for the model (same endpoint), applies the cached
capability profile when it is agent-capable or probes the endpoint
(`tokio::time::timeout(120 s)`) — a cached profile that failed validation is
probed again rather than reused,
and swaps it into the project; new sessions are created with it. Existing
sessions keep the model name they were created with for display, while runs
always use the project's current model (`ProjectInfo.model`). Allowed while
a task is running: a run takes the provider when it starts, so the running
turn keeps its model and the new one applies from the next `task/run`
(a `task/steer` message joins the running turn, so it keeps the old model
too). In `--mock` mode the switch succeeds without any network access.
```json
"capability":{"probed_at":"…"|null,"agent_capable":true,"streaming":true,"tool_calling":true,"parallel_tool_calls":true,"reasoning_controls":false,"issues":[]}
```

### `suggest/next` `{ "project_id", "session_id" }` → `{ "suggestion": "Run the tests" | null }`
The message the user is likely to type next, for the composer's ghost text
after a run. The project's current model reads the last ten user / agent
text turns of the session (runtime notes and tool results left out,
redacted, each clipped to 1 500 characters) and answers in at most a dozen
words; there is no tool use, a 64-token cap and a 20 s timeout. `null` when
the agent has not answered last, the model says there is no clear next
step (`NONE`), the answer is empty or longer than 160 characters, or the
call fails or times out (logged at `info`, never an RPC error). Nothing is
persisted. In `--mock` mode the answer is always `"Show me where that is
implemented"`.

### `suggest/starters` `{ "project_id" }` → `{ "suggestions": [{ "kind": "fix"|"explain"|"refactor"|"test"|"docs"|"review"|"feature"|"cleanup"|"other", "text": "Fix the flaky retry test" }] }`
Prompts listed under the empty chat's composer. The project's current
model reads the project name, its top languages, build files and
validation commands, up to 20 uncommitted paths, the last 8 commit
subjects and the titles of the 8 most recent chats (all redacted) and
answers with three `kind | prompt` lines; there is no tool use, a
160-token cap and a 25 s timeout. At most three suggestions, each at most
90 characters, duplicates dropped; a kind the model invents becomes
`other`. An empty list when the call fails or times out (logged at
`info`, never an RPC error); the app then shows its own fallback list.
Nothing is persisted. In `--mock` mode the answer is a fixed list of
three.

### `git/status` `{ "project_id" }` → `{ "branch":"main","head":"…","files":[{"path":"a.rs","status":"M"|"A"|"D"|"R"|"?"|"MM"…,"staged":false,"insertions":12|null,"deletions":3|null}] }`
`insertions` / `deletions` are the file's line changes against HEAD (`git
diff --numstat --no-renames HEAD`; on an unborn branch the index and
working-tree diffs summed); an untracked text file counts its lines as
insertions (up to 4 MB). Both are `null` for binary files and when unknown.
Runs `git status --porcelain=v1 -z --untracked-files=all` (NUL-separated;
for `R`/`C` entries the following field is the original path), with
`status = XY.trim()` (`"??"` → `"?"`) and `staged = X != ' ' && X != '?'`;
branch/head from `RepositoryDiscovery::git_info`. All git commands use
`tokio::process::Command` with `.output()`, `stdin(null)`,
`GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat` — never
`commands::diff`, which inherits stdout.
### `git/diff` `{ "project_id", "path"?: "a.rs", "staged"?: false }` → `{ "diff": "<unified diff text>", "truncated": false }` (`git -c color.ui=never -c core.quotepath=false -c diff.renames=true --no-pager diff --no-ext-diff --no-color --src-prefix=a/ --dst-prefix=b/ [--cached] [-- path]`, so the user's `diff.external`, colour, prefix and quotepath settings cannot change the format; capped at 2 MB).
### Git actions (the app's Changes tab)
All shell out to `git` in the project root with the user's own
configuration (identity, hooks, credential helpers; `GIT_TERMINAL_PROMPT=0`,
so nothing prompts). `paths` are repository-relative file paths as
`git/status` lists them; an absolute path or one with `..` is `-32602`. A
failing git command is `-32000` kind `other` with git's first stderr line
(or last stdout line, e.g. `nothing to commit`) in the message. Actions
that rewrite the working tree — `git/discard`, `git/checkout`, `git/pull`,
`git/undo_commit` — are refused with `-32002` while a task runs.

- `git/stage` `{ "project_id", "paths": [] }` → `{}` — `git add -A [-- paths]`; empty `paths` stages everything.
- `git/unstage` `{ "project_id", "paths": [] }` → `{}` — `git reset -q [-- paths]` (works on an unborn branch too).
- `git/discard` `{ "project_id", "paths": [] }` → `{ "restored": 2, "deleted": 1 }` — tracked files go back to the index (`git restore --worktree`), untracked files are deleted (empty parent directories too); empty `paths` means every change.
- `git/commit` `{ "project_id", "message", "amend": false, "all": false, "paths": [] }` → `{ "commit": "<sha>", "subject": "…" }` — `all` runs `git add -A` first; `paths` stages those files and commits only them (`git commit -- paths`). An empty message is `-32602`.
- `git/undo_commit` `{ "project_id" }` → the undone `{ "commit", "subject" }` — `git reset --soft HEAD~1` (its changes stay staged); refused for the root commit.
- `git/push` `{ "project_id", "remote"?: "origin", "branch"?: "main", "set_upstream": false, "force": false }` → `{ "output": "…" }` — `git push --porcelain [--set-upstream] [--force-with-lease] [remote [branch]]` (a plain `git push` without remote); never `--force`. 120 s timeout.
- `git/pull` `{ "project_id", "rebase": false }` → `{ "output" }` — `git pull --no-edit [--rebase]`, 120 s timeout.
- `git/fetch` `{ "project_id", "prune": false }` → `{ "output" }` — `git fetch --all [--prune]`, 120 s timeout.
- `git/branches` `{ "project_id" }` → `{ "current": "main"|null, "local": [{"name","upstream": "origin/main"|null}], "remote": [{"name": "origin/feature", "short": "feature"}] }` (`origin/HEAD` left out).
- `git/checkout` `{ "project_id", "branch", "create": false }` → `{ "branch": "feature" }` — `git switch [-c] <branch>`; a bare remote branch name tracks it (git's DWIM). Names failing `git check-ref-format --branch` are `-32602`.
- `git/log` `{ "project_id", "limit": 20 }` → `{ "commits": [{"hash","short","subject","author","date"}] }` (empty on an unborn branch; limit 1–200).
- `git/commit_message` `{ "project_id" }` → `{ "message": "…", "source": "model"|"heuristic", "error": "…"|null }` — a commit message for what a commit would take now (the staged changes, else every change): the project's model reads the file list and the redacted diff (24k chars max, 60 s) with a fixed system policy and only the message is returned (fences / quotes / "Commit message:" stripped). In mock mode, when the probe or the call fails or the model returns nothing, a heuristic message from the file list ("Add a and b", "Update 3 files" + a path per line) is returned with `source: "heuristic"` and the reason in `error`. A clean tree is `-32000`.

`git/status` also carries `head_subject`, `upstream` (`origin/main` or
`null`), `ahead`, `behind` (`git rev-list --left-right --count HEAD...@{u}`)
and `remotes: [{name, url}]`, and `branch` is filled on an unborn branch
(`git symbolic-ref`).

### Worktrees (parallel sessions)
`task/run` allows one run per project root, so a second prompt in a busy
project queues. A linked git worktree is a second checkout of the same
repository on its own branch; opened as a project of its own it has its own
run slot, index, chats and Changes tab, and two tasks never edit the same
files. Worktrees live under `<data_dir>/worktrees/<parent repository id>/<slug>`
on branch `ctx/<slug>`. `git.worktree` of a `ProjectInfo` is set for any
root that is a linked worktree (`git rev-parse --git-common-dir` differs
from `--git-dir`): `parent_root` is the main checkout's root (the parent
project's `root`, so the app nests the worktree under it) and `managed`
whether it lives in the data dir. The parent's `.context-agent.toml` applies
to the worktree because it is checked in; the user-level configuration,
credentials and remembered model are shared as for any project.

- `worktree/create` `{ "project_id", "name"?: "Fix the login bug", "base"?: "HEAD" }` → `ProjectInfo` of the new project. `project_id` may be the parent or one of its worktrees (the main checkout is used either way). `name` is slugified (ASCII letters, digits and dashes, 40 bytes: `fix-the-login-bug`); without one a random `adjective-noun` pair is used. A taken folder or branch gets `-2`, `-3`, …. `git worktree add -b ctx/<slug> <path> <base>` runs in the main checkout (`-32000` with git's error when it fails, e.g. an unknown `base`), the parent's index store is copied to the new project's (`VACUUM INTO`, best effort: a failure only logs), and the folder is opened exactly like `project/open {exact: true}`, so `index/progress` / `index/ready` follow for the new `project_id`; because the files are the parent's, that first pass reports them as `files_unchanged` instead of parsing and embedding them again. `-32602` when the project is not inside a git repository.
- `worktree/list` `{ "project_id" }` → `{ "worktrees": [{"path","branch": "ctx/x"|null,"head","is_main": true,"managed": false}] }` — `git worktree list --porcelain` of the project's repository, the main checkout first; `managed` marks those under the data dir.
- `worktree/remove` `{ "project_id", "delete_branch": false, "force": false }` → `{ "removed": "/path", "branch": "ctx/x"|null, "branch_deleted": false }` — `project_id` must be a linked worktree (`-32602` otherwise) and idle (`-32002` while its task runs). The project is closed as by `project/close`, then `git worktree remove [--force] <path>` runs in the main checkout (`--force` discards uncommitted changes; without it a dirty worktree is `-32000`), and with `delete_branch` the branch is deleted (`git branch -D`; a failure there only logs and leaves `branch_deleted: false`). The worktree's stores under `repositories/<id>` are kept, so a worktree created again at the same path finds its chats.
- `worktree/merge` `{ "project_id", "delete_after": false }` → `{ "merged": "<sha>", "subject": "Merge branch 'ctx/x'", "removed": false }` — lands the worktree's branch in the main checkout. `project_id` must be a linked worktree (`-32602` otherwise); `-32002` while a task runs in the worktree or in the parent project (the open project whose root is the main checkout). `-32000` when the worktree has uncommitted changes (commit them in its Changes tab first), when the parent checkout is on no branch, when the branch has nothing the parent lacks, or when `git merge` refuses because the parent's working tree is dirty in a way it cannot keep (git decides; its first stderr line is the message). `git merge --no-ff --no-edit ctx/<slug>` runs in the main checkout; on a conflict the merge is aborted (`git merge --abort`, the parent is left as it was) and the `-32000` message lists the conflicting paths. With `delete_after` a successful merge is followed by `worktree/remove {delete_branch: true}` (`removed: true`; a failure there is `-32000` whose message starts with the merge that did land). `merged` is the parent's new HEAD.

### `fs/list` `{ "path"?: "/abs/dir" | "~/code", "dirs_only"?: false }` → `{ "path": "/abs/dir", "parent": "/abs" | null, "home": "/home/alice", "entries": [{ "name", "path", "is_dir", "is_git", "is_hidden" }] }`
A directory of the machine the server runs on, with no project involved:
the desktop app's folder picker and file tree for a backend on another
machine (**Remote hosts**), where `NSOpenPanel` and `FileManager` cannot
see the files. `path` is `~`-expanded; absent or empty lists the home
directory. The listed path is canonical; `parent` is absent at the root.
Entries come directories first, then files, each sorted by name, without
`.git` and `.DS_Store`; `is_git` marks a directory holding a `.git` entry (a
repository root), `is_hidden` a dotfile, and `dirs_only` leaves files out. A
relative path, a missing one or a file is `-32602`. Symlinked directories
list as directories.

### `file/read` `{ "project_id", "path": "src/a.rs", "max_bytes"?: 262144 }` → `{ "path":"…","content":"…","truncated":false,"language":"rust"|null,"size":1234,"hash":"sha256:…" }`
At most `max_bytes` (clamped to 1 … 16 MiB) are read from the file — never
the whole file — and `size` is the file's size on disk.
Paths are repository-relative or absolute and are resolved with the
project's `PathPolicy` (`ctx.policy.paths().resolve`, then `relative`):
anything outside the allowed workspace roots (the repository plus
`permissions.extra_workspace_roots`) is refused with `-32000` kind
`permission_denied`; directories and binary files are refused with kind
`invalid_argument`. Files matching the credential patterns (`.env`, keys)
are read and written like any other — this is the user's editor; the
agent's file tools and the index still leave them out.
`language` is the registry's name for the path (`rust`, `python`,
`markdown`, `text`, …) and `null` when the registry reports `other`.
`hash` is the SHA-256 of the bytes returned (so of the whole file unless
`truncated`), the handle `file/write.expected_hash` takes.

### `file/write` `{ "project_id", "path": "src/a.rs", "content": "…", "expected_hash"?: "sha256:…" }` → `{ "path":"…","hash":"sha256:…","size":1234,"language":"rust"|null,"created":false }`
Replaces the file's whole content (or creates it, parent folders included)
with the same path rules as `file/read`; the write is atomic (temp file in
the same folder, then rename, keeping the file's mode). With
`expected_hash` the write only happens when the file's current content
still hashes to it — a file edited meanwhile (by the agent, another editor)
or one that no longer exists is `-32000` kind `conflict` and nothing is
written; the app then offers to reload or overwrite. The index is updated
for the path right away (best effort: a failure only logs), so
`symbols/find` and the agent's retrieval see the new content. `hash` is the
hash of what was written.

### `symbols/find` `{ "project_id", "query": "add", "mode": "prefix"|"exact"|"contains", "limit": 50 }` → `{ "symbols": [{"name","kind":"function"|"struct"|"method"|…,"path":"src/lib.rs","parent":"Foo"|null,"line":12,"end_line":14,"signature":"pub fn add(a: i32, b: i32) -> i32"|null}] }`
Symbol definitions from the project's index (`IndexStore::find_symbols`),
for the editor's completion (`prefix`, the default, shortest names first)
and go-to-definition (`exact`). `line` / `end_line` are 1-based. A blank
`query` is an empty list; `limit` is clamped to 1 … 500. Nothing is read
from disk: a file edited outside `file/write` is as stale as the index.

### Language servers (`lsp/*`)
The desktop editor's "type sense" beyond the index: one language server per
project and language (`ctx_lsp::ServerCatalog`: rust-analyzer, pyright,
typescript-language-server, gopls, clangd, vscode-html/css-language-server, marksman, yaml-language-server, taplo, …, or `CTX_LSP_COMMAND_<LANGUAGE>`
= "exe arg …" to choose one), started by the first `lsp/open` of a document
of that language in the project root, shut down with `project/close`.
Documents are synced from the editor's buffer, never read from disk, so the
answers cover unsaved text. Positions are 0-based like the protocol (the
`line` of `symbols/find` is 1-based). A language without a known or
installed server answers every call with the empty shape below and never
errors; a server that fails to start or answer is `-32000` (its message
names the server). Paths follow the `file/read` rules.
- `lsp/status` `{ "project_id", "path" }` → `{ "path", "language": "rust"|null, "server": "rust-analyzer"|null, "installed": true, "running": false, "install_hint": "rustup component add rust-analyzer"|null }`.
- `lsp/open` `{ "project_id", "path", "text" }` → `{ "path", "language", "server": "rust-analyzer"|null, "version": 1|null, "trigger_characters": [".", "::"] }` — `textDocument/didOpen` with `text` (a `didChange` when the document is already open). `trigger_characters` are the server's completion triggers.
- `lsp/change` `{ "project_id", "path", "text" }` → `{ "version": 2|null }` — full-text `didChange` (opens first when needed). The editor sends it debounced and right before a completion request.
- `lsp/close` `{ "project_id", "path" }` → `{}` — `didClose`; a no-op when nothing is open.
- `lsp/completion` `{ "project_id", "path", "line", "character" }` → `{ "items": [{"label","kind":"function"|"method"|"field"|"variable"|"struct"|"keyword"|"snippet"|…,"detail":"fn(a: i32) -> i32"|null,"documentation":"…"|null,"insert_text":"add(a, b)","filter_text":null,"sort_text":null,"range":{"start_line","start_character","end_line","end_character"}|null,"deprecated":false}], "incomplete": false, "server": "rust-analyzer"|null }` — `textDocument/completion`, at most 200 items sorted by `sort_text` then `label`. `insert_text` is the server's `textEdit.newText` / `insertText` / label with snippet placeholders reduced (`${1:x}` → `x`, `$0` → nothing); `range` is the text the server wants replaced when it said so (the editor otherwise replaces the identifier before the caret).
- `lsp/hover` `{ "project_id", "path", "line", "character" }` → `{ "contents": "fn add(a: i32, b: i32) -> i32\n\ndocs…"|null }` — `textDocument/hover`, `MarkedString[]` / `MarkupContent` flattened to one text.
- `lsp/definition` `{ "project_id", "path", "line", "character" }` → `{ "locations": [{"path","line","column"}] }` (1-based, as `find_references`).

### `tasks/list` `{ "project_id", "limit": 50 }` → `{ "tasks":[TaskSummary] }` (as in `session/history.turns[].task` plus `session_id`).
### `memory/list` `{ "project_id", "category"?: "project_knowledge", "limit": 50 }` → `{ "memories":[{"id":"…","category":"…","verification":"…","content":"…","evidence_paths":["…"],"created_at":"…","invalidated":false}] }`
### `doctor/run` `{ "project_id"?, "probe": false }` → `DoctorReport`
Rebuilt from the same inputs as `ctx doctor` (configuration, repository,
validation commands, language servers, model provider); `probe: true` runs
`provider.probe` under a 120 s timeout and saves the profile; refused with
`-32002` while a task is running (a probe sends real model requests).
```json
{"problems":0,"sections":[{"title":"configuration","rows":[{"label":"data dir","value":"…","ok":true}]},{"title":"repository","rows":[…]},{"title":"validation commands","rows":[…]},{"title":"language servers","rows":[…]},{"title":"model provider","rows":[…]}]}
```
### MCP servers and apps (`mcp/*`)

`ctx serve` connects the user's `[mcp]` servers once per process (shared by every
project's runs; `--mock` connects none) in the background as it starts, and their
tools join each run's tool set as `mcp__<server>__<tool>` (see docs/configuration.md
"MCP servers"). Stdio servers without a `cwd` run in the home directory.

- `mcp/status` `{}` → `{ "servers": [McpServerStatus] }`, sorted by name:
  `{ "name", "enabled", "app"?, "state": "disabled"|"connecting"|"connected"|"failed", "error"?: "…", "transport": "stdio"|"http"|"builtin", "target": "<command line or URL>", "server_name"?, "server_version"?, "protocol_version"?, "instructions"?, "tools": [{ "name", "qualified_name": "mcp__<server>__<tool>", "description", "read_only", "destructive", "enabled" }] }`.
  `tools` lists everything the server offers; `enabled` is false for a tool the
  entry's `tools` allowlist leaves out (the app's per-tool permission toggles patch
  that allowlist).
  `disabled` is a server (or the whole table) switched off; `failed` carries the
  error (`could not start …`, the server's stderr tail, an HTTP status, or a timeout).
- `mcp/reconnect` `{ "server"?: "<name>" }` → the same shape, after reconnecting
  that server (or every active one) and waiting for the result (bounded by
  `startup_timeout_secs`). `-32602` for an unknown name.
- `config/patch` with `mcp.*` keys (`"mcp.enabled": false`,
  `"mcp.servers.<name>": { … }` whole or `null` to remove) reconfigures the live
  servers: removed or changed entries are closed and (re)connected in the
  background; a run that starts meanwhile waits for them.
- Notification `mcp/status` `{ "servers": [McpServerStatus] }` follows every
  change of a server's state or tool list, so the app's MCP settings stay current
  without polling.
- `doctor/run` adds an `mcp servers` section (one row per configured server, `ok`
  false for a failed one) whenever servers are configured.
- `mcp/catalog` `{}` → `{ "apps": [CatalogApp] }`: the curated apps the client shows
  on its Apps page: `{ "id", "name", "tagline", "example_prompt", "description", "icon": "<SF Symbol>",
  "logo"?: "<brand mark the client bundles as logo-<name>>", "logo_monochrome",
  "category", "vendor", "homepage"?, "builtin", "default_installed", "access": [{ "title",
  "description", "tools": ["<pattern>"], "required" }] (the connect page's "What Context can
  access"; unchecked groups' patterns are sent as `exclude_tools`), "requirements":
  ["Node.js (npx)"], "inputs": [{ "key", "label", "help", "secret", "required", "kind":
  "env"|"header" }], "template": McpServerConfig }`. An app is installed when
  `mcp.servers` has its `id`.
- `mcp/install` `{ "id", "inputs"?: { "<key>": "<value>" }, "exclude_tools"?: ["create_*"] }` → the `config/get` result:
  writes `mcp.servers.<id>` from the template with the inputs in `env` / `headers`
  (`app = id`), takes `id` out of `mcp.uninstalled`, and connects it like any
  `mcp.*` patch. Reinstalling keeps the entry's `bypass_permissions` and `permissions`. `-32602` for an unknown id or a missing required input.
- `mcp/uninstall` `{ "id" }` → the `config/get` result: removes the entry; a default
  app is added to `mcp.uninstalled` so it stays removed.
- Sign-in (`CatalogApp.oauth`: `{ "provider": "github", "credential": "github", "input":
  "GITHUB_PERSONAL_ACCESS_TOKEN", "scopes": [...], "available": bool }`; `available` is false
  when the build has no OAuth client ID, and the client then shows the token field):
  `mcp/oauth/start` `{ "app" }` → `{ "device_code", "user_code", "verification_uri",
  "expires_in", "interval" }`; `mcp/oauth/poll` `{ "app", "device_code" }` →
  `{ "status": "pending", "interval" } | { "status": "approved", "account" } |
  { "status": "denied" } | { "status": "expired" }` (on approval the token is stored in
  `credentials.toml`, never returned, and an installed app reconnects);
  `mcp/oauth/account` `{ "app" }` → `{ "account": "octocat" | null }`;
  `mcp/oauth/signout` `{ "app" }` → `{ "signed_out": bool }`. The client installs the app
  with the input set to `${credential:<credential>}`. `mcp/uninstall` forgets the
  credentials the removed entry referred to.
- `@` mentions need no method of their own: `task/run` and `task/steer` prompts that
  name installed apps as `@<name>` get a trusted instruction for that turn pointing the
  model at those apps' tools, and prompts that name files as `@path` / `@"path"` send
  those files' current text with the message (a hidden user message whose text is
  `FILE_MENTIONS_NOTE`, which `session/history` leaves out) (docs/configuration.md
  "`@` file mentions"). The client completes apps from `mcp/status` and files from
  `files/search` `{ "project_id", "query", "limit"?: 20 }` → `{ "files": ["src/app.rs", …] }`
  (fuzzy-matched over an ignore-aware walk cached for ten seconds; `limit` at most 100).
- `mcp/permissions` `{}` → `{ "screen_recording": bool, "accessibility": bool, "platform": "macos" }`:
  the Computer Use permissions as the `ctx serve` process sees them (it is the parent
  of `ctx mcp-serve computer-use`, so this is what the server gets). Screen Recording
  applies to processes started after the grant, so the client offers a backend restart
  when its own check says granted and this one does not.
- Tool results may carry images (the `screenshot` tool of Computer Use): the runtime
  appends them to the conversation as one user message after the batch of tool
  results, with the text `[Context runtime] The images returned by the tool call(s)
  above, in call order.`, which `session/history` hides like other runtime notes.

### Spending (`spend/summary`, `pricing/lookup`)

- `spend/summary` `{}` → `{ "period": "month", "since": "2026-10-01T07:00:00+00:00", "total_usd": 12.5, "max_usd": 50.0 | null, "models": [ { "model": "gpt-4.1", "cost_usd": 10.2, "requests": 140, "unpriced_requests": 0, "limit_usd": 20.0 | null } ] }`: this period's spend from the shared ledger, biggest first, followed by every model that has a limit but no spend yet. `unpriced_requests` were answered without a known price (their cost is missing from the sums).
- `pricing/lookup` `{ "models": ["gpt-4.1", "kimi-k2"] }` → `{ "prices": [ { "model": "gpt-4.1", "price": { "input": 2.0, "output": 8.0, "cache_read": 0.5 } | null, "source": "custom" | "catalog" | "default" | null } ] }`: the price each model is charged at and where it comes from (see `initialize`).

### ModelGateway (`gateway/*`)

The desktop app's optional "Sign in with ModelGateway" (the `new-api`-based
gateway at `https://www.modelgateway.co`; `apps/macos/ARCHITECTURE.md`
"Account"). The app never speaks HTTP itself and never sees the key: these
methods relay to the gateway's `/api/device/*` endpoints and return the
`data` object of the gateway's `{success, message, data}` reply. The device
key the gateway creates at approval (a *hidden* key, not listed under the
user's API Keys) is written to the credential store
`<data_dir>/credentials.toml` (mode 0600) as the entry `modelgateway`
(`api_key`, `origin`, `account`), and `model.credential = "modelgateway"`
makes the provider use it; no environment variable is involved. `base_url`
is the gateway origin (a trailing `/v1` is dropped; not an absolute http(s)
URL is `-32602`); the authenticated calls may omit it and use the stored
origin. Failures are `-32000` with `data.kind`: `"transport"` (unreachable,
non-JSON reply, HTTP error without envelope), `"unauthorized"` (401/403: the
key was revoked), `"signed_out"` (no stored key) or `"gateway"` (`success:
false`, with the gateway's `code` such as `DEVICE_CODE_EXPIRED`); `message`
is the gateway's own. Every call has a 20 s timeout.

- `gateway/status` → `{ "signed_in": true, "origin": "https://…", "account": "alex" }` (all but `signed_in` null when no key is stored).

- `gateway/device/start` `{ "base_url", "device_name"?: "Alex's MacBook" }` → `{ "device_code", "user_code": "BCDF-GHJK", "verification_uri": "https://…/device", "verification_uri_complete": "https://…/device?user_code=BCDF-GHJK", "expires_in": 900, "interval": 5 }`. The device name defaults to this Mac's `ComputerName`; the gateway names the key it will create "Context · <device name>".
- `gateway/device/poll` `{ "base_url", "device_code" }` → `{ "status": "pending", "interval": 5 }` until the user decides in the browser, then once `{ "status": "approved", "account": GatewayAccount }` after the key has been stored (the gateway serves it exactly once; a further poll is a `"gateway"` error), or `{ "status": "denied" }` / `{ "status": "expired" }`. The app polls every `interval` seconds until `expires_in` has passed.
- `gateway/account` `{ "base_url"? }` → `GatewayAccount`: `{ "id", "username", "display_name", "email", "group", "quota", "used_quota", "request_count", "quota_per_unit": 500000, "display_in_currency": true, "token": { "id", "name", "used_quota", "created_time" }, "billing_preference": "subscription_first", "subscriptions": [GatewaySubscription] }`. Quotas are gateway units; dollars = quota ÷ `quota_per_unit`. `subscriptions` lists the user's *active* plans (empty: pay as you go from the balance), each `{ "id", "plan_id", "status": "active", "source": "order" | "admin", "title": "Pro", "subtitle", "price_amount": 20, "currency": "USD", "duration_unit": "month", "duration_value": 1, "quota_reset_period": "never" | "daily" | "weekly" | "monthly" | "custom", "amount_total", "amount_used", "amount_reserved", "start_time", "end_time", "next_reset_time" }`: amounts are quota units (`amount_total` 0 = unlimited), times are Unix seconds (`next_reset_time` 0 = no reset), and the plan fields are absent when the plan was deleted. `billing_preference` (`subscription_first` · `wallet_first` · `subscription_only` · `wallet_only`) says whether requests draw on the plan or the balance first. Gateways older than this field omit both keys. `context_credit` is `{ "balance", "granted", "claimed", "eligible", "offer_quota", "deposit_types": ["stripe", "apirone"] }` (quota units; absent from older gateways): credit only requests made with the device key can spend, used before any plan or the balance (what it cannot cover goes to the balance). `offer_quota` ($10) is the welcome credit an account may claim once, and `eligible` is true while it is unclaimed and the account has a completed Stripe or Apirone top-up.
- `gateway/usage` `{ "base_url"?, "start_timestamp"?, "end_timestamp"? }` (Unix seconds; at most 400 days, the gateway clamps) → `{ "start_timestamp", "end_timestamp", "quota_per_unit", "display_in_currency", "rows": [ { "created_at": 1760000400, "model_name": "gpt-4.1", "project": "context-agent" | "", "count": 3, "quota": 1234, "token_used": 5000 } ] }`: the user's hourly usage rows (`created_at` is the hour, UTC) per model and per client project (`project` is what the `X-ModelGateway-Project` header carried; empty for requests without it). The client folds them into local days.
- `gateway/billing` `{ "base_url"? }` → `{ "billing_preference", "quota_per_unit", "display_in_currency", "wallet_url", "topup": { "enabled", "in_app", "min_amount", "max_amount", "amount_options": [10, 20], "discounts": { "100": 0.95 }, "amount_unit": "usd" | "quota" }, "plans": [ { "id", "title", "subtitle", "price_amount", "currency", "duration_unit", "duration_value", "quota_reset_period", "total_amount", "stripe", "balance", "max_purchase_per_user" } ], "has_active_subscription" }` (`GET /api/device/billing`): what the Billing settings show.
- `gateway/billing/quote` `{ "amount" }` → `{ "amount", "pay_money", "currency" }`: what adding `amount` to the balance costs.
- `gateway/billing/topup` `{ "amount" }` → `{ "checkout_url", "trade_no" }`: a checkout page the app opens in the browser; the balance grows when the payment completes. `code: "TOPUP_UNAVAILABLE"` when the gateway has no in-app method (the app then opens `wallet_url`).
- `gateway/billing/subscribe` `{ "plan_id", "method": "stripe" | "balance" }` → `{ "checkout_url" }` (stripe) or `{ "subscribed": true }` (paid from the balance at once).
- `gateway/billing/preference` `{ "billing_preference": "subscription_first" | "wallet_first" | "subscription_only" | "wallet_only" }` → `{ "billing_preference" }` (`PUT /api/device/billing/preference`).
- `gateway/key` `{ "base_url"? }` → `GatewayKeySettings`: `{ "id", "name", "exhausted", "unlimited_quota", "remain_quota", "used_quota", "quota_per_unit", "display_in_currency", "price_limits": { "default"?: PriceRule, "models": { "<model>": PriceRule } } }` (`GET /api/device/key`): the device key's own settings, the ones the website's key drawer edits for ordinary keys. A `PriceRule` is `{ "discount_percent"?, "max_price_per_m"? }`: requests are only routed to providers charging at most `100 − discount_percent` percent of the model's standard output-token price and at most `max_price_per_m` dollars per million output tokens; a model's rule replaces the default. With no provider under the cap a request fails with `price_limit_exceeded` (HTTP 503). `remain_quota` (quota units) is what the key may still spend unless `unlimited_quota`; `exhausted` means it reached the cap.
- `gateway/key/update` `{ "price_limits"?, "unlimited_quota"?, "remain_quota"? }` → `GatewayKeySettings` (`PUT /api/device/key`): changes the fields given. `price_limits` replaces every rule (`{}` clears them); raising the cap of an exhausted key enables it again. Only the hidden device key may call either (`code: "DEVICE_KEY_REQUIRED"`, HTTP 403).
- `gateway/context_credit/claim` `{ "base_url"? }` → the new `context_credit` object (`POST /api/device/context-credit/claim`, device key only): grants the welcome credit. `code: "CONTEXT_CREDIT_DEPOSIT_REQUIRED"` without a completed card or crypto top-up, `"CONTEXT_CREDIT_CLAIMED"` when it was claimed before.
- Billed costs: when a run that used the `modelgateway` credential finishes, the backend posts the replies' `X-Oneapi-Request-Id`s to `POST /api/device/costs` (`{ "request_ids": […] }` → `{ "quota_per_unit", "costs": [ { "request_id", "model_name", "quota", "prompt_tokens", "completion_tokens", "created_at" } ] }`) and replaces each estimate with `quota ÷ quota_per_unit`; ids the gateway has not logged yet are asked for once more 1.5 s later, then keep their estimate.
- `gateway/signout` `{ "base_url"? }` → `{ "revoked": true }` after the gateway deleted this device's key (a 401 counts: it was already gone). The stored credential is removed even when the gateway cannot be reached, which is then a `"transport"` error; with no stored key the result is `{ "revoked": false }`.

### `shutdown` → `{}` then the process exits after cancelling running tasks, waiting up to 5 s for their `task/finished`, and flushing. Stdin EOF behaves the same.

## Notifications (server → client)

- `event` `{ "project_id", "session_id", "event": AgentEvent }` — every
  `AgentEvent` the runtime emits (`{"event":"model_text_delta","task_id":"…","text":"…"}` …),
  passed through the project's `Redactor` like the persisted copy, so the
  live stream and `session/history` render the same text.
  The runtime emits `tool_call_output`
  (`{"event":"tool_call_output","task_id","call_id","name","content","full_len","is_error"}`)
  right after `tool_call_finished` with the (truncated, redacted) text the
  model saw; it is live only (the raw `tool_output` row is the persisted
  copy, so `session/history` fills outputs from `messages` instead).
  A sub-agent's approval requests arrive wrapped in `agent_progress`; the
  server unwraps them for its approval bookkeeping, and `approval/respond`
  answers them like the lead's.
- `task/finished` `{ "project_id", "session_id", "task_id": "…"|null, "status": "completed"|"blocked"|"failed"|"cancelled"|null, "error": "…"|null, "error_kind": "provider_auth"|null, "message_id": 41|null, "snapshot": true }` — always sent once per `task/run`, after the last `event`. `message_id` is the conversation id the prompt became (the handle for `session/rewind`; `null` when the run wrote no message, e.g. cancelled early) and `snapshot` whether a working-tree snapshot was taken before the run. Provider failures inside a step do not make `run` return `Err`: the runtime records them in `model_requests.error` and finishes the task with `status = failed` (summary "runtime error: …"). The server therefore, on a failed outcome, reads the last `model_requests` row **this run** wrote for the task (rows of earlier runs of a continued task are ignored) and, when its `error` is set, copies it into `error` and derives `error_kind` from the message prefix (`authentication failed` → `provider_auth`, `rate limited` → `provider_rate_limited`, `provider request timed out` → `provider_timeout`, `connection failure` → `provider_connection`, `stream interrupted` → `provider_stream_interrupted`, `context length exceeded` → `provider_context_length`, `invalid provider response` → `provider_invalid_response`, `model returned malformed tool call` → `provider_malformed_tool_call`, `capability not supported` → `provider_unsupported`, `provider api error` → `provider_api`, `discount dropped` → `provider_discount_drop`). A failed run without such a row (storage, tool, checkpoint failures) reports the runtime's summary minus its `runtime error: ` prefix as `error`, with `error_kind` derived the same way from the `CtxError` display prefix (`storage error` → `storage`, `tool error` → `tool`, `io error` → `io`, …, else `other`). `Err` from `run` itself (storage, config) maps `CtxError::code()` into `error_kind`; a panic inside the runtime is reported as `error_kind = other`, `error = "agent run panicked"`. `error` is passed through the redactor.
- `browser/request` `{ "project_id", "session_id", "request_id": "browser_7", "action": { "action": "navigate"|"back"|"forward"|"reload"|"read"|"snapshot"|"click"|"type"|"select"|"scroll"|"evaluate"|"screenshot", "url"?, "ref"?, "text"?, "submit"?, "value"?, "direction"?, "amount"?, "script"?, "query"? } }` — a run's `browser` tool call; the client performs it in the project's embedded browser and answers with `browser/respond` (the tool waits up to 90 s). Absent action fields are `null`.
- `question/request` `{ "project_id", "session_id", "request_id": "question_3", "questions": [{ "question", "header"?, "options": [{ "label", "description"? }], "multi_select" }] }` — a run's `ask_user` tool call; the client shows the questions and answers with `question/respond`. The run waits without a timeout; `task/cancel` and `task/finished` drop the request.
- `lsp/diagnostics` `{ "project_id", "path": "src/a.rs", "diagnostics": [{"line","character","end_line","end_character","severity":"error"|"warning"|"information"|"hint","message","source":"rustc"|null,"code":"E0425"|null}] }` — the server's `textDocument/publishDiagnostics` for a document opened with `lsp/open`, 0-based; an empty list clears them.
- `mcp/status` `{ "servers": [McpServerStatus] }` — an MCP server's state or tool list changed (connected, failed, reconfigured; see **MCP servers**).
- `index/progress` `{ "project_id", "done": 120, "total": 310 }`
- `index/ready` `{ "project_id", "generation": 13, "files_indexed": 4, "files_removed": 0, "files_unchanged": 306, "duration_ms": 80, "rebuilt": false, "error": "…"|null }`
- `project/changed` `{ "project_id", "git": {…} }` — optional, when the watcher notices a branch change (may be omitted in v1).
- `log` `{ "level": "warn", "message": "…" }` — warnings the TUI would have printed (probe notices, cached profile issues), redacted (API keys, URL credentials). In `--mock` mode a probe logs `mock provider: no capability probe is performed` instead of naming the configured endpoint.

## AgentEvent reference (JSON)

Tagged with `"event"`; every variant has `task_id`.

| event | fields |
|---|---|
| `task_started` | `objective`, `continued` |
| `state_changed` | `from`, `to` ∈ `IDLE PREPARING INVESTIGATING IMPLEMENTING VALIDATING RECOVERING COMPLETED BLOCKED CANCELLED FAILED` |
| `context_compiled` | `stats: {estimated_prompt_tokens, stable_prefix_tokens, repository_snippets, repository_tokens, memory_records, memory_tokens, conversation_messages, conversation_tokens, retrieval_budget_tokens, context_window, compacted}` |
| `model_request_started` | `step`, `estimated_prompt_tokens` |
| `prompt_sent` | `step`, `messages: [{kind ∈ policy task_instructions context user assistant tool_result, text, estimated_tokens, images}]`, `tools: [name]` (not persisted; only with `task/run.options.debug_prompt`) |
| `model_text_delta` | `text` (not persisted) |
| `model_reasoning_delta` | `text` (not persisted) |
| `model_response` | `text` (nullable), `tool_call_count`, `usage` (nullable `TokenUsage`) |
| `tool_call_started` | `call_id`, `name`, `arguments` (object) |
| `tool_call_finished` | `call_id`, `name`, `success`, `summary`, `duration_ms` |
| `tool_call_output` | `call_id`, `name`, `content`, `full_len`, `is_error` (live only; emitted by the runtime) |
| `approval_requested` | `request: {id, tool, risk ∈ read_only write execute network destructive, summary, details, reason}` |
| `approval_resolved` | `request_id`, `decision ∈ approve deny approve_for_session` |
| `validation_started` | `command` |
| `validation_finished` | `record: {command, passed, exit_code, duration_ms, summary}` |
| `file_changed` | `path`, `kind ∈ created modified deleted` |
| `checkpoint` | `checkpoint_id`, `reason` |
| `warning` | `message` |
| `usage` | `cumulative: TokenUsage`, `cost_usd` (nullable) |
| `steered` | `text`, `images`, `files` (counts): a `task/steer` message reached the model, before the step it starts; it is a user message of the task from here on (`session/history` lists it) |
| `agent_started` | `agent_id`, `agent_task_id`, `name`, `mode ∈ explore implement`, `objective` |
| `agent_progress` | `agent_id`, `inner: AgentEvent` (one of the agent's own events, whose `task_id` is the agent's task; persisted when the inner event is) |
| `agent_finished` | `agent_id`, `report: {agent_id, name, mode, status, summary, modified_files[], unresolved[], steps, tool_calls, usage: TokenUsage, duration_ms}` |
| `task_completed` | `outcome: TaskOutcome` |

## Sub-agents

With `model.reasoning_effort = "ultra"` the lead agent may call the
`spawn_agents` tool (`{agents: [{name, mode: "explore"|"implement",
objective}], context?}`; at most `agent.max_agents_per_call`). Each agent is
a bounded `AgentRuntime` of its own: a fresh session of mode `agent` (hidden
from `sessions/list`, `metadata: {parent_session, parent_task, agent_id,
name, mode}`) whose task's `parent_task_id` is the lead task, the same
provider, permission profile and remembered approvals, `explore` → the
inspection tool set (no edits, no shell), `implement` → the full set minus
`spawn_agents` (agents never nest), at most `agent.max_parallel_agents`
running at once, `agent.agent_max_steps` steps each (0: no limit), no automatic
validation (the lead validates). The lead task's stream carries
`agent_started`, every agent event wrapped in `agent_progress` (tool calls,
outputs, deltas, approvals, warnings …) and `agent_finished`; the agents'
files appear as the lead's `file_changed` / `modified_files` and their usage
is folded into the lead's `usage` events. Approval request ids of an agent
are suffixed `@<agent_id>` (`approval_call_0@agent-1a2b3c4d`) so two agents
whose providers both number calls `call_0` never collide; the client maps
them back to the agent's tool row. `task/cancel` cancels the agents with
the lead.

`TokenUsage`: `{prompt_tokens, completion_tokens, cached_prompt_tokens, reasoning_tokens, source ∈ reported estimated}` — the two optional counters are always present and `null` when unknown.
`TaskOutcome`: `{task_id, status, summary, modified_files[], validation[ValidationRecord], unresolved[], steps, model_calls, tool_calls, usage, cost_usd?, duration_ms}` — `cost_usd` is **absent** (not null) when no request of the task had a price.

## Concurrent sessions

Sessions of one project share its checkout and run at the same time (one
run per session). Nothing is locked; the agents are kept from overwriting
each other's work instead (`ctx_tools::ProjectActivity`, one per
`RepositoryContext`, so every runtime of the project shares it):

- **Read before edit.** Every session remembers the content hash of each
  file it has seen: read with `read_file`, retrieved as context, returned
  by a search, or written itself. `apply_patch` and an overwriting
  `write_file` refuse a file the session has not seen ("has not been
  read in this session") and one whose content changed since ("changed
  since you last read it (edited by 'Fix login')"), naming the session
  that wrote it when that is known. The agent re-reads and rebuilds the
  edit. The optional `expected_file_hash` still applies on top.
- **Change notes.** Before every model request the runtime checks the
  files the session has seen (by modification time and size, hashing
  only those that moved) and, for each that changed, appends one
  `[Context runtime]` user message with who changed it and a short diff
  (40 lines per file, 160 in all, from content up to 64 KB kept per file).
  Each change is reported once; the edit is still refused until the file is
  read again.
- **Other sessions.** While other sessions of the project run, the task
  instructions name them with the files they wrote this run, and ask the
  agent to stay out of those files and to avoid commands that rewrite the
  whole tree (`git add -A`, `git stash`, `git checkout .`, `git reset
  --hard`, repository-wide formatters). A session's own sub-agents and its
  lead are not "other sessions"; sibling sub-agents are.
- **Rewind** restores only the session's own files (`session/rewind`).

Read state lives in the `ctx serve` process: separate processes (a `ctx`
TUI next to the app) do not see each other's sessions, though the hash
checks still catch their edits. After a restart a session reads files
again before editing them.

## Tool names

`read_file`, `search_code`, `find_symbol`, `find_references`, `list_files`,
`apply_patch`, `write_file`, `execute_command`, `run_validation`,
`git_status`, `git_diff`, `retrieve_memory`, `inspect_context`,
`web_search`, `web_fetch` (each absent when its `[web_search]` /
`[web_fetch]` table sets `enabled = false`), under Ultra effort
`spawn_agents`, and in `ctx serve` only `browser` and `ask_user` (below). Signed in to
ModelGateway, `web_search` goes through the
gateway (billed to the account) unless `web_search.provider` says otherwise.
Every tool of a connected MCP server appears as `mcp__<server>__<tool>`
(`<server>` is the `[mcp.servers.<name>]` key, both parts with characters outside
`[A-Za-z0-9_-]` replaced by `_`, the whole cut to 64 characters); the client splits
on the first `__` after the prefix to show "server › tool". Their `tool_call_*`
events are ordinary tool events; approvals carry `tool: "mcp__…"` and a summary
`call <tool> on the <server> MCP server`. An `explore` sub-agent gets only the
MCP tools their servers mark read-only.

## The browser tool

`ctx serve` gives every lead run (not sub-agents, not the TUI or headless
runs) a `browser` tool: the embedded browser of the desktop app's Inspector
*Browser* tab. The tool does no browsing itself. Each call sends one
`browser/request` notification and waits for the client's `browser/respond`
(`serve/browser.rs`, `BrowserState` per project keyed like approvals by
session and request id). The client performs the action in the project's
page **without switching the Inspector to the Browser tab**; the tab only
shows a dot and a status line while the agent is at work.

Arguments: `{action, url?, ref?, text?, submit?, value?, direction?, amount?,
script?, query?}`. `navigate {url}` (http/https only; a bare host gets
`https://`) loads a page and answers with its text (first 6 000 characters)
and its interactive elements; `read` returns the page text (15 000
characters); `snapshot` lists the visible interactive elements as
`[N] role "name" extra` lines (links show their target, fields their value,
selects their options, offscreen ones are marked; `query` filters the lines
by text, numbering is unchanged) — the client tags the elements with
`data-ctx-ref="N"` and `click {ref}`, `type {ref, text, submit?}`,
`select {ref, value}` and `scroll {ref}` act on those numbers until the next
snapshot or navigation renumbers them; `scroll {direction: up|down|top|bottom,
amount?}`; `back`, `forward`, `reload`; `evaluate {script}` runs a
JavaScript function body in the page (`return` a JSON-serializable value;
`await` allowed) and returns it; `screenshot` writes a PNG of the page to a
temporary file and returns its path. Clicks and submits wait for a
navigation they start (up to 30 s) and answer with the new page's summary.
Page dialogs (`alert`, `confirm`, `prompt`) are dismissed by the client
(confirm → Cancel) and listed at the end of the reply. The tool prepends
`Page: <title> — <url>` to every successful reply.

Policy (`ActionKind::Browser { url, private, interactive }`): `read`,
`snapshot`, `scroll`, `screenshot`, `back`, `forward` and `reload` are
read-only and always allowed. `navigate`, `click`, `type`, `select` and
`evaluate` are network actions: allowed under `workspace_autonomous` and
`bypass_permissions`, asked for under `approval_required` unless
`permissions.allow_network_commands` is on. `private` follows `web_fetch`
(LAN, link-local and `.local` addresses need the user's say-so under every
profile but bypass) **except loopback** — `localhost`, `*.localhost`,
`127.0.0.1`, `::1` count as public, since trying the user's own dev server
is what the browser is for. Interactions on one host share one approval
summary (`interact with <host> in the browser`), so "approve for session"
covers the rest of a flow; `navigate` names its URL.

## The ask_user tool

`ctx serve` also gives every lead run (not sub-agents, which are told to
decide on their own, not the TUI or headless runs) an `ask_user` tool, the
counterpart of Claude Code's and Codex's question prompts: `{questions:
[{question, header?, options?: [{label, description?}], multi_select?}]}`,
1–4 questions of at most 6 options each (option labels unique per
question, case-insensitively). The tool sends one `question/request`
(`serve/question.rs`, `QuestionState` per project keyed like approvals by
session and request id) and waits for `question/respond`; the user can
always answer in their own words, or skip. The model reads the answers as
`N. <question> → <labels; own words>` lines (or a skip note) and the tool
row's summary is "the user answered …". Policy: `ActionKind::AskUser`,
read-only and always allowed. The persisted tool call (arguments) and its
output are the history's record of the exchange.

## Remote hosts

The desktop app can run `ctx serve` on another machine over SSH, the way
Claude Code and Codex run on a remote host: the app spawns
`ssh [-p port] [-i identity] [-l user] -T <host> -- <ctx> serve` and talks
the same newline-delimited JSON-RPC over the SSH session's stdin/stdout,
one child per configured host next to the local one. Nothing in the
protocol changes for the server: it is an ordinary `ctx serve` whose
client happens to be far away. What follows from that:

- **Everything is the remote machine's.** `project/open` paths, `file/read`,
  the index, sessions, memories, `~/.context-agent` configuration, API keys
  and MCP servers are all the remote host's. `initialize` reports the
  remote `data_dir`, `model` and `host`; `config/patch` edits the remote
  configuration file. The remote `ctx` must be installed and configured
  (a key in its `config.toml` or environment, or `ctx` signed in to
  ModelGateway there) — the app never copies credentials over.
- **No shared filesystem.** The app browses folders with `fs/list` and
  sends attachments inline (`task/run` `images` as `{name, data}` and
  `files` as `{name, content}`); `session/history` lists them by name.
  Paths in events and history are remote paths, shown as such.
- **stdout is the protocol.** `ssh` adds nothing to stdout itself; the
  remote command runs under the user's shell as `$SHELL -c`, which
  sources no interactive rc files, so a `.bashrc` that prints does not
  corrupt the stream. Logs go to stderr as always and reach the app's
  backend log through the SSH session's stderr, prefixed with the host.
- **Authentication is ssh's.** The app runs `ssh` with `BatchMode=yes`:
  keys, `ssh-agent` and `~/.ssh/config` (ProxyJump, IdentityFile, User)
  work as in a terminal; a password prompt cannot be answered and fails
  the connection with ssh's message. `ServerAliveInterval` keeps idle
  sessions open and detects a dropped link, which the app reports as the
  host's backend having exited; the user reconnects from the sidebar.

## Environment

The app passes the API key to the child through the environment variable
named by `model.api_key_env` (from the macOS Keychain, else the user's login
shell environment). While signed in to ModelGateway the key comes from the
backend's own credential store instead (`model.credential`), so nothing is
passed through the environment. `CTX_DATA_DIR`, `CTX_MODEL_NAME`, `CTX_MODEL_BASE_URL`
and `CTX_MODEL_API_KEY_ENV` keep their CLI meaning.
