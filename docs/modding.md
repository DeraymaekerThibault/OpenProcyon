# Modding

Mods that ship with the game live in `mods/` (the checkout's, or next to the executable in a download; `tools/package_release.py` copies it) and are listed in the Mods screen, off until ticked (except those marked `"default": true`). Installing another: unzip it into `<user data dir>/mods/` (`$XDG_DATA_HOME/openprocyon`, `%APPDATA%\openprocyon`), then switch it on in the Mods button in the corner of the main menu (a table: click a column title to sort, the category button filters, click a row to switch it on or off). The choice is stored in `mods.json` beside that folder (`{"enabled": ["id", ...], "disabled": ["id", ...]}`, load order, later wins); a mod that is not listed stays off, except a bundled mod whose `mod.json` has `"default": true`, which is on until the player switches it off (`disabled` remembers that; `default` in a user-installed mod is ignored, so dropping a folder in never switches anything on); a mod in the user folder replaces a bundled one with the same `id`. Every folder in `mods/` needs a `mod.json` (`tests/test_bundled_mods.py`). The overlay is built at launch, so changes apply on the next start. Examples: `mods/silver-skirmish` (Silver's ship at 255 points, flagged `skirmish`) `mods/faction-colors` (wakes tinted per fleet type, green engine flames on the Procyon ironclads; JSON only, no art) and `mods/game-speed` (on by default: the not-retail 1x-16x speed buttons beside the command bar). `mod.json` takes `id` (default: folder name), `name`, `version`, `description`, `category` (the Mods table's filter; default "Other"), `default` (bundled mods only, see above); `game_version` and `requires` are not read yet.

For development, run with `--mod <folder>` (repeatable, later mods win): `cargo run --release -- --mod ../mods/silver-skirmish --ship Ship_Pirate_Silver --map red_rover`.

A mod folder mirrors `assets/`. At launch `game/src/modding.rs` builds `local/overlay/` (symlinks to the base files, mod files on top) and the
game, Bevy's asset server included, reads only that tree.

| Mod file | Effect |
|---|---|
| any file with a base path (`hud/tex/fe_05.png`, `ships/Ship_X/model.glb`, `fonts/Sans.otf`, `sound/<kind>/*.ogg`) | replaces the base file: texture packs, upscaled art, new fonts, sounds |
| `ships/<Id>/ship.json` containing `"extends": "Ship_Navy_Cutter"` | deep-merged over that ship: object keys override one by one, arrays and scalars replace; a new ship or weapon is a few lines (see `mods/silver-skirmish`) |
| an `extends` file may also carry `"set": {"/engines/0/fx": "X"}` (JSON pointers, applied after the merge) to change one value inside an array, which a merge cannot do; a pointer that does not exist is an error |
| `menu/strings.json` | merged key by key with the retail string table: new ids for names a mod's ships and screens use (e.g. a ship's `display_name`) |
| `fx/<Name>.json` | a new effect file is loaded without being listed anywhere; `extends` + `set` recolours a copy of a retail effect |
| `data/hud/speed.json` (`{"max": 16}`) | shows the game-speed buttons beside the command bar (not retail), stepping 1x to `max` (default 16); absent: no speed control, as in retail. `--speed N` on the command line works either way |
| `data/fx/wake_tint.json` (`{"Navy": [r, g, b], "Pirate": ..., "Procyon": ..., "Civilian": ...}`) | multiplies the wake texture per fleet type (multiplied in linear light, values may exceed 1); absent: retail colours |
| `models/sky/<sky>_cube.png` (six square cube faces stacked vertically, +X -X +Y -Y +Z -Z in Bevy's cube-map frame) | drawn as the battle camera's skybox instead of that retail sky's sphere (not retail); with a render profile's `sky_light` it also lights the scene. A sky made from the player's own imported sky is derived from game content: keep it local, never ship it |
| a new `ships/<Id>/` folder (`ship.json`, plus `geometry.json`/`model.glb` or `geometry`/`model` pointing at another ship), a new `maps/<id>/` folder (`map.json`, `backdrop.png`; `"skirmish": true` lists it), `models/<any folder>/*.glb`, `sound/<any folder>/*.ogg` | new content (schema in `docs/data-format.md`) |

Limits (provisional): the fleet screen lists, per fleet type (`race` Navy/Pirate/Procyon), every ship whose `ship.json` has `"skirmish": true` (the importer copies retail's "Available in Multiplayer?" flag; a mod can set it on any ship); `mod.json` (`name`, `version`, `description`) is only printed at startup. Weapons and bullets are shared files, `data/weapons/guns.json` and `bullets.json`; a mod's copies are merged key by key, so adding a gun is one entry. Computer captains fit the guns retail computers were seen to fit (carronades, Gattling, plasma, light/medium cannons, star mortar) unless a mod ships `data/ai/weapons.json` (`{"Gun_Name": true|false}`, merged the same way) to restrict or extend what they may pick. The full layout is in `docs/data-format.md`.

## UI art and fonts
UI text is vector (`content/fonts/`: Chancery = URW Z003 for the BlackChancery slots, Nimbus Sans for Univers/Immi; both URW base35 fonts, AGPL with font exception, so they ship with the code).
Sprite sheets are cleaned by `tools/fix_alpha_bleed.py` (transparent texels take their neighbours' colour, which removed the light hairlines at fractional scales).
An upscaled/HD pack is just a mod that replaces `hud/tex/*.png` at 2x or 4x; sprite rectangles are in reference pixels, so a pack must keep the layout
(scale the atlas and the rects together) until the HUD loader reads a per-sheet scale (not done).

### HD UI sheets and 9-slice strips
Layout is always in retail reference pixels. A mod can replace any `hud/tex/<sheet>.png` with a higher-resolution copy and declare the factor in
`hud/scales.json` (`{"fe_05": 2}`, merged across mods); only the source rectangles scale. Buttons and ship-name ribbons are single 9-slice nodes built from
`hud/tex/slice_<button|ribbon>_<skin>_<state>.png` (made by `tools/make_slices.py`); replace those (plus a `scales.json` entry named like the file) for HD buttons.

## Graphics mods: the render profile (not retail)
Graphics upgrades are mods, never engine forks. The engine (`game/src/render/`) provides building blocks; a mod switches
them on and tunes them with one file, `data/render/look.json`, and ships the shaders most of them draw with
(`shaders/sail.wesl`, `shaders/plume.wesl`, `shaders/wake.wesl`, `shaders/tracer.wesl`, `shaders/surface.wesl`,
`shaders/burst.wesl`, `shaders/smoke.wesl`; the engine ships none, and a block whose shader is
missing stays off with a warning). Without `look.json` every part renders as classic, and a profile only changes drawing,
never rules. Every key is optional; an absent block leaves that feature classic.

| Key | Effect |
|---|---|
| `camera` | HDR camera: `bloom` (intensity), `bloom_threshold` (only light brighter than this blooms; 0 = Bevy's natural bloom, which hazes the image), `tonemapping` (`tony_mc_mapface`, `agx`, `aces`, `blender_filmic`, `reinhard_luminance`), `saturation`, `contrast`, `exposure` (stops), `smaa` (replaces MSAA), `ssao` (crashes with some effects meshes, keep false), `vignette` (corner darkening), `aberration` and `aberration_pulse` (colour fringing at rest, and added by a blast's camera jolt), `contact_shadows` (screen-space sun shadows where parts touch; adds a depth prepass) |
| `sky_light`, `ambient` | image-based light (cd/m²) computed once from a mod's cube-map sky (`models/sky/<sky>_cube.png`), and the multiplier on the flat ambient light while it is on |
| `sun` | `shadows`, `shadow_distance` (cascades out to this many units), `color` (multiplies the map's sun colour), `intensity` (multiplies its strength) |
| `fx_gain`, `shell_gain` | multiply ONE/ONE effect colour and the shells' spheres, so they pass 1 and bloom |
| `lit_smoke` | blended particles (smoke, dust) take the scene's lights: explosions light their own smoke |
| `lights` | point lights on effects: `effects` (a fragment of the effect file name, first entry that matches wins), `color`, `lumens`, `range`, `seconds` (quadratic fade; 0 = steady while the effect lives), `flicker` (share) |
| `sails` | glTF materials whose name contains one of `match` (default `["sail"]`) get `shaders/sail.wesl`: `glow` RGB (0 for names in `plain`), `speed`, `cells` (pattern cells across the sail), `params` (four numbers for the shader, also settable one by one as `line`, `wave`, `steady`, `fill`), `backlit`, `transmission`, `opaque` (solid, casts shadows), `billow` (bellies out between its edges, more with speed) |
| `flags` | `flag::Cloth`: `flutter` (ripples across the flag), `lit`, `wind` (follows the ship's speed) |
| `engines` | a plume per jet (an exhaust whose effect name contains `effects`, default `Engine`; Procyon funnels are `SmokeStack_Smoke`) drawn with `shaders/plume.wesl`: `layers` (`[{width, length, inner}]`, shares of `width`/`length`, which are shares of the ship's length), `brightness`, `core` and per-race `colors` (`default` otherwise), `particles` (share of the retail flame sprites kept, jets only) |
| `wakes` | the retail wake drawn as one ribbon per `planes` tilt (degrees, default `[0, 60, -60]`; `[0]` = the retail ribbon) with `shaders/wake.wesl`: `height` (squashes tilted ribbons), `brightness`, `hot`, `core`; the faction tint comes from `data/fx/wake_tint.json` |
| `tracers` | a streak of light behind every shell in flight and a glowing head, facing the camera, drawn with `shaders/tracer.wesl`; the streak shrinks into the shell's last point once it is gone: `bullets` (`[{match, color, width, seconds, head, stretch, smoke}]`; `stretch` draws the head longer along the flight, `smoke` is a `bursts` smoke part whose `count` is puffs per second and needs the `bursts` block, first whose `match` is in the bullet type's name wins, unset keys from `default`; width and head 0 = none), `core`, `brightness`, `particles` (share of the retail flight effects' particles kept) |
| `surfaces` | textured, lit, non-blended glTF materials (not sails) get `shaders/surface.wesl`, which derives relief from the colour texture while drawing: `rules` (`[{match: [...], roughness, metallic, reflectance, bump, cavity, gloss}]` against the material name, first match wins, unset keys from `default`), `skip` (names left retail), `rim` and `rim_strength` (light along the silhouettes). A cloaking ship's meshes (and sails) wear their retail material while faded |
| `bursts` | particles thrown when a retail effect starts whose name contains a rule's `match` (`effects`, first match wins): parts `flash`, `sparks`, `embers`, `fireball`, `ring` (flat shockwave), `smoke`, each `{count, color, size, seconds, speed, spread (degrees about the effect's forward; a muzzle's is its shot's), drag, rise}`; `every` (seconds: keep throwing while the effect lives, e.g. fires), `shake` (camera jolt). `scorch` (a part: glow where a shell scars a hull), `motes` (a part: `count` specks of dust within `speed` units of the camera). Glows draw with `shaders/burst.wesl`, smoke with `shaders/smoke.wesl` |

### Shaders
The shaders are fragment shaders in Bevy's WESL. Each gets the mesh's `VertexOutput` (world position and normal,
UV, vertex colour) and one uniform at `@group(constants::MATERIAL_BIND_GROUP) @binding(100)`, whose layout is a Rust
struct the engine fills; the field order and types must match. A replacement keeps the uniform and may use its fields as it
likes.

| Shader | Drawn as | Uniform (`game/src/render/`) | Engine-provided values |
|---|---|---|---|
| `sail.wesl` | the sail's StandardMaterial extended: call `pbr_input_from_standard_material` and `apply_pbr_lighting`, then add light | `SailGlowUniform` in `sails.rs`: `color`, `params`, `sun`, `motion`, `pattern` | sun direction, the ship's smoothed speed share, a per-sail phase and clock, pattern cells per UV unit |
| `plume.wesl` | additive (ONE/ONE: write unpremultiplied light, alpha ignored) on a unit cone, u around, v from nozzle (0) to tail (1) | `PlumeUniform` in `trails.rs`: `core`, `edge` (w = 1 on the inner layer), `params` (brightness, thrust, clock, seed) | thrust, clock, seed |
| `wake.wesl` | additive on each ribbon, u across, v along (newest at the ship), vertex colour = retail fade | `WakeUniform` in `trails.rs`: `tint`, `hot`, `params` (length, clock, core) | the ribbon's length and the clock |
| `tracer.wesl` | additive on one mesh holding every tracer: streaks with u 0..1 across and v the point's age over `seconds` (0 at the shell), heads as quads stretched along the flight with u 2..3 along it (3 at the front) and v 0..1 across; vertex colour rgb = the bullet's colour, a = what is left of a gone shell's streak | `TracerUniform` in `tracers.rs`: `core` (a = brightness), `params` (clock) | the clock |
| `surface.wesl` | the retail StandardMaterial extended (roughness, metallic, reflectance already set from the rule): call `pbr_input_from_standard_material`, change `N`, colour and roughness, then `apply_pbr_lighting`; read `base_color_texture` for relief | `SurfaceUniform` in `surfaces.rs`: `params` (bump, cavity, gloss), `rim` (colour, strength) | none |
| `burst.wesl` | additive on one mesh of every glowing particle: uv.x = kind × 4 + u, uv.y = seed × 2 + v (u, v 0..1 across the quad; kinds 0 flash, 1 spark (u tail to head), 2 ember, 3 fireball, 4 ring (flat), 5 scorch, 6 dust); vertex colour rgb = colour, a = age share of its life (dust: its fade) | `BurstUniform` in `bursts.rs`: `params` (clock) | the clock |
| `smoke.wesl` | alpha-blended camera-facing puffs sorted far to near, same vertex layout (kind 7) | `SmokeUniform` in `bursts.rs`: `sun` (direction, clock), `light` (sun colour), `shade` (ambient colour) | the sun, the ambient light, the clock |

To make a graphics mod: start from an empty `data/render/look.json`, add the blocks you want, and put the shaders of the
`sails`, `engines`, `wakes`, `tracers`, `surfaces` and `bursts` blocks in the mod's `shaders/` folder. Run with `--mod <folder>`;
the profile is read at launch, so restart after edits. New kinds of material (a shader for hulls, say) need a Rust building
block in `game/src/render/` first: add it there, keep classic unchanged without the profile key, and document its uniform here.
