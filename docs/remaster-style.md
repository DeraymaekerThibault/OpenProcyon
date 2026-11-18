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
5. **Space**: skies, nebulae, black holes and Etherium currents are alive through relative motion and change, never through
   a rigid turn of the picture: layers of gas that slide against each other as the camera moves, gas that churns in place
   (domain-warped noise), a breath in the bright cores, and events that light it (a storm's bolt, a big blast, a swallowed
   ship). **Replace, don't stack**: when the profile draws a nebula, a storm or a black hole itself (`replace`), the retail
   drawing of it is hidden (its simulation keeps running), never drawn underneath with ours on top: gas that only adds light
   on the retail shell reads as a flat wash, and retail glows and beams contradict a redesigned hole. Gas is a volume (emission
   and absorption, lit by the sun, ending at the depth of the hulls and islands inside it), so it has depth and dark lanes.
   It may thicken the view but never hide a ship: one cloud's opacity is capped (`max_opacity`, lower where the ray ends on a
   hull), a bolt is drawn along its real segment, and the sky's events light the gas, never the stars.
   The dangerous parts of the world must read at least as clearly as in classic: a black hole's black horizon is exactly its
   kill core and carries a thin ring that stays visible at any zoom, with the pull radius faintly outlined; a current shows
   which way it flows at a glance. Navy solar sails fold like a hand fan, canvas like a roller blind.
6. **Cost**: a steady 60 fps on a mid-range GPU (RX 6700 XT) in a full battle. Measure with `PROCYON_PROFILE=1`
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
- **Screen-space lensing of ships and effects behind a black hole** and **motes riding a current**: not built (optional in the
  plan); the hole bends only the sky behind it, which is almost always what the camera sees there.
- **Additive gas on the retail nebula shell, and the retail black-hole layers left under ours** (the first `nebulae` and
  `black_holes` blocks): additive-only light cannot show dark dust, silhouettes or self-shadow, so the clouds read as a flat
  lilac wash or, dimmed, a faint haze; the march ran only where the shifting retail shell had fragments, so a cloud's edge
  was the shell's polygon silhouette and a close camera met a flat textured wall; and the retail pink cone, blue beams and glow
  blobs sat in the black kill zone of the new hole. Replaced by `replace`: the retail drawing is hidden, the cloud is a proxy
  box marched as a volume (premultiplied, ending at the depth prepass), and the hole is drawn from scratch.
- **The lensed far side of the accretion disc** (the hump arching over the shadow): not built (optional in the plan); the disc is
  a thin flat ring and the sky behind the hole bends, which carries the sense of weight at the game's camera angles.
- **Warm or contrasty colour grading**: colours turned red and too dark, so grading stays neutral.

## Rules
- The engine (`game/src/render/`) gets only the building blocks: profile settings, uniforms, docs, tests. The look lives in
  `mods/remaster`, and the classic presentation stays the default.
- Nothing derived from game content is committed or shared: baked skies, or maps generated from retail textures. Those
  are made on each player's machine (`python -m tools.bake_sky`, or the Mods screen's "Bake HD skies" button in the download; `python -m tools.bake_surfaces` for surface maps; see "HD skies" and "Baked surfaces" in `modding.md`).

## Notes on `look.json`

- `bursts.effects` lists `Lancer` and `Plasma` before `_Impact` / `_Muzzle` on purpose: the first matching rule wins, so the
  energy weapons' own entries (no smoke) take their impact and muzzle effects before the generic rules do.
- `sun.shadow_distance` is the biggest cost of the look: the four cascades redraw every island mesh, so on busy maps (`border_dispute`,
  3700 entities) 2500 units cost 20 ms a frame and 800 cost 14 ms (`red_rover` 17.5 -> 12 ms). The battle camera never looks further
  than a few hundred units, so keep it near 800; removing the `surfaces` block gave the same saving, so the two multiply.
