# Remaster style guide

Direction for the bundled `mods/remaster` look (a graphics mod, off by default; see "Graphics mods" in `modding.md`).

The direction for the remastered look: what it should feel like, what we tried, and what to check before adding more.
Judge every change by playing, not from screenshots.

## North star

Keep the character and art of the original, and bring it into 2026. The film it comes from set the rules:

- **70/30**: about 70% traditional (18th-century sailing ships, brass, wood, canvas) and 30% sci-fi. When old and new
  pull apart, favour the old.
- **Lit like an oil painting**: the art direction drew on the Brandywine school of illustration (Howard Pyle,
  N.C. Wyeth), with painterly 3D environments built with Disney's Deep Canvas. Aim for warm golden key light, cool
  fill from the sky, and soft but rich colour, not hyper-real metal.
- **The painted textures are the character.** Light them better; don't bury them under fake relief or noise.
- **Readability first**: the battle must read at least as well as classic. Team colours, shells and ships stay clear
  at every zoom.

Reference: the SIGGRAPH 2002 talk "Creating 3D painterly environments for Disney's Treasure Planet" (Odermatt and
Springfield), https://history.siggraph.org/learning/creating-3d-painterly-environments-for-disneys-treasure-planet-by-odermatt-and-springfield

## Pillars

1. **Projectiles**: layered bolts, as in current game effects: a white-hot nose stretched along the flight, a
   coloured glow, a trail that frays into wisps, and smoke behind powder shot. Muzzle blasts (a flash, sparks, smoke
   blown along the shot), impact sparks and embers, and hull scars that glow and cool.
2. **Light**: a warm sun, contact shadows where parts touch, a cool rim light on silhouettes, and reflections only on
   real metal (gold, brass, guns). Relief from the textures stays subtle.
3. **Drama**: explosions with a fireball that cools as it swells, a shockwave ring, sparks, floating embers and dark
   smoke. Burning ships throw embers and a smoke column. Big blasts jolt the camera.
4. **Finish**: a gentle vignette, and a hint of colour fringing on big blasts only.
5. **Cost**: a steady 60 fps on a mid-range GPU (RX 6700 XT) in a full battle. Measure with `PROCYON_PROFILE=1`
   (it logs `frame_ms`).

## Tried and dropped

- **Etherium haze and light shafts** (distance fog in the sky's colours; Bevy volumetric fog lit by the sun): a foggy
  mess at the game's camera distances. The engine code was removed too. Any future attempt must stay far in the
  background and never veil the ships.
- **Sail glow from the retail texture**: the textures are too small, so the procedural honeycomb replaced it.
- **Retail flame sprites under the new engine plumes**: they clashed (`engines.particles` 0).
- **Thin even-width tracer lines**: they read as debug lines, so tracers became the layered bolts above.
- **Strong relief from texture brightness**: it came out as embossed noise on the small painted textures, so it was
  cut to about a third.
- **Rigid sky drift** (`sky_drift`: the whole baked sky turning at 0.6 degrees a minute): nobody sees 9 degrees in a battle, and
  when seen, every star and cloud moving together reads as the camera rolling. Replaced by the `sky` dome: relative motion
  (parallax, a current) instead of rotation.
- **Warm or contrasty colour grading**: colours turned red and too dark, so grading stays neutral.

## Rules
- The engine (`game/src/render/`) gets only the building blocks: profile settings, uniforms, docs, tests. The look lives in
  `mods/remaster`, and the classic presentation stays the default.
- Nothing derived from game content is committed or shared: baked skies, or maps generated from retail textures. Those
  are made on each player's machine (`python -m tools.bake_sky`, see "HD skies" in `modding.md`).

## Notes on `look.json`

- `bursts.effects` lists `Lancer` and `Plasma` before `_Impact` / `_Muzzle` on purpose: the first matching rule wins, so the
  energy weapons' own entries (no smoke) take their impact and muzzle effects before the generic rules do.
