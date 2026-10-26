#!/usr/bin/env bash
# Creates a throwaway git repository for browser-preview checks (never point
# the preview at a real working tree: the Changes tab stages, commits and
# discards for real). Usage: scripts/make-demo-repo.sh <new-dir>
set -euo pipefail
dest="${1:?usage: make-demo-repo.sh <new-dir>}"
here="$(cd "$(dirname "$0")/../../.." && pwd)"
src="$here/tests/repositories/polyglot"
if [ -e "$dest" ]; then echo "refusing to overwrite $dest" >&2; exit 1; fi
mkdir -p "$dest"
(cd "$src" && tar --exclude='target' --exclude='__pycache__' -cf - .) | (cd "$dest" && tar -xf -)
cd "$dest"
git init -q -b main
git config user.name "Demo"
git config user.email "demo@example.invalid"
git add -A
git commit -q -m "Initial import of the polyglot service"
printf '\n## Deployment\n\nRun `make deploy` after the tests pass.\n' >> README.md
git commit -q -am "Document deployment"
git checkout -q -b feature/rate-limits
git checkout -q main
# Working-tree changes: one staged, one modified, one untracked, one deleted.
sed -i.bak 's/def handle_request/def handle_request_v2/' services/api/app.py && rm -f services/api/app.py.bak
git add services/api/app.py
printf '\nexport const RETRY_LIMIT = 3\n' >> web/index.mjs
printf 'Release notes\n=============\n\n- Rate limits per client\n' > CHANGELOG.md
git rm -q --cached docs/ARCHITECTURE.md >/dev/null && rm -f docs/ARCHITECTURE.md
echo "$dest"
