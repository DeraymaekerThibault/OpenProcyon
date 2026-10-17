#!/usr/bin/env bash
# Run every documented --script smoke with its argument set (docs/development.md) and report pass/fail per run.
# usage: tools/smokes.sh [name-filter-regex]   Screenshots and logs go to local/smokes/. Needs imported assets.
# Runs use a scratch XDG_DATA_HOME so the player's bindings.json cannot remap the keys the smokes press.
set -uo pipefail
cd "$(dirname "$0")/.."
out=local/smokes; mkdir -p "$out"
export XDG_DATA_HOME; XDG_DATA_HOME=$(mktemp -d)
trap 'rm -rf "$XDG_DATA_HOME"' EXIT
[ -d assets ] && export PROCYON_ASSETS="${PROCYON_ASSETS:-$PWD/assets}"
# name | game args (shot.sh adds --shot <png>)
smokes=$(cat <<'EOF'
briefing-skip|--mission 1 --script briefing-skip --shot-frame 180 --late-frame 0
pause|--map red_rover --script pause --shot-frame 50 --late-frame 0
pause-restart|--map red_rover --script pause-restart --shot-frame 90 --late-frame 0
sight|--script sight --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0
fleet|--map red_rover --allies Ship_Navy_Cutter,Ship_Navy_Cutter,Ship_Navy_Cutter --script fleet --shot-frame 210 --late-frame 260
fleet-primary-m3|--mission 3 --script fleet-primary --shot-frame 300 --late-frame 0
storm-m3|--mission 3 --script storm-m3 --speed 16 --shot-frame 1050 --late-frame 0
hunt-m3|--mission 3 --script hunt-m3 --speed 16 --shot-frame 140 --late-frame 0
current-m3|--mission 3 --script current-m3 --shot-frame 90 --late-frame 0
lifeboat-m1|--mission 1 --script lifeboat-m1 --shot-frame 3000 --late-frame 0
lifeboat-m2|--mission 2 --script lifeboat-m2 --shot-frame 3000 --late-frame 0
lifeboat-m2-ladyj|--mission 2 --script lifeboat-m2-ladyj --shot-frame 3000 --late-frame 0
lifeboat-m4|--mission 4 --script lifeboat-m4 --shot-frame 3000 --late-frame 0
lifeboat-m9|--mission 9 --script lifeboat-m9 --shot-frame 3000 --late-frame 0
lifeboat-kill-m5|--mission 5 --script lifeboat-kill --shot-frame 3000 --late-frame 0
fxtest|--script fxtest --shot-frame 120 --late-frame 0
victory|--script victory --shot-frame 300 --late-frame 0
defeat|--script defeat --shot-frame 300 --late-frame 0
restart|--script restart --shot-frame 300 --late-frame 0
course|--script course --shot-frame 200 --late-frame 0
target|--script target --shot-frame 200 --late-frame 0
damage|--script damage --shot-frame 300 --late-frame 0
dock|--script dock --shot-frame 600 --late-frame 0
EOF
)
cargo build --release --manifest-path game/Cargo.toml -q || exit 1
pass=0; fail=()
while IFS='|' read -r name args; do
    [[ "$name" =~ ${1:-.} ]] || continue
    # shellcheck disable=SC2086
    if timeout 600 tools/shot.sh "$out/$name.png" $args >"$out/$name.log" 2>&1; then
        pass=$((pass + 1)); echo "ok   $name"
    else
        fail+=("$name"); echo "FAIL $name (log: $out/$name.log)"
    fi
done <<<"$smokes"
echo "$pass passed, ${#fail[@]} failed${fail[*]:+: ${fail[*]}}"
[ ${#fail[@]} -eq 0 ]
