# Writing a mission

A custom mission is a map folder in a mod: `maps/<id>/` with `map.json`, `mission.json` and `media.json` (and `speech/*.ogg`). The
game plays it with the same rule interpreter as the retail campaign. This page is the guide for writing one; `docs/modding.md` covers
mods in general (installing, sharing, ships, art) and "Mission rule extensions" there lists the engine-added `OP ` Types.
A mission is data you write yourself: do not copy or edit a retail mission, which is game content.

The worked example, `docs/examples/gate-ambush.mission.json`, is a complete `mission.json` (ours; a unit test keeps it valid). The
walk-through below uses it.

## 1. The folder

```
mods/<mod>/
  mod.json                      {"id", "name", "version", "category": "Missions", "description"}
  maps/<id>/
    map.json                    the world: sky, music, players, scenery objects, nebulae, bounds
    mission.json                the story: ships, groups, volumes, paths, points, tasks, rules
    media.json                  the texts: speech lines, end messages, ship names
    speech/<event>.ogg          the voice of each speech line (the file name is the event, any case)
  models/...                    scenery models your map places (see "Placing scenery")
```

Start it with `--map <id>` (add `--mod mods/<mod>` if the mod is not installed), or from the Single Player menu's Custom Missions.
A mod is one campaign: `mod.json` `"missions": ["<id>", ...]` gives the order, and winning one offers the next. Ids `mission01`..`mission13`,
`tutorial` and `basic_tutorial` mean the retail missions, so do not use them. Use the folder name as the `id` in `map.json`.

## 2. `map.json`: the world

Keys a working mission uses (the numbers are game units; axes are x east, y north, z up):

| key | meaning |
|---|---|
| `id`, `name`, `description` | the folder name, and what the mission list shows |
| `sky` | a sky name, as the retail ones (`skybox_22_Layer1`) or your own (`models/sky/`, see `docs/modding.md` "HD skies") |
| `music` | the track the mission opens with |
| `backdrop` | the star map's background image name |
| `lighting` | `{"ambient", "roof", "floor"}` colours (RGBA, 0..1) and `"roof_direction"`: the sun's direction; hulls are lit from it |
| `world_half` | half the side of the map's square: the star map's extent, and where computer ships search and patrol. Keep your objects and ships inside it |
| `players` | one entry per player name a rule uses: `{"name", "team": -1, "start": [x, y, z], "forward": [x, y, z], "formation": 0, "playable": true, "race": <fleet type number>, "color": [r, g, b, a]}` |
| `objects` | placed scenery: `{"type", "id", "pos", "rot", "player": -1, "scene"}`; the `id` is how rules reach an island or base (`*State Init* Setup Island`) |
| `nebulae` | clouds and storms, see "Storms" |
| `alliance_change`, `teams`, `ship_names`, `name_id`, `desc_id`, `assemble`, `islands_make_sounds` | skirmish-lobby and retail-text fields; a mission can leave them at `true`, `[]`, `{}`, `""`, `false`, `false` |

`rot` is a row-major 3x3 rotation whose second row is the forward vector (ships use it as well). To face a heading, with
`f = (sin a, cos a)`: `[fy, -fx, 0, fx, fy, 0, 0, 0, 1]`.

## 3. `mission.json`: the story

Top-level keys, as in the worked example and in the retail missions:

| key | meaning |
|---|---|
| `players`, `alliances` | the player names; `[[a, b], ...]` are allied from the start, every other pair is hostile (a rule changes it with `Set alliance between ...`) |
| `player_start` | `{"name": <the human's player>, "start", "forward", ...}`: which player the human is. The human's ship is that player's placed ship with `"primary": true`, else the player's first |
| `ships` | `{"id", "type", "group", "name", "primary", "pos", "rot"}`. `type` is a ship class (retail or from a mod); `group` is the owning player. A ship with no class is not simulated |
| `groups` | `{"Name": [ship ids]}`: named groups a rule can address. A bare name in a rule means a group, else a player (everything that player owns) |
| `volumes` | `{"Name": [[x, y], ...]}` closed polygons for `Enter Volume`, `Is Group In Volume` |
| `paths` | `{"Name": [[x, y, z], ...]}` for `Group to follow path` and the cutscene camera |
| `points` | `[{"Name", "Position"}]` objective markers for `Set Current Objective Point` |
| `environment` | `{"point_sets": {"Name": [{"pos": [x, y, z]}]}}`, plus animals and nebula setups |
| `tasks` | the objectives: `Name`, `text` (what the player reads), `Active`; engine-added `complete_when`, `fail_when`, `required` |
| `rules` | the rule list (section 5) |
| `map_text` | `[{"name", "text", "pos", "visible"}]`: labels on the star map (`Set Map Text Visibility`) |
| `landmarks` | named places (a cutscene camera or a doc can read them) |
| `victory`, `victory_message`, `defeat_message` | engine-added: `"objectives"` lets the required tasks end the mission |
| `crew_pool`, `arms_pool`, `play_end_movie`, `journal*`, `opening_speech`, `unresolved_*`, `handoff_music` | retail fields; leave them `[]`, `false`, empty |

Naming a ship in a rule: `"Group"` is every member; `"Group,Unit"` is one unit by its `name`; `"Player0 Group,HUMAN CONTROLLED COMMAND
SHIP"` is the human's flagship (the human's player name works wherever `Player0 Group` does). Give each ship you will address on its
own a group of its own: that is the reliable way to name one ship, and the checker warns about a name that resolves to nothing.

## 4. `media.json`: the words

```json
{
  "speech":   {"EVENT_ID": {"text": "subtitle", "duration": 4.2, "file": "EVENT_ID", "face": "TalkingHead_Character_Amelia_Happy",
                            "color": [1.0, 0.5, 0.0, 1.0], "speaker": "AMELIA"}},
  "messages": {"MY_LOST": "The Dauntless is lost."},
  "ship_names": {}
}
```

A `Play speech event` shows `text` for `duration` seconds and plays `maps/<id>/speech/<file>.ogg` (found by name in any case; a line with
no file is shown for about its length / 14 seconds, at least 2). Set `duration` to the audio's length plus a breath (0.3-0.4 s) so lines do
not overlap. `face` is a retail talking-head sprite; `color` tints the subtitle per speaker. An end message goes in `messages`, and `End
Game` (`Loser - Custom Message String ID`) or the mission's `defeat_message` names its id. Use `Speech Event Played Once` to wait for a line
to finish, or `OP Wait Until Speech Done` inside a sequence.

## 5. Rules, step by step

A rule is `{"name", "Run Once", "Is Active", "conditions": [...], "actions": [...]}`: when **all** its conditions hold, its actions run in
order. `Run Once: true` fires it once; `false` makes it fire every step the conditions hold (use an edge condition such as `Enter Volume`,
a counter or a flag to keep it from firing forever). Booleans are the strings `"TRUE"` / `"FALSE"`. Flags, timers and counters need no
declaration: they exist once a rule sets them. A `Type` and its fields are listed by `--dump-rule-schema`, or in the console with
`schema <text>`; unknown Types are logged and ignored.

Walk-through of `docs/examples/gate-ambush.mission.json`:

1. **`Start`** (`World Initialize`: true only in the first step) activates the first task, points the objective marker at the gate and sets
   the counter `raiders sunk` to 0.
2. **`Arrive at the gate`**: `Enter Volume` (the human's group enters `Gate Area`, once, on entry). Its actions are a cutscene: `NIS Start`,
   aim the camera at the raiders, `OP Wait` four seconds, `NIS End`; then start the second task, order the raiders to attack, and switch
   on the next rule with `OP Set Rule Active`. The actions after `OP Wait` run four seconds later, in this same rule; the rest of the rule
   list keeps running.
3. **`Count raiders sunk`** starts with `"Is Active": false`, so none of its conditions is evaluated until rule 2 switches it on. It is
   repeating (`Run Once: false`); every ship the human sinks adds 1 to the counter.
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

## 6. Things that behave in a way you would not guess

Observed while building larger missions; each is also visible in `game/src/rules/eval/`:

- **`Teleport Group/Unit` sends every member of the group to the first point of the set**, so a group of two lands stacked. Use one group
  and one point set per ship, offset by hand. It changes the position only: heading, speed and damage stay.
- **Damage is a fraction, 0..1**: `Damage Group/Unit by X percent` and `Group/Unit has >,<,= X damage` take 0.3 for 30 per cent (30 would
  kill). The condition reads the worst-hit member.
- **Event quotas count "more than N"**: `Team has captured a ship from Group/Unit` and the other event conditions fire when the count is
  above `Number Of Captures` / `Number Of Ships Destroyed`, so use 0 to mean "at least one".
- **`Group/Unit Docked`** (`Dockers Group/Unit Name`, `Targets Group/Unit Name`) is the docking condition; `Is Ship In Tow` takes
  `Towing Group`, `Target to Tow`, `Boolean`. The tow action is `Tow Ship` (`Docking Group` is the tower), and `Break Tow` lets the
  towed ship coast. A tug within about 50 units of its tow links at once; further away it sails over first.
- **Lifeboats come with a death by damage**, not by `Destroy Group/Unit`; a ship that only limps on launches none. To stage an
  abandon-ship scene, place lifeboat ships beside the hull and keep them more than 30 units from any live ship (a ship touching one
  collects it).
- **Animals on a campaign-style map start hidden**: a whale or flock needs `Set Group/Unit Visibility` TRUE (the console's `reveal`
  does it for everything) before it shows.
- **Boarding by the player needs a harpoon mount** on the player's ship; no rule starts a boarding. A capture can be staged with
  `Transfer Group/Unit to Group`.
- **Throttle caps**: `Set Group Throttle Percent` sets the current speed, `Set Max Throttle Percent (Max user settable)` caps what the
  player may choose; `Speed 0` on the human's ship stops it without capping it.
- **Rules are evaluated every frame, including while paused**; timers wait out an opening briefing. A `Run Once` rule is spent when it
  fires, even if one of its actions was a no-op because a name did not resolve: run `--check-mission`.

## 7. Storms and clouds

A storm needs three things that agree on one name: a `nebulae` entry in `map.json` (its `polygon`, `groups` and cloud settings, in the
layout of the retail maps), a `*State Init* Setup Nebula` action in the mission's `environment.nebulae` (the mission side), and a
`volumes` polygon with the same name. The map side is what is drawn and what drains and damages; the mission side and the volume are
what hide ships (`Unit is Within any Nebula`, sight occlusion). Occlusion is one-way by polygon edge: ships outside cannot see in, ships
inside see each other. A storm slows ships only when its cloud has `drain` on. Lightning and wind chip at ships rather than kill them.
Mods can restyle the clouds (`docs/modding.md`, "Graphics mods").

## 8. Placing scenery

`objects` entries place retail scenery by `type`, or your own models: a `.glb` under `models/scenery/` named by the entry's `scene`, with
collision and dock points described in `data/collision.json` and `data/docks.json` (see "What a mod can change" in `docs/modding.md`).
Dockable scenery's first dock point sets its height. Keep every object, ship and point inside `world_half`; `--check-mission` and the
console's `show` help you see where things landed.

## 9. The test loop

1. `--check-mission <id>` (add `--mod <dir>`): every name a rule uses must be defined, every Type and field known, no flag read and never
   set. Run it after every edit; fix errors, read warnings. `--dump-rule-schema` prints the vocabulary.
2. Play it with the developer console on (`~`; `docs/modding.md` "The developer console"): `rules` shows what has fired, `why <rule>` says what
   a waiting rule waits for, `fire <rule>` runs a beat now, `tp`, `god`, `win`, `lose`, `skip` and `restart` get you around, `show
   volumes|paths|points` draws your geometry with its names, `where` and `mark <name>` give coordinates to paste back into the files.
   Using a cheat marks the session so it never counts toward campaign points.
3. To start late without the console: `--fire-rule "<rule>"` (repeatable) and `--start-at x,y`. To watch why rules are not firing:
   `--rule-trace` or `--why "<rule>"`.
4. Let the autopilot play it: `--script mission` (`docs/development.md`) sails the human's ship through objective points and the volumes
   the unfired rules wait for, at any `--speed`. It has no tactics, so losing rules and optional detours never fire, and the list of rules that
   never fired it prints is exactly what you still have to test by hand.
5. Screenshots: `tools/shot.sh out.png --map <id> --mod <dir> --fire-rule "<rule>" --shot-frame 300`.

## 10. Before you share it

- `--check-mission` is clean; every speech event has a `media.json` entry and a `.ogg` (or you accept the timed subtitle).
- Every file is yours or under a licence that allows redistribution, with the licence next to it (`docs/modding.md` "Sharing a mod").
- No retail data: no copied mission, no converted model, texture or sound. Voices you synthesise yourself are yours to ship; check the
  service's terms for the use you intend.
- The mission is playable from its first rule to a win and to each loss, with the console's `win` and `lose` covering the end screens
  and the hand-off to the next mission of the mod.
