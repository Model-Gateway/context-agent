#!/usr/bin/env bash
# Grade a run's predictions with the official SWE-bench harness.
# Usage: ./grade.sh runs/pilot [max_workers]
set -euo pipefail
cd "$(dirname "$0")"
RUN="${1:?usage: grade.sh <run-dir> [max_workers]}"
WORKERS="${2:-2}"
PRED="$RUN/predictions.jsonl"
[ -s "$PRED" ] || { echo "no predictions in $PRED"; exit 1; }
IDS=$(.venv/bin/python -c "import json,sys; print(' '.join(json.loads(l)['instance_id'] for l in open(sys.argv[1]) if l.strip()))" "$PRED")
RUN_ID="ctx-$(basename "$RUN")"
mkdir -p "$RUN/grading"
cd "$RUN/grading"
../../../.venv/bin/python -m swebench.harness.run_evaluation \
  --dataset_name SWE-bench/SWE-bench_Verified \
  --predictions_path "../predictions.jsonl" \
  --instance_ids $IDS \
  --max_workers "$WORKERS" \
  --timeout 1800 \
  --run_id "$RUN_ID"
