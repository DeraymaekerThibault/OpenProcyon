# Modding

Installing: unzip a mod into `<user data dir>/mods/` (`$XDG_DATA_HOME/openprocyon`, `%APPDATA%\openprocyon`), then switch it on in Options > Mods. The choice is stored in `mods.json` beside that folder (`{"enabled": ["id", ...]}`, load order, later wins); a mod that is not listed stays off. The overlay is built at launch, so changes apply on the next start. `mods/silver-skirmish` is a one-ship example (Silver's ship at 255 points). `mod.json` takes `id` (default: folder name), `name`, `version`, `description`; `game_version` and `requires` are not read yet.

For development, run with `--mod <folder>` (repeatable, later mods win): `cargo run --release -- --mod ../mods/example --ship Ship_Navy_Cutter_Fast --map red_rover`.

A mod folder mirrors `assets/`. At launch `game/src/modding.rs` builds `local/overlay/` (symlinks to the base files, mod files on top) and the
game, Bevy's asset server included, reads only that tree.

| Mod file | Effect |
|---|---|
| any file with a base path (`hud/tex/fe_05.png`, `ships/Ship_X/model.glb`, `fonts/Sans.otf`, `sound/<kind>/*.ogg`) | replaces the base file: texture packs, upscaled art, new fonts, sounds |
| `ships/<Id>/ship.json` containing `"extends": "Ship_Navy_Cutter"` | deep-merged over that ship: object keys override one by one, arrays and scalars replace; a new ship or weapon is a few lines (see `mods/example`) |
| a new `ships/<Id>/` folder (`ship.json`, plus `geometry.json`/`model.glb` or `geometry`/`model` pointing at another ship), a new `maps/<id>/` folder (`map.json`, `backdrop.png`; `"skirmish": true` lists it), `models/<any folder>/*.glb`, `sound/<any folder>/*.ogg` | new content (schema in `docs/data-format.md`) |

Limits (provisional): the fleet screen lists, per fleet type (`race` Navy/Pirate/Procyon), every ship folder except story/NIS/tutorial hulls (`NOT_OFFERED` in `menu.rs`); a mod ship with `"skirmish": true` is always listed; `mod.json` (`name`, `version`, `description`) is only printed at startup. Weapons and bullets are shared files, `data/weapons/guns.json` and `bullets.json`; a mod's copies are merged key by key, so adding a gun is one entry. Computer captains fit the guns retail computers were seen to fit (carronades, Gattling, plasma, light/medium cannons, star mortar) unless a mod ships `data/ai/weapons.json` (`{"Gun_Name": true|false}`, merged the same way) to restrict or extend what they may pick. The full layout is in `docs/data-format.md`.

## UI art and fonts
UI text is vector (`content/fonts/`: Chancery = URW Z003 for the BlackChancery slots, Nimbus Sans for Univers/Immi; both URW base35 fonts, AGPL with font exception, so they ship with the code).
Sprite sheets are cleaned by `tools/fix_alpha_bleed.py` (transparent texels take their neighbours' colour, which removed the light hairlines at fractional scales).
An upscaled/HD pack is just a mod that replaces `hud/tex/*.png` at 2x or 4x; sprite rectangles are in reference pixels, so a pack must keep the layout
(scale the atlas and the rects together) until the HUD loader reads a per-sheet scale (not done).

### HD UI sheets and 9-slice strips
Layout is always in retail reference pixels. A mod can replace any `hud/tex/<sheet>.png` with a higher-resolution copy and declare the factor in
`hud/scales.json` (`{"fe_05": 2}`, merged across mods); only the source rectangles scale. Buttons and ship-name ribbons are single 9-slice nodes built from
`hud/tex/slice_<button|ribbon>_<skin>_<state>.png` (made by `tools/make_slices.py`); replace those (plus a `scales.json` entry named like the file) for HD buttons.
