# Game data format (what you edit)

Everything the game simulates lives in plain JSON under `assets/`, pretty-printed so it diffs and edits by hand. `tools/build_assets.sh`
regenerates it from a retail install and ends with `tools/pack_data.py`, `tools/format_json.py` and `tools/pack_layout.py` (regroups per-type exports into the ship/map folders); hand edits are lost when you regenerate,
so put changes in a mod (`docs/modding.md`). The authored HUD layout `data/hud/bar.json` is an exception; regeneration consumes it rather than replacing it.

| Path | Contents |
|---|---|
| `data/docks.json` | dock points of bases and islands (ships keep theirs in `ships/<id>/docks.json`): `pos` local game frame, `yaw` radians about z, `tender` (WOT `Is Tender`: dockable by default) (`tools/export_docks.py`) |
| `data/mesh_detail.json` | per-mesh detail settings: `levels` ([max camera distance, detail fraction]), `no_draw` distance, `collision_lod` (level whose triangles retail collides with); the empty name is the default (`tools/export_mesh_detail.py`) |
| `data/mesh_effects.json` | ambient effect points of mesh scenes (fire stacks, smoke, lighthouse, volcano fountains): `kind` PlayEffect/VolcanoSmoke, node `name`, `descriptor`, `pos` (`tools/export_mesh_effects.py`). Exported, not placed yet |
| `ships/<Id>/` | **one folder per ship**; the folder name is the ship id and every `ships/*/ship.json` is loaded (sorted by id). Mods add a folder. |
| `ships/<Id>/ship.json` | stats: `physics` (flat `Mass`, `MaxThrust`, `MaxSpeed`, ...), `core_hp`, `sections` and `guns` keyed by label, `engines`, `wakes`, `flags`, sounds/FX names, `sight` (AI `Default SightRange`), `repair_fx`, lifeboat metadata (`lifeboats` = boats left when destroyed with Create Lifeboats set, `is_lifeboat` = the class flag, `response` = collision response type, `sphere` = `{center, radius, authored}` bounding sphere in game axes; mods that omit them get 0 / false / derived bounds), `skirmish` (true = always offered on the fleet screen; an `extends` inherits it). Each section may carry `destruct` (`{fx, start, end, vel}`: its part of the final breakup, effect or null, seconds, ship-axes velocity; WOT `destructFx`), `debris` (`{model, pivot}`: the GLB of the piece that breaks off when the section is destroyed, in the hull's model space, and its pivot in game axes; WOT `debrisMeshScene`) and `debris_fx`; all optional. `geometry` and `model` name the ship folder whose `geometry.json` / `model.glb` to use (variants such as `_Feather` or `_NIS` share their base hull); `scene` is the retail mesh name |
| `ships/<Id>/geometry.json` | generated per-section hit meshes keyed by layer; do not edit |
| `ships/<Id>/model.glb` | hull mesh (only in the folder that owns it) |
| `ships/<Id>/names.json`, `docks.json` | optional: pool of English ship names open maps pick from; dock points (`tools/export_docks.py`) |
| `data/nebula.json` | weather damage potentials from `Terrain_Nebula.wot`: `lightning`, `wind`, `meteor`, each mapping material names to `[lower, upper, critical chance, fire chance, initial fire strength]`; separate from weapon bullets (`tools/export_nebulae.py`) |
| `data/asteroids.json` | ordinary asteroid type definitions: `scene`, `hp`, `mass`, `radius`, ten ordered `children` slots (empty = unused), `effect`; armed asteroids are excluded (`tools/export_asteroids.py`) |
| `data/collision.json` | generated scenery collision shapes by entity type: `detection`, `response`, `min`/`max`, optional indexed `vertices`/`indices` in **Bevy axes**. Compact JSON, not hand-authored. Ships reuse physics `MinExtents`/`MaxExtents` instead (`tools/export_collision.py`, `docs/re/collision.md`) |
| `data/weapons/guns.json` | every gun: range, rate of fire, accuracy, arcs, turret meshes, sounds; optional `Victory Point Cost` (non-negative integer, per gun), `Exclusion Race` (`None` or a race), and `Mount Type` (e.g. `Medium Mount`, `Heavy  Mount`, `Non-Selectable Special Mount`, whitespace normalised on load); ships reference them by name (`guns.<mount>.gun`). Missing cost disables purchasing rather than inventing a price |
| `data/weapons/bullets.json` | every projectile: lifetime, FX, impact sounds, damage per material; torpedoes also carry optional `physics` (`Mass`, `MaxThrust`, `MaxSpeed`, `RotationalFriction`, `MaxAngularAcceleration`) for guided flight |
| `maps/<id>/` | **one folder per map or mission**: the 14 skirmish maps, `mission01`..`mission13`, `tutorial`, `basic_tutorial` |
| `maps/<id>/map.json` | sky, music, players with fleet type and team, `teams`, pre-placed `ships` on historical maps (optional `"primary": true` = the map's Setup Ship rule makes it that player's command ship), start positions, scenery (named asteroid `group` on campaign maps; dockable scenery automatically subtracts its first converted dock's local height from the root placement, preserving authored map altitude; optional `scene_origin` in game axes overrides that datum and is subtracted after rotation, keeping meshes, guns, docks and colliders aligned; scenery without docks/override is unchanged), `world_half`, `backdrop` (retail name); `"skirmish": true` puts it in the skirmish menu |
| `maps/<id>/backdrop.png` | the map picture used by the lobby and the star map |
| `maps/<id>/script.json` | skirmish maps only: the map's own rule script (convoys, timers, team points, ...) in the `mission.json` layout below; the game runs it with the campaign rules interpreter during a skirmish. Its presence does not make the map a mission |
| `maps/<id>/mission.json` | campaign maps only: `crew_pool`/`arms_pool` (the lists the Crew/Arms screens offer), `play_end_movie`, `journal_music`; single-player ships, points, paths, volumes, tasks and rules; `groups` (TWT named groups: name -> world object ids), `players` and `alliances` (allied player pairs; every other pair starts hostile); `environment` adds animal placements/groups, nebula setup, point sets and whale settings. See `docs/re/campaign.md` |
| `maps/<id>/media.json` | campaign maps only: speech and music cues with subtitles and durations (each speech event's sound `file` and talking-head `face` come from the TWT speech list); audio files are copied only for playable missions (`PLAYABLE` in `tools/export_campaign_media.py`) |
| `sound/<kind>/*.ogg` | `weapons`, `impacts` (damage, explosions, collisions), `ships` (engines, cloak, rebuild), `ambience` (animals, storms), `music`, `ui` (HUD, front end), `speech` (dialogue shared by several missions). Referenced by bare name from the JSON; the loader finds a file by name wherever it sits (`audio::sound`), so names must be unique |
| `maps/<id>/speech/*.ogg` | dialogue only that mission uses |
| `data/boarding.json` | per Ship_/Base_/Island_ type: `points` (BoardingEffectPoint nodes, game frame) and, for ships, `crew` (WOT `Max number of` Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts); `tools/export_boarding.py`, see `docs/re/boarding.md` |
| `data/crew.json` | all 297 crew members: race, species, name, points, active skills, portrait; campaign recruitment uses `points`, assigned skills feed gunnery/repair/boarding (portrait UI remains open) |
| `data/hud/bar.json` | authored HUD bar part order and optional decorative backdrops; not overwritten by retail export |
| `hud/hud.json`, `hud/scales.json` | UI sprite rectangles and per-sheet HD scale |
| `hud/tex/bar_<skin>_<sprite>.png` | generated isolated bar sprites; regenerate with `tools/trim_hud_rects.py` after atlas changes |

Axes in the files are the game's (x starboard, y forward, z up); the loader converts to Bevy's. Numbers keep the retail units; values marked
PROVISIONAL in `docs/` are guesses. The campaign economy (points, availability, purchased crew, ship outfits and replay debt) is stored separately at `$XDG_DATA_HOME/openprocyon/campaign-points.json` (or `~/.local/share/openprocyon/campaign-points.json` when unset); it is user data, not an asset or mod.

**Campaign saves** are small JSON files `{name, mission, points, crew, arms, outfit, replay_debt}` (older files without the last two fields remain supported): shipped mission-start saves `assets/saves/missionNN.json` (13, read-only; `tools/export_saves.py` copies the counters from the retail `SavedGames/Mission NN.twt`; mission 13 has no retail snapshot and reuses mission 12's, PROVISIONAL) and the player's own in `<data dir>/openprocyon/saves/<name>.json` (written by the in-game Save page; `QuickSave.json` is F3/F4). Loading restores the counters/outfits and restarts that mission; running rule/ship state is not saved. `outfit` has `hired` (purchased crew IDs) and `ships` keyed by hull type + script name (world object ID if unnamed). Each ship has eight `crew` lists in the boarding role order and `banks` mapping bank number to gun ID. `replay_debt` offsets replayed mission rewards when a checkpoint carries purchases funded by those rewards. The automatic economy file accepts the old bare-number format and now writes an object with `points`, `crew`, `arms`, `outfit`, `replay_debt`. These remain user data, not mod files.

## Example: a faster cutter with a heavier gun
`mods/example/ships/Ship_Navy_Cutter_Fast/ship.json`:
```json
{ "extends": "Ship_Navy_Cutter", "entity": "Ship_Navy_Cutter_Fast", "physics": { "MaxSpeed": 22.0 } }
```
and a new weapon is a key in a mod's own `data/weapons/guns.json` (object files are merged key by key across mods).

## Not exported yet
Wrecked hulls, `Ship_Debris` and two ships without meshes (`Ship_Pirate_Silver`, `Ship_Procyon_RefuelingStation`); three scenery meshes
(`AsteroidXlarge`, `IslandDragon1`, `IslandDragon7`) that have no layers in the retail MSB (listed under `unresolved_scenery` in each mission);
base/asteroid turrets (`Gun_*` not used by any ship); fleet profile (`.ckb`) files; the other-language string tables.
