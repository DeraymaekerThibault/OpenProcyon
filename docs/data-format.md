# Game data format (what you edit)

Everything the game simulates lives in plain JSON under `assets/`, pretty-printed so it diffs and edits by hand. `tools/build_assets.sh`
regenerates it from a retail install and ends with `tools/pack_data.py`, `tools/format_json.py` and `tools/pack_layout.py` (regroups per-type exports into the ship/map folders); hand edits are lost when you regenerate,
so put changes in a mod (`docs/modding.md`). The authored HUD layout `data/hud/bar.json` is an exception; regeneration consumes it rather than replacing it.

Short form first (paths by area), the full description of every path under "Details by path" below.

**Import**

| Path | What it is |
|---|---|
| `import.json` | Written by the import screen: the game version and install folder of the last import |
| `importer.json` | Written by the importer: layout version of the files (`format`); the game ignores it |

**Ships**

| Path | What it is |
|---|---|
| `ships/<Id>/` | One folder per ship; the folder name is the ship id |
| `ships/<Id>/ship.json` | Ship stats, sections, guns, engines, crew and sound/FX names (keys below) |
| `ships/<Id>/geometry.json` | Generated per-section hit meshes; do not edit |
| `ships/<Id>/model.glb` | Hull mesh with its hardpoint nodes |
| `models/scenery/SpaceDragon.glb`, `SpaceWhale.glb` | The two animals: skinned mesh plus looping clip |
| `ships/<Id>/names.json`, `docks.json` | Optional: ship-name pool for open maps; dock points |
| `textures/<retail texture name>.png` | Mods only (not in an import; not retail): replaces the retail texture of that name (file name without `.tga`, any case) inside every `ships/` and `models/` model that uses it, any resolution. Never placed in the overlay; the models that use it are rewritten there. `textures/surface/` is a different thing (baked surface maps) |

**Weapons and AI**

| Path | What it is |
|---|---|
| `data/weapons/guns.json` | Every gun: range, rate, accuracy, arcs, meshes, sounds, optional cost/race/mount |
| `data/ai/weapons.json` | Optional, mods: the guns computer captains may fit |
| `data/weapons/bullets.json` | Every projectile: lifetime, FX, impact sounds, damage per material |
| `data/boarding.json` | Boarding points per type and crew maxima per ship |
| `data/crew.json` | All 297 crew members: race, species, name, points, skills, portrait |

**Maps and missions**

| Path | What it is |
|---|---|
| `maps/<id>/` | One folder per map or mission |
| `maps/<id>/map.json` | Sky, music, players, ships, scenery, lighting, victory flags |
| `maps/<id>/backdrop.png` | The map picture for the lobby and star map |
| `maps/<id>/script.json` | Skirmish maps only: the map's own rule script |
| `maps/<id>/mission.json` | Campaign maps only: pools, journal, ships, paths, tasks, rules |
| `maps/<id>/media.json` | Speech, messages and ship names of a mission (required for missions) |
| `maps/<id>/speech/*.ogg` | Dialogue only that mission uses |

**World data**

| Path | What it is |
|---|---|
| `data/docks.json` | Dock points of bases and islands |
| `data/crew_nodes.json` | Optional, mods only: crew places of map scenery by type |
| `data/mesh_detail.json` | Per-mesh detail levels, no-draw distance, collision LOD |
| `data/animations.json` | Mesh scene animation clips |
| `data/mesh_effects.json` | Ambient effect points of mesh scenes (fires, smoke, volcanoes) |
| `data/island_sounds.json` | Islands with an ambient loop and how far it carries |
| `data/nebula.json` | Weather damage potentials (lightning, wind, meteor) |
| `data/asteroids.json` | Ordinary asteroid types: scene, hp, mass, radius, children, effect |
| `data/collision.json` | Scenery collision shapes by entity type (Bevy axes) |

**Sound**

| Path | What it is |
|---|---|
| `sound/<kind>/*.ogg` | Sound files by kind, referenced by bare name |

**HUD and UI**

| Path | What it is |
|---|---|
| `data/hud/bar.json` | Authored HUD bar part order and backdrops (not overwritten by import) |
| `data/hud/slices.json` | The 9-slice groups |
| `data/hud/speed.json` | Optional, mod-only: turns on game-speed buttons |
| `data/camera.json` | Optional, mod-only: turns on the free camera |
| `hud/hud.json` | Layout offsets, timings and font names of the retail HUD |
| `hud/sprites/<component>/<skin>/<state>.png` | The art of one component, art set and cursor state |
| `hud/sprites/<component>/<skin>.json` | Sizes and trims of one component's sprites |
| `hud/bar/<skin>.png`, `hud/bar/<skin>.json` | The composed static main bar and its part boxes |
| `hud/scales.json` | HD factor of the 9-slice strips |
| `menu/shipscreen.png` | Exported but unused |
| `hud/tex/*.png` | Waypoint line art and the 9-slice strips |

### Details by path

**Import**

- `import.json`: written by the game's import screen after a successful import: `version` (the game version that imported) and `game` (the install folder). A player's per-user folder (`%APPDATA%\openprocyon\assets`, `~/.local/share/openprocyon/assets`) is only used when `version` is the running game's; otherwise the import screen opens and converts the game again, into `assets.importing` beside it, replacing `assets` only when it succeeds. `PROCYON_ASSETS` and the checkout's `assets/` are used without it (`game/src/setup.rs`)
- `importer.json`: written by the importer: `format`, the layout version of the files it writes (raised when a file's meaning or location changes). The game does not read it. All exported JSON is written the same way (indent 2, number lists on one line, final newline; `tools/jsonio.py`)

**Ships**

- `ships/<Id>/`: **one folder per ship**; the folder name is the ship id and every `ships/*/ship.json` is loaded (sorted by id). Mods add a folder.
- `ships/<Id>/ship.json`: stats: `physics` (flat `Mass`, `MaxThrust`, `MaxSpeed`, ...), `core_hp`, `sections` and `guns` keyed by label, `engines`, `wakes`, `flags`, sounds/FX names, `sight` (AI `Default SightRange`), `repair_fx`, lifeboat metadata (`lifeboats` = boats left when destroyed with Create Lifeboats set, `is_lifeboat` = the class flag, `response` = collision response type, `sphere` = `{center, radius, authored}` bounding sphere in game axes; mods that omit them get 0 / false / derived bounds), `skirmish` (true = offered on the fleet screen; the importer copies retail's "Available in Multiplayer?" flag; an `extends` inherits it). Each section may carry `destruct` (`{fx, start, end, point}`: its part of the final breakup, effect or null, seconds, burst point in the section's frame in game axes; WOT `destructFx`), `debris` (`{model, pivot}`: the GLB of the piece that breaks off when the section is destroyed, in the hull's model space, and its pivot in game axes; WOT `debrisMeshScene`) `debris_fx` and `adjacent` (layer names of the neighbouring sections a destroyed section passes 0.4 of a hit on to; WOT `adjacent`); all optional. `geometry` and `model` name the ship folder whose `geometry.json` / `model.glb` to use (variants such as `_Feather` or `_NIS` share their base hull); `scene` is the retail mesh name
- `ships/<Id>/geometry.json`: generated per-section hit meshes keyed by layer; do not edit. A mod's ship may omit it: the loader then builds the hit meshes from `model.glb` (docs/modding.md "A ship from a model")
- `ships/<Id>/model.glb`: hull mesh (only in the folder that owns it); its hardpoint nodes (`extras.kind`: `GunPlacement`, `EnginePortPlacement`, `WakePlacement`, `FlagAttachmentPoint`, `BoardingEffectPoint`, `ToweePoint`, `TowerPoint`, `TorpedoHomingPoint`) are what a mod ship's `ship.json` may leave out
- `models/scenery/SpaceDragon.glb`, `SpaceWhale.glb`: the animals: one mesh skinned to the scene skeleton (`JOINTS_0`/`WEIGHTS_0`, a skin) and its looping clip as glTF animation 0 (`tools/skin.py`, docs/re/effects.md "Animal skeletons"). Older imports without the skin still load, unanimated
- `ships/<Id>/names.json`, `docks.json`: optional: pool of English ship names open maps pick from; dock points (`tools/export_docks.py`)

**Weapons and AI**

- `data/weapons/guns.json`: every gun: range, rate of fire, accuracy, arcs, turret meshes, sounds; optional `Victory Point Cost` (non-negative integer, per gun), `Exclusion Race` (`None` or a race), and `Mount Type` (e.g. `Medium Mount`, `Heavy  Mount`, `Non-Selectable Special Mount`, whitespace normalised on load), and `EffectiveRange_RicochetFactor` / `LongRange_RicochetFactor` / `MaximumRange_RicochetFactor` (numbers, 0 when absent; `docs/re/gunnery.md` Ricochet); ships reference them by name (`guns.<mount>.gun`). Missing cost disables purchasing rather than inventing a price
- `data/ai/weapons.json`: optional, mod-supplied: the guns computer captains may fit when a computer fleet is built, `{"Gun_Name": true|false}` (merged key by key across mods like `guns.json`). Absent: the guns retail computers were seen to fit (light/medium/heavy carronades, Gattling, light/medium/heavy plasma, light/medium cannons, star mortar; PROVISIONAL, `docs/re/skirmish-setup.md`)
- `data/weapons/bullets.json`: every projectile: lifetime, FX, impact sounds, damage per material; torpedoes also carry optional `physics` (`Mass`, `MaxThrust`, `MaxSpeed`, `RotationalFriction`, `MaxAngularAcceleration`) for guided flight
- `data/boarding.json`: per Ship_/Base_/Island_ type: `points` (BoardingEffectPoint nodes, game frame) and, for ships, `crew` (WOT `Max number of` Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts); `tools/export_boarding.py`, see `docs/re/boarding.md`
- `data/crew.json`: all 297 crew members: race, species, name, points, active skills, portrait; campaign recruitment uses `points`, assigned skills feed gunnery/repair/boarding (portrait UI remains open)

**Maps and missions**

- `maps/<id>/`: **one folder per map or mission**: the 14 skirmish maps, `mission01`..`mission13`, `tutorial`, `basic_tutorial`
- `maps/<id>/map.json`: sky, music, players with fleet type and team, `teams`, pre-placed `ships` on historical maps (optional `"primary": true` = the map's Setup Ship rule makes it that player's command ship), start positions, scenery (named asteroid `group` on campaign maps; dockable scenery automatically subtracts its first converted dock's local height from the root placement, preserving authored map altitude; optional `scene_origin` in game axes overrides that datum and is subtracted after rotation, keeping meshes, guns, docks and colliders aligned; scenery without docks/override is unchanged), `world_half`, `backdrop` (retail name); `mines` (`[{type, id, pos}]`: star mines the map places before the battle, Border Dispute and Red Rover; the map script hands each field to a player; optional; engine-added: a custom mission's `map.json` may list them too, retail missions' are never read); `"skirmish": true` puts it in the skirmish menu; `lighting` (ambient, floor and roof colours, roof light direction; the game uses the roof light as the sun and the floor colour as ambient, PROVISIONAL), `alliance_change`, `islands_make_sounds`, player `color`; `objects[].rot` is nine numbers (a 3x3 matrix, second triple = forward). In a file that `extends` another, the lists `objects`, `ships`, `mines` (by `id`), `nebulae` and `players` (by `name`) merge by key instead of replacing (`docs/modding.md` "Changing one entry of a list"). A custom mission's `map.json` is read for `name`, `description`, `sky`, `lighting`, `world_half`, `backdrop` (any string: the picture is `maps/<id>/backdrop.png`), `objects`, `nebulae` and `mines` only; its `music` and `players` are not used (`docs/missions.md` section 2)
- `maps/<id>/backdrop.png`: the map picture used by the lobby and the star map
- The keys of `map.json` above are described by `map.schema.json` (`--dump-schemas`, `docs/modding.md` "JSON schemas"), generated from `data::MapDoc`, the typed view of the file: every key optional, unknown keys allowed, a value of the wrong type counts as absent (as before). PROVISIONAL shapes in it: `backdrop` (`true` or a name), `teams[].lock`, `islands_make_sounds`, `name_id` / `desc_id` (exported, not read), and the `victory[].if` condition records, which list only the keys the engine reads. Readers on `MapDoc`: the custom mission list (`name`, `description`), the skirmish list (`skirmish`, `name`, `assemble`, playable `players`), `--check-mission`'s `world_half` and `backdrop` checks; the others still read the JSON value.
- `maps/<id>/script.json`: skirmish maps only: the map's own rule script (convoys, timers, team points, ...) in the `mission.json` layout below; the game runs it with the campaign rules interpreter during a skirmish and applies its actions with the same code. Its presence does not make the map a mission. Engine-added (not retail): the top-level `"victory": "script"` (or `"objectives"`) lets the script's `End Game` / `Team X Wins` / `Remaining Team Wins` rules and per-team `tasks` (`team`) decide the game; without it the end rules stay with `map.json` `victory` (`docs/modding.md`, "Skirmish map scripts")
- `maps/<id>/mission.json`: campaign maps only: `crew_pool`/`arms_pool` (the lists the Crew/Arms screens offer), `play_end_movie`, `journal_music`, `journal` (`title_id` and pages of `text_id`, `speech`, `picture`; the media file has the texts and durations); single-player ships, points, paths, volumes, tasks and rules; `groups` (TWT named groups: name -> world object ids), `players` and `alliances` (allied player pairs; every other pair starts hostile); `environment` adds animal placements/groups, nebula setup, point sets and whale settings. See `docs/re/campaign.md`; the guide for writing your own, with the keys a custom mission reads (`ships[]` `id`/`type`/`group`/`name`/`primary`/`pos`/`rot`, `player_start`, `landmarks` and the retail-only fields are not read), is `docs/missions.md`
- `maps/<id>/media.json`: campaign maps and custom missions (required: the game stops at the start without it); a skirmish map may carry one too (engine-added, optional: its script's speech lines, messages and ship names): `speech` (`{EVENT: {text, duration, file, face, color}}`: subtitle, seconds held (the engine adds 0.3 s; no `duration` = text only), sound file name, talking-head sprite, subtitle tint; each retail event's `file` and `face` come from the TWT speech list; the retail export also writes `speaker`/`head`, which are ignored), `messages` (end-of-mission texts by id) and `ship_names`; audio files are copied only for playable missions (`PLAYABLE` in `tools/export_campaign_media.py`). Details and a minimal file: `docs/missions.md` section 5
- `maps/<id>/speech/*.ogg`: dialogue only that mission uses

**World data**

- `data/docks.json`: dock points of bases and islands (ships keep theirs in `ships/<id>/docks.json`): `pos` local game frame, `yaw` radians about z, `tender` (WOT `Is Tender`: dockable by default) (`tools/export_docks.py`)
- `data/crew_nodes.json`: optional, a mod's (engine extension, no retail counterpart): `{"<scenery type>": [{"name": "Crew_Walk_A_0", "pos": [x, y, z], "rot": [..]}]}`, the deck-crew places a scenery model would carry as `Crew_*` nodes (grammar in `docs/modding.md` "A ship from a model"); `pos`/`rot` in the scenery's own frame, game axes, as a gun's; same-named model nodes are replaced, others added; mods merge it key by key
- `data/mesh_detail.json`: per-mesh detail settings: `levels` ([max camera distance, detail fraction]), `no_draw` distance, `collision_lod` (level whose triangles retail collides with); the empty name is the default (`tools/export_mesh_detail.py`)
- `data/animations.json`: mesh scene animation clips: `{scene: [{name, duration, motions: [{node, target, channels}]}]}`, six channels per motion (x, y, z, yaw, pitch, roll in radians), keyframe `[time, value, smoothing, tension, continuity, bias, in tangent, out tangent]` (`tools/export_animations.py`, `tools/msb.py parse_scene`). The animals' clips are also baked into their GLBs (below)
- `data/mesh_effects.json`: ambient effect points of mesh scenes (fire stacks, smoke, lighthouse, volcano fountains): `kind` PlayEffect/VolcanoSmoke, node `name`, `descriptor`, `effect` (the effect file the world object's attribute names; null for VolcanoSmoke), `pos` (`tools/export_mesh_effects.py`). The effect files are in `fx/`. Every point is placed by the game
- `data/island_sounds.json`: islands with an ambient loop (the volcanoes): `sound` name (in `sound/ambience/`), `distance` it carries (`tools/export_island_sounds.py`). The game plays the loop from the island, fading with the camera distance (volume PROVISIONAL)
- `data/nebula.json`: weather damage potentials from `Terrain_Nebula.wot`: `lightning`, `wind`, `meteor`, each mapping material names to `[lower, upper, critical chance, fire chance, initial fire strength]`; separate from weapon bullets (`tools/export_nebulae.py`); each `maps/<id>/map.json` `nebulae` entry also has `groups`: the script groups that hold the nebula's world object (rules move a storm by its group's name, which can differ from the nebula's own name; additive field)
- `data/asteroids.json`: ordinary asteroid type definitions: `scene`, `hp`, `mass`, `radius`, `acceleration` (m/s^2, the push on a belt rock), ten ordered `children` slots (empty = unused), `effect`; armed asteroids are excluded (`tools/export_asteroids.py`)
- `data/collision.json`: generated scenery collision shapes by entity type: `detection`, `response`, `min`/`max`, optional indexed `vertices`/`indices` in **Bevy axes**. Compact JSON; a mod adds an entry per new type (merged, `docs/modding.md`): `vertices`/`indices` are the triangles shells and line of fire hit, `layers` (`[{offset, nodes: [{c, a, h, l, k}]}]`: centre, three axes, half sizes, level, child indices; node 0 the root) the box tree ships are pushed out of, levels past 6 are not descended. Ships reuse physics `MinExtents`/`MaxExtents` instead (`tools/export_collision.py`, `docs/re/collision.md`)

**Sound**

- `sound/<kind>/*.ogg`: `weapons`, `impacts` (damage, explosions, collisions), `ships` (engines, cloak, rebuild), `ambience` (animals, storms), `music`, `ui` (HUD, front end), `speech` (dialogue shared by several missions). Referenced by bare name from the JSON; the loader finds a file by name wherever it sits (`audio::sound`), so names must be unique

**HUD and UI**

- `data/hud/bar.json`: authored HUD bar part order and optional decorative backdrops; input of `tools/compose_bar.py` at import (the game reads only `hud/bar/`); not overwritten by retail export
- `data/hud/slices.json`: the 9-slice groups: `{"<group>": {"left", "center"?, "right", "states"?, "caps"?}}` (component names, cursor states, end widths in reference px); input of `tools/make_slices.py` and read by the game for the caps. A mod adds a group by overlaying it (see `docs/modding.md`)
- `data/hud/speed.json`: optional, mod-only (`mods/game-speed`): `{"max": 16}` turns on the not-retail game-speed buttons; absent, the HUD has none
- `data/camera.json`: optional, mod-only (`mods/free-camera`): `{"pitch": [0.05, 1.5]}` turns on the not-retail free camera (right-drag tilts between those pitches); absent, the camera is the retail one, whose tilt follows the zoom (`docs/re/camera.md`)
- `hud/hud.json`: the retail `hud.hdt` components without their art: layout offsets (`TopLeft X/Y`, gauge and frame offsets), timings, font names. The game reads offsets and numbers from here
- `hud/sprites/<component>/<skin>/<state>.png`: the art of one component in one art set (`Navy`, `Pirate`, `Procyon`) and cursor state (`NoCursor`, `CursorOver`, `CursorDown`, `Disabled`), with the fully transparent rows and columns cut off all four sides. A state that is drawn exactly like `NoCursor` has no file; the game uses `NoCursor`. Replace a file at any resolution (see "UI art" in `docs/modding.md`)
- `hud/sprites/<component>/<skin>.json`: `{"TopLeft X": .., "TopLeft Y": .., "states": {"NoCursor": {"size": [w, h], "trim": [left, top, right, bottom]}, ..}}`: `size` is the untrimmed reference size (the hud.hdt rectangle, which layout always uses) and `trim` the reference pixels cut off each edge, so the PNG is `size - trim` reference pixels of art; a sprite with no visible pixel is a 1x1 transparent file with `trim` covering the rest. `TopLeft` is a copy for reference (the game reads `hud/hud.json`). Written by `tools/export_hud.py` (layout in `tools/hud_sprites.py`)
- `hud/bar/<skin>.png`, `hud/bar/<skin>.json`: the static main bar of one art set, composed at import by `tools/compose_bar.py` from the `hud/sprites/` art and `data/hud/bar.json`: one image with every part's idle art butted exactly, and `{"width", "height", "parts": [{"sprite", "x", "width", "height"}]}` in reference pixels (the part boxes the game puts buttons, gauge fills and hover art on; the order is the order of `bar.json`). The game draws only these two files for the bar; a mod can ship a redesigned pair
- `hud/scales.json`: HD factor of the 9-slice strips only (`hud/tex/slice_*.png`); sprites need none, their scale is their PNG size over their reference size
- `menu/shipscreen.png`: exported for compatibility but unused: the Ships / Crew / Arms panels are drawn by the engine over `menu/gamelobby.png`
- `hud/tex/*.png`: waypoint line art and the 9-slice strips `slice_<group>_<skin>_<state>.png` of `data/hud/slices.json` (`tools/make_slices.py`). The 240 retail UI atlases are no longer written

Axes in the files are the game's (x starboard, y forward, z up); the loader converts to Bevy's. Numbers keep the retail units; values marked
PROVISIONAL in `docs/` are guesses. The campaign economy (points, availability, purchased crew, ship outfits and replay debt) is stored separately at `$XDG_DATA_HOME/openprocyon/campaign-points.json` (or `~/.local/share/openprocyon/campaign-points.json` when unset); it is user data, not an asset or mod.

**Skirmish profiles** (the fleet screen's Load / Save Fleet, Crew and Arms) are JSON files `{"ships": [{"class", "crew", "arms"}]}` in `<data dir>/openprocyon/profiles/{fleets,crews,arms}/<name>.json`: `class` is the hull's class name, `crew` the eight post lists (boarding role order) of named crew IDs, `arms` the guns swapped in by bank number. A fleet file holds only the hulls (`class`; they load with stock guns and no crew, and older files' `crew`/`arms` are ignored); a crew file only `crew` and an arms file only `arms` for one ship, loadable onto any ship of that hull class. User data, not mod files.

**Campaign saves** are small JSON files `{name, mission, points, crew, arms, outfit, replay_debt}` (older files without the last two fields remain supported): shipped mission-start saves `assets/saves/missionNN.json` (13, read-only; `tools/export_saves.py` copies the counters from the retail `SavedGames/Mission NN.twt`; mission 13 has no retail snapshot and reuses mission 12's, PROVISIONAL) and the player's own in `<data dir>/openprocyon/saves/<name>.json` (written by the in-game Save page; `QuickSave.json` is F3/F4). Loading restores the counters/outfits and restarts that mission; running rule/ship state is not saved. `outfit` has `hired` (purchased crew IDs) and `ships` keyed by hull type + script name (world object ID if unnamed). Each ship has eight `crew` lists in the boarding role order and `banks` mapping bank number to gun ID. `replay_debt` offsets replayed mission rewards when a checkpoint carries purchases funded by those rewards. The automatic economy file accepts the old bare-number format and now writes an object with `points`, `crew`, `arms`, `outfit`, `replay_debt`. These remain user data, not mod files.

**Engine-added mission fields (not retail; optional, `mission.json`/`script.json`).** Rule `conditions`/`actions` records with a `Type` starting `OP ` (`OP Any Of` `{Conditions: [..]}`, `OP Not` `{Condition: {..}}`, `OP Counter Condition` `{Counter Name, Equivalence, Value}`, `OP Wait` `{Seconds}`, `OP Wait Until Speech Done` `{Speech Event Name}`, `OP Set Counter` / `OP Add To Counter` `{Counter Name, Value}`, `OP Set Rule Active` `{Rule Name, Boolean}`, and the ship and world Types `OP Board Ship`, `OP Abandon Ship`, `OP Teleport`, `OP Follow Group`, `OP Place Mines`, `OP Etherium Current`, `OP Group Is Cloaked`, `OP Group Speed`, `OP Task State` and the campaign counters, whose fields are in `docs/rule-types.md`); the rule field `"Is Active"` (retail writes `true` on every rule; `false` now means "not evaluated"); `tasks[]` keys `complete_when` / `fail_when` (lists of condition records, ANDed) and `required` (bool, default false), plus `team` (a skirmish team `name_id`: in a skirmish script the task decides for that team); and the top-level `"victory": "objectives"` (`"script"` in a skirmish `script.json`) with optional `victory_message` / `defeat_message` (media `messages` ids). Their meaning is in "Mission rule extensions" in `docs/modding.md`.

## ship.json keys

The keys of `ships/<Id>/ship.json` the loader reads (`game/src/data/ships.rs`). "Required" means the file fails to load without it, with a
message naming the file; "default" is what a missing key gives. The importer writes all of them; a mod's ship needs only the required
ones and whatever it changes (a ship from a model: `docs/modding.md` "A ship from a model").

*Source* says where a key comes from: `retail` keys carry a value the importer reads from the original's ship data (the WOT
fields keep their retail names inside `physics`, `Mass`, `MaxThrust`, ...; the outer keys are this engine's snake_case names for
them), `engine` keys have no retail counterpart and exist for mods (`ENGINE_SHIP_KEYS` in `game/src/data.rs`). The full list of
engine extensions, with what switches each on, is "Engine extensions (not retail)" in `docs/modding.md`.

Gun entries in `data/weapons/guns.json` are retail apart from one engine key, `beam` (a beam gun, `ENGINE_GUN_KEYS` in
`game/src/data.rs`; its fields are in "A new weapon" in `docs/modding.md`).

| Key | Source | Type | Required | Default when missing |
|---|---|---|---|---|
| `extends` | engine | ship id | no | none; deep-merged over that ship's file at overlay time, then dropped |
| `race` | retail | string | no | empty |
| `skirmish` | retail | bool | no | `false`: the ship is not offered on the fleet screen |
| `points` | retail | integer | no | `0` (fleet cost) |
| `display_name`, `icon` | retail | string | no | the hull's name string, an icon named from the id |
| `model`, `geometry` | engine | string | no | the ship's own folder (`model.glb`, `geometry.json`); another ship's id takes that ship's file |
| `scene` | retail | string | no | none |
| `core_hp` | retail | number | no | with a model: a quarter of the hull sections' HP, at least 50; without one: 100 |
| `physics` | retail | object | no | `Mass` 4000, `MaxThrust` 60000, `MaxSpeed` 8, `RotationalFriction` 20, `MaxAngularAcceleration` 3 (PROVISIONAL, about a Navy Cutter's); `MinExtents`, `MaxExtents`, `CenterOfMass` default to the model |
| `sections` | retail | object by label | no | built from the model's layers; each section needs `layer`, `material`, `hp`, `core_pct`, `vital_ship`, `vital_speed`, `vital_turn`; optional `vital_mission` (`false`), `swappable` (`true`), `adjacent` and `criticals` (empty), `destruct`, `debris`, `debris_fx` |
| `guns` | retail | object by mount | no | none; each mount needs `gun` (a key of `guns.json`) and `arc_deg`; `pos`/`rot` default to the model's hardpoint node of that name, `bank` to the nearest weapon-panel slot, `parent_layer` none |
| `engines`, `wakes`, `flags` | retail | arrays | no | from the model's `EnginePortPlacement`, `WakePlacement`, `FlagAttachmentPoint` nodes |
| `explosion_fx`, `repair_fx` | retail | string | no | by race and size |
| `engine_snd` | retail | array of names | no | by race and size |
| `sight` | retail | number | no | `400` (AI sight range) |
| `ring` | retail | number | no | `0.65` |
| `cloakable` | retail | bool | no | `true` only for ids containing `Submersible` |
| `fuel` | engine | bool | no | `false` |
| `crew_nodes` | engine | array of `{name, pos, rot?}` | no | none; crew places for deck crews, added to the model's `Crew_*` nodes (a node of the same name replaces the model's); `name` is a `Crew_<Kind>_...` node name, `pos`/`rot` as a gun's, ship frame |
| `tow` | engine | `{"tower": [x, y, z], "towee": [x, y, z]}` | no | the model's `TowerPoint` / `ToweePoint` nodes, else the hull ends; where the tow line is held (tower) and fastened (towee), ship frame, game axes as a gun's `pos`; wins over the nodes when present |
| `homing` | engine | `[x, y, z]` | no | the model's `TorpedoHomingPoint` node, else the vital hull's centre; where homing torpedoes aim, ship frame, game axes; wins over the node when present |
| `crew_posts` | retail | object | no | one gunner per two guns, a captain, mate, navigator, engineer, a rigger per sail, a fighter or two |
| `lifeboats` | retail | integer | no | `0` |
| `is_lifeboat` | retail | bool | no | `false` |
| `sphere` | retail | `{center, radius}` | no | from the model |

## Where a missing key comes from

The published ship schema (`authoring::schemas`, `--dump-schemas`) marks each key the engine fills in when `ship.json` leaves it out with
`"x-default-from"`, on the key and, where the engine fills a part of an entry, on that part (`guns` `pos`, `rot`, `bank`, `parent_layer`;
`engines` `pos`, `rot`, `fx`, `parent_layer`; `physics` `MinExtents`, ...). One table in `game/src/data/ships.rs` (`SHIP_FILLS`) says
which, and a test keeps the schema equal to it. Every property of the ship, guns and bullets schemas also has `"x-origin"`, the *Source*
column above: `retail` or `engine`. The entry of the guns and bullets schemas carries `"x-required-new"`, a list of the fields a new entry must set (dotted: `visual.pivot`); it is an annotation, not JSON Schema `required`, because the entries of a mod are merged key by key and a change to a retail entry needs none.

| `x-default-from` | Meaning | Keys |
|---|---|---|
| `model` | derived from the ship's `model.glb` (layers, hardpoint nodes, geometry) | `core_hp`, `sections`, `engines`, `wakes`, `flags`, `sphere`, `physics` extents and centre of mass, a gun's or engine's `pos`, `rot`, `parent_layer` |
| `race_and_size` | a choice by the hull's race and size | `crew_posts`, `explosion_fx`, `repair_fx`, `engine_snd`, an engine's `fx` |
| `navy_cutter` | the PROVISIONAL Navy Cutter numbers | `physics` (`Mass`, `MaxThrust`, `MaxSpeed`, `RotationalFriction`, `MaxAngularAcceleration`) |
| `hull` | the model's node, else the hull's ends or vital centre | `tow`, `homing` |
| `weapon_panel` | the weapon-panel slot nearest the gun on the generated outline | a gun's `bank` |

## Silently ignored

The loader never rejects a key it does not know, and nothing is logged for it, so a typo is indistinguishable from a feature that does
nothing. Check the spelling against the tables above when a change has no effect:

- **Unknown keys** in `ship.json`, in its `sections`, `guns`, `engines`, `wakes` and `flags` entries, and in `physics` (`Mass`, not `mass`):
  the key is dropped.
- **A misspelt key** (`sigth`, `Maxspeed`) is an unknown key, so it is dropped, while a known key with a value of the wrong type is an
  error that stops the game at launch naming the file.
- **`mod.json`**: unknown keys are ignored; `game_version`, `requires` and `format` are read and only warn (`docs/modding.md` "Sharing a mod"). A `$schema` key is ignored in every file (`docs/modding.md` "JSON schemas").
- **Object files that are not valid JSON objects** in a merged path (`docs/modding.md` rule table) are not ignored; they replace the
  table below them.

PROVISIONAL: the list describes how the loader behaves in this checkout, not a promise; keys that hold a malformed
value in the optional theme and render files (`data/hud/theme.json`, `data/render/look.json`) keep their default instead, also
without a message.

## Example: a faster cutter with a heavier gun
A mod's `ships/Ship_Navy_Cutter_Fast/ship.json`:
```json
{ "extends": "Ship_Navy_Cutter", "entity": "Ship_Navy_Cutter_Fast", "physics": { "MaxSpeed": 22.0 } }
```
and a new weapon is a key in a mod's own `data/weapons/guns.json` (object files are merged key by key across mods).

## Not exported yet
Wrecked hulls, `Ship_Debris` and two ships without meshes (`Ship_Pirate_Silver`, `Ship_Procyon_RefuelingStation`); three scenery meshes
(`AsteroidXlarge`, `IslandDragon1`, `IslandDragon7`) that have no layers in the retail MSB (listed under `unresolved_scenery` in each mission);
base/asteroid turrets (`Gun_*` not used by any ship); fleet profile (`.ckb`) files; the other-language string tables.
