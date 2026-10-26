# Examples

- `config.toml` — an annotated configuration for a self-hosted OpenAI-compatible endpoint with workspace-autonomous permissions and explicit validation commands. Copy it to `~/.context-agent/config.toml` or pass it with `ctx --config examples/config.toml`.
- `headless-ci.sh` — how to run a task non-interactively in CI with a timeout, JSON output and stable exit codes.

Programmatic use: the `ctx-agent` crate exposes `RepositoryContext`, `SessionManager` and `AgentRuntime`; see `crates/agent/tests/runtime.rs` for a complete driver using the scripted `MockProvider`.
