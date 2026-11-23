#!/usr/bin/env bash
# WorktreeCreate: stdin has {"name": "<slug>"}; the last stdout line must be the worktree path. worktree.sh logs to stdout, so
# everything it prints goes to stderr and only the path is printed here. The branch and the directory are named after the slug.
set -euo pipefail
name=$(jq -er .name)
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"
tools/worktree.sh new "$name" >&2
cd "../openprocyon-$name" && pwd
