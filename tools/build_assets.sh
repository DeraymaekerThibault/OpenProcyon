#!/usr/bin/env bash
# Import: convert the original install into the game's asset folder. Nothing from the install is modified.
# usage: tools/build_assets.sh [--game DIR] [--out DIR] [Ship_Name ...]   (default ships below; first is the default player ship)
#   --game  the original install (default: PROCYON_GAME_ROOT from .env)
#   --out   where the converted assets go (default: assets/ in this checkout; the game also finds them in
#           ${XDG_DATA_HOME:-~/.local/share}/openprocyon/assets, see assets_dir() in game/src/lib.rs)
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] && { set -a; . ./.env; set +a; }
OUT=assets
while [ $# -gt 0 ]; do case "$1" in
  --game) PROCYON_GAME_ROOT="$2"; shift 2 ;;
  --out) OUT="$2"; shift 2 ;;
  *) break ;;
esac; done
: "${PROCYON_GAME_ROOT:?pass --game <install dir> or set PROCYON_GAME_ROOT}"
[ -d "$PROCYON_GAME_ROOT/WorldObjects" ] || { echo "not a game install (no WorldObjects/): $PROCYON_GAME_ROOT" >&2; exit 1; }
export PROCYON_GAME_ROOT OUT
# Every Ship_*.wot is exported; the curated retail skirmish ships come first (first = default player ship). Hulls that are not combat
# ships (wrecks, debris) fail the exporter and are skipped.
FIRST=(Ship_Navy_Cutter Ship_Navy_Frigate Ship_Navy_FastFrigate Ship_Pirate_Schooner Ship_Pirate_Sloop Ship_Pirate_Gunboat Ship_Pirate_Barque)
if [ $# -gt 0 ]; then WANT=("$@"); else
  WANT=("${FIRST[@]}"); for f in "$PROCYON_GAME_ROOT"/WorldObjects/Ship_*.wot; do n=$(basename "$f" .wot); [[ " ${FIRST[*]} " == *" $n "* ]] || WANT+=("$n"); done
fi
mkdir -p "$OUT"/models/{ships,sky,scenery,ui}
SHIPS=()
for s in "${WANT[@]}"; do
  if err=$(python3 -m tools.export_combat "$PROCYON_GAME_ROOT" "$s" "$OUT/$s.json" 2>&1 >/dev/null); then
    scene=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["scene"])' "$OUT/$s.json" | tr -d '\r')
    [ -f "$OUT/models/ships/$scene.glb" ] || python3 -m tools.mdb_to_glb "$PROCYON_GAME_ROOT" "$scene" "$OUT/models/ships/$scene.glb" || { rm -f "$OUT/$s.json"; echo "skip $s (no mesh)"; continue; }
    SHIPS+=("$s")
  else
    rm -f "$OUT/$s.json"; echo "skip $s: ${err##*$'\n'}"  # wrecks/debris are not combat ships; anything else is an exporter bug
  fi
done
[ -f "$OUT/models/sky/skybox_07.glb" ] || python3 -m tools.mdb_to_glb "$PROCYON_GAME_ROOT" skybox_07 "$OUT/models/sky/skybox_07.glb"
for i in Island01 Island02 IslandSmall01 IslandMed01 Islandrocky1; do [ -f "$OUT/models/scenery/$i.glb" ] || python3 -m tools.mdb_to_glb "$PROCYON_GAME_ROOT" $i "$OUT/models/scenery/$i.glb"; done
printf '%s\n' "${SHIPS[@]}" | python3 -c "import sys,json;print(json.dumps([l.strip() for l in sys.stdin if l.strip()]))" > "$OUT/ships.json"
for part in COMPASS ARROW; do
  python3 -m tools.mdb_to_glb "$PROCYON_GAME_ROOT" "SELECTION_INDICATOR_${part}_SMALL" "$OUT/models/ui/SELECTION_INDICATOR_${part}_SMALL.glb"
done
python3 -m tools.export_hud "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.trim_hud_rects "$OUT"
# Waypoint/course art lives in BD_Textures (DDS), not BD_GUITextures.
python3 -m tools.export_waypoint_art "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.fix_alpha_bleed "$OUT/hud/tex"
python3 -m tools.make_slices "$OUT"
# Sounds: original Ogg Vorbis, copied with lowercase names (WOT names differ in case from the files).
# Music: one skirmish track (Red Rover's script plays BTL_NavyBig_Full02); the other maps use other tracks.
mkdir -p "$OUT/sound"
for f in "$PROCYON_GAME_ROOT"/Sound/SoundEffects/*.ogg "$PROCYON_GAME_ROOT"/Sound/GUI/*.ogg "$PROCYON_GAME_ROOT"/Sound/Music/BTL_NavyBig_Full02.ogg "$PROCYON_GAME_ROOT"/Sound/Music/FrontEndMusic.ogg; do
  n=$(basename "$f"); [ -f "$OUT/sound/${n,,}" ] || cp "$f" "$OUT/sound/${n,,}"
done
# Ship flags: converted to PNG under $OUT/flags
mkdir -p "$OUT/flags"
python3 - <<'PYEOF'
import os, pathlib
from PIL import Image
for f in pathlib.Path(os.environ["PROCYON_GAME_ROOT"], "BD_FlagTextures").glob("*.tga"):
    Image.open(f).convert("RGBA").save(pathlib.Path(os.environ["OUT"], "flags", f.stem.lower() + ".png"))
PYEOF
# Maps (World/*.twt), their scenery/skybox GLBs, music, backdrops; front-end screen layouts (Screens/*.tst) and strings.
python3 -m tools.export_maps "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_campaign "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_overlays "$PROCYON_GAME_ROOT" "$OUT"  # HUD texture overlays (credits, tutorial pages)
python3 -m tools.export_decals "$PROCYON_GAME_ROOT" "$OUT"  # hull/sail damage decals
python3 -m tools.export_nebulae "$PROCYON_GAME_ROOT" "$OUT"  # nebula/storm setups into the map files (run after export_campaign)
python3 -m tools.export_saves "$PROCYON_GAME_ROOT" "$OUT"  # mission-start campaign saves (counters from the retail SavedGames)
python3 -m tools.export_docks "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_boarding "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_asteroids "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_campaign_media "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_crew "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_voices copy "$PROCYON_GAME_ROOT" "$OUT"  # crew lines named by content/voice/events.json
# Campaign may append classes when a custom ship list was supplied.
mapfile -t SHIP_JSON < <(python3 -c 'import json, os; [print(os.path.join(os.environ["OUT"], s + ".json")) for s in json.load(open(os.path.join(os.environ["OUT"], "ships.json")))]' | tr -d '\r')  # Windows Python prints CRLF
python3 -m tools.export_effects "$PROCYON_GAME_ROOT" "$OUT" "${SHIP_JSON[@]}"
# Final step: split the raw per-ship exports into the editable data/ layout (docs/data-format.md).
python3 -m tools.pack_data "$OUT"
python3 -m tools.export_ship_names "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.export_collision "$PROCYON_GAME_ROOT" "$OUT"
python3 -m tools.format_json "$OUT"
# Regroup into one folder per ship ($OUT/ships/<Id>/) and per map/mission ($OUT/maps/<id>/); the exporters above write per-type files.
python3 -m tools.pack_layout "$OUT"
# Armed bases (island gun towers) join the ship folders; needs data/weapons from pack_data.
python3 -m tools.export_bases "$PROCYON_GAME_ROOT" "$OUT"
