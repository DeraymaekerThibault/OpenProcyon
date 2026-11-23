#!/usr/bin/env bash
# PostToolUse (Edit|Write|MultiEdit): format only the edited file. Reads the hook JSON on stdin. Silent, never fails.
f=$(jq -r '.tool_input.file_path // empty' 2>/dev/null) || exit 0
[ -f "$f" ] || exit 0
root=${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}
[ ! -d "$root/.venv/bin" ] || PATH=$root/.venv/bin:$PATH
case $f in
    *.rs) rustfmt --edition 2024 --config-path "$root/game/rustfmt.toml" "$f" >/dev/null 2>&1 ;;
    *.py) (cd "$root" && ruff format -q "$f") >/dev/null 2>&1 ;;
esac
exit 0
