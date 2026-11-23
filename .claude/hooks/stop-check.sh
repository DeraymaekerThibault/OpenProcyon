#!/usr/bin/env bash
# Stop: run the quick tier only. Silent when it passes; otherwise the last lines go to stderr and exit 2 makes Claude fix them.
# Does not block twice in a row (stop_hook_active), so a failure it cannot fix does not loop.
[ "$(jq -r '.stop_hook_active // false' 2>/dev/null)" != true ] || exit 0
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0
out=$(tools/check.sh --quick 2>&1)
case $(printf '%s\n' "$out" | tail -n 1) in
    "GATE OK"*) exit 0 ;;
esac
printf '%s\n' "$out" | tail -n 18 >&2
exit 2
