#!/usr/bin/env bash
# Opt-in: enabled through .claude/settings.local.example.json, not the shared settings.
# WorktreeRemove: stdin has {"worktree_path": "..."}. Removes the worktree with its build (worktree.sh refuses a dirty one),
# then the branch if it is merged (git branch -d; an unmerged branch stays).
set -euo pipefail
wt=$(jq -er .worktree_path)
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"
branch=$(git -C "$wt" rev-parse --abbrev-ref HEAD 2>/dev/null || true)
tools/worktree.sh rm "$wt" >&2
[ -z "$branch" ] || [ "$branch" = HEAD ] || git branch -d "$branch" >&2 || true
