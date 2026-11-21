# Modding

A mod is a folder of files that the game lays over the player's imported assets at launch: data (JSON), models (glTF `.glb`),
images (PNG) and sounds (OGG). No code and no patching: ships, guns, maps, custom missions and campaigns, sounds, UI art, fonts
and the graphics look are all files. When something needs the engine to change, see "Sharing a mod" below.

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
ship and changes only the keys you give. Try it with `./openprocyon --mod /path/to/my-mod` (from a source checkout:
`make run ARGS='--mod /path/to/my-mod'`; repeatable, later mods win), or install it as a player does (below). The overlay is built at launch, so restart after every change. More examples in `mods/`: `faction-colors` (JSON only),
`game-speed`, `remaster` (a whole graphics look). A ship of your own needs only a model and a short `ship.json`
(["A ship from a model"](#a-ship-from-a-model)).

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

Unzip a mod into `<user data dir>/mods/` (`~/.local/share/openprocyon/mods` on Linux, `%APPDATA%\openprocyon\mods` on Windows),
then switch it on with the Mods button in the corner of the main menu: a sortable list (the Category pull-down filters; click a
row for its details; tick its box, double-click it or use Switch on / Switch off). The choice is stored in `mods.json` beside that
folder (`{"enabled": ["id", ...], "disabled": ["id", ...]}`, load order, later wins). A user mod stays off until switched on;
dropping a folder in never switches anything on. A user mod replaces a bundled one with the same `id`.

Mods that ship with the game live in `mods/` (the checkout's, or next to the executable in a download) and are off until ticked,
except those marked `"default": true` (such as `game-speed`), which are on until the player switches them off.

`mod.json` (every mod folder needs one; `tests/test_bundled_mods.py`):

| Key | Meaning |
|---|---|
| `id` | default: the folder name |
| `name`, `version`, `description` | shown in the Mods screen |
| `category` | the Mods screen's filter (default "Other") |
| `missions` | the order of a mod campaign's missions (see the custom-mission row below) |
| `default` | bundled mods only: on until switched off; ignored in user mods |
| `game_version`, `requires` | not read yet |

## What a mod can change

At launch `game/src/modding.rs` builds `local/overlay/` (symlinks to the base files, mod files on top) and the
game, Bevy's asset server included, reads only that tree.
| Mod file | Effect |
|---|---|
| any file with a base path (`hud/sprites/MainBar_PanicButton/Navy/NoCursor.png`, `ships/Ship_X/model.glb`, `fonts/Sans.otf`, `sound/<kind>/*.ogg`) | replaces the base file: texture packs, upscaled art, new fonts, sounds |
| `ships/<Id>/ship.json` containing `"extends": "Ship_Navy_Cutter"` | deep-merged over that ship: object keys override one by one, arrays and scalars replace; a new ship or weapon is a few lines (see `mods/silver-skirmish`) |
| an `extends` file may also carry `"set": {"/engines/0/fx": "X"}` (JSON pointers, applied after the merge) to change one value inside an array, which a merge cannot do; a pointer that does not exist is an error |
| `data/collision.json` | merged key by key: a new scenery type's collision (an entry per `type` a map's `objects` name, layout in `docs/data-format.md`), so a mod's own island or rock blocks ships and shells. Without an entry the model is drawn but nothing collides with it |
| `data/render/skies.json` | merged key by key: `{"<sky>": {...any `sky` key of `look.json`...}}`, the layers, base and stars of a sky a content mod bakes itself (`models/sky/<sky>_cube.png`). Added under the active render profile's `sky.skies` (the profile's own entry for a sky wins); without a profile with a `sky` block it does nothing, so a mission mod never switches the look on |
| `models/scenery/<Scene>.glb` with glTF animations | every clip in the file loops from spawn, all together (cranes, flags, beacons: node animation, the player on the model's root). Retail scenery has none; its windmills play `data/animations.json` |
| `menu/strings.json` | merged key by key with the retail string table: new ids for names a mod's ships and screens use (e.g. a ship's `display_name`) |
| `fx/<Name>.json` | a new effect file is loaded without being listed anywhere; `extends` + `set` recolours a copy of a retail effect |
| `data/hud/speed.json` (`{"max": 16}`) | shows the game-speed buttons beside the command bar (not retail), stepping 1x to `max` (default 16); absent: no speed control, as in retail. `--speed N` on the command line works either way |
| `data/fx/wake_tint.json` (`{"Navy": [r, g, b], "Pirate": ..., "Procyon": ..., "Civilian": ...}`) | multiplies the wake texture per fleet type (multiplied in linear light, values may exceed 1); absent: retail colours |
| `models/sky/<sky>_cube.png` (six square cube faces stacked vertically, +X -X +Y -Y +Z -Z in Bevy's cube-map frame) | drawn as the battle camera's skybox instead of that retail sky's sphere (not retail); a render profile's `sky` block also reads `<sky>_stars.png` (the stars on their own) and `<sky>_clouds.png` (painted clouds, straight alpha) when the mod has them, and `data/render/skies.json` (below) tunes the layers per sky; with a render profile's `sky_light` it also lights the scene. A sky made from the player's own imported sky is derived from game content: keep it local, never ship it |
| a new `ships/<Id>/` folder (`ship.json` and a `model.glb`, see "A ship from a model"; or `geometry`/`model` pointing at another ship), a new `maps/<id>/` folder (`map.json`, `backdrop.png`; `"skirmish": true` lists it), `models/<any folder>/*.glb`, `sound/<any folder>/*.ogg` | new content (schema in `docs/data-format.md`) |
| a new `maps/<id>/` folder with a `mission.json` (and `media.json`, the campaign's layout) | a custom mission: listed under Single Player > Custom Missions (the button in the corner, not retail) with its `map.json` `name` and `description`, and started with Play or a double-click (`--map <id>` also works). Each mod is one campaign named after its `mod.json` `name`: its missions play in the order of `mod.json` `"missions": ["<id>", ...]` (missions it does not list follow, by id), and winning one offers the next (N, as in the retail campaign). Ids `mission01`..`mission13`, `tutorial` and `basic_tutorial` are not listed: a mod's copy replaces that retail mission. Victory points of custom missions do not count toward the retail campaign total. Dialogue: each `media.json` `speech` entry gives the subtitle `text`, `duration`, `file`, talking-head `face`, `speaker` (a string id) and colour, and the sound is `maps/<id>/speech/<file>.ogg` (found by name, any case). The `mission.json` layout (ships, paths, volumes, tasks, rules) is in `docs/data-format.md` and `docs/re/campaign.md`; read the imported retail missions as examples, but write your own: a copied or edited retail mission is game content. To test a late part of a mission without playing the start, launch it with `--fire-rule "<rule name>"` (repeatable; `--map <id>`): once the world-initialise rules have run, the actions of each named rule run in the order given as if its conditions had held, and a run-once rule does not fire again. An unknown name is a warning listing the rule names. `--start-at x,y` also moves the primary ship there (game units), since a rule does not. A debug aid for authors, not retail behaviour |

Limits (provisional): the fleet screen lists, per fleet type (`race` Navy/Pirate/Procyon), every ship whose `ship.json` has `"skirmish": true` (the importer copies retail's "Available in Multiplayer?" flag; a mod can set it on any ship); Weapons and bullets are shared files, `data/weapons/guns.json` and `bullets.json`; a mod's copies are merged key by key, so adding a gun is one entry. A gun with a `beam` block (not retail) is a beam gun: `{"charge": s, "duration": s, "tick": s, "ramp": per s, "max_power": x, "width": units, "color": [r, g, b]}`. It charges for `charge` seconds with a glow at its first muzzle, then holds a beam along its barrel for `duration` seconds and reloads for `TimeBetweenBursts`. The beam burns the first enemy ship it crosses once per `tick` with the bullet's damage table, times a power of 1 + `ramp` x the seconds it has stayed on that ship (at most `max_power`); `SoundName` plays once when the charge starts, so one sound can carry the charge and the beam. It passes through bases, rocks and scenery (PROVISIONAL). A gun's look is its `visual` block: `base` and `barrel` (GLB file names, found by name anywhere under `models/`, e.g. the mod's `models/weapons/gun_X_Layer1.glb`; either may be left out), `pivot` (where the barrel pitches) and `muzzles` (one point per barrel, where shells and the beam start), in game axes (x starboard, y forward, z up) in the gun's own frame. The barrel mesh and the muzzles are authored in the base's frame, not about the pivot; a retail gun's block in the imported `guns.json` is a worked example. Its `MuzzleSpeed` only sets the turret's lead (use a large value), and `EffectiveRange_Range` is where computer captains with a bow gun turn away. A mod may ship sounds of its own under `sound/`, found by name. Computer captains fit the guns retail computers were seen to fit (carronades, Gattling, plasma, light/medium cannons, star mortar) unless a mod ships `data/ai/weapons.json` (`{"Gun_Name": true|false}`, merged the same way) to restrict or extend what they may pick. The full layout is in `docs/data-format.md`.

## A ship from a model

A ship needs no retail ship to extend: a folder `ships/<Id>/` with a `model.glb` and a small `ship.json` is a complete ship. The
engine reads the model and derives what the file leaves out, so the ship behaves like a retail one (hits, damage sections, wake,
engines, crew, boarding, and the command-bar and weapon-panel art). Whatever `ship.json` states wins over what is derived; every derived
value is PROVISIONAL (the retail files state each one, the engine only needs a sensible value).

**Layers.** Every mesh node of the model is a layer. With no `geometry.json` (and no `"geometry"` naming another ship), the hit mesh of
each layer comes from the model's own triangles. With no `sections`, each intact layer becomes one damage section: hull (150 HP, vital to
the ship, an equal share of the core) or, when its node name contains `sail`, `mast`, `cloth` or `canvas`, cloth (70 HP, vital to speed,
no core share). Layers that touch (boxes within 0.3) pass hits on to each other. Name a damaged variant `<A>Damage_<B>` for layer
`<A>_<B>` to have it swapped in when the section is destroyed, as the retail hulls do; a destroyed section with none just disappears. A
`sections` object in `ship.json` replaces the derived ones (the retail schema, `docs/data-format.md`); its `layer` names the mesh node.

**Hardpoints.** Empty nodes mark where things go, in the ship's own frame (glTF axes: x starboard, y up, z aft; the exporter and
Blender's glTF export both write this). Give the node an extra `kind` (Blender: a custom property on the empty), or just name it so the
engine can tell:

| `kind` | name contains | derives |
|---|---|---|
| `GunPlacement` | `mount`, `lancer`, `turret` | a mount, joined to the `guns` entry of the **same name**; `ship.json` says which `gun` and its `arc_deg` |
| `EnginePortPlacement` | `engine` | an engine plume (`<Race>Engine_<Light/Medium/Heavy/ExtraHeavy>` by hull length) |
| `WakePlacement` | `wake` | a wake (`Wake_Light/Medium/Heavy`); with none, one at the stern's waterline |
| `FlagAttachmentPoint` | `flag` | the flag pole's ends (names with `top` and `bottom`) |
| `BoardingEffectPoint` | `boarding` | where boarders cross |
| `ToweePoint`, `TowerPoint`, `TorpedoHomingPoint` | `tow` (front/back), `homing` | tow and homing points |
| (none) | `Crew_Walk_<lane>_<k>`, `Crew_Gun_*`, `Crew_Post_*`, `Crew_Climb_<shroud>_a`/`_b` (the whole name) | where deck crews walk, man guns, stand and climb (drawn only by a render profile's `deckhands` block, see below) |

A node's heading about up is its rotation about glTF y (or the extra `rot_raw[0]`): 0 faces forward, a quarter turn to port is +90
degrees. A gun entry may still give `pos`, `rot`, `parent_layer` itself; `parent_layer` otherwise is the nearest hull layer.

**What `ship.json` can leave out.** `physics` (defaults about a Navy Cutter's: mass 4000, thrust 60000, speed 8, turn 3), `core_hp` (a
quarter of the hull sections' HP, at least 50), `sections`, `engines`, `wakes`, `flags`, `explosion_fx`, `engine_snd`, `repair_fx` (by
race and size), `crew_posts` (one gunner per two guns, a captain, mate, navigator, engineer, a rigger per sail, a fighter or two),
`sphere`, extents, `icon`. `race` picks the engines, sounds and HUD skin; `display_name` is a string id the mod adds in
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

## Graphics mods: the render profile (not retail)
Graphics upgrades are mods, never engine forks. The engine (`game/src/render/`) provides building blocks; a mod switches
them on and tunes them with one file, `data/render/look.json`, and ships the shaders most of them draw with
(`shaders/sail.wesl`, `shaders/plume.wesl`, `shaders/wake.wesl`, `shaders/tracer.wesl`, `shaders/surface.wesl`,
`shaders/burst.wesl`, `shaders/smoke.wesl`, and any a `materials` rule names; the engine ships none, and a block whose shader is
missing stays off with a warning). Without `look.json` every part renders as classic, and a profile only changes drawing,
never rules. Every key is optional; an absent block leaves that feature classic.

| Key | Effect |
|---|---|
| `camera` | HDR camera: `bloom` (intensity), `bloom_threshold` (only light brighter than this blooms; 0 = Bevy's natural bloom, which hazes the image), `tonemapping` (`tony_mc_mapface`, `agx`, `aces`, `blender_filmic`, `reinhard_luminance`), `saturation`, `contrast`, `exposure` (stops), `smaa` (replaces MSAA), `ssao` (crashes with some effects meshes, keep false), `vignette` (corner darkening), `aberration` and `aberration_pulse` (colour fringing at rest, and added by a blast's camera jolt), `contact_shadows` (screen-space sun shadows where parts touch; adds a depth prepass) |
| `sky` | a dome over the baked sky (needs `models/sky/<sky>_cube.png` and `shaders/sky.wesl`): `near` (one layer object or an array of at most 3 layers: `distance`, `opacity` 0..1, `threshold` and `softness` of the brightness kept, `drift` the map-wide current in world units a second, game axes, `scale` noise patches across the layer (0 = the plain copy of the sky), `tint`, `source`, `haze`), sparse soft copies of the sky's gas that slide against it as the camera moves (parallax; a far and a near layer give two speeds), broken into wisps by slowly churning noise. `source` is `"sky"` (default: the gas of the baked sky) or `"clouds"`: the layer then reads the painted clouds cube `models/sky/<sky>_clouds.png` (beside `_cube.png`, same six stacked faces and direction mapping, 8-bit sRGB RGBA with straight alpha: rgb the lit cloud colour in the cube's brightness convention, alpha the coverage) instead: coverage is alpha times `opacity`, colour is rgb times `tint`, `threshold` and `softness` act on alpha (both default 0 for clouds: alpha passes unchanged), `scale` noise eats the thin edges and leaves dense cores, each layer index looks the painting up through its own turn, and the coverage of cloud layers hides the stars behind them (gas layers never do). A cloud layer on a sky with no clouds cube draws nothing. `haze` (0..1, default 0) is aerial perspective: that share of the layer's colour sinks into the backdrop in its direction (and loses contrast), so a far layer is paler than a near one. Cost per cloud layer: one cube read plus the noise reads. `base` (`flow` radians, `cycle` seconds, `breath` depth, `period` seconds, `tint`, `brightness` multiplier, default 1) also redraws the far sky at alpha 1 from two lookups that flow along noise and cross-fade, with a slow breath in the bright cores; `brightness` leaves the separate star layer and scene lights unchanged. `flash` (`color`, `strength`, `seconds`, `reach`) is how a storm's lightning (`nebula::StormFlash`) lights the gas in its direction, at half strength `reach` world units from the camera and zero at twice that distance (default 1200, 0 = no falloff); `events` (`[{match, color, strength, seconds}]`, a fragment of an effect's name) warms the gas on the side of a big explosion; `shooting_star` (`every`, `brightness`) and `sheet_lightning` (`every`, `strength`, `color`) come from the shader's own clock. `skies` (`{"skybox_05": {...any of the keys above...}}`) merges per-sky accents over the defaults, by the baked cube's base name. `stars` (`brightness`, `twinkle` depth 0..1 for the bright ones, `speed` twinkles a second, `gain` decoding the 16-bit bake, 16 like the baker's) draws a separate stars cube (`models/sky/<sky>_stars.png`, baked with `--split-stars`; the stars are then left out of `_cube.png`), each star twinkling on its own phase, the brightest above 1 so they bloom; events never light the stars. Replaces the retired `sky_drift`, a rigid turn of the whole sky that read as the camera rolling |
| `sky_light`, `ambient`, `ambient_floor` | image-based light (cd/m²) computed once from a mod's cube-map sky (`models/sky/<sky>_cube.png`), the multiplier on the flat ambient light (default 1), and an optional minimum for each channel of the map's flat ambient colour (linear RGB, 0..1). Both ambient settings also apply without a baked sky |
| `sun` | `shadows`, `shadow_distance` (cascades out to this many units), `color` (multiplies the map's sun colour), `intensity` (multiplies its strength) |
| `fx_gain`, `shell_gain` | multiply ONE/ONE effect colour and the shells' spheres, so they pass 1 and bloom |
| `lit_smoke` | blended particles (smoke, dust) take the scene's lights: explosions light their own smoke |
| `lights` | point lights on effects: `effects` (a fragment of the effect file name) and optional `also_matches` (a second required fragment, e.g. `Plasma` and `_Impact`; first entry that matches wins), `color`, `lumens`, `range`, `radius` (apparent source radius in world units for broader specular highlights; default 0, independent of `range`), `seconds` (quadratic fade; 0 = steady while the effect lives), `flicker` (share), `cone` (full angle in degrees: a spot light facing along the effect's forward instead of a point light shining all round; 0 = point), `offset` (units along that forward: down the shot for a muzzle or out from the surface for a hit with a known normal), `shadows` (casts shadows; a point light renders the scene six times, so use it rarely). Effect lights are otherwise not occluded: a point light on a gun also lights the deck and hull behind it |
| `deckhands` | crews on the decks of ships (and townsfolk on map scenery) whose model marks their places with empty nodes (`Crew_Walk_<lane>_<k>` lanes, joined in order and wherever two points are within 2.3 units; `Crew_Gun_*` places by a deck mount, facing it; `Crew_Post_*` officers and lookouts; `Crew_Work_*` a figure working on the spot, hauling or loading by turns; `Crew_Climb_<shroud>_a`/`_b` the foot and top of a shroud; the node's heading is the way the figure faces). The figure and its moves (`idle`, `walk`, `run`, `haul`, `load`, `climb`, `cheer`, `fall`) come from the mod's `data/render/deckhand.json` and `.bin` (a vertex animation texture, posed on the GPU; every figure is one batch) and are shaded by `shaders/deckhands.wesl`. Gun crews load while their mount reloads, the rest wander (running for 10 s after a gun fires), riggers climb the shrouds when the sails fold or open, figures fall as the hull's core HP drops (30% stay to the end) and the player's side cheers a victory. Scenery figures wear the `Civilian` outfits, one wanders per three walk points (at most `per_ship`), and they only wander, stand, work and cheer. `per_ship` (most figures on a ship, default 60), `range` (camera distance beyond which they are not drawn, default 160), `outfits` (`race`, `officer` for the posts, and `skin`, `coat`, `trousers`, `shoes`, `hat` linear RGB; at most 16). No shadows. PROVISIONAL: a prototype |
| `sails` | glTF materials whose name contains one of `match` (default `["sail"]`) get `shaders/sail.wesl`: `glow` RGB (0 for names in `plain`), `speed`, `cells` (pattern cells across the sail), `params` (four numbers for the shader, also settable one by one as `line`, `wave`, `steady`, `fill`), `backlit`, `transmission`, `opaque` (solid, casts shadows), `billow` (bellies out between its edges, more with speed), `furl` (share of each sail's height folded up to its top edge, the yard, over 1.5 s while its ship is ordered to All Stop; 0 = never), `fan` (`races` (default `["navy"]`) and `match` (material name fragments): those sails fold like a hand fan instead. A fan is a half disc of wedges meeting at a hub (found where many triangles meet; pieces under 40 vertices, the jibs, keep the roller fold). It shuts by the `furl` share (0.9: a slim bundle, since a fan shut all the way has no width): a main sail (middle rib pointing up) swings in from both sides and up onto its mast, every rib keeping its length, and its hub slides off the yard onto the mast, so the folded sail lies against it. The mast is the most upright long spar on the ship's centre line beside the fans, found in the other parts of the sail's model node. A fan under the hull folds down along the same line, and a side sail closes flat to a level line. The cloth between the ribs pleats out of the sail's plane as it shuts (`pleats` ribs, `depth` a share of the sail's size). Needs `furl`) |
| `black_holes` | a disc of the hole's pull radius in its plane, drawn with `shaders/blackhole.wesl` (without `replace` over the retail ribbons, which become the infalling streams). Every size is a share of the pull radius, so all three hole sizes scale: `horizon` (default 0.25, exactly `Pull::core()`'s share: the black area is the kill zone), `ring` (thickness of the bright ring on the horizon's edge, never thinner than a pixel and a half), `lensing` (how hard the sky is bent toward the centre, 1 = a radian at the core's edge; drawn by the `sky` dome, which bends the far sky, its gas layers and its stars around each hole and blacks out a round shadow just past the core, always behind the ships; without the dome nothing bends), `disc` (`color` the cooler outer colour, `inner` and `outer` radii, `spin` turns a second clockwise seen from above like the pull, `brightness` past 1 to bloom), `replace` (default false: hide the retail glow, beam and particle layers, `BlackHoleRibbons_*`, so only the profile draws the hole, and draw the hole's own vortex strips, the retail geometry whose spiral motion stays, as thin, hot, streaking threads with `shaders/ribbon.wesl`: bright toward the core, cooling and fading outward, gone at the kill core; `ribbons` scales their brightness and `ribbon_color` sets their cooler colour (default `[1, 0.38, 0.12]`). The hole keeps pulling and its trail sim keeps running, only the drawing changes), `glow` (warm halo), `drop` (how far under the hole's plane the disc lies, so hulls draw over it), `funnel` (`depth` a share of the radius, default 0 = the flat disc; `throat` share, default 0.06; `rim` the outer water's colour): with a depth the hole is drawn as a whirlpool, a funnel sinking under the plane the ships sail (a 1 / r gravity well: nearly flat at the pull radius, plunging toward the core), its water swirling clockwise in log-spiral bands from faint violet at the rim through pink and orange to a bright gold lip at the kill core, past which the throat falls into black; the hole's vortex strips sink along it into the throat, and the bent sky and the round shadow are centred at the bottom of the throat and shrink to the throat's size, so the plane itself never bends. The disc is thin and crisp: a hard hot white-gold inner edge cooling to orange and red, fine log-spiral arms, a sharp photon ring on the horizon, Doppler-brighter on the side moving toward the camera (the lensed far side of the disc arching over the shadow is not drawn). It only adds light (its alpha blacks out the core alone), flashes for a second when the hole swallows a ship, and changes nothing about the pull |
| `nebulae` | nebula clouds and solar storms (the retail additive shell stays as the envelope: its texture, shifting vertices, colour loop and edge fade) get `shaders/nebula.wesl`, which also marches a short ray through the cloud's ellipsoid and adds churning, domain-warped gas to it: `bolt` (`{color, core, max_length, width, glow, brightness, seconds, segments, jitter, branches}`, draws each storm bolt itself, with `shaders/bolt.wesl`: the storm-wide random segment `nebula::StormFlash` now carries (`from`, `to`, `hit`; a bolt that finds no ship is announced too, PROVISIONAL, dimmer, and silent) as a jagged, branching camera-facing ribbon, a white-hot thread of `width` world units in a glow `glow` times as wide, `brightness` past 1 to bloom, flickering out over `seconds`; `max_length` caps the visible span in world units, centred on a miss or ending at the ship on a hit (0 = full segment, the default); the volume is lit along the displayed segment), `replace` (block or per rule, default false: hide the retail shell, and for a `SolarStorm` rule (with `bolt` present) the retail `Lightning` sprite, so only the profile draws the cloud; the cloud sim, storm timers, damage and sounds are untouched), `rules` (`[{match, replace, absorption, dust, max_opacity, light_steps, sun, density, steps, scale, churn, color_low, color_high, shell, inside, flash, flash_reach}]`, a fragment of the cloud's effect name (`Nebula`, `SolarStorm`), first match wins; `density` strength, the dense cores past 1 bloom; `steps` 4..24; `scale` noise patches across the cloud; `churn` noise units a second; `color_low`/`color_high` thin and dense gas; `shell` share of the retail shell kept; `inside` share of the gas drawn when the camera is inside the cloud, where the far wall marches from the camera; `flash` and `flash_reach` (share of the radius) how a bolt of storm lightning, `nebula::StormFlash`, lights the gas around it from inside for 0.3 s; the flash fades as the camera moves away from the displayed bolt, from full brightness at `flash_reach` times the cloud radius to zero at three times that distance). Without `replace` the gas only adds light (ONE/ONE), so it can never hide a ship, and a cloud no rule matches keeps its retail material. With `replace` the cloud is a volume (`shaders/volume.wesl`, needs the camera's depth prepass, which the profile turns on): the cloud's proxy box is drawn on its back faces (so also from inside) and its ray march keeps emission and absorption, composited premultiplied, ending at the scene's depth so a ship or island inside the gas is only veiled by the gas in front of it. `absorption` is the optical depth across a radius at full density, `dust` the strength of dark lanes (extinction without light), `max_opacity` the most one cloud covers (0.6; 60 % of that where the ray ends on a hull), `light_steps` (0..3) steps toward the sun for the self-shadow, `sun` the sun's strength on the gas; `core_glow` (0..12, default 0) adds HDR light only inside dense pockets without raising the opacity cap. Dust still absorbs it, and it is reduced inside the cloud and in front of ships and islands. `density` is then the baseline emission, `color_low`/`color_high` the thin and dense gas, tinted by the retail colour loop; `inside` scales the gas with the camera inside. `color_alt` and `variation` (0..1) let the gas drift into a second hue in large patches, so a cloud is not one flat colour. For a solar storm, `swirl` (turns a second near the centre, slower outward; two phases are blended so the gas never winds up) wheels the gas round the centre like a cyclone, `filaments` is the brightness of hot plasma seams along the edges of the billows, and `flicker` of lightning flickering deep inside it a few times a second, so it reads as a storm, not a quiet cloud. The shell's breathing (`Cloud::breath()`) sets the ellipsoid, the mission's fadeout distance fades it. The noise is baked once at startup into a 64³ tiling texture |
| `currents` | the two scrolling strips of an Etherium current (retail mesh, UVs and scroll kept: u across, v along the path from its first point to its last, which is the way the force pushes) get `shaders/current.wesl`: flowing streaks stretched along the flow, soft edges, sparkles riding downstream faster than the streaks, slow pulses that run down the current so its direction reads at a glance, and a core past 1 that blooms: `brightness`, `streaks` (across the strip), `sparkle`, `core`, `texture` (share of the retail texture kept), `color`, `pulse`. Added light only (ONE/ONE) |
| `hide_effects` | a list of fragments of retail effect file names (`["Explosion_", "MeteorStorm"]`, not case-sensitive): those effects are not drawn, so a mod can draw its own instead of stacking on the retail look. Drawing only: the effect keeps running (particles, timers, lights, sounds, and whatever the `lights` and `bursts` blocks hang on it through `fx::EffectStarted`). `black_holes.replace` and a replacing `SolarStorm` rule hide their own effects without it |
| `flags` | `flag::Cloth`: `flutter` (ripples across the flag), `lit`, `wind` (follows the ship's speed) |
| `engines` | a plume per jet (an exhaust whose effect name contains `effects`, default `Engine`; Procyon funnels are `SmokeStack_Smoke`) drawn with `shaders/plume.wesl`: `layers` (`[{width, length, inner}]`, shares of `width`/`length`, which are shares of the ship's length), `brightness`, `core` and per-race `colors` (`default` otherwise), `particles` (share of the retail flame sprites kept, jets only) |
| `wakes` | the retail wake drawn as one ribbon per `planes` tilt (degrees, default `[0, 60, -60]`; `[0]` = the retail ribbon) with `shaders/wake.wesl`: `height` (squashes tilted ribbons), `brightness`, `hot`, `core`; the faction tint comes from `data/fx/wake_tint.json` |
| `tracers` | a streak of light behind every shell in flight and a glowing head, facing the camera, drawn with `shaders/tracer.wesl`; the streak shrinks into the shell's last point once it is gone: `bullets` (`[{match, color, width, seconds, head, stretch, smoke}]`; `stretch` draws the head longer along the flight, `smoke` (`{color, width, widen, seconds, rise}`) is a ribbon of smoke the shell leaves hanging in the world along its path: `width` at the shell, `widen` times wider at its oldest end, lasting `seconds`, drifting up `rise` units per second; it needs the `bursts` block for its material; first whose `match` (a name or a list of names) is in the bullet type's name wins, unset keys from `default`; width and head 0 = none), `core`, `brightness`, `particles` (share of the retail flight effects' particles kept) |
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
leaves no cut-off files and no half-made mod in the list. The `sky` block (above) adds a layer of gas in front of it; with `--split-stars` the baker writes the stars as their own 16-bit cube (`<sky>_stars.png`) for the block's twinkling stars, and the sky cube is then without them (a profile without the block shows none).

### Baked surfaces (`tools/bake_surfaces.py`)
`python -m tools.bake_surfaces` (release download: `openprocyon-import --bake-surfaces`) bakes a surface map for every textured
retail material into the local mod `<user data dir>/mods/baked-surfaces`: `textures/surface/<material name without .tga, lowercase>.png`,
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

Every one of these materials also binds the frame data at `@binding(99)`, a storage buffer the engine writes once a frame
(`game/src/render/frame.rs`): a shader reads it after `import openprocyon::render::frame::frame;` (the module ships inside the
engine, `game/src/render/frame.wesl`, which documents the fields). It holds `clock` (real seconds never wrapped, battle seconds,
both frame deltas), `sun` (the direction the sunlight travels, illuminance in 10000 lux) and `sun_color` (w: the flat ambient
light), `camera` (the battle camera's position), the storm's last bolt (`bolt_at`, w the real seconds since it; `bolt_from`, w 1
when it struck a ship; `bolt_to`; both ends are of the displayed segment) and the black holes (`holes_info`: count and the profile's lensing; `holes[4]`: where the sky
bends round each, w its shadow's radius; `hole_reach[4]`: pull radius, kill core). Use it instead of adding a clock, the sun or
the camera to a block's uniform: a material whose uniform holds none of them is never written again after it is made.

| Shader | Drawn as | Uniform (`game/src/render/`) | Engine-provided values |
|---|---|---|---|
| `sail.wesl` | the sail's StandardMaterial extended: call `pbr_input_from_standard_material` and `apply_pbr_lighting`, then add light | `SailGlowUniform` in `sails.rs`: `color`, `params`, `sun`, `motion`, `pattern`, then `fold` and `shape` (for the engine's vertex stage; a sail shader may leave them out of its struct) | sun direction, the ship's smoothed speed share, a per-sail phase and clock, pattern cells per UV unit. The engine ships the sail's vertex stages itself (`game/src/render/sail_vertex.wesl`, `sail_prepass.wesl`, `sail_fold.wesl`): they belly and fold the cut mesh on the GPU from the sail's fold data at `@binding(101)`. A cut sail (`billow` or `furl` on) also has `uv_b` (guard with `@if(VERTEX_UVS_B)`): x 0 where its light gathers (a fan's hub, else the sail's bottom edge) .. 1 at the rim or top, y 0..1 across (a fan's span, else the sail's width) |
| `plume.wesl` | additive (ONE/ONE: write unpremultiplied light, alpha ignored) on a unit cone, u around, v from nozzle (0) to tail (1) | `PlumeUniform` in `trails.rs`: `core`, `edge` (w = 1 on the inner layer), `params` (brightness, thrust, unused, seed) | thrust (written only when it moves), seed; the clock is the frame data's |
| `wake.wesl` | additive on each ribbon, u across, v along (newest at the ship), vertex colour = retail fade | `WakeUniform` in `trails.rs`: `tint`, `hot`, `params` (length, clock, core) | the ribbon's length and the clock |
| `tracer.wesl` | additive on one mesh holding every tracer: streaks with u 0..1 across and v the point's age over `seconds` (0 at the shell), heads as quads stretched along the flight with u 2..3 along it (3 at the front) and v 0..1 across; vertex colour rgb = the bullet's colour, a = what is left of a gone shell's streak | `TracerUniform` in `tracers.rs`: `core` (a = brightness), `params` (unused) | none: the clock is the frame data's |
| `surface.wesl` | the retail StandardMaterial extended (roughness, metallic, reflectance already set from the rule): call `pbr_input_from_standard_material`, change `N`, colour and roughness, then `apply_pbr_lighting`; read `base_color_texture` for relief, or the baked map | `SurfaceUniform` in `surfaces.rs`: `params` (bump, cavity, gloss, 1 when baked), `rim` (colour, strength); `@binding(101)`/`(102)` the baked surface map and its sampler | none |
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
