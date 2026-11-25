# Writing a mission

A custom mission is a map folder in a mod: `maps/<id>/` with `map.json`, `mission.json` and `media.json` (and `speech/*.ogg`). The
game plays it with the same rule interpreter as the retail campaign. This page is the guide for writing one; `docs/modding.md` covers
mods in general (installing, sharing, ships, art) and "Mission rule extensions" there lists the engine-added `OP ` Types;
`docs/rule-types.md` is the reference of every condition and action. A mission is data you write yourself: do not copy a retail
mission, which is game content. A diff over one (`extends` plus the changed keys) is fine, as over a retail ship or a skirmish map.

The worked example, `docs/examples/gate-ambush.mission.json`, is a complete `mission.json` (ours; a unit test keeps it valid). With the
`map.json` and `media.json` of section 1 it is a playable mission. Section 6 walks through it.

The claims below were checked against the code (`game/src/campaign.rs`, `scenario/spawn.rs`, `rules/`); what is not verified is marked
PROVISIONAL.

## 1. A mission in five minutes

```
<anywhere>/gates/
  mod.json
  maps/gate_ambush/
    map.json
    mission.json          <- docs/examples/gate-ambush.mission.json, copied
    media.json
```

`mod.json`:

```json
{"id": "gates", "name": "Gates", "version": "0.1", "category": "Missions", "description": "A one-mission test.", "missions": ["gate_ambush"]}
```

`map.json`, the smallest that loads (every key is optional; these are the ones worth having):

```json
{
  "id": "gate_ambush",
  "name": "Gate Ambush",
  "description": "Two raiders wait at the gate.",
  "sky": "skybox_22_Layer1",
  "world_half": 1200.0,
  "objects": [
    {"type": "Island04", "id": 1, "pos": [-300.0, 300.0, 0.0], "rot": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], "player": -1, "scene": "Island04"}
  ]
}
```

`media.json`, the smallest that loads (the file itself is required; the game stops at the start if it is missing):

```json
{"speech": {}, "messages": {}, "ship_names": {}}
```

Then:

```
make run ARGS='--mod <path>/gates --check-mission gate_ambush'     # no game window: ends with "gate_ambush: 0 errors, 0 warnings"
make run ARGS='--mod <path>/gates --map gate_ambush'               # play it
```

Retail ids `mission01`..`mission13`, `tutorial` and `basic_tutorial` mean the retail missions: do not use them. The folder name is the
id (`--map`, `--check-mission`, `"missions"`); the `id` key inside `map.json` is informational.

### The mod and the mission list

- The mod must be switched on (Mods screen) or passed with `--mod`. Each mod is one campaign, named by its `mod.json` `name` (the
  folder name when it has none).
- Single Player > Custom Missions lists every `maps/<id>/mission.json` of the active mods under the map's `name` (the id when it
  has none) and `description`. `"missions": ["a", "b"]` in `mod.json` gives the order; unlisted ones follow by id. Winning a mission
  offers the next one of the same mod (key N on the end screen). A mod's mission with a retail id replaces that retail mission and is
  not listed.
- Custom missions never add to the retail campaign's victory points (only `mission01`..`mission13` do).

## 2. `map.json`: the world

For a mission, the loader reads only the keys marked "yes" below. Numbers are game units; axes are x east, y north, z up.

| key | read for a mission? | meaning |
|---|---|---|
| `name`, `description` | yes | what the mission list shows |
| `id` | no | informational |
| `sky` | yes | a sky name: a retail one (`skybox_22_Layer1`: the `_LayerN` suffix is dropped and `models/sky/skybox_22.glb` is drawn) or your own sky mesh or HD sky (`docs/modding.md` "HD skies"); default `skybox_07` |
| `music` | **no** | a mission's music comes from its rules: `Play Music Track` (`File Name`, found by name under `sound/`). With no such action the mission is silent |
| `backdrop` | as a flag | any string turns on the star map picture, which is the file `maps/<id>/backdrop.png` of your mod (the string's name is not used). Without the key the star map has no picture |
| `lighting` | yes | `{"ambient", "roof", "floor"}` colours (RGBA, 0..1) and `"roof_direction"`, the sun's travel direction. The engine uses `roof` as the sun and `floor` as the ambient light (PROVISIONAL, `docs/data-format.md`); without the key: plain white light from above |
| `world_half` | yes | half the side of the map's square: the star map's extent and where computer ships search. Default 1000. Keep your objects and ships inside it (nothing stops you leaving it) |
| `objects` | yes | scenery, section 9 |
| `nebulae` | yes | clouds and storms, section 8 |
| `mines` | yes, custom missions only | `[{"type": "Bullet_StarMine_Med", "id": 7, "pos": [x, y, z]}]`: mines resting where you put them, hostile to every ship until a rule hands one over with `Set Group/Unit Owner` (give each a unique `id`, and list it in a `groups` entry to address it). A mine answers to its group only, never to its owner's name. `OP Place Mines` lays them from a rule instead. `docs/modding.md` "Mission rule extensions" |
| `players` | no | skirmish lobby slots. `--check-mission` reads the names as known players, but a mission's players live in `mission.json`. Leave it out, or fill it as the retail maps do: `{"name", "team": -1, "start": [x, y, z], "forward": [x, y, z], "formation": 0, "playable": true, "race": 4, "color": [r, g, b, a]}` (`race`: 0 Navy, 2 Pirate, 3 Procyon, 4 open; other numbers are unverified) |
| `alliance_change`, `teams`, `ship_names`, `name_id`, `desc_id`, `assemble`, `islands_make_sounds` | no | skirmish-lobby and retail-text fields; leave them out |

`openprocyon --dump-schemas <dir>` writes `map.schema.json`, the keys above with their types and defaults, for an editor's hints (`docs/modding.md` "JSON schemas").

`rot` (objects, ships, animals) is nine numbers: a 3x3 rotation matrix in the order the file stores it (column by column: the first
triple is the starboard side, the second the forward vector). `[1,0,0, 0,1,0, 0,0,1]` faces north (+y). To face a heading `a`
(clockwise from north), with `f = (sin a, cos a)`: `[fy, -fx, 0, fx, fy, 0, 0, 0, 1]`. Missing entries read as the identity; four
numbers are not a quaternion, they read as a broken matrix.

## 3. `mission.json`: the story

The top-level keys the engine reads. `ships` and `rules` must exist (the game stops at the start without them), `player_start` should.

| key | meaning |
|---|---|
| `players`, `alliances` | the player names; `[[a, b], ...]` are allied from the start. Every other pair of *listed* players is hostile (a rule changes it with `Set alliance between ...`). A ship whose owner is not listed is hostile to nobody |
| `player_start` | `{"name": <the human's player>, "start", "forward", ...}`. Only `name` matters while the human has a ship (see "The human's ship"); `start` and `forward` place the stand-in ship |
| `ships` | `{"id", "type", "group", "name", "primary", "pos", "rot"}`, section 4 |
| `groups` | `{"Name": [ids]}`: named groups a rule can address. The ids are the `id` of ships, scenery objects and animals, so keep them unique across all three. Optional |
| `volumes` | `{"Name": [[x, y], ...]}` closed polygons (at least 3 points) for `Enter Volume`, `Is Group In Volume`. Optional |
| `paths` | `{"Name": [[x, y, z], ...]}` for `Group to follow path` and the cutscene camera. Optional |
| `points` | `[{"Name", "Position": [x, y, z]}]` objective markers for `Set Current Objective Point`. Optional |
| `environment` | `{"point_sets": {"Name": [{"pos": [x, y, z]}]}}` for `Teleport Group/Unit` and the like; `nebulae` (section 8); `animals` with `whale` (whales and their sounds: each animal needs a `group`, `game/src/environment.rs` has the keys) and `dragons`. Optional |
| `tasks` | the objectives: `Name`, `text` (what the player reads), `Active` (a JSON boolean); engine-added `complete_when`, `fail_when`, `required` |
| `rules` | the rule list (section 6) |
| `map_text` | `[{"name", "text", "pos", "visible"}]`: labels on the star map (`Set Map Text Visibility`). Optional |
| `victory`, `victory_message`, `defeat_message` | engine-added: `"victory": "objectives"` lets the required tasks end the mission. The messages are ids into `media.json` `messages` |
| `landmarks` | **not read** for a custom mission (only the retail mission 1's opening uses it); keep your coordinates in a notes file |
| `crew_pool`, `arms_pool`, `play_end_movie`, `journal*`, `opening_speech`, `unresolved_*`, `handoff_music` | retail fields no custom mission uses; leave them out |

### Naming a ship in a rule

| spec | means |
|---|---|
| `"Name"` | the group `Name` from `groups`; else every ship the player `Name` owns. When a player and a group share a name, the group wins (except for the human's own player) |
| `"Player0 Group"` | the human's player, whatever it is called (the human's real name works too) |
| `"Group,Unit"` | the one ship whose `name` is `Unit`. **The game does not use the part before the comma to find the ship** (the checker still wants it to be a known group or player), so ship names must be unique in the whole mission |
| `"Player0 Group,HUMAN CONTROLLED COMMAND SHIP"` | the human's flagship |

Give every ship you address on its own a `name`, and a group of its own as well when a condition must read it (`Group Destroyed` of one
ship): that is the reliable way. `--check-mission` warns about a `Unit` no ship has; a group name that resolves to nothing is an error.

## 4. Ships

Each entry of `ships`:

```json
{"id": 10, "type": "Ship_Navy_Cutter", "group": "Hero", "name": "Dauntless", "primary": true,
 "pos": [0.0, 0.0, 0.0], "rot": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]}
```

| field | meaning |
|---|---|
| `id` | an integer, unique in the mission (the game stops at the start on a ship without one) |
| `type` | the ship class: the name of a folder in `assets/ships/` (`ls assets/ships`, or `%APPDATA%\openprocyon\assets\ships` for an installed game) or of a `ships/<Id>/` folder of an active mod. Names start with their fleet: `Ship_Navy_*`, `Ship_Pirate_*`, `Ship_Procyon_*`, `Ship_Civilian*`. Variants ending `_NIS`, `_Feather`, `_Tutorial`, `_Wrecked` belong to retail cutscenes: prefer the plain hulls. **A `type` that names no class drops the ship silently**: no error, no warning, and the checker does not look at it |
| `group` | the owning player (a name from `players`) |
| `name` | what the ship's banner and fleet bar show, and what `"Group,Unit"` finds. A ship with no name shows its class (`Pirate Sloop`) and cannot be addressed alone |
| `primary` | `true` marks the human's flagship (below) |
| `pos`, `rot` | placement; z is 0 for a ship on the flight plane |

### The human's ship

The human sails the placed ship with `"primary": true` whose `group` is `player_start.name`; with none marked, that player's first
ship. With no ship for that player at all the log says `mission has no player ship; spawning a stand-in Ship_Navy_Cutter at the start
point` and a cutter appears at `player_start.start`. The other ships of the human's group are the fleet the human commands.

### Allies, enemies and stances

- **Fleet ships**: more ships in the human's group.
- **An allied computer fleet**: a second player in `players`, listed with the human's player in `alliances`.
- **Enemies**: another player that is in no alliance.
- A computer ship starts in the *neutral* stance: it shoots hostile ships it can see within range but does not chase them. For
  hunters, put a `*State Init* Setup Ship` action in a `World Initialize` rule: `{"Type": "*State Init* Setup Ship", "World Object ID":
  11, "AI Stance": "Stance_Aggressive"}` (also `Stance_Dummy`, `Stance_Neutral`, `Stance_Defensive`, `Stance Persistent`), or order an
  attack with `GroupA to attack GroupB`. Setup Ship also reads `Ship Path` / `Follow Mode`, and, at spawn, `Crew Skill Level` and
  `Boardable`; its `Ship Name`, `Player/Owner` and `Primary Ship` fields are not applied (they come from `ships`).
- `Set Ship Name` (`New Ship Name` is a key of `media.json` `ship_names`) renames a ship on the fleet bar; the banner, once built,
  keeps the placed name (PROVISIONAL).

## 5. `media.json`: the words

```json
{
  "speech": {
    "GATE_HINT": {"text": "Gate ahead, Captain.", "duration": 3.2, "file": "gates_hint",
                  "face": "TalkingHead_Character_Amelia_Happy", "color": [1.0, 0.5, 0.0, 1.0]}
  },
  "messages": {"GATE_LOST": "The Dauntless is lost."},
  "ship_names": {}
}
```

| key | meaning |
|---|---|
| `speech.<EVENT>` | what `Play speech event` and `Speech Event Played Once` name. Event names are case-sensitive |
| `text` | the subtitle |
| `duration` | seconds the line is held; **the engine adds 0.3 s**, so give the audio's length. Without `duration` the line is text only: no sound, shown `max(2, characters / 14)` s |
| `file` | the sound `<file>.ogg`; the event name when omitted |
| `face` | the talking-head portrait: the name of a `hud/sprites/TalkingHead_Character_*` folder of the imported assets (`ls assets/hud/sprites`); a name that is no sprite shows no portrait. Your own portraits: `docs/modding.md` "UI art" (PROVISIONAL) |
| `color` | the subtitle tint, RGBA 0..1; default white |
| `speaker`, `head` | the retail exporter writes them; the engine ignores them |
| `messages.<ID>` | the text of an `End Game` (`Loser - Custom Message String ID`) or of `victory_message` / `defeat_message`. An unknown id falls back to "Mission failed" / "Mission complete." |
| `ship_names.<ID>` | the display name `Set Ship Name` applies |

**Speech files.** Put each line at `maps/<id>/speech/<file>.ogg` in your mod (Ogg Vorbis). The file is found by its name without
extension, in any case, among *every* sound under `sound/` and every map's `speech/` folder, so the name must be unique across all
active content: prefix the files with your mission's tag (`gates_hint.ogg`, not `hint.ogg`). To wait for a line, use `Speech Event
Played Once` as a condition, or `OP Wait Until Speech Done` between the actions of a sequence (after the `Play speech event` it waits
for). Space skips the line being spoken.

## 6. Rules, step by step

A rule is `{"name", "Run Once", "Is Active", "conditions": [...], "actions": [...]}`: when **all** its conditions hold, its actions run in
order. `Run Once: true` fires it once (it is also the default; the value must be a JSON boolean: the string `"FALSE"` still means once);
`false` makes it fire every step the conditions hold (use an edge condition such as `Enter Volume`, a counter or a flag to keep it from
firing forever). Booleans inside conditions and actions are the strings `"TRUE"` / `"FALSE"`; `Run Once`, `Is Active`, a task's
`Active` and `required` are JSON booleans. Flags, timers and counters need no declaration: they exist once a rule sets them. A `Type` and
its fields are listed by `--dump-rule-schema`, or in the console with `schema <text>`; unknown Types are logged and ignored.
`docs/rule-types.md` is the reference of every Type, its fields and what it really does.

Walk-through of `docs/examples/gate-ambush.mission.json`:

1. **`Start`** (`World Initialize`: true only in the first step) activates the first task, points the objective marker at the gate and sets
   the counter `raiders sunk` to 0.
2. **`Arrive at the gate`**: `Enter Volume` (the human's group enters `Gate Area`, once, on entry). Its actions are a cutscene: `NIS Start`,
   aim the camera at the raiders, `OP Wait` four seconds, `NIS End`; then start the second task, order the raiders to attack, and switch
   on the next rule with `OP Set Rule Active`. The actions after `OP Wait` run four seconds later, in this same rule; the rest of the rule
   list keeps running.
3. **`Count raiders sunk`** starts with `"Is Active": false`, so none of its conditions is evaluated until rule 2 switches it on. It is
   repeating (`Run Once`: false); every ship the human sinks adds 1 to the counter.
4. **The tasks finish themselves**: `Reach the gate` completes when the human is inside the volume, `Clear the gate` when the counter is
   above 1 (`OP Counter Condition`), and fails if the human's group is destroyed. `"victory": "objectives"` wins the mission when every
   `required` task is complete and loses it when one fails, so no rule calls `Goto Next Level` or `End Game`. (Without that key, end the
   mission yourself: `Goto Next Level` to win, `End Game` to lose.)

Patterns that recur:

- **Beats in order**: each beat sets a flag (or starts a timer) that the next beat's rule reads; a rule per beat, named for what it waits
  for ("Targets sunk"), so `why` and `--rule-trace` read like a script.
- **Cutscenes**: one rule, `NIS Start`, `OP Wait`s between `Teleport Group/Unit` / camera actions / speech, `NIS End`. While a NIS runs the
  HUD hides and input is locked.
- **Failure**: a repeating or run-once rule with `Group Destroyed` on the ship that must survive, and `End Game` with a message id; or a
  `fail_when` on a required task.
- **Several things at once**: `OP Any Of` for "or", a rule's condition list for "and", `OP Not` for "not" (not of an edge condition is
  true on every other frame; see `docs/modding.md`).

## 7. Things that behave in a way you would not guess

Observed while building larger missions and re-read against `game/src/rules/eval/`:

- **`Teleport Group/Unit` puts a group round the first point of the set**: the original stacks every member on that point; this engine keeps
  the members' layout round it (one ship lands on it) and moves a ship that would land inside another hull to the nearest free spot. It stops every
  teleported ship dead (speed, turn rate; the human's ship also its throttle and course; a computer ship keeps its order, so a path set afterwards is
  followed) and keeps heading and damage. Engine-added, docs/re/collision.md "Teleport and overlap".
  `OP Teleport` takes a point index and an optional heading for each unit (`docs/modding.md` "Mission rule extensions"); it shares the same placing.
- **Damage is a fraction, 0..1**: `Damage Group/Unit by X percent` and `Group/Unit has >,<,= X damage` take 0.3 for 30 per cent (30 would
  kill; the checker reports it). The condition reads the worst-hit member.
- **Event quotas count "more than N"**: `Team has captured a ship from Group/Unit` and the other event conditions fire when the count is
  above `Number Of Captures` / `Number Of Ships Destroyed`, so use 0 to mean "at least one".
- **`Group/Unit Docked`** (`Dockers Group/Unit Name`, `Targets Group/Unit Name`) is the docking condition; `Is Ship In Tow` takes
  `Towing Group`, `Target to Tow`, `Boolean`. The tow action is `Tow Ship` (`Docking Group` is the tower), and `Break Tow` lets the
  towed ship coast. A tug within 50 units of its tow (hull to hull) links at once; further away it sails over first.
- **Lifeboats come with a death by damage**, not by `Destroy Group/Unit`; a ship that only limps on launches none. `OP Abandon Ship`
  launches them from a hull that stays afloat, disabled (the ship needs `Set Lifeboat Creation State` TRUE). To stage the scene with
  boats you place yourself, keep them clear of every live ship: a ship whose bounding sphere touches one collects it.
- **Animals on a mission map start hidden**: a whale or flock needs `Set Group/Unit Visibility` TRUE (the console's `reveal`
  does it for everything) before it shows.
- **Boarding needs a harpoon mount**, for the player and for `OP Board Ship` alike; a boarder without one does nothing. `OP Board Ship`
  starts a boarding from a rule (a ship that cannot grapple, or a target that is not hostile or not Boardable, is logged and skipped).
  A capture can also be staged with `Transfer Group/Unit to Group`.
- **Following and "stopped alongside"**: `OP Follow Group` keeps a ship on a station beside, ahead of or behind another (a campaign
  mission's AI carries it out); `OP Group Speed` with `Fastest` `Less Than` a small number is "the whole group has stopped".
- **Throttle caps**: `Set Group Throttle Percent` sets the current speed, `Set Max Throttle Percent (Max user settable)` caps what the
  player may choose; `Speed 0` on the human's ship stops it without capping it.
- **Time**: timers wait out only the retail mission 1's opening briefing, so a custom mission has no such pause. Whether rule time keeps
  running while the game is paused is unverified (PROVISIONAL).
- **A `Run Once` rule is spent when it fires**, even if one of its actions was a no-op because a name did not resolve: run
  `--check-mission` and read the start-of-mission audit in the log (section 11).
- **Hostility needs listed players**: a ship whose owner is not in `players` is hostile to nobody.
- **Music is a rule action**, not a `map.json` key (section 2).

## 8. Storms and clouds

A storm is described three times, and the names must agree:

| where | what | links to |
|---|---|---|
| `map.json` `nebulae[]` | `{"name", "groups", "polygon", "cloud", "storm", "meteors", "sound_distance"}` in the layout of the retail maps: what is drawn, what drains and damages | `name` = the record's `New Nebula Name` (a rule that moves a storm addresses its `groups`) |
| `mission.json` `environment.nebulae[]` | a raw `*State Init* Setup Nebula` record (not a rule): `"New Nebula Name"`, `"Polygon Name"`, `"Nebula Occlusion On/Off": "TRUE"` to hide ships, the effect and point-set names, `"World Object ID"` | `Polygon Name` = a key of `volumes` |
| `mission.json` `volumes` | the polygon with that name | what `Unit is Within any Nebula` and sight occlusion read |

The simplest is one name for all three. The map side is what is drawn, drains and damages; the mission side and the volume are what
hide ships. Occlusion is one-way by polygon edge: ships outside cannot see in, ships inside see each other, and only a record with
`"Nebula Occlusion On/Off": "TRUE"` occludes. A storm slows ships only when its `cloud` has `"drain": true`. Lightning and wind chip at
ships rather than kill them. Mods can restyle the clouds (`docs/modding.md`, "Graphics mods"). The layout of a `nebulae` entry is
easiest to learn from any imported `assets/maps/<id>/map.json`; write your own polygon and points, do not copy the game's.

## 9. Placing scenery

Each entry of `objects`: `{"type", "id", "pos", "rot", "player": -1, "scene"}`.

- `type` names the scenery kind and `scene` its mesh. The retail pairs are the `type` / `scene` of the imported maps' objects
  (`Island04` / `Island04`, `Base_Grant` / `BaseGrant`); the kinds that collide or dock are the keys of `data/collision.json` and
  `data/docks.json`. `scene` is loaded as `<scene>.glb`, found by file name anywhere under `models/` (mods keep theirs in
  `models/scenery/`).
- Your own scenery: ship the `.glb`, and an entry under your `type` in your `data/collision.json` and (for docks) `data/docks.json`
  (merged key by key with the base files; `docs/modding.md` "What a mod can change"). Collision and dock points are looked up by `type`,
  not by `scene`; an object with no collision entry is only a picture.
- `id` makes the object addressable (`groups`, `*State Init* Setup Island`, docking rules). It is optional for decoration.
- Dockable scenery's first dock point sets its height: the placement subtracts that point's local height from `pos`. An optional
  `scene_origin` overrides that datum (`docs/data-format.md`).
- Keep every object, ship and point inside `world_half`. `--check-mission` does not look at geometry; the console's `show` does.

## 10. The test loop

1. `--check-mission <id>` (add `--mod <dir>`): every name a rule uses must be defined, every Type and field known, no flag read and never
   set. Run it after every edit; fix errors, read warnings. It exits 1 on errors. `--dump-rule-schema` prints the vocabulary. It does
   **not** check ship `type`s, that an `.ogg` exists, geometry or the keys of `map.json`.
2. Play it with the developer console on (`~`; `docs/modding.md` "The developer console"): `rules` shows what has fired, `why <rule>` says what
   a waiting rule waits for, `fire <rule>` runs a beat now, `tp`, `god`, `win`, `lose`, `skip` and `restart` get you around, `show
   volumes|paths|points` draws your geometry with its names, `where` and `mark <name>` give coordinates to paste back into the files.
   Using a cheat marks the session so it never counts toward campaign points.
3. **Starting at a later beat.** `--fire-rule "<rule>"` (repeatable) runs the named rule's actions at the start, after the
   `World Initialize` rules and in the order given, as if its conditions had held; a run-once rule counts as fired. A beat depends on
   the flags, counters and tasks earlier beats set, so fire the earlier beats too, in order. `--start-at x,y` moves the human's ship
   (rules do not): `--map gate_ambush --fire-rule "Arrive at the gate" --start-at 380,590`. An unknown name is a warning listing the
   rule names. To watch why rules are not firing: `--rule-trace` or `--why "<rule>"`.
4. Let the autopilot play it: `--script mission` (`docs/development.md`) sails the human's ship through objective points and the volumes
   the unfired rules wait for, at any `--speed`. It has no tactics, so losing rules and optional detours never fire, and the list of rules that
   never fired it prints is exactly what you still have to test by hand.
5. Screenshots: `tools/shot.sh out.png --map <id> --mod <dir> --fire-rule "<rule>" --shot-frame 300`.

## 11. Common mistakes and what you see

`--check-mission` prints `<id>: <ERROR|warning>: rule "<rule>": <condition|action> #N '<Type>', field "<field>": <message>` per
problem, then `<id>: N errors, M warnings`.

| mistake | what you see | fix |
|---|---|---|
| misspelled group, player or unit | `ERROR: rule "R": condition #1 'Group Destroyed', field "Group Name": unknown group 'Fleat' (did you mean 'Fleet'?); it has no members`; for a unit, `warning: ...: unit 'Dauntles' is not the name of any ship the mission places` | use a name from `groups`, `players` or `ships[].name` |
| misspelled volume, path, point, point set, task or rule | `ERROR: ... unknown volume 'Harbor Mouth' (did you mean 'Harbour Mouth'?); it is an empty region that nothing is ever inside`; likewise `unknown path`, `unknown objective point`, `unknown point set`, `unknown objective task`, `unknown rule '...'; nothing is activated` | match the key exactly |
| a volume with fewer than 3 points | `ERROR: ... volume 'X' has 2 vertices; fewer than 3 is an empty region` | at least 3 `[x, y]` points |
| unknown Type or field | `ERROR: ... field "Type": unknown action Type (did you mean 'Play speech event'?); the interpreter ignores it`; `field "Timer Nam": unknown field for 'Start Timer' (did you mean 'Timer Name'?); it is ignored`; `field "Seconds": missing; reads as 0` | `docs/rule-types.md` |
| `true` instead of `"TRUE"` | `ERROR: ... field "Boolean Value": true is not "TRUE" or "FALSE"; it reads as FALSE` | quote it |
| damage as a percentage | `ERROR: ... field "Percent": 30 is outside 0..1 (a fraction, not a percentage)` | `0.3` |
| flag, timer or counter read but never written | `ERROR: ... field "Flag Name": flag 'armed' is never set, so this can never hold`; `timer 'T' is never started, so this can never hold`; `counter 'n' is never set (it reads 0), so this can never hold` | set it in some rule's actions |
| speech or message with no `media.json` entry | `warning: ... speech event 'GATE_HINT' has no media entry (no text, no sound)`; `message 'GATE_LOST' has no media entry; the game falls back to a default` | add the entry |
| no `media.json` | `warning: rule "(mission)": files, field "media.json": missing: speech events, end messages and ship names are not checked`, and **the game stops at the start** with `missing maps/<id>/media.json: ...` | at least `{"speech": {}, "messages": {}, "ship_names": {}}` |
| no or broken `map.json` | `ERROR: rule "(mission)": files, field "map.json": missing or not valid JSON; the map cannot be played` | add it (section 1) |
| `"victory": "objectives"` without a required task | `warning: ... field "victory": no task has "required": true, so the mission never ends this way`; `victory_message` without it: `is ignored unless "victory" is "objectives"` | mark a task `"required": true` |
| a rule with no name, or two with one name | `warning: ... field "name": the rule has no name (--fire-rule and --why cannot address it)` / `another rule has the same name (--fire-rule and --why take the first)` | name each rule once |
| wrong ship `type` | nothing: the ship is simply absent (the audit below may then complain about its group) | compare with `ls assets/ships` and your mod's `ships/` |
| a ship without an integer `id`, a mission without `ships` or `rules` | the game panics at the start | give every ship an `id` |
| a rule names something that is not there | in the log, once per rule and name: `mission rules: rule 'X': group 'Fleat' in field 'Group Name' of 'Group Destroyed' names no group, player or unit of the mission` (also `is no volume (or has fewer than 3 vertices)`, `is no point set (or has no points)`, `is no path (or has no points)`) | fix the name |
| a speech line shows its text but is silent | the `<file>.ogg` is not among the sound files (misspelt, or named like another file) | check the name and that it is unique |

## 12. Before you share it

- `--check-mission` is clean; every speech event has a `media.json` entry and a `.ogg` (or you accept the timed subtitle).
- Every file is yours or under a licence that allows redistribution, with the licence next to it (`docs/modding.md` "Sharing a mod").
- No retail data: no copied mission, no converted model, texture or sound. Voices you synthesise yourself are yours to ship; check the
  service's terms for the use you intend.
- The mission is playable from its first rule to a win and to each loss, with the console's `win` and `lose` covering the end screens
  and the hand-off to the next mission of the mod.
