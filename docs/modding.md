# Modding

A mod is a folder of files that the game lays over the player's imported assets at launch: data (JSON), models (glTF `.glb`),
images (PNG) and sounds (OGG). No code and no patching: ships, guns, maps, custom missions and campaigns, sounds, UI art, fonts
and the graphics look are all files. When something needs the engine to change, see "Sharing a mod" below.

## Your first mod in 5 minutes

You need a source checkout with the game imported (`make assets`, see the README). Make Silver's Man-o'-War cheaper in skirmish:

1. Copy the example: `cp -r mods/silver-skirmish local/my-mod` (`local/` is git-ignored scratch).
2. Open `local/my-mod/ships/Ship_Pirate_Silver/ship.json` and change `"points": 255` to `"points": 100`.
3. In `local/my-mod/mod.json` change `id` to `my-mod` and `name` to your own (the folder name is the id; the example mods need
   `id`, `name`, `version`, `category`, `description`).
4. Run it from the checkout root: `make run ARGS='--mod local/my-mod'`. Open Skirmish, set your row's fleet type to Pirate in the
   lobby (Silver's ship is a Pirate hull, and the fleet screen lists only your fleet type's ships), then open the fleet screen:
   it is listed under Available Ships as "Pirate Man-O-War" at its new cost, 100.
5. To see exactly what the game got, open `local/overlay/ships/Ship_Pirate_Silver/ship.json` (one line of JSON): the
   overlay holds the base files with your files laid over them, rebuilt at every launch. Restart after every edit.

Three things to know before you go further:

- `"extends": "<Id>"` copies that ship and changes only the keys you give. The same id as the folder changes that ship in place; a
  new id (a new folder) adds a ship beside it.
- A key the game does not know is ignored without a word: a typo such as `"sigth"` does nothing. Compare the spelling with
  [data-format.md](data-format.md#shipjson-keys).
- Nothing changes? Check the folder nesting (`my-mod/mod.json`, not `my-mod/my-mod/mod.json`), that you restarted, and
  ["Troubleshooting"](#troubleshooting).

The longer walk-through below ("Your first mod") builds a mod from scratch.

## Where things live

| Doc | What it holds |
|---|---|
| `docs/modding.md` (this page) | making, testing, installing and sharing mods: the overlay, ships, weapons, sounds, strings, UI art, the render profile |
| `docs/missions.md` | the guide to writing a custom mission (folder layout, `map.json` / `mission.json` / `media.json` keys, a worked example, the test loop) |
| [`docs/rule-types.md`](rule-types.md) | the reference for every mission rule Type: its fields, defaults and what it does (the field tables are generated from `--dump-rule-schema` by `tools/gen_rule_types.py`) |
| `docs/data-format.md` | the layout of the imported `assets/` folder and its JSON files: what a mod file replaces or extends |
| `docs/development.md` | for engine contributors: the daily loop, checks, smoke tests, module ownership |
| `docs/wot-format.md` | notes on the retail property files the importer reads |
| `docs/examples/` | tested example data (`gate-ambush.mission.json`, the mission the guide walks through) |
| `docs/re/*.md` | specifications of the original game's behaviour (rules, numbers, formulas) written from data files, the manual and play, one file per system; read them to learn what a rule does, not to copy |
| `mods/` | the example mods the project maintains: `faction-colors`, `free-camera`, `game-speed`, `remaster`, `silver-skirmish` |

## Your first mod

A Pirate Man-o'-War in skirmish at 255 points, with its own name (`mods/silver-skirmish` is a working copy):

```
my-mod/
  mod.json                                  {"name": "Silver in skirmish", "version": "1.0", "category": "Ships",
                                             "description": "Silver's ship as a skirmish Man-o'-War."}
  ships/Ship_Pirate_Silver/ship.json        {"extends": "Ship_Pirate_Silver", "skirmish": true, "points": 255,
                                             "display_name": "MY_SILVER_NAME"}
  menu/strings.json                         {"MY_SILVER_NAME": "Silver's Man-o'-War"}
```

Paths inside the folder mirror the imported `assets/` folder (its layout: [data-format.md](data-format.md)). `extends` copies a
ship and changes only the keys you give: the parent is the ship folder named by `extends`, and a ship that extends its own id
(as above) changes that ship in place, while a new id adds a copy beside it. Try it with `./openprocyon --mod /path/to/my-mod`
(from a source checkout: `make run ARGS='--mod /path/to/my-mod'`; repeatable, later mods win), or install it as a player does
(below). The overlay is built at launch, so restart after every change; ["Testing a mod"](#testing-a-mod) lists the quick ways to
see the result and ["Troubleshooting"](#troubleshooting) what to check when nothing changes. More examples in `mods/`:
`faction-colors` (JSON only: recoloured effects and a wake tint), `free-camera`, `game-speed`, `remaster` (a whole graphics look),
`silver-skirmish` (this page's example, which also names an existing hull icon). A ship of your own needs only a model and a short
`ship.json` (["A ship from a model"](#a-ship-from-a-model)); a weapon of your own is a gun entry and a bullet entry
(["A new weapon"](#a-new-weapon)).

## Sharing a mod

- **Your mod is yours.** Keep it in your own repository or a zip; players drop it into their mods folder. The OpenProcyon
  repository does not take in third-party mods: `mods/` holds only the examples the project maintains.
- **Nothing from the original game.** Do not ship retail files or anything converted from them (models, textures, sounds,
  strings, maps), not even edited or upscaled. Every player has their own import, so a mod may *use* retail content by
  reference: `extends` a retail ship, name a retail sound or effect, or map a retail texture onto your model with a
  `retail:<texture>` material (see "A ship from a model"). What you ship must be your own work or under a licence that allows
  redistribution; put a `LICENSE` file in the folder.
- **Say which engine you need.** The engine is pre-1.0: file layouts can still change and a mod may need small fixes after an
  update. Give the OpenProcyon version (or later) you tested with. `game_version` and `requires` in `mod.json` are not read yet.
- **You support your mod.** The project maintains the engine and the examples in `mods/`, nothing else. Questions and bugs
  about a mod go to its author. Report a problem to OpenProcyon only when it also happens with no third-party mods switched on,
  or when a few files of your own show it. This guide is the help there is for making mods: there is no promise of an answer to
  questions about a particular mod.
- **Engine changes go upstream.** If a mod needs something files cannot express, open an issue describing it, or a pull request
  ([CONTRIBUTING.md](../CONTRIBUTING.md)). Generic features that help every mod are welcome; a feature only one mod uses is better
  done in that mod.

## Installing and switching on

Unzip a mod into `<user data dir>/mods/` (`~/.local/share/openprocyon/mods` on Linux, `%APPDATA%\openprocyon\mods` on Windows;
`$XDG_DATA_HOME/openprocyon/mods` when that variable is set), so that the mod's own `mod.json` sits at
`mods/<folder>/mod.json`, not one folder deeper (a zip that unpacks to `name/name/` leaves the mod listed but empty). Then switch it
on with the Mods button in the corner of the main menu: a sortable list (the Category pull-down filters; click a
row for its details; tick its box, double-click it or use Switch on / Switch off). The choice is stored in `mods.json` beside that
folder (`{"enabled": ["id", ...], "disabled": ["id", ...]}`). The order of `enabled` is the load order, later wins; the screen has
no reorder control and saves the switched-on mods in list order, so edit `mods.json` by hand when two mods touch the same file.
Ids in `mods.json` that no folder has are ignored. A user mod stays off until switched on; dropping a folder in never switches
anything on. A user mod replaces a bundled one with the same `id` (of two user folders with one `id`, the first found is kept).

Mods that ship with the game live in `mods/` (the checkout's, or next to the executable in a download) and are off until ticked,
except those marked `"default": true` (`free-camera`, `game-speed`), which are on until the player switches them off.

Layers, bottom to top: the imported assets, `content/` (what ships with the code), the switched-on mods in `mods.json` order, then
the `--mod` folders in command-line order. A `--mod` folder needs no switching on and does not appear in the Mods screen.

`mod.json` is optional for the loader (a folder without one, or with one that is not valid JSON, is listed under its folder name
in the category "Other"), but write one: the project's own `tests/test_bundled_mods.py` requires `id` (equal to the folder name),
`name`, `version`, `category` and `description` of every mod in `mods/`. `mod.json` itself is not copied into the overlay; the game
prints `mod <name> <version> <description>` to the console when it lays each mod over the assets.

| Key | Meaning |
|---|---|
| `id` | default: the folder name |
| `name`, `version`, `description` | shown in the Mods screen (`name` defaults to the id) |
| `category` | the Mods screen's filter (default "Other") |
| `missions` | the order of a mod campaign's missions (see the custom-mission row below) |
| `default` | bundled mods only: on until switched off; ignored in user mods |
| `game_version`, `requires` | not read yet (nothing in the engine looks at them; say in your README which version you tested) |

## Testing a mod

| Goal | How |
|---|---|
| load a mod without installing it | `--mod <folder>` (repeatable; a relative path is relative to where you start the game; no need to switch it on; later `--mod`s win). From a checkout: `make run ARGS='--mod mods/x'` |
| start a battle at once | `--map <id>` (a skirmish map such as `red_rover`, or your mod's map), `--mission N`, with `--ship Ship_Id` (your own ship), `--allies A,B`, `--enemies A,B` (`none` for no enemies) and `--speed N` (fast-forward); `--menu skirmish` (or `mods`) opens that screen; `--lobby "map=border_dispute;points=1000;limit=10;slots=human,7*computer"` sets the skirmish lobby up (see `docs/development.md`) |
| a picture | `./openprocyon --mod x --map red_rover --ship Ship_Id --shot out.png --shot-frame 300 [--dist D --yaw Y --pitch P]` writes a 1280x720 PNG and exits; a list `--shot-frame 100,300` writes `out_100.png`, `out_300.png`. In a checkout `tools/shot.sh local/x.png` does the same through cargo and checks the picture is not black (`--script damage`, `fxtest`, `victory`, ... stage a situation; the names are listed in `game/src/smoke/mod.rs`) |
| what the merged files look like | open them in `local/overlay/` (above) |
| mission mistakes | `--check-mission <map id> --mod <dir>`, `--rule-trace`, `--why "<rule>"`, `--fire-rule`, the developer console (`~`); `--dump-rule-schema` needs no assets (all in "Authoring missions" and `docs/missions.md`) |
| numbers | `PROCYON_TRACE=<file.csv>` writes every ship's position, velocity, heading and section hit points (and, in `<file>.shells.csv`, every shell) once per simulation step: compare a gun's range or a ship's speed with what you meant. `PROCYON_PROFILE=1` logs frame times, for the cost of a look (`docs/development.md`) |
| logs | the console shows everything; the same text is written to `<user data dir>/logs/openprocyon.log` (the run before is `openprocyon.previous.log`), starting with the version and your arguments and ending with any panic. The overlay's `mod <name> <version> <description>` line and the retail-texture warning are plain console output and may not be in the log file |

Restart the game after every change: the overlay, the merged tables, the string table, the sound index and the render profile are
all read once at launch.

## What a mod can change

At launch `game/src/modding.rs` builds the overlay, a folder `local/overlay/` beside the imported assets (`<assets>/../local/overlay`:
in a checkout `local/overlay/`, for a player `<user data dir>/local/overlay`). It holds links to the base files (hard links on
Windows, copies when the overlay is on another volume, symlinks elsewhere) with the mod files on top, and is deleted and rebuilt on
every launch. The game, Bevy's asset server included, reads only that tree, so open a file there to see exactly what the game got
after merging (merged JSON is written compact). Mods are laid on in order, `content/` first, and for each mod the files that
carry `"extends"` go last, so they see the mod's own other files.

How a file combines with the one below it depends on its path:

| Path | Rule |
|---|---|
| `data/weapons/guns.json`, `data/weapons/bullets.json`, `data/ai/weapons.json`, `data/collision.json`, `data/docks.json`, `menu/strings.json`, `hud/scales.json`, `data/render/skies.json`, `data/render/nebulae.json` | **merged**, deep: objects combine key by key, so a new key is added and an existing one changes only the keys you give; arrays and scalars replace. A file that is not valid JSON, or not an object, does not just get ignored: the merge replaces the whole table below it with that bad value and the game then fails or shows blank text (see "Troubleshooting") |
| a `.json` file containing `"extends"` | merged over its parent (below); the `extends` key is dropped |
| any other file (every other JSON such as `data/render/look.json`, `data/hud/theme.json`, `data/hud/slices.json`, `data/fx/wake_tint.json`, images, models, sounds, shaders) | **replaced** whole; with two mods the later one wins and the earlier one's file is gone |

`extends` names the parent: for `ships/<Id>/ship.json` and `maps/<Id>/map.json` (and the other files at that depth) it is another
ship or map id, for any other file such as `fx/<Name>.json` it is a sibling file `<parent>.json` in the same folder. The parent is read
from the overlay as built so far, so it may come from the imported assets or from an earlier mod or the same mod. An unknown parent
stops the game at launch (`mod <folder>: extends unknown '<parent>'`).

| Mod file | Effect |
|---|---|
| any file with a base path (`hud/sprites/MainBar_PanicButton/Navy/NoCursor.png`, `ships/Ship_X/model.glb`, `fonts/Sans.otf`, `sound/<kind>/*.ogg`) | replaces the base file: texture packs, upscaled art, new fonts, sounds. A replaced retail `model.glb` keeps that ship's retail `geometry.json` (hit meshes) and sections, so a ship that looks different but hits the same |
| `ships/<Id>/ship.json` containing `"extends": "Ship_Navy_Cutter"` | deep-merged over that ship: object keys override one by one, arrays and scalars replace; a new ship is a few lines (see `mods/silver-skirmish`) |
| an `extends` file may also carry `"set": {"/engines/0/fx": "X"}` (JSON pointers, applied after the merge) to change one value inside an array, which a merge cannot do; a pointer that does not exist stops the game at launch (`mod <folder>: <file> sets '<pointer>', which does not exist`). `mods/faction-colors` uses it to point a ship's four engines at recoloured effect copies |
| `data/collision.json` | merged key by key: a new scenery type's collision (an entry per `type` a map's `objects` name, layout in `docs/data-format.md`), so a mod's own island or rock blocks ships and shells. Without an entry the model is drawn but nothing collides with it |
| `data/docks.json` | merged key by key: `{"<scenery type>": {"points": [{"label", "pos", "yaw"}], "tender": true}}` (layout in `docs/data-format.md`), so a mod's own base or island gets dock points; scenery no rule owns is dockable by the player's side like a retail island |
| `data/render/nebulae.json` | merged key by key: `{"rules": [...], "bolt": {...}}`, the storm and nebula looks a content mod wants on its own maps (same keys as the `nebulae` block of `look.json`). Its `rules` go in front of the profile's (first match wins), its other keys merge over the profile's `nebulae`; a rule matches by effect name only, so it applies on every map while the mod is on. Without a profile with a `nebulae` block it does nothing |
| `data/render/skies.json` | merged key by key: `{"<sky>": {...any `sky` key of `look.json`...}}`, the layers, base and stars of a sky a content mod bakes itself (`models/sky/<sky>_cube.png`). Added under the active render profile's `sky.skies` (the profile's own entry for a sky wins); without a profile with a `sky` block it does nothing, so a mission mod never switches the look on |
| `models/scenery/<Scene>.glb` with glTF animations | every clip in the file loops from spawn, all together (cranes, flags, beacons: node animation, the player on the model's root). Retail scenery has none; its windmills play `data/animations.json` |
| `menu/strings.json` | merged key by key with the retail string table: new ids for names a mod's ships and screens use (e.g. a ship's `display_name`); see "Strings and names" |
| `fx/<Name>.json` | a new effect file is loaded without being listed anywhere (the effect's name is its file name, which a gun's `MuzzleFlashEffect`, a bullet's `hit_fx` / `bullet_fx` or a ship's engine `fx` then names); `extends` (the sibling file `<parent>.json`) + `set` recolours a copy of a retail effect, as `mods/faction-colors` does |
| `data/hud/speed.json` (`{"max": 16}`) | shows the game-speed buttons beside the command bar (not retail), stepping 1x to `max` (default 16); absent: no speed control, as in retail. `--speed N` on the command line works either way |
| `data/camera.json` (`{"pitch": [0.05, 1.5]}`) | the free battle camera (not retail): right-drag up and down tilts it between the two pitches (radians), the wheel and zoom keys move it 10 to 300 units out without easing; absent: the retail camera, whose tilt follows the zoom (`docs/re/camera.md`) |
| `data/fx/wake_tint.json` (`{"Navy": [r, g, b], "Pirate": ..., "Procyon": ..., "Civilian": ...}`) | multiplies the wake texture per fleet type (multiplied in linear light, values may exceed 1); absent: retail colours |
| `models/sky/<sky>_cube.png` (six square cube faces stacked vertically, +X -X +Y -Y +Z -Z in Bevy's cube-map frame) | drawn as the battle camera's skybox instead of that retail sky's sphere (not retail); a render profile's `sky` block also reads `<sky>_stars.png` (the stars on their own) and `<sky>_clouds.png` (painted clouds, straight alpha) when the mod has them, and `data/render/skies.json` (below) tunes the layers per sky; with a render profile's `sky_light` it also lights the scene. A sky made from the player's own imported sky is derived from game content: keep it local, never ship it |
| a new `ships/<Id>/` folder (`ship.json` and a `model.glb`, see "A ship from a model"; or `geometry`/`model` pointing at another ship), a new `maps/<id>/` folder (`map.json`, `backdrop.png`; `"skirmish": true` lists it), `models/<any folder>/*.glb`, `sound/<any folder>/*.ogg` | new content (schema in `docs/data-format.md`) |
| a new `maps/<id>/` folder with a `mission.json` and a `media.json` (both required: the game stops at launch with `missing maps/<id>/media.json` without it; this is the campaign's layout) | a custom mission: listed under Single Player > Custom Missions (the button in the corner, not retail) with its `map.json` `name` and `description`, and started with Play or a double-click (`--map <id>` also works). Each mod is one campaign named after its `mod.json` `name`: its missions play in the order of `mod.json` `"missions": ["<id>", ...]` (missions it does not list follow, by id), and winning one offers the next (N, as in the retail campaign). Ids `mission01`..`mission13`, `tutorial` and `basic_tutorial` are not listed: a mod's copy replaces that retail mission. Victory points of custom missions do not count toward the retail campaign total. Dialogue: each `media.json` `speech` entry gives the subtitle `text`, `duration`, `file`, talking-head `face` and colour (`speaker` and `head` are not read), and the sound is `maps/<id>/speech/<file>.ogg` (found by name, any case, so speech file names must be unique across all sound folders). The engine holds a line for `duration` plus 0.3 s, so set `duration` to the audio's length; an entry without `duration` is subtitle text only, with no sound. In a mission's `map.json`, `music` is not read (the mission's `Play Music Track` rule chooses the music) and `backdrop` is only a flag: the picture is `maps/<id>/backdrop.png`. The `mission.json` layout (ships, paths, volumes, tasks, rules) is in `docs/data-format.md` and `docs/re/campaign.md`; read the imported retail missions as examples, but write your own: a copied or edited retail mission is game content. To test a late part of a mission without playing the start, launch it with `--fire-rule "<rule name>"` (repeatable; `--map <id>`): once the world-initialise rules have run, the actions of each named rule run in the order given as if its conditions had held, and a run-once rule does not fire again. An unknown name is a warning listing the rule names. `--start-at x,y` also moves the primary ship there (game units), since a rule does not. A debug aid for authors, not retail behaviour. To find mistakes before playing, see "Authoring missions" below (`--check-mission` also checks that a ship `type` exists and that the `.ogg` files and the backdrop are there). The same can be done while playing with the developer console (the backquote key or F12): see "The developer console" below. |

Limits (provisional):

- The fleet screen lists, per fleet type (`race` Navy/Pirate/Procyon), every ship whose `ship.json` has `"skirmish": true` (the importer copies retail's "Available in Multiplayer?" flag; a mod can set it on any ship) and gives it `points`.
- Weapons and bullets are shared files, `data/weapons/guns.json` and `bullets.json`, merged key by key across mods, so adding a gun is one entry in each ("A new weapon" below). A ship names a gun, it never contains one.
- Computer captains fit only the guns retail computers were seen to fit (carronades, Gattling, plasma, light/medium cannons, star mortar) unless a mod ships `data/ai/weapons.json` (`{"Gun_Name": true|false}`, merged the same way) to restrict or extend what they may pick.
- A mod may ship sounds of its own under `sound/`, found by name ("Sounds and music" below).
- The full layout of every file is in `docs/data-format.md`.

## Engine extensions (not retail)

This is the one list of what OpenProcyon adds to the original. Everything else aims to behave as the retail game does. Every item
here is marked in the code with a comment that starts `Not retail:` on the module or item that owns it (`game/src/rules/eval/`
marks its additions with `Origin::Engine` instead), and `tests/test_extensions.py` checks that this table and those markers agree:
a file listed here must carry the marker, and a file that carries it must be listed here. A new extension adds a row and the
marker in the same change.

*Gate* is what switches it on: a file in the overlay (a mod, or `content/`), a key in a data file, or "always" for additions that
need no data. *Changes* is `simulation` when it can change what happens in a battle (who hits, moves, wins), `presentation` when it
changes only what is drawn or heard, and `control` when it adds a way for the player to act (keys, menus, the clock) without new
rules. Data keys the engine adds are also listed per file: ship and gun keys in `docs/data-format.md` ("ship.json keys", the
`Source` column; `ENGINE_SHIP_KEYS` / `ENGINE_GUN_KEYS` in `game/src/data.rs`), mission keys in "Mission rule extensions" below.

| Extension | Gate | Changes | Code |
|---|---|---|---|
| Beam guns: charge, then a held beam that burns ships | a `beam` block on a gun in `data/weapons/guns.json` | simulation | `game/src/combat/beam.rs`, `game/src/data.rs`, `game/src/data/weapons.rs` |
| Hulls that burn fuel, so a nebula's drain does not slow them | `"fuel": true` in `ship.json` | simulation | `game/src/data.rs` |
| Mission rule extensions (`OP ` Types, `Is Active`, self-completing tasks, `victory`) | the keys in `mission.json` / `script.json` | simulation | `game/src/rules/eval/schema.rs`, `game/src/rules/check.rs` |
| Mission rules that act on ships and the world (`OP Board Ship`, `OP Abandon Ship`, `OP Teleport`, `OP Follow Group`, `OP Place Mines`, `OP Etherium Current`, the campaign counters) | the `OP ` Types in a rule, and a custom mission's `mines` in `map.json` | simulation | `game/src/boarding.rs`, `game/src/boarding/order.rs`, `game/src/boarding/fight.rs`, `game/src/lifeboats.rs`, `game/src/combat/gunnery.rs`, `game/src/environment.rs`, `game/src/campaign.rs`, `game/src/scenario/spawn.rs`, `game/src/rules/systems.rs`, `game/src/rules/systems/apply.rs`, `game/src/rules/systems/mission_ai.rs` |
| `OP Fade` (the picture fades to black and back) | the `OP Fade` Type in a mission rule | presentation | `game/src/classic/mission_ui.rs`, `game/src/rules/systems/presentation.rs` |
| Skirmish map scripts and their speech | `maps/<id>/script.json`, `maps/<id>/media.json` | simulation | `game/src/rules/skirmish.rs`, `game/src/classic/mission_ui.rs` |
| Custom missions a mod adds, and the Single Player > Custom Missions page | a mod's `maps/<id>/mission.json` + `media.json` (the button is always there) | control | `game/src/campaign.rs`, `game/src/menu.rs`, `game/src/classic/menu.rs` |
| Debug launch options `--fire-rule`, `--start-at` | the command line | control | `game/src/campaign.rs` |
| Developer console | always (`~` or F12; in a release build Display Options > Developer Console or `--console`) | control (it runs rule actions) | `game/src/rules/console.rs`, `game/src/bindings.rs`, `game/src/menu/settings.rs` |
| Game-speed buttons | `data/hud/speed.json` (the bundled `game-speed` mod, on by default) | control | `game/src/panels.rs`, `game/src/classic/panels.rs` |
| Pause key | always | control | `game/src/view.rs` |
| Number keys: primary ship on 1, secondaries on 2..9, 0, `\` | always | control | `game/src/bindings.rs` |
| Mods page and its main-menu button, the HD skies bake | always | control | `game/src/menu/mods.rs`, `game/src/classic/menu.rs` |
| Free camera | `data/camera.json` (the bundled `free-camera` mod, on by default) | presentation | `game/src/view.rs` |
| Cube-map skies | `models/sky/<sky>_cube.png` | presentation | `game/src/view.rs` |
| Wake tint per fleet type | `data/fx/wake_tint.json` | presentation | `game/src/wake.rs` |
| Render profile: sails, plumes, wakes, tracers, bursts, surfaces, deck crews, beams, skies, nebulae, currents, black holes, lights | `data/render/look.json` plus the mod's shaders | presentation | `game/src/render/` |
| Vsync and anti-aliasing options | always (Display Options) | presentation | `game/src/menu/settings.rs` |
| Scene-wide particle budget | always | presentation | `game/src/fx/sim.rs` |
| Debris smoke that stops after a few seconds | always | presentation | `game/src/combat/damage.rs` |
| Damage decals cleared by a full dock repair | always | presentation | `game/src/decals.rs` |
| Flags hung on the nearest staff | always | presentation | `game/src/flag.rs` |
| Loading cover over a battle that is still loading | always | presentation | `game/src/loading.rs`, `game/src/classic/world_hud.rs` |

`game/src/render/` stands for its modules: each one that is part of the render profile starts `Not retail:` (shared helpers such
as `batch.rs`, which the retail effects use too, do not). The mission rule extensions are described in "Mission rule extensions"
and "Skirmish map scripts" below, the render profile in "Graphics mods", the console in "The developer console".

## Authoring missions

The step-by-step guide is `docs/missions.md`; this section is the reference for the checking and tracing flags, and `docs/rule-types.md` lists every condition and action Type with its fields.

The rule interpreter is forgiving: a misspelled group has no members, so a "Group Destroyed" condition never holds; a missing field
reads as "" or 0; an unknown volume is an empty region; a JSON `true` reads as FALSE (booleans are the strings `"TRUE"` / `"FALSE"`).
Three launch options make those mistakes loud. None of them is retail behaviour and none changes how a mission plays.

| flag | what it does |
| --- | --- |
| `--check-mission <map id>` | Loads `maps/<id>/` (`mission.json`, or a skirmish map's `script.json`, plus `map.json` and `media.json`; add `--mod <dir>` as in a normal launch) and checks every rule against the rule schema and the mission's own definitions, without starting the game. It prints one line per problem with the rule, the condition or action and the field, and exits 1 if there are errors. Errors: an unknown Type, a campaign-only Type in a skirmish `script.json`, an unknown or missing field, a group, player, volume, path, objective task or point, point set or map text the mission does not define, a flag or timer that is read but never set or started, a boolean that is not `"TRUE"`/`"FALSE"` (a JSON `true` or `false` is rejected: booleans in rules are always the strings), a number that is not a number or is outside its range (damage, throttle and volume fields are fractions 0..1, not percentages), a value outside an enum such as `Equivalence`. Warnings: a speech event, end message or ship name with no `media.json` entry, an empty or memberless group, a rule with no actions, a repeated rule name, a unit name no ship has. Errors also for a ship `type` with no `ships/<type>/ship.json`, a `Play Music Track` / `Play Special Effect` file or a speech event's `.ogg` that is in no sound folder, and a `map.json` backdrop without `backdrop.png` (skipped without imported assets). A mission whose `mission.json` or `media.json` is missing or invalid logs one error naming the file and the mod, and the game returns to the main menu. `--check-mission all` checks every map the game can see. When a mission from a mod (or a mod's copy of a retail mission) loads in a normal launch, the same check runs and its lines are logged, each prefixed with `mod <name>:`; retail missions are not re-checked. The retail missions pass with warnings only (the original's own typos, such as mission 10's etherium path `ebend`). A line names the map folder and file, then the rule, the condition or action and the field: `mission03/mission.json: ERROR: rule "Convoy Arrives": condition #1 'Enter Volume', field "Volume Name": unknown volume 'Harbor Mouth' (did you mean 'Harbour Mouth'?); it is an empty region that nothing is ever inside` |
| `--dump-rule-schema [path]` | Writes the rule vocabulary as JSON (to `path`, or stdout) and exits; it needs no game assets. Every condition and action Type is listed with `role` (`condition` / `action`), `origin` (`retail`, or `engine` for Types this engine adds), `handler` (`interpreter`, or `victory` for the skirmish end conditions `victory.rs` reads), `campaign_only` (true: a skirmish `script.json` cannot use the Type, `--check-mission` reports an error) and its `fields`: `name`, `kind` (`group`, `player`, `path`, `volume`, `task`, `point`, `timer`, `flag`, `speech_event`, `point_set`, `map_text`, `message`, `team`, `ship_name`, `bool`, `number`, `enum` with its `values`, `text`), `default` (what the interpreter reads when the field is missing), `required`, `used` (false for fields the retail editor exports and the engine ignores), `range` and `soft`. A top-level `kinds` list gives the editor placeholders (`NO PATH`, `NO PLAYER`, ...) that mean "nothing chosen". Use it to lint or generate missions instead of scraping the source. The table is `game/src/rules/eval/schema.rs`; a unit test fails when it and the interpreter's match arms disagree |
| `--rule-trace`, `--why "<rule name>"` | While the mission plays, log why rules have not fired: `--rule-trace` (or `PROCYON_RULE_TRACE=1`) for every rule that has not fired, `--why` (repeatable) for the named rule only (a name that is part of a rule's name picks every rule containing it). Each line lists the conditions that are false and what the world says about the names they read (group size and living members, flag value, timer reading, speech played or not, volume vertices). A rule is reported when it first waits and again only when the set of false conditions changes, so the log stays short. A `--why` name that matches no rule is a warning listing the rule names |

At the start of every mission the interpreter also warns, once per rule and name, about a group, volume, path or point set a rule
names that the mission does not define (`mission rules: rule 'X': group 'Fleat' in field 'Group Name' of 'Group Destroyed' names no group, player or unit of the mission`).

## Mission rule extensions (engine-added, not retail)

A custom `mission.json` can use a few additions to the retail rule vocabulary. **None of this is in the retail game**: every new Type starts with `OP ` (so it can never clash with a retail name), every new key is optional, and a mission that uses none of it plays exactly as before. The retail rule format is unchanged: a rule is `{"name", "Run Once", "Is Active", "conditions": [...], "actions": [...]}`, its conditions are ANDed, its actions run in order. Boolean fields of the new Types are the retail strings `"TRUE"` / `"FALSE"` (a JSON `true` reads as FALSE, `--check-mission` reports it); only the rule field `"Is Active"` and the task key `required` take JSON booleans.

**Conditions**

| Type | Fields | Meaning |
|---|---|---|
| `OP Any Of` | `Conditions`: list of condition objects | true if any of them is true (an empty list is false). Every member is evaluated, so edge conditions inside (`Enter Volume`, event quotas) advance their state exactly as at the top level |
| `OP Not` | `Condition`: one condition object | true when the inner condition is false. It inverts that frame's answer: an edge condition (`Enter Volume` in a repeating rule, a kill or capture quota) is true for one frame only, so `OP Not` of it is false on that frame and true on every other. Use a level condition (`Is Group In Volume`, a flag) or `Exit Volume` when you want "not inside". An event the inner condition would report is never consumed. Missing `Condition`: warning, false |
| `OP Counter Condition` | `Counter Name`, `Equivalence` (`Greater Than`, `Less Than`, `Equal To`, as `Timer Condition`), `Value` | compares a counter; a counter that was never set reads 0 |
| `OP Group Is Cloaked` | `Group Name`, `Boolean` (default TRUE) | TRUE: every live member is fully cloaked; FALSE: none is. A ship that is cloaking or decloaking is not cloaked; a group with no live member holds neither way |
| `OP Group Speed` | `Group Name`, `Which` (`Fastest` or `Slowest`), `Equivalence`, `Speed` | the fastest or slowest live ship's own speed in units per second, along its heading (a current's or a collision's push is not counted), against `Speed`; only ships have one. "The whole group has stopped" is `Fastest` `Less Than` a small number |
| `OP Task State` | `Objective Task`, `State` (`Active`, `Complete`, `Failed`) | `Active` is shown and neither complete nor failed; the flags are however they were set (rules or `complete_when` / `fail_when`) |
| `OP Campaign Counter Condition` | `Counter Name`, `Equivalence`, `Value` | as `OP Counter Condition`, on a campaign counter (below) |

Both combinators nest (`OP Not` inside `OP Any Of` and so on). "And" is what a rule's condition list already is.

**Actions**

| Type | Fields | Meaning |
|---|---|---|
| `OP Set Counter` / `OP Add To Counter` | `Counter Name`, `Value` (number, may be negative) | counters are named numbers local to the mission, 0 until set |
| `OP Wait` | `Seconds` | pause this rule's action list; the actions after it run `Seconds` later |
| `OP Wait Until Speech Done` | `Speech Event Name` (empty: the whole speech queue) | pause until that speech event is neither queued nor playing; with an empty name, until nothing is queued or playing. Waiting for an event that is not pending ends at once, so put it after the `Play speech event` it waits for |
| `OP Set Rule Active` | `Rule Name`, `Boolean` | activates or deactivates every rule of that name (a warning if none exists) |
| `OP Set Campaign Counter` / `OP Add To Campaign Counter` | `Counter Name`, `Value` | a counter the next mission of the same mod campaign starts with (below) |
| `OP Board Ship` | `Group Name`, `Target Group` | each ship of the group boards the nearest live ship of the target group, by the order a player gives (below) |
| `OP Abandon Ship` | `Group Name` | the crews launch their lifeboats and the hulls stay afloat, disabled (below) |
| `OP Teleport` | `Group/Unit Name`, `Point Set Name`, `Point Index` (optional, default 0), `Heading` (optional, degrees) | `Teleport Group/Unit` with a chosen point, a heading, no stacking and no speed left (below) |
| `OP Follow Group` | `Group Name`, `Target Group`, `Station` (`Astern`, `Ahead`, `Abeam Starboard`, `Abeam Port`), `Distance` | the ships keep station on a ship of the target group (below) |
| `OP Place Mines` | `Point Set Name`, `Bullet` (optional, default `Bullet_StarMine_Med`), `Player/Owner` (optional) | lays a mine at every point of the set (below) |
| `OP Etherium Current` | `Etherium Path`, `Strength`, `Width`, `Boolean` (all optional) | sets up, changes or switches off an etherium current (below) |
| `OP Fade` | `Seconds` | fades the picture to black and back (below) |

*Ship orders and the world (engine-added).* All of these are applied by the code that owns the behaviour, with the message or state a player's own order uses, and a mission that does not use them plays as before. A name that matches no ship does nothing (`--check-mission` and the start-of-mission audit say so as for any group).

- **`OP Board Ship`** makes each ship of `Group Name` take the Grapple and Board orders of the right-click menu against the nearest live ship of `Target Group` (bases and islands are not targets). The boarder sails into harpoon range, fires its harpoon, and once the hook holds the boarding starts: the same odds, winch pull, capture (the owner changes, `Team has captured a ship from Group/Unit` counts it) and release as a player's boarding. **No harpoon:** a ship with no live harpoon mount cannot grapple, so nothing happens; the log says "cannot board". The same when the target is not hostile to the boarder (an alliance, or an owner not in `players`) or the target has `Set Group/Unit Boardable` FALSE (the order then waits as long as the grapple holds, and boards when it is set TRUE). **Out of range:** the grapple order steers the ship toward the target until it is within harpoon range (100 units hull to hull), so a boarder that can move gets there; one that cannot (pinned, towed) never does. The order is dropped when either ship is destroyed, the grapple is lost, the fight has started, or the target stops being an enemy while the hook holds (someone else captured it, an alliance changed). A ship that was abandoned (`OP Abandon Ship`) takes no board order and loses one it had. The player's own ship takes the same order. The boarding fight itself, including its odds, is unchanged (`docs/re/boarding.md`).
- **`OP Abandon Ship`** sends the group's ships through the lifeboat code a death by damage uses: the boats, their drift and their pickup are the same, and the ship's Create Lifeboats flag (`Set Lifeboat Creation State` TRUE; a class that carries no boats leaves none) decides whether there are boats. The hull stays afloat and alive for the rules (`Group Destroyed` is false) but disabled: its throttle cap is held at 0 so it cannot move (a tow still moves it), it takes no orders from its captain and no gun fires, and a later destruction leaves no second set of boats. Its beams stop at once, and it collects no lifeboat, its own or anyone's (the boats it launched drift off as after a death). It can be boarded and towed. Nothing reverses it. PROVISIONAL: the boarding crew strength is not reduced, so boarding an abandoned hull is a fight as before.
- **`OP Teleport`** places the group's ships at the point `Point Index` of the point set (0 is the first; an index that is negative, not whole or past the end of the set moves nothing and warns once, and `--check-mission` reports it). The group's centre goes to the point and each member keeps its current offset from the centre, so a group of two does not stack. `Heading` is in degrees in game axes as a compass bearing: 0 faces +y (up the map), 90 faces +x, the same sense as "clockwise from north" in `docs/missions.md` section 2. With it every ship takes that heading and the layout turns with the group's first ship (rotated about the centre by the change of its heading); without it every ship keeps its heading and the layout is not rotated. Speed, turn rate and the push of a contact or a current are cleared, and as with `Teleport Group/Unit` the course (waypoints) is dropped. Animals and scenery move too, without heading. `Teleport Group/Unit` is unchanged.
- **`OP Follow Group`** gives each ship of `Group Name` a standing order to keep `Station` at `Distance` units from the centre of a ship of `Target Group`, in the leader's frame: `Astern` behind it, `Ahead` in front, `Abeam Starboard` / `Abeam Port` beside it. The follower chooses the nearest ship of the group when it first needs one and keeps that one until it is destroyed; it runs at full speed until near its station and then matches the leader's speed (the fleet followers' rule, `ship.rs`), keeps clear of the leader's hull, and still fires at its target. The order ends with `Clear all AI commands`, `Group to follow path`, `GroupA to attack GroupB`, `GroupA to Ram GroupB` or `Set Group/Unit Hold Position` TRUE, or a new `OP Follow Group`; there is no separate stop. With no live ship in the target group the follower stops and resumes when one appears. Ships given the same station and distance on the same group line up one behind the other along the station's direction (each a hull and 10 units further out, in spawn order) instead of sharing one slot. A ship is never told to follow a group it is itself in (the action skips those ships and warns); two different groups told to follow each other chase each other's stations, so do not write that. The mission AI carries it out, so only a campaign mission follows: a skirmish script stores the order and nothing steers (PROVISIONAL, like the attack orders in "Skirmish map scripts"). The human's own ship does not follow. PROVISIONAL: the station is where the leader is now plus the offset, with no lead for its turns, so a following ship swings wide on a leader that turns hard.
- **`OP Place Mines`** lays one mine of `Bullet` at each point of `Point Set Name` when it runs, the kind a map places before the battle (homing mines that hit ships; only point-defence guns clear them). With `Player/Owner` they fight for that player's side (hostile to the human's side when that player is, by the mission's alliance table); without it they are hostile to every ship, the human's included. An owner that is neither hostile nor allied to the human (not in `players`, for instance) counts as the human's side, as that owner's ships do. A bullet that does not exist lays none and the log says so. Mines a custom mission's `map.json` lists under `mines` (`[{"type": "Bullet_StarMine_Med", "id": 7, "pos": [x, y, z]}]`) are placed at the start, give each an `id` so groups can name it, and `Set Group/Unit Owner` hands one to a player as in a skirmish. **In a mission a mine answers only to an explicit group** (its `id` in `groups`), never to its owner's name: `Group Destroyed Pirates` holds once the Pirates' ships are gone although their mines rest, `Group has X members Pirates` counts ships, and an owner-wide order (destroy, teleport, set owner) does not reach the mines. Mines laid by `OP Place Mines` have no id, so no rule addresses them afterwards. (A skirmish map's script keeps its existing behaviour: there a mine also answers to its owner's name.) The retail missions' `mines` key is never read (only custom missions' is).
- **`OP Etherium Current`** is a setup and control action for a current along a mission path (`paths`). It finds the current by the name of that path (a current the retail `*State Init* Setup Etherium Current` made counts) and makes one when none runs there. `Strength` multiplies the retail push (default 1; the push is a force divided by the ship's mass, so a light ship gains much more speed than a heavy one), `Width` is the lane's full width in units (default 120), `Boolean` FALSE switches the push and the drawn ribbon off, TRUE on. A setting that is left out stays as it is. Without this action currents behave exactly as before.
- **`OP Fade`** darkens the whole picture to black and back over `Seconds` in all, half each way, so the screen is black at the middle (time a `Teleport` or a scene change there with an `OP Wait` of half the time). It is drawing only: nothing in the simulation reads it, a new fade replaces one that is running, and it follows the mission clock (it holds while the game is paused). The cover lies over the world, the HUD and the cutscene bars but under the mission's texts: the cutscene speech banner, the gameplay subtitle, the hint and the end text stay readable, so a fade can be spoken over. A mission that ends during a fade shows its end text on a clear picture. Campaign missions only (`--check-mission` reports it in a skirmish script).
- **Campaign counters.** `OP Set Campaign Counter`, `OP Add To Campaign Counter` and `OP Campaign Counter Condition` are the counters of `OP Set Counter` that outlive the mission. A custom mission starts with the values the mission before it in the same mod campaign (the order of `mod.json` `missions`) had when it was won; a mission that is lost or restarted hands on nothing, so a replay starts from the same values as the first try, and the first mission of a campaign, or one started on its own from the Custom Missions list after an unplayed predecessor, starts with none. They live in memory for the session: they are not written to a save or to disk, and quitting the game forgets them (PROVISIONAL: saving them is future work). Retail missions and skirmish scripts have them too, but nothing is handed on from or to them. `--check-mission` does not report a campaign counter that is read and never set in the mission, because an earlier mission may have set it.

*Optional fields.* On these Types an optional field written as the empty string `""` is the same as leaving it out (`Heading`, `Point Index`, `Strength`, `Width`, the optional `Boolean`s): the interpreter and `--check-mission` both read it so.

*Sequences.* When a rule fires, its actions run until the first wait that is not over yet; the interpreter keeps a cursor for the rule, and the remaining actions run in the step the wait ends (rules are evaluated after the resumed actions, so a flag set by the tail is seen by later rules in the same step). A cutscene is therefore one rule: `NIS Start`, `OP Wait`, `Teleport Group/Unit`, `OP Wait`, `NIS End`. Time waits stand still while the opening-briefing pause holds the timers, and speech waits follow the speech queue, which is paused then as well. If a repeating rule's conditions hold again while its sequence is still waiting, that firing is skipped: sequences never overlap and nothing is queued (an event condition keeps its pending event, so it is reported once the sequence has finished; a level condition simply fires again after it). A run-once rule has already fired and is not affected. A rule that is deactivated (`OP Set Rule Active` false, by itself or another rule) while its sequence waits stops: the rest of its actions never run, so a deactivated cutscene does not finish.

*Rule activation.* The rule field `"Is Active"` is honoured: `false` (or `"FALSE"`) means the rule is not evaluated at all, so none of its conditions advance their edge state either, until `OP Set Rule Active` turns it on. A rule without the field is active; every retail rule says `true`. Activating a run-once rule that has already fired does not make it fire again.

**Self-completing objectives.** In `mission.json` `tasks`, a task may carry `complete_when` and `fail_when` (lists of condition objects, ANDed; any condition Type, `OP ...` included) and `required` (bool, default false). While a task is active and neither complete nor failed, the engine marks it failed when `fail_when` holds, otherwise complete when `complete_when` holds (failing wins if both hold in the same step); a decided task stays decided. Condition state advances only while the task is active. The task's active state is still set by `Set Objective Task Active State`, or `"Active": true` in the task. A task without these keys is driven by rules as before. The optional mission-level key `"victory": "objectives"` then ends the mission: lost ("Mission failed") as soon as any `required` task has failed, won once every `required` task is complete (a required task that was never activated keeps the mission going, and a mission with no required task never ends this way). The end text is "Mission complete." / "Mission failed" unless the mission names media `messages` ids in the optional `victory_message` / `defeat_message` (as `End Game` names its loser message). Rules such as `Goto Next Level` and `End Game` keep working next to it.

*Checking and tracing.* `--check-mission` knows all of this (see "Authoring missions"): it checks the conditions nested in `OP Any Of` / `OP Not` like top-level ones (a flag or timer that is never set inside a combinator is a warning, not an error), reports a counter that is read but never set, an `OP Set Rule Active` that names no rule of the mission (an error), a task's `complete_when` / `fail_when` / `required`, and the mission keys `victory`, `victory_message` and `defeat_message`. `--dump-rule-schema` lists the `OP ` Types with `"origin": "engine"`. `--rule-trace` / `--why "<rule>"` also say when a rule is inactive (not evaluated) or busy (its sequence is waiting, so a new firing is skipped), and explain the nested conditions and counters.

## Skirmish map scripts (engine-added, not retail)

A skirmish map may carry `maps/<id>/script.json`: the same rule list as a mission's `mission.json` (`docs/data-format.md`), run by the same interpreter, so its rules can use the whole rule vocabulary (`docs/rule-types.md`) and the `OP ` Types above. This section is about what differs from a mission. A map without any of the keys below plays exactly as before, and the 14 retail skirmish maps are unchanged.

*What works.* Every action is applied by the code the campaign uses, so `Group to follow path`, `Set Group/Unit Owner`, `Teleport Group/Unit`, `Destroy Group/Unit`, `Set Group/Unit Vulnerability`, `Set Ship Name`, `Dock Ships`, `Tow Ship`, cloaks, hold position, stances, effects and the rest do the same in both. **Not yet:** the orders the campaign's mission AI carries out (`GroupA to attack GroupB`, `GroupA to Ram GroupB`, border zones and their return points, restarting a looping path) are stored on the ship but the skirmish captain AI does not read them yet (PROVISIONAL), so they have no visible effect in a skirmish. Group specs (`Group`, `Group,Unit`) resolve against the map's `groups`, the players and the names of placed ships. Two things stay specific to a skirmish: `Set Group/Unit Owner` also moves a ship to the side its new owner fights for, and `Set Group/Unit Hold Position` is the captain's hold. The script's units are the map's ships, space animals and dragons, moving asteroids and placed mines; bases are not script units. Some Types only make sense in a story mission (the cutscene camera `NIS ...`, the HUD overlay, `Goto Next Level`, `Set Current Objective Point`, `Set Fleet Primary Ship`, `Set Group/Unit AI Captain`, the tutorial and mission 9 hooks): the schema marks them `campaign_only` (`--dump-rule-schema`, and "campaign only" in `docs/rule-types.md`). `--check-mission <id>` reports one in a `script.json` as an error, and the game logs a warning naming the rule and Type when the map loads; the rule runs but that action does nothing. `Play Music Track` is read but the skirmish keeps the map's own music. The rules step reads only the battle's state and its own timers (no wall clock, no randomness), so a script behaves the same on every machine.

*Who decides the game.* By default the map's end rules are the ones in `map.json` `victory` (what victory.rs reads, as for the retail maps), and a script's `End Game`, `Team X Wins` and `Remaining Team Wins` actions are ignored. To let the script decide, set `"victory": "script"` at the top of `script.json` (`"objectives"` means the same here). Then:

- `Team X Wins` (`Team Name`: a `map.json` `teams` `name_id`) names the winner; `Remaining Team Wins` (or a rule with only `End Game`) lets the survivors win when they are one team (a mutual alliance is one team). `End Game` supplies the end message ids, `Winner - Custom Message String ID` and `Loser - Custom Message String ID` (game string ids starting `IDGS_`; a placeholder such as `GAME STRING` means the default line). The first rule that speaks in a step decides; the end rules are checked like the retail ones, about once a second.
- The self-completing objectives (`complete_when` / `fail_when` / `required` in `tasks`) work per team: a task names its `team` (a team `name_id`). A team with a failed required task loses and the other teams win; the first team whose required tasks are all complete wins. A required task without a `team` never decides a skirmish (`--check-mission` says so). `victory_message` / `defeat_message` name the end message ids.
- `map.json` `victory` rules keep working next to it and are tested first. Their conditions may now name a group of the script instead of listing object ids: `"objects": [12, "Convoy"]` or `"group": "Convoy"` (an unknown group disables the map's `victory` rules with a warning).
- The game summary shows the human's result: Victory when their team wins, Defeat when it loses, with the end message of the side they are on.

*Speech and messages.* An optional `maps/<id>/media.json` (the mission layout: `speech`, `messages`, `ship_names`; `docs/missions.md` section 5) gives the script its lines. `Play speech event` plays the sound `maps/<id>/speech/<file>.ogg` and shows the subtitle above the bottom bar (the cutscene banner and talking head stay with missions); `Set Ship Name` reads `ship_names`. Without a `media.json` a speech event does nothing, as before. `--check-mission` checks references against it when it exists.

*Example.* Two convoys race; the first team to deliver wins:

```json
{
  "victory": "script",
  "rules": [
    {"name": "Navy delivers", "Run Once": true,
     "conditions": [{"Type": "Enter Volume", "Group Name": "Navy Convoy", "Volume Name": "Harbour"}],
     "actions": [{"Type": "Play speech event", "Speech Event Name": "NAVY_WINS"},
                 {"Type": "Team X Wins", "Team Name": "IDGS_TPTEAMNAMES_NAVY"},
                 {"Type": "End Game", "Winner - Custom Message String ID": "IDGS_TPINGAMEMESSAGE_GAME_WON",
                  "Loser - Custom Message String ID": "IDGS_TPINGAMEMESSAGE_GAME_GENERAL_NAVYDEFEAT"}]}
  ]
}
```

## Writing a mission

`docs/missions.md` is the guide to writing a custom mission: the folder layout, every key of `map.json`, `mission.json` and
`media.json`, the worked example `docs/examples/gate-ambush.mission.json` rule by rule, storms, scenery, behaviours that surprise,
the test loop (`--check-mission`, the console, the autopilot) and a checklist before you share it.

## The developer console

The console key (the backquote key `~` by default, rebindable as "Open Developer Console" in the Controls screen; F12 always works) opens a console in a battle that runs a mission's rules (not skirmish scripts). It is a development aid, not retail
behaviour. It is on in debug builds; in a release build turn on Display Options > Developer Console, or start the game with
`--console` or `PROCYON_CONSOLE=1`. While it is open the battle is paused and the game sees no keyboard or mouse. A command either
builds a rule action or condition and runs it through the same interpreter as the mission's rules, or reads the mission's state, so
every action a mission can use is available (`act {json}`). `help` lists the commands; `help <command>` gives usage and an example.

Tab completes the command, then its arguments: rule, group, point set, flag, timer, task, counter, volume, path and speech names
from the mission, the rule schema's Types, and inside `act {json}` the Type, the field names and a field's values. Tab again cycles
through several matches. A name the mission does not have is refused with the closest one ("no group 'Raider'; did you mean
'Raiders'?"). A name with spaces goes in "quotes"; Tab adds them. A unit is written `Group,Unit`.

The output keeps the last 2000 lines. PageUp / PageDown or the mouse wheel read back through it, Ctrl+Home / Ctrl+End jump to the top and bottom, and the panel says how many lines lie above or below. New output moves the view only if you were at the bottom; pressing Enter returns to it.

Using a command that changes the game marks the battle as cheated: "CHEATS USED" shows on screen and its victory points are not
added to the campaign total. Commands marked * below do that; queries and view-only commands do not.

| group | commands |
|---|---|
| Cheats | `god [on\|off]`* (the player's fleet takes no damage), `repair [group]`*, `kill`* (the current target), `reveal`* (shows hidden units), `tp <x> <y> [z]` or `tp <point set>`* |
| Mission state | `fire <rule>`* (run a rule's actions now, as `--fire-rule`; part of the name is enough), `why <rule>` (which conditions hold, and whether it is inactive or waiting in an `OP Wait`), `flags`, `timers`, `tasks`, `counters`, `set <flag> [on\|off]`*, `timer <name> start\|stop`*, `task <name> active\|complete\|failed [on\|off]`*, `counter <name> <value>`* |
| Mission flow | `rules [text]` (every rule: fired, ran, never fired, inactive, waiting), `skip`* (end the speech line playing), `nis end`*, `win`*, `lose`*, `restart` |
| Ships | `ships [text]`, `inspect` (the current target: orders, tow), `primary <group>`*, `damage <0..1> [group]`*, `attack <group> <group>`*, `stance <group> <stance>`*, `ally` / `hostile <player> <player>`*, `tow` / `untow <group> <group>`* |
| Finding names | `list groups\|paths\|volumes\|points\|tasks\|timers\|flags\|counters\|players\|speech\|objects [text]` (groups show their member ids and names, objects their id, type and position, speech the event id and the start of its subtitle), `find <text>` (searches all of them, and ships and rules, and marks each hit with its kind) |
| Authoring | `where` (the player's position in game units), `mark <name>` (appends `{"name", "pos"}` to `console_marks.txt` in the user data folder, ready to paste into a mission), `show volumes\|paths\|points [name]` and `hide ...` (draw them in the world, labelled) |
| Camera and sound | `cam free\|player\|follow <group>` (free camera: arrows move, PageUp / PageDown rise and sink, right-drag looks, Shift is faster), `hud on\|off`, `fps [on\|off]` (a frame-rate counter in the top-right corner: smoothed rate, frame time, the worst frame of the last second, and the game speed when it is not 1; it stays with `hud off`, no argument toggles, and the choice is saved in `settings.json`), `say <speech event>`*, `music <track>` |
| Raw rule access | `act <json>`* and `cond <json>` (checked against the rule schema; JSON `true` / `false` are accepted), `schema <text>` (the fields of a Type) |

## A ship from a model

A ship needs no retail ship to extend: a folder `ships/<Id>/` with a `model.glb` and a small `ship.json` is a complete ship. The
engine reads the model and derives what the file leaves out, so the ship behaves like a retail one (hits, damage sections, wake,
engines, crew, boarding, and the command-bar and weapon-panel art). Whatever `ship.json` states wins over what is derived; every derived
value is PROVISIONAL (the retail files state each one, the engine only needs a sensible value).

**Layers.** Every mesh node of the model is a layer. With no `geometry.json` (and no `"geometry"` naming another ship), the hit mesh of
each layer comes from the model's own triangles. With no `sections`, each intact layer becomes one damage section: hull (150 HP, vital to
the ship, an equal share of the core) or, when its node name contains `sail`, `mast`, `cloth` or `canvas`, cloth (70 HP, vital to speed,
no core share). Layers that touch (boxes within 0.3 of each other; hulls with hulls, cloth with cloth) pass hits on to each other. Name a damaged variant `<A>Damage_<B>` for layer
`<A>_<B>` to have it swapped in when the section is destroyed, as the retail hulls do; a destroyed section with none just disappears. A
`sections` object in `ship.json` replaces the derived ones (the retail schema, `docs/data-format.md`); its `layer` names the mesh node.

**Hardpoints.** Empty nodes mark where things go, in the ship's own frame (glTF axes: x starboard, y up, z aft; the exporter and
Blender's glTF export both write this). Give the node an extra `kind` (Blender: a custom property on the empty), or just name it so the
engine can tell:

| `kind` | name contains | derives |
|---|---|---|
| `GunPlacement` | `mount`, `lancer`, `turret` | a mount, joined to the `guns` entry of the **same name**; `ship.json` says which `gun` and its `arc_deg` |
| `EnginePortPlacement` | `engine` | an engine plume (`<Race>Engine_<Light/Medium/Heavy/ExtraHeavy>` for hulls shorter than 14 / 22 / 32 units, else the last; `NavyEngine`, `PirateEngine`, `ProcEngine`, `CivillianEngine` (sic)) |
| `WakePlacement` | `wake` | a wake (`Wake_Light/Medium/Heavy` for hulls shorter than 14 / 28 units, else Heavy); with none, one at the stern's waterline |
| `FlagAttachmentPoint` | `flag` | the flag pole's ends (names with `top` and `bottom`) |
| `BoardingEffectPoint` | `boarding` | where boarders cross |
| `ToweePoint`, `TowerPoint`, `TorpedoHomingPoint` | `tow` (front/back), `homing` | tow and homing points |
| (none) | `Crew_Walk_<lane>_<k>`, `Crew_Gun_*`, `Crew_Post_*`, `Crew_Work_*`, `Crew_Climb_<shroud>_a`/`_b` (the whole name) | where deck crews and townsfolk walk, man guns, stand, work (and carry loads between work places) and climb (drawn only by a render profile's `deckhands` block, see below) |

A node's heading about up is its rotation about glTF y (or the extra `rot_raw[0]`): 0 faces forward, a quarter turn to port is +90
degrees. A gun entry may still give `pos`, `rot`, `parent_layer` itself; `parent_layer` otherwise is the nearest hull layer.

**What `ship.json` can leave out.** `physics` (keys `Mass`, `MaxThrust`, `MaxSpeed`, `RotationalFriction`, `MaxAngularAcceleration`; defaults about a Navy Cutter's: 4000, 60000, 8, 20, 3), `core_hp` (a
quarter of the hull sections' HP, at least 50), `sections`, `engines`, `wakes`, `flags`, `explosion_fx`, `engine_snd`, `repair_fx` (by
race and size), `crew_posts` (one gunner per two guns, a captain, mate, navigator, engineer, a rigger per sail, a fighter or two),
`sphere`, extents, `icon`. Other optional keys the loader reads: `sight` (default 400), `ring` (0.65), `cloakable` (true only for ids containing `Submersible`), `fuel`, `lifeboats`, `is_lifeboat`, `model` and `geometry` (another ship's folder to take `model.glb` / `geometry.json` from), `names.json` (a list of names open skirmish maps pick from). Unknown keys are ignored. `race` picks the engines, sounds and HUD skin; `display_name` is a string id the mod adds in
`menu/strings.json`; `points` and `"skirmish": true` list the ship on the fleet screen. Crew and boarding points of a ship that
`data/boarding.json` does not list come from `crew_posts` and the model.

**Retail textures in a mod model.** A mod may not ship game art, but its models can use the player's own: name a glTF material
`retail:<texture>` (the retail texture's file name without `.tga`: `retail:o_main_body`, `retail:proc_front_tri_sail`; the names are the
materials of the imported `ships/*/model.glb`). When the overlay is built, each such material is given that texture from the import
and renamed to the retail name (`o_main_body.tga`), so the engine's name rules (a name containing `sail` is a sail, render-profile
`surfaces` rules) work as for a retail model. The model needs UV coordinates; repeat the texture as you like (the sampler repeats), and
give the material a flat `baseColorFactor` as a fallback look. The baked model exists only in the overlay (`local/overlay`, git-ignored),
so nothing converted from the game is committed; a name the import does not have leaves that material untextured and logs a warning.
Applies to `ships/` and `models/` GLBs of a mod.

**UI art.** A ship with no `hud/sprites/ShipBar_ShipIcon_<art>` and `WeaponBar_ShipIcon_<art>` (`<art>` = its `icon` without the prefix, else
its id without `Ship_`) gets them made from the model when the overlay is built: a solid pale side silhouette for the command bar and
fleet cells, and a thin top-down hull outline for the weapon panel, drawn like the retail ones (`game/src/shipart.rs`). Ship your own PNGs
(`hud/sprites/...`, see "UI art and fonts") to replace either. A gun with no `bank` takes the weapon-panel slot nearest to its place on the outline
(the panel has 80 fixed slots), one bank per gun; name the same `Bank<NN>` on several guns to have them fire together.

**If it will not load.** Every ship folder is read at launch, so one bad ship stops the game before the menu, with a message that
names the file (it is also the last thing in the log, see "Testing a mod"):
`ships/<Id>/ship.json: <parse error with line and column>`; `sections.<label>.layer references missing layer "<name>" in
ships/<Id>/model.glb (derived)` (the `layer` is the mesh node's name); `guns.<mount>.gun references unknown weapon "<gun>"`
(the gun is not in the merged `guns.json`); `guns.<mount> has no pos/rot and the model has no hardpoint node named "<mount>"`
(name an empty node like the mount, case does not matter, or give `pos` and `rot`). A model with no mesh nodes gives a ship with
no sections.

## A new weapon

A gun is an entry in `data/weapons/guns.json`, the shell it fires an entry in `data/weapons/bullets.json`, and a ship mounts it in
its `guns` object. Both files are merged key by key, so the mod's files hold only your entries. Do not copy a retail entry: read
one in your imported `assets/data/weapons/` to learn what a field means, then write your own numbers. The names and numbers
below are an illustration (PROVISIONAL: tune them in play).

`data/weapons/guns.json`:

```json
{
  "Gun_Spark_Light": {
    "MuzzleSpeed": 160.0,
    "TimeBetweenBursts": 4.0,
    "BulletsPerBurst": 1,
    "DuringBurstReloadTime": 0.0,
    "EffectiveRange_Range": 120.0, "LongRange_Range": 180.0, "MaximumRange_Range": 240.0,
    "EffectiveRange_AccuracyDeviation": 0.01, "LongRange_AccuracyDeviation": 0.02, "MaximumRange_AccuracyDeviation": 0.04,
    "BulletAffectedByGravity": false,
    "HorizontalRotationSpeed": 1.2, "VerticalRotationSpeed": 1.0, "VerticalMinAngle": -0.1, "VerticalMaxAngle": 0.5,
    "BulletTypeName_Primary": "Bullet_Spark",
    "MuzzleFlashEffect": "PlasmaCannon_Heavy_Muzzle",
    "SoundName": "Weapon_Spark_Fire", "SoundVolume": 1.0, "SoundMaxDistance": 600.0,
    "Mount Type": "Medium Mount", "Victory Point Cost": 6,
    "visual": {"base": "gun_Spark_Base.glb", "barrel": "gun_Spark_Barrel.glb", "pivot": [0.0, 0.3, 0.3], "muzzles": [[0.0, 1.5, 0.3]]}
  }
}
```

`data/weapons/bullets.json`:

```json
{
  "Bullet_Spark": {
    "lifetime": 6.0, "hitpoints": 1,
    "hit_fx": "Lancer_Medium_Impact",
    "impact_snd": {"Wood": ["Weapon_Spark_Hit"]}, "impact_vol": 0.8, "impact_max": 400.0,
    "damage": {"Wood": [8, 12, 0, 0, 0], "Reinforced": [6, 9, 0, 0, 0], "Iron": [4, 6, 0, 0, 0],
               "Cloth": [8, 12, 0, 0, 0], "Stone": [2, 3, 0, 0, 0], "Dragon": [6, 9, 0, 0, 0]}
  }
}
```

and in `ships/<Id>/ship.json` (`Forward_Mount` is an empty node of that name in `model.glb`, or the entry has `pos` and `rot`):

```json
{"guns": {"Forward_Mount": {"gun": "Gun_Spark_Light", "arc_deg": 40}}}
```

`PlasmaCannon_Heavy_Muzzle` and `Lancer_Medium_Impact` are retail effect names, used by reference (an effect is a file `fx/<name>.json`,
yours or the import's). `gun_Spark_Base.glb` and `gun_Spark_Barrel.glb` are your own models, shipped as `models/weapons/<name>.glb`;
`Weapon_Spark_Fire` and `Weapon_Spark_Hit` are your sounds (see "Sounds and music").

**Gun keys.** These must be present, or the game stops at launch with a bare `called Option::unwrap() on a None value` or
`number in ship json` panic that does not name the key (the location is `game/src/data.rs`): `MuzzleSpeed`, `TimeBetweenBursts`
(seconds between bursts), `BulletsPerBurst`, `DuringBurstReloadTime` (between the shots of a burst), the three `*_Range` and three
`*_AccuracyDeviation` keys (effective, long and maximum range band; deviation in radians), `BulletAffectedByGravity` (a JSON
`true` / `false`; true makes the shell drop, with a gravity computed from `MuzzleSpeed` and the maximum range), the four turret keys
(`HorizontalRotationSpeed`, `VerticalRotationSpeed` in rad/s, `VerticalMinAngle`, `VerticalMaxAngle` in rad), `BulletTypeName_Primary`
(a key of `bullets.json`), and `visual.pivot` and `visual.muzzles` (a list of points; a shell leaves each muzzle). Optional:
`MuzzleFlashEffect`, `SoundName` / `SoundVolume` (default 1) / `SoundMaxDistance` (600), `Is PointDefense Gun`,
`Is TorpedoLauncher Gun`, `Is Lobbing Gun` with `LobAngle` (a fixed high arc), `Can Be Fired While Cloaked`,
`CalculateAccuracyDeviationDuringBurst`, the three `*_RicochetFactor` keys, and `visual.base` / `visual.barrel` (GLB file names,
found by name anywhere under `models/`, so keep them unique). `pivot` is where the barrel pitches and `muzzles` are the shell origins,
in game axes (x starboard, y forward, z up) in the base's frame; the barrel mesh is authored in the same frame, not about the pivot.
A retail gun's `visual` block in your import is a worked example. What is drawn for a gun with neither `base` nor `barrel` is not
tried (PROVISIONAL).

**Bullet keys.** Required: `lifetime` (seconds; a shell older than this is removed), `impact_vol`, `impact_max` (the impact sounds'
volume and maximum distance), `impact_snd` (an object, may be `{}`) and `damage`. `damage` is keyed by the six materials `Wood`,
`Reinforced`, `Iron`, `Cloth`, `Stone`, `Dragon`, each a list of five numbers; the engine reads the first two as the low and high of
the roll per hit (PROVISIONAL: a search of `combat/` finds no use of the other three), and a material you leave out takes 0 damage. A
section's material string picks its key by the first of those words it contains (else `Wood`). Optional: `hitpoints`, `hit_fx` (the
impact effect), `bullet_fx` (an effect that rides the shell; with neither `bullet_fx` nor `glb` the shell is a small placeholder
sphere), `glb` (the shell's model, a name under `models/`), `impact_snd` lists per material (one is picked at random),
`ricochet_snd` (`{material: sound}`), `travel_snd` (`[sound, volume, max distance]`, a loop while the shell flies), `decal`
(`Small`, `Medium`, `Large`), `physics` (a guided shell, as the torpedoes have) and the payload keys `blast` (`radius`, `duration`),
`mine`, `net` and `kind` (`NovaMortar`, `GravCharge`); read the retail bullets that use them.

**Mounting and buying.** A ship's `guns.<mount>` has `gun` (a key of the merged `guns.json`); `arc_deg` (the width of the firing arc
centred on the mount's heading: the engine allows the gun to fire up to half of it to either side; gun arcs are PROVISIONAL);
`bank` (`Bank00` and on, the weapon-panel slot: guns with the same bank fire together and are swapped together; with none each gun
takes the free slot nearest its place on the outline); and `pos`, `rot` (`[heading in radians, 0, 0]`) and `parent_layer`, which the
model's node supplies when left out. On the Arms screen a bank can be swapped to your gun when the gun has a `Victory Point Cost`
(without one it cannot be bought), its `Mount Type` is the same string as the stock gun's (a stock gun whose type starts with
`Non-Selectable` cannot be swapped at all) and its `Exclusion Race` is not the ship's race (`None` or absent: any); a gun marked
`Is PointDefense Gun` is never offered. Computer captains fit it only when `data/ai/weapons.json` says
`{"Gun_Spark_Light": true}`.

**Beam guns (not retail).** A gun with a `beam` block charges and then holds a beam instead of firing shells:
`{"charge": s, "duration": s, "tick": s, "ramp": per s, "max_power": x, "width": units, "color": [r, g, b]}` (defaults 1.5, 3, 0.25, 0,
1, 0.4 and [0.5, 1.5, 2.0]; every key optional). It charges for `charge` seconds with a glow at its first muzzle, then holds a beam
along its barrel for `duration` seconds and reloads for `TimeBetweenBursts`. The beam burns the first enemy ship it crosses once per
`tick` with the bullet's damage table (so one tick's damage; give the bullet `lifetime` 0), times a power of 1 + `ramp` x the
seconds it has stayed on that ship (at most `max_power`); `SoundName` plays once when the charge starts, so one sound can carry
the charge and the beam. It passes through bases, rocks and scenery (PROVISIONAL). For a beam gun `MuzzleSpeed` only sets the
turret's lead (use a large value), and `EffectiveRange_Range` is where computer captains with a bow gun turn away.
A beam that is on a ship lights the hull where it bites, brighter as the power ramps up. The classic look is a core and a halo; a
render profile with a `beams` block draws the beam as light instead ("Graphics mods: the render profile", `beams`).

## UI art and fonts
UI text is vector, laid out with the retail font advances (`hud/fonts/<name>.json` holds metrics only, no glyph sheets). Slots: BlackChancery* uses `fonts/Chancery.otf`, names containing Bold use `fonts/SansBold.otf`, every other Univers/Immi face uses `fonts/Sans.otf`; a mod replaces a slot by dropping a file at that path (`content/fonts/`: Chancery = URW Z003 for the BlackChancery slots, Nimbus Sans for Univers/Immi; both URW base35 fonts, AGPL with font exception, so they ship with the code).
Every UI sprite is its own PNG, `hud/sprites/<component>/<skin>/<state>.png` (`<component>` is the retail `hud.hdt` name such as `MainBar_PanicButton`, `<skin>` is
`Navy`, `Pirate` or `Procyon`, `<state>` is `NoCursor`, `CursorOver`, `CursorDown`), without its fully transparent border and with the colour under transparent pixels
cleaned (so scaling never pulls a halo in). `hud/sprites/<component>/<skin>.json` says how big the untrimmed sprite is and how much was trimmed (`docs/data-format.md`).

### HD UI art
Layout is always in retail reference pixels. **To replace a sprite, ship `hud/sprites/<component>/<skin>/<state>.png` at any size**: the game draws the whole image
where the retail one was, and a sprite's scale is its PNG size divided by the size of its trimmed retail art, so a 2x, 4x or odd-sized replacement needs no other
file. Keep the art inside the trimmed rectangle (a pixel-exact redraw of the retail picture at N times the size); art that should reach further needs the
`<skin>.json` replaced too (`size`, `trim`; a JSON file replaces the base one whole). A state with no file of its own (`CursorOver` on most buttons) uses
`NoCursor`; to give one its own art, add the file and a matching entry under `states` in `<skin>.json`. Each sprite is replaced on its own, so a pack can redo
only the bar. Offsets and numbers stay in `hud/hud.json`; a mod that ships it replaces the whole file, so leave it alone unless the layout should move.

The main bar is one image per faction, `hud/bar/<skin>.png` with `hud/bar/<skin>.json` listing the part boxes (see `docs/data-format.md`); the importer composes
it from the sprites above (`tools/compose_bar.py`), so a pack that only redoes some part art needs no extra step beyond re-running the import. To redesign the
bar, ship both files of a skin in the mod: draw the whole bar in the PNG (any resolution; its json is in reference pixels) and give each part's `x`, `width`
and `height` where its button, gauge or ship icon sits. The part names are the sprite names of `data/hud/bar.json`; parts the game drives (the buttons' hover and
pressed art, gauge fills, the ship portrait) are placed on those boxes.

The old way, replacing a whole atlas `hud/tex/<sheet>.png` at 2x and declaring it in `hud/scales.json`, is gone: the atlases are not exported or read any more.
The front-end chrome (panels, text buttons, pull-downs, check boxes, scrollbars, sliders, slot and stat-bar frames, framed windows) is not art any more: it is drawn from
`data/hud/theme.json` (see "Front-end theme" below). Bitmap replacement applies to the pictorial sprites (portraits, ship and weapon icons, flags, the logo, the scenery backdrops,
the ornate in-game window and dialog frames) and to the battle HUD. The stretchy strips of the battle HUD (the tooltip plate, the
ship-name ribbon) are single 9-slice nodes built from `hud/tex/slice_<group>_<skin>_<state>.png`. The groups are data, `data/hud/slices.json`:

```json
{"plate": {"left": "MyPlate_Left", "center": "MyPlate_Center", "right": "MyPlate_Right",
           "states": ["NoCursor", "CursorOver"], "caps": [12, 12]}}
```

`left`, `center` and `right` are component names (the sprites under `hud/sprites/`); `center` may be left out (two pieces are butted); `states`
defaults to `["NoCursor"]`; `caps` are the reference pixels of each end that must not stretch, default the widths of `left` and `right`. The game
reads the caps from this file; `python3 -m tools.make_slices <assets dir> [slices.json [out dir]]` joins the pieces into the strip PNGs (the importer runs
it for the file in `content/`). So a mod that adds a group overlays `data/hud/slices.json` and ships the strip PNGs it names (for HD, a `hud/scales.json`
entry named like the file, e.g. `{"slice_tooltip_navy_nocursor": 2}`, merged across mods, which is the only use of `scales.json` left). Code that draws
a group asks for it by name, so a new group only changes art that already uses one of the names above, or a mod's own screens.

**Front-end theme** (`data/hud/theme.json`, shipped in `content/`): the palette and geometry of the chrome the engine draws itself instead of stretching art, currently the
gold-rimmed panels and dark list boxes of the Ships / Crew / Arms screens (the picture `menu/shipscreen.png` is no longer drawn there; `menu/gamelobby.png` is the scenery
under them). Colours are `#RRGGBB` (`panel_fill`, `box_fill`, `band_fill`, `edge`, `rim`, `rim_light`), lengths are 800x600 reference px (`rim_width`, `edge_width`,
`panel_radius`, `box_radius`, `shadow_blur`), `panel_alpha` and `shadow_alpha` are 0..1. A mod overlays the file; a key it leaves out keeps the shipped value. All values
are PROVISIONAL (measured on the retail Navy art). `button` (text buttons, drawn as pills) and `pulldown` (pull-down boxes) each name the five states `idle`,
`hover`, `pressed`, `selected` and `disabled`; a state is `{"fill": [five colours], "rim": [two colours]}` (`#RRGGBB` or `#RRGGBBAA`): the fill is a vertical gradient (top edge, lit peak, dark body, bottom
reflection, bottom edge; round buttons use the lit peak and the dark body as a sphere highlight), the rim colours are a disabled button's grey gold or a pull-down's top and bottom
glow. `rim_width` is the gold of a button's rim in reference px. Rims are tubes: a dark `rim_outline`, gold (`rim` on the sides, `rim_light` on top, a `rim_core` line down its middle), a dark
`rim_keyline` and a hard `rim_shadow` band under the bottom edge; `rim_width` at the top level is the gold of the big panels. Fills are recessed by an `inset_shadow` (`inset_size` px in
panels, `box_inset_size` in boxes) and boxes carry a soft `glow` (`glow_size` px) along their inside edge. Scrollbar and slider tracks use `track_fill` (top, bottom) and thumbs and
knobs `thumb` (left, right) for a cylinder gloss; the check-box tick is `tick` with a white core. Framed windows (map list, summary table, lobby players, Arms help box) use `window_fill` and `window_rim` (with a glow of the rim colour), `heading` is the band across a table's top and `picked` the bar behind the chosen list row; the ornate in-game menu window and dialog frame stay bitmap pictures. Check boxes use `box_fill` and `rim_light`; scrollbar and slider tracks use `box_fill` and `edge`, their thumbs, knobs and round step buttons the `button` looks, and the slot, ship-icon and stat-bar frames `box_fill`, the rim colours and `slot_radius`. The retail text-button, pull-down and scrollbar strips are no longer built or drawn.

**Front-end backdrops** (`menu/gamelobby.png`, `menu/shipscreen.png`, the `menu/frontendscreen<N>.png` scene and `menu/frontendmap.png` overlay) fill the whole
window: the 800x600 canvas that every retail screen is laid out on stays centred on top at its own scale, while the picture is scaled up until it covers its frame
and cropped about the centre (the scene and the overlay cover the band between the grid strips, which run on to the window edges). A mod ships a wider backdrop
under the same file name at any aspect ratio (2560x1080 for 21:9, say) and it fills a wide window without code; the picture shows whole, whereas the retail
files carry spare rows below their picture, which the engine leaves out (the visible height is `min(height, width x 0.75)`, for the scene `min(height, width x 394/800)`).

## Sounds and music

Sounds are Ogg Vorbis (`.ogg`; no other format is built in). The game finds a sound by **name**, not by path: the name is the file
name without extension, compared in lower case, and the file may sit anywhere under `sound/` (or, for dialogue, under
`maps/<id>/speech/`). So `"SoundName": "Weapon_Spark_Fire"` is `sound/weapons/weapon_spark_fire.ogg` (or `sound/mine/spark.ogg`
renamed to match). The index is built once at launch from the overlay. A name no file has falls back to `sound/<name>.ogg`, which
the asset loader fails to load (a message in the log); nothing plays.

- **Add a sound**: ship `sound/<folder>/<name>.ogg` with a name no retail sound has (names must be unique across the whole tree).
- **Replace a sound**: ship a file at the same path as the base one (`sound/weapons/...`), so there is still one file of that name.
  A file of the same name in a different folder makes two candidates and which one wins is not defined.
- **Where names are written**:

| Sound | Named in |
|---|---|
| a gun's shot | the gun's `SoundName` (+ `SoundVolume`, `SoundMaxDistance`); one sound per shell of a burst |
| a shell's impact | the bullet's `impact_snd` (`{material: [names]}`, one picked at random), `impact_vol`, `impact_max`; ricochets `ricochet_snd`; a flight loop `travel_snd` |
| a ship's engine | the ship's `engine_snd`: three names, for the Half, Full and Emergency orders (a ship from a model gets `Engine_Navy_*`, `Engine_Pirate_*` or `Engine_Proc_*` by race); a loop, silent at All Stop |
| ship and scenery damage | built-in pools by name: `Explosion_Heavy01`..`08` (a ship or base destroyed), `Explosion_Med01` (a section lost), `Damage_Wood01`..`05`, `Damage_Metal*`, `Damage_Sail*`, `Damage_Stone*` (storms, meteors, rocks, a death sequence), `Ship_Collision_Wood01`..`05`, `Ship_Collision_Metal*` (ships touching). Replace the files to change them. Which event uses which pool is PROVISIONAL (`docs/re/sound.md`) |
| music | a map's `music` key in `map.json` (a track name; default `BTL_NavyBig_Full02`), played in a loop; the main menu plays `FrontEndMusic` |
| speech | `maps/<id>/speech/<file>.ogg`, named by the mission's `media.json` |
| interface sounds | by name in `sound/ui/` (`HUD_*`); replace the file |
| the helmsman's lines | `voice/<file>.ogg`, chosen by `content/voice/events.json`; replace a file at the same path |

How loud a sound is: the player's volume sliders times the `*Volume` of the data; a positional sound plays at full volume within
25 units of the camera and falls off with distance (halving per doubling, silent past its maximum distance; PROVISIONAL, as are
the engine-loop level 0.25 and the music level). A sound with no position (interface, music) ignores distance. Use
`SoundMaxDistance` / `impact_max` (a few hundred units for a gun, thousands for a big explosion) to make a sound carry further.
To hear one quickly: the developer console's `music <track>` and `say <speech event>` (see "The developer console"), or fire the
gun in a `--map` test (see "Testing a mod").

## Strings and names

`menu/strings.json` is one flat object, string id to text (`{"IDGS_MY_SHIP": "Silver's Man-o'-War"}`). A mod's file is merged into
the retail table key by key, so ship only your own ids and use a prefix no retail id has. Where an id is read: a ship's
`display_name` (the name the front-end menus show: fleet and ship screens). A `display_name` with no entry, or a ship with
none, shows the folder name with `Ship_` removed and `_` made a space (`Ship_Pirate_Silver` -> `Pirate Silver`); other screens read
retail ids and show nothing for an id the table lacks. The table is read once at launch. Only the one table (the language the
import made) is used: there is no per-language switching. A name pool for open skirmish maps is `ships/<Id>/names.json`, a plain
list of strings. Mission texts (briefing, speech subtitles, end messages) live in the mission's own `media.json`, see `docs/missions.md`.

## Graphics mods: the render profile (not retail)
Graphics upgrades are mods, never engine forks. The engine (`game/src/render/`) provides building blocks; a mod switches
them on and tunes them with one file, `data/render/look.json`, and ships the shaders most of them draw with
(`shaders/sail.wesl`, `plume.wesl`, `wake.wesl`, `tracer.wesl`, `surface.wesl`, `burst.wesl`, `smoke.wesl`, `sky.wesl`,
`blackhole.wesl`, `ribbon.wesl`, `nebula.wesl`, `volume.wesl`, `bolt.wesl`, `current.wesl`, `deckhands.wesl`, and any a `materials`
rule names; the engine ships none, and a block whose shader is missing stays off with a warning in the log). The shaders of
`mods/remaster/shaders/` are a working set to start from. Without `look.json` every part renders as classic, and a profile only
changes drawing, never rules. Every key is optional; an absent block leaves that feature classic. `look.json` replaces as a whole
file (it is not merged), so with two graphics mods the later one's profile is the only one, and a syntax error in it silently leaves
the classic look; `data/render/skies.json` and `nebulae.json` are the two parts a content mod can add without owning the profile.

| Key | Effect |
|---|---|
| `camera` | HDR camera: `bloom` (intensity, default 0.15), `bloom_threshold` (only light brighter than this blooms; 0 = Bevy's natural bloom, which hazes the image), `tonemapping` (`tony_mc_mapface`, `agx`, `aces`, `blender_filmic`, `reinhard_luminance`), `saturation`, `contrast`, `exposure` (stops), `ssao` (crashes with some effects meshes, keep false; the player's MSAA option falls back to SMAA with it; anti-aliasing itself is a player option in Display Options, not part of the profile), `vignette` (corner darkening), `aberration` and `aberration_pulse` (colour fringing at rest, and added by a blast's camera jolt), `contact_shadows` (screen-space sun shadows where parts touch; adds a depth prepass) |
| `sky` | a dome over the baked sky (needs `models/sky/<sky>_cube.png` and `shaders/sky.wesl`): `near` (one layer object or an array of at most 3 layers: `distance`, `opacity` 0..1, `threshold` and `softness` of the brightness kept, `drift` the map-wide current in world units a second, game axes, `scale` noise patches across the layer (0 = the plain copy of the sky), `tint`, `source`, `haze`), sparse soft copies of the sky's gas that slide against it as the camera moves (parallax; a far and a near layer give two speeds), broken into wisps by slowly churning noise. `source` is `"sky"` (default: the gas of the baked sky) or `"clouds"`: the layer then reads the painted clouds cube `models/sky/<sky>_clouds.png` (beside `_cube.png`, same six stacked faces and direction mapping, 8-bit sRGB RGBA with straight alpha: rgb the lit cloud colour in the cube's brightness convention, alpha the coverage) instead: coverage is alpha times `opacity`, colour is rgb times `tint`, `threshold` and `softness` act on alpha (both default 0 for clouds: alpha passes unchanged), `scale` noise eats the thin edges and leaves dense cores, each layer index looks the painting up through its own turn, and the coverage of cloud layers hides the stars behind them (gas layers never do). A cloud layer on a sky with no clouds cube draws nothing. `haze` (0..1, default 0) is aerial perspective: that share of the layer's colour sinks into the backdrop in its direction (and loses contrast), so a far layer is paler than a near one. Cost per cloud layer: one cube read plus the noise reads. `base` (`flow` radians, `cycle` seconds, `breath` depth, `period` seconds, `tint`, `brightness` multiplier, default 1) also redraws the far sky at alpha 1 from two lookups that flow along noise and cross-fade, with a slow breath in the bright cores; `brightness` leaves the separate star layer and scene lights unchanged. `flash` (`color`, `strength`, `seconds`, `reach`) is how a storm's lightning (`nebula::StormFlash`) lights the gas in its direction, at half strength `reach` world units from the camera and zero at twice that distance (default 1200, 0 = no falloff); `events` (`[{match, color, strength, seconds}]`, a fragment of an effect's name) warms the gas on the side of a big explosion; `shooting_star` (`every`, `brightness`) and `sheet_lightning` (`every`, `strength`, `color`) come from the shader's own clock. `skies` (`{"skybox_05": {...any of the keys above...}}`) merges per-sky accents over the defaults, by the baked cube's base name. `stars` (`brightness`, `twinkle` depth 0..1 for the bright ones, `speed` twinkles a second, `gain` decoding the 16-bit bake, 16 like the baker's) draws a separate stars cube (`models/sky/<sky>_stars.png`, baked with `--split-stars`; the stars are then left out of `_cube.png`), each star twinkling on its own phase, the brightest above 1 so they bloom; events never light the stars. Replaces the retired `sky_drift`, a rigid turn of the whole sky that read as the camera rolling |
| `sky_light`, `ambient`, `ambient_floor` | image-based light (cd/m²) computed once from a mod's cube-map sky (`models/sky/<sky>_cube.png`), the multiplier on the flat ambient light (default 1), and an optional minimum for each channel of the map's flat ambient colour (linear RGB, 0..1). Both ambient settings also apply without a baked sky |
| `sun` | `shadows`, `shadow_distance` (cascades out to this many units), `color` (multiplies the map's sun colour), `intensity` (multiplies its strength) |
| `fx_gain`, `shell_gain` | multiply ONE/ONE effect colour and the shells' spheres, so they pass 1 and bloom |
| `lit_smoke` | blended particles (smoke, dust) take the scene's lights: explosions light their own smoke |
| `lights` | point lights on effects: `effects` (a fragment of the effect file name) and optional `also_matches` (a second required fragment, e.g. `Plasma` and `_Impact`; first entry that matches wins), `color`, `lumens`, `range`, `radius` (apparent source radius in world units for broader specular highlights; default 0, independent of `range`), `seconds` (quadratic fade; 0 = steady while the effect lives), `flicker` (share), `cone` (full angle in degrees: a spot light facing along the effect's forward instead of a point light shining all round; 0 = point), `offset` (units along that forward: down the shot for a muzzle or out from the surface for a hit with a known normal), `shadows` (casts shadows; a point light renders the scene six times, so use it rarely). Effect lights are otherwise not occluded: a point light on a gun also lights the deck and hull behind it |
| `deckhands` | crews on the decks of ships (and townsfolk on map scenery) whose model marks their places with empty nodes (`Crew_Walk_<lane>_<k>` lanes, joined in order and wherever two points are within 2.3 units; `Crew_Gun_*` places by a deck mount, facing it; `Crew_Post_*` officers and lookouts; `Crew_Work_*` a figure working on the spot, loading or hauling by turns and now and then carrying a load to the nearest other work place; `Crew_Climb_<shroud>_a`/`_b` the foot and top of a shroud; `Crew_Count_<n>` the figures the model wants in all; the node's heading is the way the figure faces). The figures and their moves come from the mod's `data/render/deckhand.json` (bake format version 2, written by the expansion's `tools/build_deckhand.py`), `deckhand.bin` (a bone-transform texture: a quaternion and a translation per bone per frame, so a second figure on the same skeleton adds no texture and any figure can play any clip) and `deckhand_mesh.bin` (each figure's rest-pose mesh, one per level of detail: position, normal, bone and colour slot per vertex); the GPU poses every vertex, one material serves all figures and `shaders/deckhands.wesl` shades them. The header lists `rigs` (a skeleton: bone count and its first texel column), `figures` (a name, a rig, bounds, and its meshes by level of detail, the first full and the last the far one: the baker makes it from one box per bone), `parts` (the eight colour slots: `skin`, `coat`, `trousers`, `shoes`, `hat`, `trim`, `metal`, `hair`) and `clips`, found by name: `idle`, `walk`, `run`, `haul`, `load`, `climb`, `cheer`, `fall` are required, `talk` and `carry` optional (they play `idle` and `walk` when missing), at most 16 clips in all. Gun crews load while their mount reloads, the rest wander (running for 10 s after a gun fires; walking to the right of their lane and stopping a little short of their walk point at a random spot; a wanderer that stops by another idle one turns to it and talks), riggers climb the shrouds when the sails fold or open, figures fall as the hull's core HP drops (30% stay to the end), sink away a second before they are removed, and the player's side cheers a victory. How many a ship carries grows with its deck (the collision box's length times beam, `(area / 16.5)^1.52`: 1-2 on a fishing boat, about 7 on a cutter, 19 on a frigate, 40 on a Man-o'-War, `per_ship` on a first rate; gun places, posts and work places are always manned, a ship with shrouds keeps one wanderer per shroud to go aloft, the rest wander) unless its model has a `Crew_Count_<n>`. Scenery figures wear the `Civilian` outfits, one wanders per three walk points (or `Crew_Count_<n>` in all; at most `per_scenery`), and they only wander, stand, work and cheer. `per_ship` (most figures on a ship, a ceiling; default 60), `per_scenery` (most on one piece of scenery; default 150), `range` (camera distance beyond which they are not drawn, default 160; a crew further than that and a 30 unit fade is not simulated either), `lod_distance` (camera distance beyond which a figure wears its far mesh, with a 10% margin either way; default 45), `outfits` (`race`, `officer` for the posts, `figure` the figure that wears it, by the bake's name, the first when absent, and the colour slots `skin`, `coat`, `trousers`, `shoes`, `hat`, `trim`, `metal`, `hair` as linear RGB; at most 32). Each figure is drawn between 94% and 106% of the baked height (1.0 is about a metre and three quarters). No shadows. PROVISIONAL: a prototype; the lane offsets, stopping spots, chat and sink times and the LOD distance are eyeballed |
| `sails` | glTF materials whose name contains one of `match` (default `["sail"]`) get `shaders/sail.wesl`: `glow` RGB (0 for names in `plain`), `speed`, `cells` (pattern cells across the sail), `params` (four numbers for the shader, also settable one by one as `line`, `wave`, `steady`, `fill`), `backlit`, `transmission`, `opaque` (solid, casts shadows), `billow` (bellies out between its edges, more with speed), `furl` (share of each sail's height folded up to its top edge, the yard, over 1.5 s while its ship is ordered to All Stop; 0 = never), `fan` (`races` (default `["navy"]`) and `match` (material name fragments): those sails fold like a hand fan instead. A fan is a half disc of wedges meeting at a hub (found where many triangles meet; pieces under 40 vertices, the jibs, keep the roller fold). It shuts by the `furl` share (0.9: a slim bundle, since a fan shut all the way has no width): a main sail (middle rib pointing up) swings in from both sides and up onto its mast, every rib keeping its length, and its hub slides off the yard onto the mast, so the folded sail lies against it. The mast is the most upright long spar on the ship's centre line beside the fans, found in the other parts of the sail's model node. A fan under the hull folds down along the same line, and a side sail closes flat to a level line. The cloth between the ribs pleats out of the sail's plane as it shuts (`pleats` ribs, `depth` a share of the sail's size). Needs `furl`) |
| `black_holes` | a disc of the hole's pull radius in its plane, drawn with `shaders/blackhole.wesl` (without `replace` over the retail ribbons, which become the infalling streams). Every size is a share of the pull radius, so all three hole sizes scale: `horizon` (default 0.25, exactly `Pull::core()`'s share: the black area is the kill zone), `ring` (thickness of the bright ring on the horizon's edge, never thinner than a pixel and a half), `lensing` (how hard the sky is bent toward the centre, 1 = a radian at the core's edge; drawn by the `sky` dome, which bends the far sky, its gas layers and its stars around each hole and blacks out a round shadow just past the core, always behind the ships; without the dome nothing bends), `disc` (`color` the cooler outer colour, `inner` and `outer` radii, `spin` turns a second clockwise seen from above like the pull, `brightness` past 1 to bloom), `replace` (default false: hide the retail glow, beam and particle layers, `BlackHoleRibbons_*`, so only the profile draws the hole, and draw the hole's own vortex strips, the retail geometry whose spiral motion stays, as thin, hot, streaking threads with `shaders/ribbon.wesl`: bright toward the core, cooling and fading outward, gone at the kill core; `ribbons` scales their brightness and `ribbon_color` sets their cooler colour (default `[1, 0.38, 0.12]`). The hole keeps pulling and its trail sim keeps running, only the drawing changes), `glow` (warm halo), `drop` (how far under the hole's plane the disc lies, so hulls draw over it), `funnel` (`depth` a share of the radius, default 0 = the flat disc; `throat` share, default 0.06; `rim` the outer water's colour): with a depth the hole is drawn as a whirlpool, a funnel sinking under the plane the ships sail (a 1 / r gravity well: nearly flat at the pull radius, plunging toward the core), its water swirling clockwise in log-spiral bands from faint violet at the rim through pink and orange to a bright gold lip at the kill core, past which the throat falls into black; the hole's vortex strips sink along it into the throat, and the bent sky and the round shadow are centred at the bottom of the throat and shrink to the throat's size, so the plane itself never bends. The disc is thin and crisp: a hard hot white-gold inner edge cooling to orange and red, fine log-spiral arms, a sharp photon ring on the horizon, Doppler-brighter on the side moving toward the camera (the lensed far side of the disc arching over the shadow is not drawn). It only adds light (its alpha blacks out the core alone), flashes for a second when the hole swallows a ship, and changes nothing about the pull |
| `nebulae` | nebula clouds and solar storms (the retail additive shell stays as the envelope: its texture, shifting vertices, colour loop and edge fade) get `shaders/nebula.wesl`, which also marches a short ray through the cloud's ellipsoid and adds churning, domain-warped gas to it: `bolt` (`{color, core, max_length, width, glow, brightness, seconds, segments, jitter, branches}`, draws each storm bolt itself, with `shaders/bolt.wesl`: the storm-wide random segment `nebula::StormFlash` now carries (`from`, `to`, `hit`; a bolt that finds no ship is announced too, PROVISIONAL, dimmer, and silent) as a jagged, branching camera-facing ribbon, a white-hot thread of `width` world units in a glow `glow` times as wide, `brightness` past 1 to bloom, flickering out over `seconds`; `max_length` caps the visible span in world units, centred on a miss or ending at the ship on a hit (0 = full segment, the default); the volume is lit along the displayed segment), `replace` (block or per rule, default false: hide the retail shell, and for a `SolarStorm` rule (with `bolt` present) the retail `Lightning` sprite, so only the profile draws the cloud; the cloud sim, storm timers, damage and sounds are untouched), `rules` (`[{match, replace, absorption, dust, max_opacity, light_steps, sun, density, steps, scale, churn, color_low, color_high, shell, inside, flash, flash_reach}]`, a fragment of the cloud's effect name (`Nebula`, `SolarStorm`), first match wins; `density` strength, the dense cores past 1 bloom; `steps` 4..24; `scale` noise patches across the cloud; `churn` noise units a second; `color_low`/`color_high` thin and dense gas; `shell` share of the retail shell kept; `inside` share of the gas drawn when the camera is inside the cloud, where the far wall marches from the camera; `flash` and `flash_reach` (share of the radius) how a bolt of storm lightning, `nebula::StormFlash`, lights the gas around it from inside for 0.3 s; the flash fades as the camera moves away from the displayed bolt, from full brightness at `flash_reach` times the cloud radius to zero at three times that distance). Without `replace` the gas only adds light (ONE/ONE), so it can never hide a ship, and a cloud no rule matches keeps its retail material. With `replace` the cloud is a volume (`shaders/volume.wesl`, needs the camera's depth prepass, which the profile turns on): the cloud's proxy box is drawn on its back faces (so also from inside) and its ray march keeps emission and absorption, composited premultiplied, ending at the scene's depth so a ship or island inside the gas is only veiled by the gas in front of it. `absorption` is the optical depth across a radius at full density, `dust` the strength of dark lanes (extinction without light), `max_opacity` the most one cloud covers (0.6; 60 % of that where the ray ends on a hull), `light_steps` (0..3) steps toward the sun for the self-shadow, `sun` the sun's strength on the gas; `core_glow` (0..12, default 0) adds HDR light only inside dense pockets without raising the opacity cap. Dust still absorbs it, and it is reduced inside the cloud and in front of ships and islands. `density` is then the baseline emission, `color_low`/`color_high` the thin and dense gas, tinted by the retail colour loop; `inside` scales the gas with the camera inside. `color_alt` and `variation` (0..1) let the gas drift into a second hue in large patches, so a cloud is not one flat colour. For a solar storm, `swirl` (turns a second near the centre, slower outward; two phases are blended so the gas never winds up) wheels the gas round the centre like a cyclone, `filaments` is the brightness of hot plasma seams along the edges of the billows, and `flicker` of lightning flickering deep inside it a few times a second, so it reads as a storm, not a quiet cloud. The shell's breathing (`Cloud::breath()`) sets the ellipsoid, the mission's fadeout distance fades it. The noise is baked once at startup into a 64³ tiling texture |
| `currents` | the two scrolling strips of an Etherium current (retail mesh, UVs and scroll kept: u across, v along the path from its first point to its last, which is the way the force pushes) get `shaders/current.wesl`: flowing streaks stretched along the flow, soft edges, sparkles riding downstream faster than the streaks, slow pulses that run down the current so its direction reads at a glance, and a core past 1 that blooms: `brightness`, `streaks` (across the strip), `sparkle`, `core`, `texture` (share of the retail texture kept), `color`, `pulse`. Added light only (ONE/ONE) |
| `hide_effects` | a list of fragments of retail effect file names (`["Explosion_", "MeteorStorm"]`, not case-sensitive): those effects are not drawn, so a mod can draw its own instead of stacking on the retail look. Drawing only: the effect keeps running (particles, timers, lights, sounds, and whatever the `lights` and `bursts` blocks hang on it through `fx::EffectStarted`). `black_holes.replace` and a replacing `SolarStorm` rule hide their own effects without it |
| `flags` | `flag::Cloth`: `flutter` (ripples across the flag), `lit`, `wind` (follows the ship's speed) |
| `engines` | a plume per jet (an exhaust whose effect name contains `effects`, default `Engine`; Procyon funnels are `SmokeStack_Smoke`) drawn with `shaders/plume.wesl`: `layers` (`[{width, length, inner}]`, shares of `width`/`length`, which are shares of the ship's length), `brightness`, `core` and per-race `colors` (`default` otherwise), `particles` (share of the retail flame sprites kept, jets only) |
| `wakes` | the retail wake drawn as one ribbon per `planes` tilt (degrees, default `[0, 60, -60]`; `[0]` = the retail ribbon) with `shaders/wake.wesl`: `height` (squashes tilted ribbons), `brightness`, `hot`, `core`; the faction tint comes from `data/fx/wake_tint.json` |
| `tracers` | a streak of light behind every shell in flight and a glowing head, facing the camera, drawn with `shaders/tracer.wesl`; the streak shrinks into the shell's last point once it is gone: `bullets` (`[{match, color, width, seconds, head, stretch, smoke}]`; `stretch` draws the head longer along the flight, `smoke` (`{color, width, widen, seconds, rise}`) is a ribbon of smoke the shell leaves hanging in the world along its path: `width` at the shell, `widen` times wider at its oldest end, lasting `seconds`, drifting up `rise` units per second; it needs the `bursts` block for its material; first whose `match` (a name or a list of names) is in the bullet type's name wins, unset keys from `default`; width and head 0 = none), `core`, `brightness`, `particles` (share of the retail flight effects' particles kept) |
| `beams` | beam guns (the `beam` block of a gun, "A new weapon" above) drawn as light on camera-facing quads with `shaders/beam.wesl` instead of the classic cylinders: while charging, a flare swelling at the muzzle, rings closing on it, motes spiralling in and arcs crackling, all quickening as the charge fills; while firing, a ribbon along the beam (a white-hot thread in a sheath of the gun's `beam.color`, heat streaming out along it, two strands winding round it) that flares wide for an instant as it lights, with a flare and arcs at the muzzle and, where it is on a ship, a flare, arcs and a spray of sparks at the hull; the beam fattens and whitens as its damage ramps up: `core` (the thread's colour), `brightness`, `glow` (the ribbon's width in beam widths, default 6), `motes` (18), `arcs` (4), `sparks` (14). The muzzle and impact lights are classic's and stay |
| `surfaces` | textured, lit, non-blended glTF materials (not sails) get `shaders/surface.wesl`, which derives relief from the colour texture while drawing, or reads it from the material's baked surface map when an enabled mod has one (see "Baked surfaces" below): `rules` (`[{match: [...], roughness, metallic, reflectance, bump, cavity, gloss}]` against the material name, first match wins, unset keys from `default`), `skip` (names left retail), `rim` and `rim_strength` (light along the silhouettes). A cloaking ship's meshes (and sails) wear their retail material while faded |
| `materials` | the generic block for any other kind of restyled material: `rules` (`[{match: [...], shader, params, params2, roughness, metallic, reflectance, opaque, shadows}]`): a glTF material whose name contains one of `match` (not listed in `skip`; first rule wins; any retail material, textured or not, not sails or surfaces, which a rule claims first) gets the rule's `shader` (a path in the mod, e.g. `shaders/hull.wesl`; a rule whose file is missing stays off with a warning). `params` and `params2` are four numbers each for the shader (missing ones 0); `roughness`, `metallic` and `reflectance` override the retail StandardMaterial's (unset = retail); `opaque` makes it solid; `shadows` (default true) lets it cast shadows. One material per retail material, rule and ship, so a ship's meshes still batch. A cloaking ship's meshes wear their retail material while faded |
| `bursts` | particles thrown when a retail effect starts whose name contains a rule's `match` (`effects`, first match wins): parts `flash`, `sparks`, `embers`, `fireball`, `ring` (flat shockwave), `smoke`, `splinters`, `fibers`, each `{count, color, size, seconds, speed, spread (degrees about the effect's forward; a muzzle's is its shot's), drag, rise}`; `every` (seconds between throws while the effect emits, e.g. fires), `flame` (`{width, height, brightness}`; width and height in world units): replaces the retail drawing with a continuous, animated flame anchored to the effect, scales with its emission rate, raises the rule's smoke origin above the base, and extinguishes the flame and its light when emission stops. Omit `flame` to keep the retail drawing. `shake` (camera jolt). `scorch` (a part: glow where a shell scars a hull), `motes` (a part: `count` specks of dust within `speed` units of the camera). Glows draw with `shaders/burst.wesl`; smoke, splinters and fibers use the alpha-blended `shaders/smoke.wesl` batch. For an effect rule with `surface_debris: true`, `impact_materials` (`[{match: ["Wood", "Reinforced"], splinters: {...}, smoke: {...}}, …]`) replaces just the rule's sparks, embers, smoke, splinters and fibers when a shell supplies a matching ship/base section material. Matching is case-insensitive and exact on the combat material key (`Wood`, `Reinforced`, `Iron`, `Cloth`, etc.); first match wins. A match also hides the retail impact sprite rather than layering it over the profile's flash. The rule's flash and other non-debris parts stay; impacts with no matching material keep the original effect and rule |

### HD skies (`tools/bake_sky.py`)
`python -m tools.bake_sky [--size 2048] [--jobs N] [--split-stars] [skybox_07 ...]` rebakes each imported battle sky as a high-resolution cube map
(`models/sky/<sky>_cube.png`) into the local mod `<user data dir>/mods/hd-skies`, which is then switched on in the Mods screen.
The result is derived from the player's own game copy, so it is made on their machine and never committed or shipped. Without it
the game draws the retail sky sphere. Needs NumPy and Pillow, or nothing in the release download: there the baker runs inside
the frozen importer (`openprocyon-import --bake-skies [the same options]`), and the Mods screen's "Bake HD skies" button (on the
HD skies and Remaster look panels) runs it with the default options (plus `--split-stars` while the Remaster look is on), shows its `[n/total] ... (p%)` progress and switches the
mod on when it is done. Each PNG is written beside its target and renamed over it, and `mod.json` comes last, so a stopped bake
leaves no cut-off files and no half-made mod in the list. The `sky` block (above) adds a layer of gas in front of it; with `--split-stars` the baker makes the sky in layers, like Shakedown's own: `<sky>_cube.png` is then the calm backdrop alone, the filamented gas is a painted clouds cube (`<sky>_clouds.png`, alpha = density) and the stars their own 16-bit cube (`<sky>_stars.png`), with `data/render/skies.json` showing the clouds as two parallax layers (a profile without a `sky` block shows only the backdrop).

### Baked surfaces (`tools/bake_surfaces.py`)
`python -m tools.bake_surfaces` (release download: `openprocyon-import --bake-surfaces`) bakes a surface map for every textured
retail material into the local mod `<user data dir>/mods/baked-surfaces` (switch it on in the Mods screen like any mod): `textures/surface/<material name without .tga, lowercase>.png`,
linear RGBA at the texture's size: R and G the height slope along u and v (per texel, times 3, 0.5 = flat), B crevice depth, A
height (the texture's luma). The relief is filtered and taken at several scales, which the shader cannot afford per pixel, and
the shader samples one map instead of five colour texels. Roughness and metal stay per material in the `surfaces` rules, and
`bump`, `cavity` and `gloss` scale the map as they scale the derived relief. Like the HD skies the maps are derived from the
player's copy, so they are made on their machine and never committed or shipped; a material without a map keeps the derived relief.

### Effect events
The blocks that decorate retail effects (`lights`, `bursts`, and `tracers` for thinning flight effects) learn that one started
from a single message, `fx::EffectStarted`, not by watching components appear. It is presentation input only: the simulation
never reads it, and existing messages (`Hit`, `Destroyed`, `Scar`, `Shoot`, `Detonate`) are unchanged. `fx::spawn` queues it as a
command, so it is written on the tick that starts the effect and read in `Update` (messages are retained across frames without a
tick; see [development.md](development.md) "Battle clock").

| Field | Meaning |
|---|---|
| `entity` | the effect (it may already be gone by the time a short one is read) |
| `effect` | the effect's `FxLib` index, as `Fx::effect()` |
| `at` | world position it starts at |
| `forward` | the effect node's own forward, except: a muzzle flash gets its shot's direction (`combat/gunnery.rs`), an impact the surface normal where the hit found one (`combat/shells.rs` `hit_fx`: ships and bases; rocks, dragons and the ground keep the node's, which faces back along the flight) |
| `shell` | the shell a muzzle flash fires, or the one a flight effect rides on (that effect is the shell's child) |
| `host` | the ship or base an impact struck |

A caller that knows more than the pose uses `fx::spawn_with(…, fx::Origin { forward, shell, host })`; everything else calls
`fx::spawn` and sends the node's forward. A new block that needs "a shell hit a hull at X with normal N" reads this message
instead of adding another query.

### Shaders
The shaders are fragment shaders in Bevy's WESL. Each gets the mesh's `VertexOutput` (world position and normal,
UV, vertex colour) and one uniform at `@group(constants::MATERIAL_BIND_GROUP) @binding(100)`, whose layout is a Rust
struct the engine fills; the field order and types must match. A replacement keeps the uniform and may use its fields as it
likes.

Every one of these materials except the surfaces' also binds the frame data at `@binding(99)`, a storage buffer the engine writes once a frame
(`game/src/render/frame.rs`): a shader reads it after `import openprocyon::render::frame::frame;` (the module ships inside the
engine, `game/src/render/frame.wesl`, which documents the fields). It holds `clock` (real seconds never wrapped, battle seconds,
both frame deltas), `sun` (the direction the sunlight travels, illuminance in 10000 lux) and `sun_color` (w: the flat ambient
light), `camera` (the battle camera's position), the storm's last bolt (`bolt_at`, w the real seconds since it; `bolt_from`, w 1
when it struck a ship; `bolt_to`; both ends are of the displayed segment) and the black holes (`holes_info`: count and the profile's lensing; `holes[4]`: where the sky
bends round each, w its shadow's radius; `hole_reach[4]`: pull radius, kill core). Use it instead of adding a clock, the sun or
the camera to a block's uniform: a material whose uniform holds none of them is never written again after it is made.

| Shader | Drawn as | Uniform (`game/src/render/`) | Engine-provided values |
|---|---|---|---|
| `sail.wesl` | the sail's StandardMaterial extended: call `pbr_input_from_standard_material` and `apply_pbr_lighting`, then add light | `SailGlowUniform` in `sails.rs`: `color`, `params`, `sun` (w `backlit`), `motion` (y a per-sail phase, w the sail's row in the sail state), `pattern`, then `fold` and `shape` (for the engine's vertex stage; a sail shader may leave them out of its struct); `@binding(102)` the sail state, `array<vec4<f32>>` with a row for every sail (x the ship's smoothed speed share, y the sail's clock) | a per-sail phase, pattern cells per UV unit, and in the sail state the speed share and clock; the sun's direction is the frame data's. What moves is in the shared sail state and not in the uniform, so a sail's material never changes after it is made (Bevy prepares a changed material again, which is slow for many sails every frame). The engine ships the sail's vertex stages itself (`game/src/render/sail_vertex.wesl`, `sail_prepass.wesl`, `sail_fold.wesl`): they belly and fold the cut mesh on the GPU from the sail's fold data at `@binding(101)`. A cut sail (`billow` or `furl` on) also has `uv_b` (guard with `@if(VERTEX_UVS_B)`): x 0 where its light gathers (a fan's hub, else the sail's bottom edge) .. 1 at the rim or top, y 0..1 across (a fan's span, else the sail's width) |
| `plume.wesl` | additive (ONE/ONE: write unpremultiplied light, alpha ignored) on a unit cone, u around, v from nozzle (0) to tail (1) | `PlumeUniform` in `trails.rs`: `core`, `edge` (w = 1 on the inner layer), `params` (brightness, thrust, unused, seed) | thrust (written only when it moves), seed; the clock is the frame data's |
| `wake.wesl` | additive on each ribbon, u across, v along (newest at the ship), vertex colour = retail fade | `WakeUniform` in `trails.rs`: `tint`, `hot`, `params` (length, unused, core) | the ribbon's length (written only when it changes); the clock is the frame data's |
| `tracer.wesl` | additive on one mesh holding every tracer: streaks with u 0..1 across and v the point's age over `seconds` (0 at the shell), heads as quads stretched along the flight with u 2..3 along it (3 at the front) and v 0..1 across; vertex colour rgb = the bullet's colour, a = what is left of a gone shell's streak | `TracerUniform` in `tracers.rs`: `core` (a = brightness), `params` (unused) | none: the clock is the frame data's |
| `beam.wesl` | additive on one mesh holding every beam gun's quads, five kinds told apart by the whole part of u / 2 (u = 2 x kind + 0..1): 0 the beam (u across, v the distance from the muzzle in beam widths), 1 a flare, 2 a ring, 3 a mote or spark (u and v 0..1 over the quad), 4 an arc (u across, v 0..1 along); vertex colour rgb = the gun's `beam.color`, a = how bright the quad is now | `BeamUniform` in `beams.rs`: `core` (a = brightness), `params` (x = the thread's share of the ribbon's half width) | none: the clock is the frame data's |
| `surface.wesl` | the retail StandardMaterial extended (roughness, metallic, reflectance already set from the rule): call `pbr_input_from_standard_material`, change `N`, colour and roughness, then `apply_pbr_lighting`; read `base_color_texture` for relief, or the baked map | `SurfaceUniform` in `surfaces.rs`: `params` (bump, cavity, gloss, 1 when baked), `rim` (colour, strength), and the baked surface map with its sampler. The material is bindless like the StandardMaterial it extends, so all surfaces draw as one batch: the shader reads its row through the mesh's material slot (index table at `@binding(100)`: uniform, map, sampler; uniforms as one array at `@binding(101)`; textures and samplers from Bevy's `bindless_textures_2d` and `bindless_samplers_filtering`, the colour texture included), and under `@if(!BINDLESS)` (hardware without bindless) from plain bindings 50, 51 and 52. Start from the remaster mod's `surface.wesl`, which has both. No frame data | none |
| `burst.wesl` | additive on one mesh of every glowing particle: uv.x = kind × 4 + u, uv.y = seed × 2 + v (u, v 0..1 across the quad; kinds 0 flash, 1 spark (u tail to head), 2 ember, 3 fireball, 4 ring (flat), 5 scorch, 6 dust, 8 anchored flame (v base to tip)); vertex colour rgb = colour (flame: brightness), a = age share of its life (dust: its fade; flame: unused) | `BurstUniform` in `bursts.rs`: `params` (unused) | none: the clock is the frame data's |
| `smoke.wesl` | alpha-blended quads sorted far to near, same vertex layout (kind 7 puffs, 9 splinters, 10 fibers; u runs along the long axis of a fragment), and the shells' smoke ribbons in a mesh of their own (kind 11; u 0..1 across the strip, colour a the point's age as a share of its life) | `SmokeUniform` in `bursts.rs`: `shade` (ambient colour) | the ambient light; the sun, its colour and the clock are the frame data's |
| `sky.wesl` | premultiplied-blended on an inverted unit sphere around the camera (behind the world by depth test), the baked sky cube bound at 101 (sampler 102), the stars cube at 103 (sampler 104; black when the sky has none), the painted clouds cube at 107 (sampler 108; none when the sky has none, and its cloud layers are then given opacity 0) and the baked tiling noise volume at 105 (sampler 106; `volnoise.rs`, the same one `volume.wesl` reads) | `SkyUniform` in `sky.rs`: `stars` (brightness, twinkle, speed, gain), `info` (layer count, base on), `base`, `base_tint`, `flash` (rgb colour times strength, w seconds) and `flash_reach` (x), `warm` and `warm_dir` (an explosion's colour times what is left, and its direction from the camera; written only while one fades), `shoot`, `sheet`, `sheet_color`, `layers[3]` (`a` opacity, threshold, softness, distance; `b` noise scale and tint; `c` the current; `d` x 1 for a clouds layer, y haze) | the camera position (`view`); the clock, the bolt the flash comes from and the black holes are the frame data's |
| `blackhole.wesl` | premultiplied-blended on one flat circle per hole, or the whirlpool's funnel with `funnel.depth` (depth-tested, never written), no textures | `HoleUniform` in `holes.rs`: `centre` (xyz, w the pull radius), `shape` (horizon, core, ring thickness; world units), `disc` (inner, outer, spin, brightness), `color`, `params` (swallow flash, glow), `funnel` (depth, throat radius, drop; world units), `rim` (rgb) | the camera position (`view`); the clock is the frame data's |
| `nebula.wesl` | additive (ONE/ONE: write unpremultiplied light) on the cloud's retail shell, vertex colour = the retail colour loop times the edge fade | `NebulaUniform` in `nebulae.rs`: `centre` (xyz, w the radius), `axes` (the ellipsoid's half-size as shares of the radius, w steps), `look` (density, scale, churn, shell), `low` (rgb, w the share drawn with the camera inside), `high` (rgb, w a bolt's brightness), `flash` (xyz where the last bolt struck, w how much of its light is left), `params` (clock, a bolt's reach) | the camera position (`view`) |
| `volume.wesl` | premultiplied over the scene on a back-face proxy box with no depth test (it ends its own rays at the depth prepass), for a `nebulae` rule with `replace` | `VolumeUniform` in `nebulae.rs`: `centre` (xyz, w the radius), `axes` (half-size shares with the breath applied, w steps), `look` (emission, noise scale, churn, absorption), `low` (rgb thin gas, w `max_opacity`), `high` (rgb dense gas, w dust), `tint` (rgb the retail colour loop, w light steps), `sun` (xyz toward the sun, w strength), `sun_color` (rgb, w the share kept inside), `flash`, `params` (clock, a bolt's reach and brightness, fade), `storm` (swirl, filaments, flicker, variation), `alt` (rgb the second hue, w core glow); the baked noise volume at binding 101 and its sampler at 102 (`volnoise.rs`: r gas, g warp, b dust) | the camera position and depth prepass |
| `bolt.wesl` | additive (ONE/ONE: write unpremultiplied light) on the camera-facing bolt ribbon, u across, vertex colour rgb the glow and a how bright the bolt is now | `BoltUniform` in `bolts.rs`: `core` (rgb the thread, w brightness), `params` (x the thread's share of the ribbon) | |
| `ribbon.wesl` | additive (ONE/ONE) on the black hole's own vortex strips (blackhole.rs), for `black_holes.replace`; vertex colour a the trail's fade, u across, v along | `RibbonUniform` in `holes.rs`: `centre` (xyz, w the pull radius), `shape` (x the kill core, y brightness, z the funnel's depth, w its throat; world units), `color` (rgb the cooler colour); also the vertex stage, which sinks the strips along the funnel | the clock is the frame data's |
| `current.wesl` | additive (ONE/ONE) on each strip, u 0..1 across, v along the flow (increasing downstream); the shader scrolls the retail texture itself (v minus the battle clock times the strip's speed, as the retail game does) before `pbr_input_from_standard_material` | `CurrentUniform` in `currents.rs`: `look` (brightness, streaks, sparkle, core), `color` (rgb, w the retail texture's share), `params` (the strip's scroll speed in texture heights a second, pulse) | the clock is the frame data's |
| any `shader` of a `materials` rule | the retail StandardMaterial extended, like `surface.wesl`: call `pbr_input_from_standard_material`, change what you like (colour, normal, roughness, emission), then `apply_pbr_lighting`; the prepass and shadows keep Bevy's shaders. Every rule shares one uniform | `RuleUniform` in `rules.rs`: `params`, `params2` (the rule's numbers), `clock` (x elapsed seconds, y frame delta), `sun` (xyz the direction the light travels, w illuminance in units of 10000 lux), `ship` (x the owning ship's smoothed share of full speed, y its heading in radians, z a 0..1 seed per ship; all 0 for a mesh of no ship) | the clock, the sun, the ship's speed, heading and seed |

To make a graphics mod: start from an empty `data/render/look.json`, add the blocks you want, and put the shaders of the
`sails`, `engines`, `wakes`, `tracers`, `surfaces`, `bursts`, `sky` and `materials` blocks in the mod's `shaders/` folder. Run with `--mod <folder>`;
the profile is read at launch, so restart after edits. A new kind of material (a shader for hulls, rock or water, say) is a
`materials` rule and a shader file in the mod, not a Rust block: only something a rule cannot express (a mesh the engine must
build or move, as for sails) needs a building block in `game/src/render/` first; then keep classic unchanged without the
profile key and document its uniform here. `sails` and `surfaces` stay as they are; a `materials` rule claims a material
before they look at it.

## Troubleshooting

| What you see | Look at |
|---|---|
| the mod is not in the Mods list | the folder must be directly inside `<user data dir>/mods/` (or `mods/` of the game); a folder is listed even without `mod.json`, but a second folder with the same `id` is skipped. Run with `--mod <folder>` instead to rule the list out |
| it is listed but nothing changes | it is not switched on (dropping a folder in never switches it on); or the files sit one folder too deep (`mods/name/name/mod.json`); or a path does not mirror the base (`ships/<Id>/ship.json`, `data/weapons/guns.json`: compare with your `assets/`); or you did not restart; or a later mod replaces the same file (replaced, not merged: `look.json`, `theme.json`, `slices.json`, images, models) |
| `--mod` does nothing and no `mod <name> ...` line appears | the path is wrong: a folder that does not exist is silently ignored (no error), and a `mod.json` is needed for the line to print |
| the game stops at launch with `extends unknown '<parent>'` or `sets '<pointer>', which does not exist` | the parent ship or sibling file is not in the overlay under that name; the `set` pointer (`/engines/0/fx`) names an array slot or key the merged file lacks |
| the game stops at launch with a ship message | see "If it will not load" under "A ship from a model" |
| the game stops at launch with `called Option::unwrap() on a None value` in `data.rs`, or `number in ship json` | a required key of a gun or bullet is missing (lists under "A new weapon"), or your `guns.json` / `bullets.json` is not valid JSON: a mod's copy that does not parse as an object is merged in as `null` and wipes the whole table |
| every menu text is blank | your `menu/strings.json` did not parse: the same wipe-out, for the string table |
| a JSON file seems ignored, with no message | optional files read with a forgiving loader ignore a syntax error silently: `data/render/look.json` (the whole profile stays classic), `data/hud/theme.json`, `data/hud/speed.json`, `data/fx/wake_tint.json`, `mod.json`. Validate the file with a JSON checker first. Inside `theme.json` a malformed colour (`#RRGGBB` or `#RRGGBBAA` only) leaves that value as shipped |
| a graphics block does nothing | the log has `render profile: `<block>` needs shaders/<name>.wesl in the mod; left off` when the shader file is missing; `sky`, `nebulae` and `black_holes` effects also need their own parts (cube PNGs, `shaders/sky.wesl`); `data/render/skies.json` and `nebulae.json` do nothing without the matching block in a profile |
| a model is untextured | the glTF material has no texture of its own and no `retail:<texture>` name, or the name is not in the import (the console says `mod model: retail texture ... is not in the import; the material stays untextured`); give the material a flat `baseColorFactor` and UV coordinates |
| a ship is on the fleet screen with the wrong name | its `display_name` id is not in the string table (shown: the prettified folder name) |
| the ship is not on the fleet screen | `"skirmish": true`, a `race` of Navy, Pirate or Procyon and `points` are needed |
| a gun never fires | the mount's `arc_deg` and heading (a firing arc of a few degrees), `parent_layer`/`pos` of the mount, the `*_Range` values (ranges are game units), and the gun's name in the ship's `guns` |
| silence | the file is not Ogg Vorbis, the name differs from the file name, two files share a name, `SoundName` is `""`, or `SoundMaxDistance` is shorter than the camera distance |
| a mod's map or mission is missing | see `docs/missions.md` |

If none of this explains it: open `<user data dir>/logs/openprocyon.log`, read from the bottom up for the first `WARN`, `ERROR`
or panic line, then reduce the mod to the one file that shows it before asking anyone.
