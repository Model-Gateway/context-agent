# Terminal-Bench with Context

Runs `ctx` on [Terminal-Bench](https://www.tbench.ai) through
[Harbor](https://github.com/laude-institute/harbor), the harness the official
leaderboard uses, so scores compare directly with the Claude Code and Codex
entries for the same model.

## How it works

`ctx_agent.py` is a Harbor agent (`--agent-import-path ctx_agent:CtxAgent`).
For each trial it:

1. uploads a static x86_64 Linux `ctx` and a generated config to
   `/installed-agent/ctx/` in the task container;
2. runs `ctx run --json --approval auto` in the task's working directory
   (`--repo` is that directory, git or not) with the `bypass_permissions`
   profile, so file tools and commands reach the whole container, network
   commands allowed, automatic validation off, and a 2000-step limit (tasks
   allow up to 8 hours);
3. writes the outcome to `agent/outcome.json`, the event log to
   `agent/events.jsonl`, ctx's data dir (sessions, per-call finish reasons)
   to `agent/ctx-data/` and its exit code to `agent/exit_code` in the trial
   directory, and reports tokens and cost to Harbor.

The verifier then grades the container as for any other agent.

## Setup

```bash
python3 -m venv .venv && .venv/bin/pip install 'harbor[modal]'

# Static Linux ctx (from the workspace root; ~2 min, native on Apple silicon)
docker run --rm -v "$PWD":/src -v "$PWD/target/musl-cargo-home":/root/.cargo/registry \
  -w /src -e CARGO_TARGET_DIR=/src/target/linux-musl \
  ghcr.io/rust-cross/rust-musl-cross:x86_64-musl \
  cargo build --release --locked -p ctx-cli --bin ctx --target x86_64-unknown-linux-musl
```

The model key comes from `CTX_API_KEY` (the ModelGateway API key). Pass it
with `--ae CTX_API_KEY=...` or export it in the shell that runs Harbor.

## Run

Terminal-Bench 4.0 has 66 tasks; three need a GPU, so the full benchmark runs
on Modal (`modal token new` first). The leaderboard uses five trials per task.

```bash
cd benchmarks/terminalbench
PYTHONPATH=. .venv/bin/harbor run -d terminal-bench/terminal-bench@4.0.0 \
  -e modal -n 16 -k 5 \
  --agent-import-path ctx_agent:CtxAgent -m glm-5.3-flash \
  --allow-agent-host www.modelgateway.co \
  -o jobs/tb4-glm-5.3-flash
```

`--allow-agent-host` lets the two no-internet tasks (`batched-eval-parity`,
`lake-temp-glm`) reach the gateway: Harbor cannot infer a custom agent's
endpoint.

Local Docker works for single CPU tasks (images are amd64, so they run under
emulation on Apple silicon):

```bash
.venv/bin/harbor download terminal-bench/terminal-bench@4.0.0 -o tasks
PYTHONPATH=. .venv/bin/harbor run -p tasks/terminal-bench -i music-harmony \
  -e docker -n 1 -k 1 --agent-timeout-multiplier 0.125 \
  --agent-import-path ctx_agent:CtxAgent -m glm-5.3-flash -o jobs/smoke
```

Agent options (`--ak key=value`): `reasoning_effort`, `max_steps`,
`max_output_tokens` (default 65536: GLM-5.3-Flash often reasons past 16k
tokens before acting), `context_window` (default 200000, the smallest window
among the gateway's GLM-5.3-Flash upstreams), `request_timeout_secs` (default
1200), `endpoint` (another OpenAI-compatible base URL), `binary` (another
Linux `ctx` build).

Inspect results with `.venv/bin/harbor view jobs`.

## Comparison points (Terminal-Bench 4.0 leaderboard, Oct 2026)

| Model | Agent | Resolution rate |
|---|---|---|
| GLM-5.3-Flash (none) | Claude Code | 35.8% ± 3.5% |
| GLM-5.3 (max) | Claude Code | 41.8% ± 3.2% |
| Opus 5.5 (max) | Claude Code | 64.8% ± 3.1% |
