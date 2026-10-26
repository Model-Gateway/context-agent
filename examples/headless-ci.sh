#!/usr/bin/env bash
# Run one task in CI. Exit codes: 0 completed, 10 blocked, 1 failed, 130 cancelled,
# 2 config error, 3 permission denied, 4 provider error, 5 timeout.
set -uo pipefail
export OPENAI_API_KEY="${OPENAI_API_KEY:?set the model API key}"
ctx run --json --approval auto --timeout 900 --new-session "$*" > outcome.json
code=$?
jq -r '.outcome | "\(.status): \(.summary)\nmodified: \(.modified_files | join(", "))"' outcome.json
exit $code
