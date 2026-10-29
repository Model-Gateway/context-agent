"""Harbor agent that runs Context (`ctx`) inside a Terminal-Bench task container.

The agent uploads a static x86_64 Linux `ctx` binary and a generated config
into the task's environment, then runs `ctx run --json --approval auto` in
the task's working directory under the bypass_permissions profile, so file
tools reach the whole container the way the task expects.

    harbor run -d terminal-bench/terminal-bench@4.0.0 -e modal -k 5 \\
        --agent-import-path ctx_agent:CtxAgent -m glm-5.3-flash \\
        --ae CTX_API_KEY="$CTX_API_KEY"

`-m` takes the gateway's model name (a `provider/` prefix is dropped). The key
comes from CTX_API_KEY (`--ae`, or the shell's environment).
"""

import json
import os
import shlex
from pathlib import Path
from typing import override

from pydantic import Field

from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.agents.options import InstalledAgentOptions
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

WORKSPACE = Path(__file__).resolve().parents[2]
DEFAULT_BINARY = WORKSPACE / "target" / "linux-musl" / "x86_64-unknown-linux-musl" / "release" / "ctx"

INSTALL_DIR = "/installed-agent/ctx"
CTX = f"{INSTALL_DIR}/ctx"
CONFIG = f"{INSTALL_DIR}/config.toml"
KEY_ENV = "CTX_API_KEY"


class CtxOptions(InstalledAgentOptions):
    binary: str = Field(default=str(DEFAULT_BINARY), description="Host path of the static Linux ctx binary.")
    endpoint: str = Field(default="https://www.modelgateway.co/v1", description="OpenAI-compatible model endpoint.")
    max_steps: int = Field(default=2000, description="ctx agent step limit (tasks allow hours of work).")
    # GLM-5.3-Flash can reason past 16k tokens before acting; 65k is the
    # budget Z.ai uses for its own Terminal-Bench runs. Some gateway upstreams
    # serve only 200k of context, so the window stays at that.
    max_output_tokens: int = Field(default=65536, description="Output token limit per model call.")
    context_window: int = Field(default=200_000, description="Model context window; must exceed max_output_tokens.")
    request_timeout_secs: int = Field(default=1200, description="Per-request model timeout (a long reasoning reply that isn't streamed takes minutes).")


class CtxAgent(BaseInstalledAgent):
    capabilities = AgentCapabilities(reasoning_effort=True)
    options_model = CtxOptions

    @staticmethod
    @override
    def name() -> str:
        return "ctx"

    @override
    def get_version_command(self) -> str | None:
        return f"{CTX} --version"

    @override
    def parse_version(self, stdout: str) -> str:
        return stdout.strip().removeprefix("ctx").strip()

    def _model(self) -> str:
        if not self.model_name:
            raise ValueError("pass the gateway model with -m, e.g. -m glm-5.3-flash")
        return self.model_name.split("/", 1)[-1]

    def _ctx_config(self) -> str:
        lines = [
            "[model]",
            f"name = {json.dumps(self._model())}",
            f"base_url = {json.dumps(self.options.endpoint)}",
            f'api_key_env = "{KEY_ENV}"',
            'transport = "chat_completions"',
            f"request_timeout_secs = {self.options.request_timeout_secs}",
            "max_retries = 20",
            f"max_output_tokens = {self.options.max_output_tokens}",
            f"context_window = {self.options.context_window}",
        ]
        if self.options.reasoning_effort:
            lines.append(f"reasoning_effort = {json.dumps(self.options.reasoning_effort)}")
        lines += [
            "",
            "[permissions]",
            'profile = "bypass_permissions"',
            "allow_network_commands = true",
            "",
            "[permissions.sandbox]",
            'kind = "none"',
            "",
            # The agent chooses which checks to run; the verifier grades.
            "[validation]",
            "auto_detect = false",
            "commands = []",
            "",
            "[agent]",
            f"max_steps = {self.options.max_steps}",
            "command_timeout_secs = 1800",
            "",
            "[index]",
            "watch = false",
            "",
        ]
        return "\n".join(lines)

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        binary = Path(self.options.binary).expanduser()
        if not binary.is_file():
            raise FileNotFoundError(f"no Linux ctx binary at {binary}; see benchmarks/terminalbench/README.md")
        await self.exec_as_root(environment, f"mkdir -p {INSTALL_DIR}")
        await environment.upload_file(binary, CTX)
        await self._upload_config_text(environment, content=self._ctx_config(), remote_path=CONFIG, filename="config.toml")
        # The agent user may not be root: the binary and config must be readable.
        await self.exec_as_root(environment, f"chmod 755 {INSTALL_DIR} {CTX} && chmod 644 {CONFIG} && {CTX} --version")

    @override
    @with_prompt_template
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        key = self._get_env(KEY_ENV)
        if not key:
            raise ValueError(f"set {KEY_ENV} (or pass --ae {KEY_ENV}=...) to the model gateway's API key")
        logs = self.environment_logs_dir
        # The data dir (sessions, per-call finish reasons) lands in the trial's
        # agent logs, so a run can be diagnosed after the container is gone.
        data_dir = f"{logs}/ctx-data"
        # ctx's exit code says how the task ended, not whether the harness
        # failed: record it and let the verifier grade the container.
        command = (
            f'repo="$(pwd)"; mkdir -p {data_dir}; '
            f'{CTX} --config {CONFIG} --data-dir {data_dir} --repo "$repo" '
            f"run --json --approval auto --no-probe --new-session {shlex.quote(instruction)} "
            f"> {logs}/outcome.json 2> {logs}/events.jsonl; "
            f"echo $? > {logs}/exit_code"
        )
        await self.exec_as_agent(environment, command, env={KEY_ENV: key, "CTX_SHELL": "bash"})

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        try:
            outcome = json.loads((self.logs_dir / "outcome.json").read_text())["outcome"]
        except (OSError, json.JSONDecodeError, KeyError):
            return
        usage = outcome.get("usage") or {}
        context.n_input_tokens = usage.get("prompt_tokens")
        context.n_output_tokens = usage.get("completion_tokens")
        context.n_cache_tokens = usage.get("cached_prompt_tokens")
        context.cost_usd = outcome.get("cost_usd")
        context.metadata = {
            **(context.metadata or {}),
            "ctx_status": outcome.get("status"),
            "ctx_steps": outcome.get("steps"),
            "ctx_model_calls": outcome.get("model_calls"),
            "ctx_tool_calls": outcome.get("tool_calls"),
        }
