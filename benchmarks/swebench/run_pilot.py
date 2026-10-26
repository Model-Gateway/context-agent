#!/usr/bin/env python3
"""Run Context on SWE-bench Verified instances.

For each instance:
  1. start the instance's published Docker image and copy /testbed to a host
     checkout (on a case-sensitive volume),
  2. restart the container with that checkout mounted at /testbed, so file
     edits happen on the host and shell commands run in the container,
  3. run `ctx run` headlessly with a sandbox wrapper that executes every shell
     command inside the container,
  4. collect the resulting diff into predictions.jsonl.

Grading is done afterwards with the official harness (see README.md).
Results are appended as they finish, so the script can be re-run to resume.

Usage:
  .venv/bin/python run_pilot.py --run runs/pilot [--only ID ...] [--limit N]
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORKSPACE = HERE.parent.parent
CTX = WORKSPACE / "target" / "release" / "ctx"
DATASET = "SWE-bench/SWE-bench_Verified"
MODEL_LABEL = "context-agent"

EXEC_SCRIPT = """#!/bin/bash
# Run a command inside the SWE-bench environment (conda env "testbed").
source /opt/miniconda3/bin/activate >/dev/null 2>&1
conda activate testbed >/dev/null 2>&1
cd /testbed
exec "$@"
"""

PROMPT = """You are working in the {repo} repository, checked out at the commit where the GitHub issue below was reported. The project's dependencies are installed, and your shell commands run inside that environment (Python is available as `python`). The environment has no internet access.

Resolve the issue by changing the repository's source code. Keep the change focused on the issue. You may write and run tests or small scripts to check your fix, but do not modify existing test files. When you are done, reply with a short summary of the change.

<issue>
{problem}
</issue>
"""


def sh(args: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(args, text=True, capture_output=True, **kw)


def ensure_case_sensitive_volume(run_dir: Path) -> Path:
    """Repositories can contain paths that differ only by case; macOS volumes
    are case-insensitive by default, so checkouts live on a sparse image."""
    mount = run_dir / "work"
    if mount.is_dir() and (mount / ".case-sensitive").exists():
        return mount
    image = run_dir / "work.sparseimage"
    if not image.exists():
        r = sh(["hdiutil", "create", "-size", "80g", "-type", "SPARSE", "-fs", "Case-sensitive APFS", "-volname", "swebench-work", str(run_dir / "work")])
        if r.returncode != 0:
            sys.exit(f"hdiutil create failed: {r.stderr}")
    mount.mkdir(parents=True, exist_ok=True)
    r = sh(["hdiutil", "attach", "-nobrowse", "-mountpoint", str(mount), str(image)])
    if r.returncode != 0 and "already attached" not in r.stderr:
        sys.exit(f"hdiutil attach failed: {r.stderr}")
    (mount / ".case-sensitive").touch()
    return mount


MODEL_OVERRIDE: str | None = None
CTX_BINARY: Path = CTX


def model_settings() -> dict:
    """The user's [model] settings, with --model applied."""
    model = dict(tomllib.loads((Path.home() / ".context-agent" / "config.toml").read_text()).get("model", {}))
    if MODEL_OVERRIDE:
        model["name"] = MODEL_OVERRIDE
    return model


def write_config(run_dir: Path, container: str) -> Path:
    """Per-instance config: the user's model settings plus a sandbox wrapper
    that runs shell commands in the instance container."""
    model = model_settings()
    lines = ["[model]"]
    for key in ("name", "base_url", "api_key_env", "context_window", "max_output_tokens", "transport", "temperature"):
        if key in model:
            lines.append(f"{key} = {json.dumps(model[key])}")
    lines += [
        "request_timeout_secs = 300",
        "max_retries = 20",
        "",
        "[permissions]",
        'profile = "sandbox_autonomous"',
        "allow_network_commands = false",
        "",
        "[permissions.sandbox]",
        'kind = "wrapper"',
        f'wrapper = ["docker", "exec", "-w", "/testbed", "{container}", "/usr/local/bin/ctx-exec"]',
        "",
        "[validation]",
        "auto_detect = false",
        "commands = []",
        "",
        "[agent]",
        "max_steps = 100",
        "command_timeout_secs = 900",
        "",
        "[index]",
        "watch = false",
        "",
    ]
    path = run_dir / "configs" / f"{container}.toml"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines))
    return path


def prepare_checkout(instance: dict, work: Path, container: str) -> Path:
    """Copy /testbed out of the image, then start the long-lived container with
    the host checkout mounted in its place."""
    checkout = work / instance["instance_id"]
    image = instance["image"]
    sh(["docker", "rm", "-f", container])
    if checkout.exists():
        # A rerun starts from the pristine image, never from a partial attempt.
        subprocess.run(["rm", "-rf", str(checkout)], check=True)
    if not (checkout / ".git").exists():
        tmp = f"{container}-copy"
        sh(["docker", "rm", "-f", tmp])
        r = sh(["docker", "create", "--platform", "linux/amd64", "--name", tmp, image])
        if r.returncode != 0:
            raise RuntimeError(f"docker create failed: {r.stderr.strip()}")
        r = sh(["docker", "cp", f"{tmp}:/testbed", str(checkout)])
        sh(["docker", "rm", "-f", tmp])
        if r.returncode != 0:
            raise RuntimeError(f"docker cp failed: {r.stderr.strip()}")
    # No network: the environment is fully installed, and an agent with
    # internet access can look up the upstream fix, which invalidates the run.
    r = sh(["docker", "run", "-d", "--platform", "linux/amd64", "--network", "none", "--name", container, "-v", f"{checkout}:/testbed", "-w", "/testbed", image, "sleep", "infinity"])
    if r.returncode != 0:
        raise RuntimeError(f"docker run failed: {r.stderr.strip()}")
    r = subprocess.run(["docker", "exec", "-i", container, "bash", "-c", "cat > /usr/local/bin/ctx-exec && chmod +x /usr/local/bin/ctx-exec"], input=EXEC_SCRIPT, text=True, capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(f"installing ctx-exec failed: {r.stderr.strip()}")
    sh(["git", "-C", str(checkout), "config", "core.fileMode", "false"])
    return checkout


def collect_patch(checkout: Path, outcome: dict | None) -> str:
    """Tracked changes plus new files the agent created with its file tools.
    Untracked files made by shell commands (scratch scripts, caches) are left
    out."""
    created = [f for f in (outcome or {}).get("modified_files", []) if (checkout / f).exists()]
    untracked = set(sh(["git", "-C", str(checkout), "ls-files", "--others", "--exclude-standard"]).stdout.split())
    new_files = [f for f in created if f in untracked]
    if new_files:
        sh(["git", "-C", str(checkout), "add", "-N", "--", *new_files])
    return sh(["git", "-C", str(checkout), "-c", "core.fileMode=false", "diff", "HEAD"]).stdout


def wait_for_gateway(max_wait_s: int = 3600) -> bool:
    """Send a tiny request until the model endpoint answers 200, so an issue
    is not started (and its tokens spent) during a gateway outage."""
    import urllib.error
    import urllib.request

    model = model_settings()
    key = os.environ.get(model.get("api_key_env", ""), "")
    body = json.dumps({"model": model.get("name"), "messages": [{"role": "user", "content": "Reply with OK."}], "max_tokens": 64}).encode()
    deadline = time.time() + max_wait_s
    announced = False
    while True:
        req = urllib.request.Request(model.get("base_url", "").rstrip("/") + "/chat/completions", data=body, headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                if resp.status == 200:
                    if announced:
                        print("    gateway healthy again", flush=True)
                    return True
        except urllib.error.HTTPError as e:
            status = e.code
        except Exception as e:  # noqa: BLE001 - network errors are retried
            status = str(e)
        else:
            status = "unexpected"
        if time.time() > deadline:
            return False
        if not announced:
            print(f"    gateway unavailable ({status}); waiting", flush=True)
            announced = True
        time.sleep(60)


def load_done(path: Path) -> set[str]:
    done = set()
    if path.exists():
        for line in path.read_text().splitlines():
            if line.strip():
                rec = json.loads(line)
                if rec.get("status") not in ("error", "failed"):
                    done.add(rec["instance_id"])
    return done


def run_instance(instance: dict, run_dir: Path, work: Path, timeout: int) -> dict:
    iid = instance["instance_id"]
    container = "ctx-" + iid.replace("__", "-").lower()
    started = time.time()
    record: dict = {"instance_id": iid, "repo": instance["repo"]}
    try:
        checkout = prepare_checkout(instance, work, container)
        config = write_config(run_dir, container)
        data_dir = run_dir / "data"
        (data_dir / "providers").mkdir(parents=True, exist_ok=True)
        for p in (Path.home() / ".context-agent" / "providers").glob("*.json"):
            (data_dir / "providers" / p.name).write_text(p.read_text())
        prompt = PROMPT.format(repo=instance["repo"], problem=instance["problem_statement"].strip())
        env = dict(os.environ, CTX_SHELL="bash")
        logs = run_dir / "logs"
        logs.mkdir(exist_ok=True)
        with open(logs / f"{iid}.events.jsonl", "w") as err:
            proc = subprocess.run(
                [str(CTX_BINARY), "--config", str(config), "--data-dir", str(data_dir), "--repo", str(checkout), "run", "--json", "--approval", "auto", "--no-probe", "--new-session", "--timeout", str(timeout), prompt],
                stdout=subprocess.PIPE, stderr=err, text=True, env=env,
            )
        outcome = None
        try:
            outcome = json.loads(proc.stdout)["outcome"]
        except (json.JSONDecodeError, KeyError):
            record["error"] = f"ctx exited {proc.returncode} without an outcome"
        patch = collect_patch(checkout, outcome)
        record.update({
            "status": (outcome or {}).get("status", "error"),
            "exit_code": proc.returncode,
            "steps": (outcome or {}).get("steps"),
            "model_calls": (outcome or {}).get("model_calls"),
            "tool_calls": (outcome or {}).get("tool_calls"),
            "usage": (outcome or {}).get("usage"),
            "modified_files": (outcome or {}).get("modified_files"),
            "summary": ((outcome or {}).get("summary") or "")[:2000],
            "patch_lines": patch.count("\n"),
            "container_network": "none",
            "model_patch": patch,
        })
    except Exception as e:  # noqa: BLE001 - recorded per instance, run continues
        record.update({"status": "error", "error": str(e), "model_patch": ""})
    finally:
        sh(["docker", "rm", "-f", container])
    record["duration_s"] = round(time.time() - started, 1)
    return record


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", type=Path, default=HERE / "runs" / "pilot")
    ap.add_argument("--only", nargs="*", help="instance ids to run (default: all in instances.json)")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--timeout", type=int, default=2400, help="per-instance agent timeout in seconds")
    ap.add_argument("--model", help="model name override (default: model.name from ~/.context-agent/config.toml)")
    ap.add_argument("--ctx", type=Path, help="ctx binary to run (default: the workspace's target/release/ctx)")
    ap.add_argument("--force", action="store_true", help="run the --only issues even if they already have a result")
    args = ap.parse_args()
    global MODEL_OVERRIDE, CTX_BINARY
    MODEL_OVERRIDE = args.model
    if args.ctx:
        CTX_BINARY = args.ctx.resolve()

    if not CTX_BINARY.exists():
        sys.exit(f"build the agent first: cargo build --release ({CTX_BINARY} missing)")
    run_dir = args.run.resolve()
    run_dir.mkdir(parents=True, exist_ok=True)
    ids = args.only or json.loads((run_dir / "instances.json").read_text())

    from datasets import load_dataset  # imported late: slow

    ds = load_dataset(DATASET, split="test")
    by_id = {d["instance_id"]: d for d in ds}
    results_path = run_dir / "results.jsonl"
    done = load_done(results_path)
    todo = list(ids) if (args.force and args.only) else [i for i in ids if i not in done]
    if args.limit:
        todo = todo[: args.limit]
    print(f"{len(ids)} instance(s); {len(done)} already done; running {len(todo)}", flush=True)

    work = ensure_case_sensitive_volume(run_dir)
    for n, iid in enumerate(todo, 1):
        if not wait_for_gateway():
            print("gateway unavailable for an hour; stopping (re-run to resume)", flush=True)
            break
        print(f"[{n}/{len(todo)}] {iid} ...", flush=True)
        rec = run_instance(by_id[iid], run_dir, work, args.timeout)
        with open(results_path, "a") as f:
            f.write(json.dumps(rec) + "\n")
        print(f"    {rec['status']} in {rec['duration_s']}s, {rec.get('steps')} steps, patch {rec.get('patch_lines', 0)} lines {rec.get('error', '')}", flush=True)

    # Predictions file for the official harness: latest record per instance.
    latest: dict[str, dict] = {}
    for line in results_path.read_text().splitlines():
        if line.strip():
            rec = json.loads(line)
            latest[rec["instance_id"]] = rec
    with open(run_dir / "predictions.jsonl", "w") as f:
        for iid in ids:
            if iid in latest:
                f.write(json.dumps({"instance_id": iid, "model_name_or_path": MODEL_LABEL, "model_patch": latest[iid].get("model_patch", "")}) + "\n")
    print(f"wrote {run_dir / 'predictions.jsonl'} ({len(latest)} prediction(s))")


if __name__ == "__main__":
    main()
