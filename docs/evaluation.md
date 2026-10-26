# Evaluation

`ctx eval run benchmarks/tasks` runs every task under the selected variants:

| Variant | Retrieval | History |
|---|---|---|
| A | model-driven tools only | retained history, no compaction |
| B | model-driven tools only | task-aware memory and compaction |
| C | proactive path + lexical + symbol | task-aware memory |
| D | C + semantic (hashing embedder) | task-aware memory |
| E | D + progressive stages and recovery re-retrieval | adaptive |

For each task and variant the runner snapshots the fixture, records which validation commands pass beforehand, runs the agent (all turns), re-runs the validation commands independently, and records success, regression (passed before, fails after), steps, model calls, tool calls, tokens (reported vs estimated), cached tokens, cost, duration, recovery cycles, model-driven reads and searches, and retrieval recall against `expected_files`. Reports (`.json` and `.md`) carry per-variant Wilson 95% intervals; prefer non-inferiority tests against variant A before claiming improvements.

`ctx eval retrieval benchmarks/retrieval/queries.jsonl` measures recall@5, recall@10, MRR and confidence calibration over labeled queries without a model.

Tasks whose `requires_tools` are missing on the machine are reported as skipped rather than failed. `--mock` runs the harness with a provider that answers immediately, which validates the pipeline without spending tokens.
