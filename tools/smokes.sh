#!/usr/bin/env bash
# Run the documented --script smokes with their argument sets (docs/development.md) and report pass/fail per run.
# usage: tools/smokes.sh [--jobs N] [--changed [--base REF]] [--list] [name-filter-regex]
#   --jobs N    smokes at once, each with its own display (PROCYON_RUN) and scratch XDG_DATA_HOME (default: half the cores)
#   --changed   only the smokes that the files changed against the merge base with origin/main (and uncommitted ones) map to
#               (tools/smokes.py); the battle check runs too when it is in that map
#   --list      print the selected names and stop
# Screenshots and logs go to local/smokes/. Needs imported assets. Runs use a scratch XDG_DATA_HOME (shot.sh) so the player's
# bindings.json cannot remap the keys the smokes press.
set -uo pipefail
cd "$(dirname "$0")/.."
root=$PWD
. tools/python-env.sh  # `py`: the checkout's virtualenv, as tools/check.sh
jobs=0; ujobs=; changed=; list=; base=origin/main; filter=.
while [ $# -gt 0 ]; do
    case $1 in
        --jobs) jobs=$2; ujobs=$2; shift ;;
        --changed) changed=1 ;;
        --base) base=$2; shift ;;
        --list) list=1 ;;
        *) filter=$1 ;;
    esac
    shift
done
out=$root/local/smokes; mkdir -p "$out"
# The scratch XDG_DATA_HOME hides the per-user import: pass on the one every tool finds (tools/import_assets.py find_assets).
[ -n "${PROCYON_ASSETS:-}" ] || { a=$($py -m tools.import_assets --print-assets); [ -z "$a" ] || export PROCYON_ASSETS="$a"; }
# name | kind | game args (shot.sh adds --shot <png>)
# kind: asserts = the script logs a PASS line when its checks held and the run fails without it (tools/smokes.py);
#       visual  = exit status 0 and a picture that is not black; whatever the script asserts with panics shows as a crash.
smokes=$(cat <<'EOF2'
briefing-skip|visual|--mission 1 --script briefing-skip --shot-frame 180 --late-frame 0
pause|visual|--map red_rover --script pause --shot-frame 50 --late-frame 0
pause-restart|visual|--map red_rover --script pause-restart --shot-frame 90 --late-frame 0
sight|visual|--script sight --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0
fleet|asserts|--map red_rover --allies Ship_Navy_Cutter,Ship_Navy_Cutter,Ship_Navy_Cutter --script fleet --shot-frame 210 --late-frame 260
fleet-primary-m3|asserts|--mission 3 --script fleet-primary --shot-frame 300 --late-frame 0
storm-m3|asserts|--mission 3 --script storm-m3 --speed 16 --shot-frame 1050 --late-frame 0
hunt-m3|asserts|--mission 3 --script hunt-m3 --speed 16 --shot-frame 140 --late-frame 0
current-m3|asserts|--mission 3 --script current-m3 --shot-frame 90 --late-frame 0
lifeboat-m1|asserts|--mission 1 --script lifeboat-m1 --shot-frame 3000 --late-frame 0
lifeboat-m2|asserts|--mission 2 --script lifeboat-m2 --shot-frame 3000 --late-frame 0
lifeboat-m2-ladyj|asserts|--mission 2 --script lifeboat-m2-ladyj --shot-frame 3000 --late-frame 0
lifeboat-m4|asserts|--mission 4 --script lifeboat-m4 --shot-frame 3000 --late-frame 0
lifeboat-m9|asserts|--mission 9 --script lifeboat-m9 --shot-frame 3000 --late-frame 0
lifeboat-kill-m5|asserts|--mission 5 --script lifeboat-kill --shot-frame 3000 --late-frame 0
mods|visual|--menu mods --shot-frame 40 --late-frame 0
fxtest|visual|--script fxtest --shot-frame 120 --late-frame 0
victory|visual|--script victory --shot-frame 300 --late-frame 0
defeat|visual|--script defeat --shot-frame 300 --late-frame 0
restart|visual|--script restart --shot-frame 300 --late-frame 0
course|visual|--script course --shot-frame 200 --late-frame 0
target|visual|--script target --shot-frame 200 --late-frame 0
damage|visual|--script damage --shot-frame 300 --late-frame 0
dock|visual|--script dock --shot-frame 600 --late-frame 0
EOF2
)
battles=
if [ -n "$changed" ]; then
    plan=$($py -m tools.smokes plan --base "$base") || exit 1
    echo "smokes: changed files map to: ${plan:-nothing (no game change)}"
    case " $plan " in *" all "*) filter=.; battles=1 ;;
        *) [ -z "$plan" ] && exit 0
           filter="^($(echo "$plan" | tr ' ' '\n' | grep -vx battles | paste -sd'|'))\$"
           case " $plan " in *" battles "*) battles=1 ;; esac ;;
    esac
else
    battles=
fi
selected=$(while IFS= read -r line; do [[ "${line%%|*}" =~ $filter ]] && echo "$line"; done <<<"$smokes")
[ -z "$list" ] || { cut -d'|' -f1 <<<"$selected"; [ -z "$battles" ] || echo battles; exit 0; }
if [ -n "$selected" ]; then
    n=$(wc -l <<<"$selected")
    [ "$jobs" -gt 0 ] || jobs=$(( $(nproc 2>/dev/null || echo 2) / 2 ))
    [ "$jobs" -ge 1 ] || jobs=1; [ "$jobs" -le "$n" ] || jobs=$n
    # Build once and run the binary directly (shot.sh builds only when PROCYON_BIN is not set).
    cargo build --profile fast --manifest-path game/Cargo.toml -q || exit 1
    export PROCYON_BIN="${CARGO_TARGET_DIR:-$root/game/target}/fast/openprocyon"
    # One run mode for every smoke (PROCYON_RUN, tools/rungame.py); PROCYON_JOBS bounds each software renderer's threads.
    mode=$($py -m tools.rungame mode) || exit 2
    echo "smokes: run mode $mode (PROCYON_RUN), $jobs at once"
    export PROCYON_JOBS=$jobs
else
    jobs=1
fi
export out root py

# One smoke: logs to $out/<name>.log, leaves $out/<name>.result (ok|FAIL) and prints one line (more on failure) in one write.
run_one() {
    IFS='|' read -r name kind args <<<"$1"
    local start=$SECONDS code why msg
    rm -f "$out/$name.result"
    # Its own mod overlay: the checkout's shared one would be rebuilt by every parallel run under the others.
    export PROCYON_OVERLAY="$out/overlay/$name"
    # shellcheck disable=SC2086
    timeout 600 "$root/tools/shot.sh" "$out/$name.png" $args >"$out/$name.log" 2>&1; code=$?
    if why=$($py -m tools.smokes verdict "$kind" "$code" "$out/$name.log"); then
        echo ok >"$out/$name.result"
        msg=$(printf 'ok   %-18s %-7s %3ds' "$name" "$kind" $((SECONDS - start)))
    else
        echo FAIL >"$out/$name.result"
        msg=$(printf 'FAIL %-18s %-7s %3ds %s\n    rerun: tools/shot.sh local/smokes/%s.png %s   (log: local/smokes/%s.log)' \
            "$name" "$kind" $((SECONDS - start)) "$why" "$name" "$args" "$name")
    fi
    printf '%s\n' "$msg"
}
export -f run_one

start=$SECONDS
if [ -n "$selected" ]; then
    printf '%s\n' "$selected" | xargs -d '\n' -P "$jobs" -I{} bash -c 'run_one "$1"' _ {}
fi
pass=0; fail=()
while IFS='|' read -r name _; do
    [ -n "$name" ] || continue
    if [ "$(cat "$out/$name.result" 2>/dev/null)" = ok ]; then pass=$((pass + 1)); else fail+=("$name"); fi
done <<<"$selected"
status=0
if [ -n "$battles" ]; then
    $py -m tools.battle_check ${ujobs:+--jobs "$ujobs"} || status=1
fi
summary="smokes: $pass ok, ${#fail[@]} FAIL${fail[*]:+: ${fail[*]} (logs: local/smokes/<name>.log)} in $((SECONDS - start))s (jobs $jobs)"
echo "$summary"
[ ${#fail[@]} -eq 0 ] && [ "$status" -eq 0 ]
