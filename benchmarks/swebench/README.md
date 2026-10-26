# SWE-bench Verified with Context

Runs `ctx` on SWE-bench Verified issues and grades the patches with the official
SWE-bench harness.

## How it works

For each issue, `run_pilot.py`:

1. starts the issue's published image (`swebench/sweb.eval.x86_64.*`, run under
   emulation on Apple silicon) and copies `/testbed` to a host checkout on a
   case-sensitive disk image (`<run>/work`);
2. restarts the container with that checkout mounted at `/testbed`, so `ctx`'s
   file tools edit host files while every shell command runs inside the issue's
   environment (conda env `testbed`) through the sandbox wrapper;
3. runs `ctx run --json --approval auto` with the issue text, the user's model
   settings from `~/.context-agent/config.toml`, the `sandbox_autonomous`
   profile, and automatic validation off (the agent chooses which tests to run);
4. records the outcome and the diff (tracked changes plus new files the agent
   created with its file tools) in `<run>/results.jsonl`, and writes
   `<run>/predictions.jsonl` for the harness.

Before each issue the driver waits for the model endpoint to answer, so gateway
outages pause the run instead of wasting attempts. A re-run resumes: finished
issues are kept, errored ones start again from a fresh copy of the image.

## Setup

```bash
python3 -m venv .venv && .venv/bin/pip install swebench
cargo build --release   # from the workspace root
```

Docker must be running. The images are about 1–3 GB each.

## Run

```bash
.venv/bin/python run_pilot.py --run runs/pilot             # issues from runs/pilot/instances.json
.venv/bin/python run_pilot.py --run runs/x --only django__django-12125
./grade.sh runs/pilot                                       # official harness
```

`runs/pilot/instances.json` holds the pilot sample: 20 issues drawn from the
500 with `random.Random(42).sample(sorted(ids), 20)`.

Per-issue event logs (every tool call and model response summary) are in
`<run>/logs/`.
