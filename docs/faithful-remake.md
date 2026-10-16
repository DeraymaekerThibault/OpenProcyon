# Direction reset: reproduce Battle at Procyon first

**Current gap checklist:** [fidelity-backlog.md](fidelity-backlog.md). This page preserves the original direction and successive milestone reports;
older “not done” lists and the early setup/runtime recommendations below are historical, not a current feature inventory.
For Mission 1 validation see [re/campaign.md](re/campaign.md).

The goal is a single-player reimplementation of the **original game**, using
assets from the user's installation first. Improvements and replacement art come
**after** a recognizable, behaviorally checked baseline. The existing 2D browser
duel is a rejected mechanics experiment, not the remake, and is not the foundation
for its presentation or control scheme. Preserve it rather than silently deleting
work, but do not extend its invented ships, controls or balance.

## What the original actually does — documented evidence

Primary reference: the user's own `Support/Treasure_Planet_eManual.chm`, specifically
`Play/Basic_Controls.htm` and `Play/Advanced_Controls.htm`, plus `hud-annotated.gif`
and `navy_cutter.jpg`. Selected files were extracted read-only with 7-Zip; no original scripts or executable were
run. Manual images and text remain outside Git.

The [official Steam description](https://store.steampowered.com/app/331970/) identifies
a 3D space-combat RTS, with age-of-sail ship designs, fleets, crew, weapon banks,
waypoints, repairs, towing/grappling and boarding. The local manual establishes:

| Area | Documented original behavior | What was wrong with the toy |
|---|---|---|
| View | 3D ship-following viewpoint, ship centered; right-drag horizontally rotates and vertically tilts | Flat top-down arena |
| Keyboard camera | Arrow keys adjust the viewpoint | Arrow keys were mapped to steering |
| Movement | Left-click destination, green marker, dotted yellow curved course | Direct WASD-centered handling |
| Engine orders | All Stop, Ahead Slow, Ahead Full, Emergency Speed; +/- adjusts order | Continuous invented throttle |
| Maneuvering | Speed, ship type/size and damage affect turning; course preview updates with speed | Uncalibrated generic yaw model |
| Multiple waypoints | Plot a sequence, then accept/check mark or cancel/X | Only one immediately accepted point |
| Command-ship firing | Left-click hostile ship/section; charged banks in range and direction fire | Two generic Q/E batteries |
| Banks | Original mount groups and weapon types; an individual bank icon can fire only that bank | Four invented guns per side |
| Damage | Aim at sails, hull, rudder, etc.; status distinguishes ship, sails and crew | One invented hit-point bar |
| Fleet | Command ship differs from secondary ships; secondary captains carry out attack orders | A generic one-rival loop |

**Manual-described is not runtime-measured.** Turning equations, speed multipliers,
unit conversions, projectile timing, damage calculations and AI decision rules are
still unverified. The manual does not justify the toy's numeric parameters. A basic-
tutorial [video reference](https://www.youtube.com/watch?v=ip-8TqMa_Q0) was located,
but automated frame extraction failed because yt-dlp was not installed; its footage
was not analyzed in this environment. Do not claim direct gameplay observations.

## Original assets available and inspected

Use only the explicitly supplied installation, read-only. Keep its absolute local
path out of tracked configuration; set `PROCYON_GAME_ROOT` in the environment.
Do not download game files from public mirrors.

For **one Navy Cutter**, the following were confirmed locally:

- `WorldObjects/Ship_Navy_Cutter.wot` references scene `NavyCutterShip`.
- `BD_MeshScenes/NavyCutterShip.msb` exists (12,197 bytes). Embedded identifiers
  include `NAVYCUTTERMASTERNULL`, named mounts and `GunPlacement` descriptors.
  This confirms relevant names, **not** decoded scene transforms/hierarchy.
- `BD_Meshes/NavyCutterShip_Layer1.mdb` through `Layer6.mdb` exist.
- `BD_Textures` contains 800 DDS files, including the material textures needed by
  the probed cutter meshes. DDS is a standard format; the proprietary mesh/scene
  interpretation is the less-established part of this pipeline.

A read-only binary probe, cross-checked against the public TPForm reader's format
layout, inspected the six cutter layers. It validated model record boundaries,
finite positions/UV candidates, triangle vertex indices and material indices:

| Layer | Model 0 vertices | Model 0 triangles | Model 1 vertices | Model 1 triangles |
|---|---:|---:|---:|---:|
| 1 | 532 | 484 | 283 | 171 |
| 2 | 790 | 639 | 458 | 267 |
| 3 | 377 | 247 | 257 | 127 |
| 4 | 374 | 247 | 254 | 125 |
| 5 | 1107 | 1206 | 318 | 254 |
| 6 | 796 | 857 | 267 | 203 |

These are record facts, not a rendered or assembled ship. Model 0 totals 3,976
vertices / 3,680 triangles before any welding. The two records are consistent with
detail variants, but the original LOD-selection behavior was not verified.

The common material table contains **25 texture references** stored with `.tga`
names. Replacing that suffix with `.dds` resolved **25/25 to existing local files**.
Blindly appending `.dds` did not work; the suffix replacement was verified. No
texture image was converted or copied into Git.

Limits: substantial trailing data remains uninterpreted (21,226 bytes in layer 1),
as do some vertex/material fields. Normals, winding, alpha, vertex colors, animation,
scene assembly, hardpoint transforms and appearance are not yet validated. This was
**a format feasibility probe, not a successful mesh conversion**.

Update: the two "unknown" vertex floats are a packed smooth normal (azimuth, elevation in radians);
outward normal in game axes = (-cos(el)·sin(az), cos(el)·cos(az), sin(el)). The axis/sign convention was
fitted against face normals of retail meshes (mean dot ~0.93), not read from the exe. `tools/mdb_to_glb.py`
writes them as glTF NORMAL. Without them Bevy used flat per-face normals, so everything looked faceted.

### Useful public tool lead, not a ready importer

[TPForm](https://github.com/ScarpMarc/TPForm), inspected at
`32f7507d62adff18748617f9cc801c5a8884d5c7`, is an archived GPL-3.0 mesh editor.
Its `TPMesh`, `TPModel`, `TPVertex`, `TPFace` and `TPMaterial` source provides useful
format evidence. No source was copied into this repository or executed. Importantly,
its inspected `TPMesh.Export()` **writes MDB**, not OBJ/glTF. Do not repeat the
unverified search-summary claim that it is already an MDB-to-OBJ converter.

## Asset-first implementation path

1. **One original ship, visibly correct.** Implement a bounded, tested importer for
   one cutter's MDB geometry/material subset and the required MSB scene assembly.
   Preserve unknown fields/bytes and report them; use byte offsets as binary
   provenance. Resolve its DDS references, produce a local-only glTF/GLB (or direct
   validated engine mesh), and visually compare the assembled ship against the
   original reference. Validate all six layers, UVs, alpha, normals/winding, scale,
   orientation, mast/sail placement and gun/engine attachment points. Do not replace
   unresolved parts with invented art and call it done.
2. **Original viewpoint and orders.** Put that ship in a 3D test scene. Implement the
   documented follow/orbit/zoom camera, selection disc, destination marker, predicted
   curved course and four engine orders. Reuse relevant original UI assets locally.
   Compare movement at each order; record unknown mappings instead of guessing
   that a WOT field means an already-chosen physics formula.
3. **One faithful combat encounter.** Add one original target, actual mounted guns
   and bank groupings from WOT/MSB, click-to-fire, charge/range/arc indicators and
   the relevant projectile type. Restore component targeting/damage, not just one
   generic health pool. Validate shot origin, direction, timing and visible damage.
4. **Expand toward the original single-player game.** Import one tutorial world's
   terrain/placements, then the necessary objectives/rules. Add secondary-ship
   orders/formations, crew effects, repair, towing/grappling/boarding and the other
   weapon behaviors in measured slices. Generalize to campaign missions only after
   each required system has a verified baseline. Full campaign parity is the goal,
   not something the first small scene can honestly claim.
5. **Only then modernize.** Replace art using the same asset identities/attachments;
   introduce intentional control, balance, rendering or content changes separately
   from the original baseline so fidelity regressions can be distinguished.

### Runtime recommendation

Rust remains reasonable. Recommend **Rust + Bevy for the 3D runtime**, Python for
file parsing/conversion, and glTF as the engine-facing asset boundary. Bevy has an
[official glTF scene-loading example](https://bevy.org/examples/gltf/load-gltf/).
The bottleneck is validating proprietary asset interpretation and original behavior,
not whether the simulation is C# or Rust. No engine is installed or validated for
this new slice yet; validate the single-ship import before expanding implementation.
Do not write another custom renderer or build parallel Bevy/Unity/Godot prototypes.

## Next acceptance gate

**The next scene must show the recognizable original textured Navy Cutter in 3D,
with the original camera and point-and-click/engine-order interaction.** Evidence
must include an actual scene run, screenshots and source-linked import checks.
A generic silhouette with synthetic WASD controls does not pass this gate, regardless
of how many tests it passes. No new art, redesign or balance additions before that.

## Status: first acceptance gate reached (slice 1)

`game/` shows the original textured Navy Cutter in 3D (Bevy 0.20), following camera with right-drag rotate/zoom
and arrow-key view, left-click destination marker with a yellow course line, four engine orders, braking to stop on
the marker. A scripted run (`--shot`) produced a screenshot showing the textured ship sailing to its waypoint.
Interactive input was not exercised in that run, only the scripted path.

**Provisional (not measured original behaviour):**
- Engine order speed fractions 0 / 0.5 / 1.0 / 1.25 of WOT `MaxSpeed` (8.06). Real mapping unknown.
- Linear motion has the exe's structure (throttle in [0,1], linear drag rate (MaxThrust/Mass)/MaxSpeed, terminal speed = throttle×MaxSpeed)
  but the time scale is a provisional ×0.1 on MaxThrust/Mass, and the order→throttle mapping is unknown (see re/movement.md).
- Turning now follows the exe's ShipPhysics update (speed-dependent torque, speed bands, quadratic rotational drag; see
  re/movement.md) but the rudder input is still my proportional heading controller, not the original curved-course planner,
  and the meaning of some torque fields is assumed. Damage effects on turning are not modelled.
- Waypoints: click replaces, Shift appends; no accept/cancel step; no dotted predicted-course curve.
- Camera zoom/pitch limits, grid and lighting are mine.
Not yet: guns/bank fire, damage sections, other ships, ship flags/wake/engine effects, sound, UI assets.

## Status: combat slice (stationary target)

`game/src/combat/`. Data from the WOT files via `tools/export_combat.py`: the cutter's 10 mounts (positions and yaw from
the MSB, gun type/arc/bank from the WOT), 4 gun types (muzzle speed, reload, burst, range bands, accuracy deviation, gravity flag),
4 bullet types (lifetime, per-material damage bounds), 6 damage sections (HP, material, vital flags, core percent) with their
real mesh triangles as hit geometry. Click an enemy ship to target it; guns in range and arc fire when loaded; shells fly,
hit the first section triangle they cross, and apply per-material damage in [lower, upper].
Verified by a scripted run: shells in flight, hits on the stern/starboard hull, stern destroyed, core 200 to 110.

**Provisional here:** gravity 9.8; the number after the gun name in the WOT read as total horizontal arc in degrees;
guns start loaded; (aim lead and deviation now exe-derived, see `re/gunnery.md`);
destroying a vital section removes its `vitalSectionCoreDamagePercent` of core HP (4 sections sum to 95%, so the exact rule
is unclear); criticals are not modelled (exe: tally + crew speech only, see re/movement.md); bank grouping is ignored (all guns fire at the target);
the enemy does not move or shoot; no debris/effects/sound; damage does not yet slow the ship or break sails.

## Status: skirmish slice (playable)

Your ship vs enemy ships on the same rules. Ship classes are data-driven (`tools/export_combat.py` per ship: physics,
guns with parent damage sections, gun/bullet tables, sections with real mesh triangles); 7 are built by default (Navy Cutter,
Frigate, FastFrigate; Pirate Schooner, Sloop, Gunboat, Barque), any `Ship_*` can be added. Added since the combat slice:
- Enemy AI (mine): close to the guns' mean effective range, then hold a broadside at slow speed and fire.
- Damage effects: a destroyed section disables the guns mounted on it (MSB parent link); destroyed vitalToMaxVelocity sections
  (masts/sails) cut top speed; core HP bars above ships; destroyed ships vanish with a bigger flash.
- Victory / defeat messages, `R` restart, case-insensitive asset lookup (Windows-era name case mismatches).
Verified by scripted runs: both sides' shells in flight and hits on both sides, stern and mast sections destroyed (gun count dropped
10 to 9), victory and defeat messages, restart resetting the scenario. A full fight is not verified to resolve on its own: ships
tend to circle and trade fire for several simulated minutes; use K/J or `--script` to test the end states.

Speed and turning loss from destroyed sections are exe-derived (re/movement.md): lost fraction of vitalToMaxVelocity /
vitalToManeuverability sections, capped at 90%.

**Provisional:** everything in the earlier lists, plus: the AI's
distances and behaviour, enemy ship choice and ring placement, which guns exist on ships with unsupported bullet types
(2 frigate grav-charge guns are skipped), torpedoes/harpoons/mines treated as ordinary shells, no formations, boarding, towing,
crew, repair, or missions.

## Status: gun models

Each mount spawns the gun's real two-part model (base turns, barrel pitches about the pivot from the gun's MSB scene) as a child
of the ship at the mount's position and orientation (yaw, plus the second angle as an inversion about the bow axis for the 24
inverted mounts such as the cutter's light lancers; that angle's meaning is inferred, and the third angle is treated as pitch).
Guns turn at the WOT `HorizontalRotationSpeed`/`VerticalRotationSpeed`, respect the vertical limits, return to rest with no target,
fire only when aligned within 0.05 rad, and shells leave from the muzzle point (dual guns alternate muzzles). Below-deck (2Deck)
guns have no mesh and stay invisible. Not done: recoil, muzzle flash/smoke (effects track), destroyed-mount visuals,
yaw limits beyond the provisional arc.

## HUD (main bar)
`tools/export_hud.py` -> `assets/hud/` (components + 179 sheets). `game/src/classic/panels.rs` builds the primary-ship main bar from
`hud.hdt` sprite rects: left cap, panic button, ship-icon frame with the class icon, health/combat/sail gauges
(fills crop from the bottom), right cap, scaled from 640x480 to the window height and centred at the bottom.
Sprite rects, icon offsets and gauge-fill offsets are retail data. The left-to-right order and the widths between parts
are inferred from the sheet (MainBar::slot 2 at centres the bar and sums slot widths; not fully decoded).
Gauges: health = core/max core; combat = guns ready/total (assumed); sail = speed factor (assumed).
Speed dial: `SpeedControl_CurrActiveN_NextActiveN` sprites (4 lever positions); the exe picks the sprite name from a
0..3 state (SpeedControlWindow slot 4). Corrected visual mapping: 0 Stop (red), 1 Slow (yellow),
2 Full (light green), 3 Emergency (dark green). The old mapping was reversed. Clicks now select the nearest coloured
segment in the curved sprite, not a vertical row. HUD builds when the player exists (including menu launch) and rebuilds
for class changes. All four lever positions were checked in GPU screenshots; exact native dial hit regions are unverified.
Compass/radar: brass frame (`COMPASS_OVERLAY`, 128x123) over the radar disk (offset 18,12) and 10-frame sweep (0.1 s per
frame, offset 17,13); enemy contacts use `HUDRADAR_ICON_ENEMYSHIP_*`, placed ship-relative with forward = up.
Screen placement of both windows and the radar range (300 units) are PROVISIONAL (Compass slot 2 at anchors to a
view rectangle; not decoded). Contacts for friendly/other types and the animated icon frames are not done.
Bar parts share their top row (gauge offsets are "from MainBar TopLeft" and every part's frame art starts on the same row; rect heights
differ only in bottom rows), docked with the shortest part's bottom on the screen edge. The weapon and ship-bar toggles stand on their
`*_ToggleDetail` arms: arm bottom `Move_Toggle*_Detail_Down` px behind the compass / talking head, button at
`Toggle*_Button_Offset_From_Detail_TopLeft` (checked against a retail 1:1 screenshot). HUD ship icons use the class's WOT icon name.
Not done: cursors, ship-name labels, tooltips.

## Sound
`game/src/audio.rs` plays the original Ogg files: gun fire, per-material bullet impacts, section loss and ship explosion
pools from the exe, engine loops per engine order, speed-control/weapon-bar clicks and one skirmish music track. Names,
volumes and max distances come from the WOTs and exe tables; event mapping, falloff curve and levels are PROVISIONAL.
See re/sound.md. Not done: ricochets, collisions, travel loops, crew speech, per-map music, stereo panning.

## Front end and skirmish maps
`tools/export_maps.py` -> `assets/maps/` (all 14 World/*.twt: players with start point/forward/team/formation, scenery
objects with position/orientation, skybox, music, backdrop) plus scenery/skybox GLBs, and `assets/menu/` (Screens/*.tst
layouts for Main Menu, Single Player, Create Skirmish Game, Skirmish Game Lobby; the English string table; FrontEndScreen0
and map backdrops reassembled from their tiles). `game/src/menu.rs` draws those screens at their retail rectangles with the
hud.hdt FrontEnd_* button/list/arrow sprites (hover/press states) and the FFT bitmap fonts.
Flow: Main Menu -> Single Player -> Start Skirmish (map list, Historical/Open radio, description, fleet size) -> Game Lobby
(player list with fleet type/team, Launch) -> on open maps Assemble Fleet = the retail Ship Selection screen (see re/skirmish-setup.md).
Historical maps use their pre-placed fleets and only let you pick a side. F10 in battle returns to the main menu. `--map <id>` / `--menu [single|skirmish|lobby]` for scripted runs.
Battle on a map: scenery, skybox and music from the map; our fleet at the first player's start point, the enemy at the
first player of another team, both facing their start vectors, in the exe's LineAbreast layout (member i one
spacing beside member max(i-2,0), odd to port; spacing = largest ship size + 30).
PROVISIONAL: every FormationType uses LineAbreast; ship size is our section-box radius; scenery yaw sign; which player slot
is ours; historical maps ignore their pre-placed ships; the lobby cell layout and the Ship Selection list/info (no 3D preview, victory points, crew, arms, load/save); map picture placed in the Lobby Information area. Menu music: FrontEndMusic loops from the main menu.
Not done: full campaign, multiplayer, options, profiles, victory points, nebulae/black holes/animals/mines on maps.

## Campaign mission 1: first playable scaffold

`--mission 1` or Single Player → Start New Campaign loads the retail Mission01 world, Jim's **RLS Comet** torpedo boat,
all 11 named ship placements, original scenery/sky/music, and a mission-specific navigation/objective sequence. Red/blue
fleets follow exported routes; the first mock battle uses the retail 95-second timer and invulnerable training ships.
Docking/refitting and asteroid destruction have explicit Enter-to-continue pauses. The pirate encounter ends at a
“playable slice complete” message, **not** campaign victory: towing, rescue failure, lifeboats and ending are absent.
The opening in-engine briefing now plays Amelia's four retail clips/subtitles with portrait and letterboxing,
retail camera paths/focus changes, locked battle controls and Enter/Esc skip. Completion restores the HUD/player camera,
starts the navigation stage and switches to the retail handoff music. Native camera interpolation, speech pauses,
portrait-expression selection and intro fleet animation remain provisional. The pre-rendered campaign movie,
second mock engagement, later speech/NIS, nebula, scoring and saves are missing. The 135 explicit retail rules are
exported as evidence, not executed as a complete rule system. See [re/campaign.md](re/campaign.md) for confirmed data,
provisional sequencing and validation. Initial load and a scripted course through the first exercise were run;
the opening briefing and skip/handoff were also run. A sampled gameplay-video comparison is documented;
a full interactive playthrough is not yet verified.

## Targeting feel (user report, 2026-10)
- Target marker: four copies of `ShipTargeted_Icon_Enemy` (a corner pointer whose tip points up-right) stand on the diagonals around the target like a
  45-degree cross, each turned 90 degrees so the tips point at the ship (original: `ObjectIndicatorInfo` Target Icon 20x20 units, radius offset 0.8,
  z-plane offset -2; ours is screen-space with PROVISIONAL size/offset). The old single large chevron is gone.
- Aim point: a click on a ship stores the clicked spot in ship-local space (`Target.1`); the guns aim at that spot (moves with the ship), not the hull centre.
  Port/starboard sections are on their own sides (unit test), so shots placed on the left damage the left section; the exe-derived burst deviation still applies.

## Physical collisions

Ships now contact one another using the authored WOT collision boxes, and islands/bases use converted mesh triangles (not oversized spheres). Asteroids use their authored boxes or triangles. Ordinary asteroid contacts now use their finite authored mass and relative velocity; collision drift persists separately from scripted path motion, and collider geometry translates with contact movement. Armed asteroid scenery remains static. `game/src/collision.rs` substeps movement, separates contacts and applies mass-weighted impulses with retained lateral velocity. Hidden scenery and destroyed ships do not block; active docking ignores only the assigned dockee.

Geometry selection is retail-derived, but response is **PROVISIONAL**: inelastic impulses, planar contact and lateral damping, with provisional contact-point torque/box inertia and no recovered impact damage, collision sound or automatic avoidance. The course preview still predicts free motion. See [re/collision.md](re/collision.md) for evidence, tests and limits; mission routes need renewed playtesting.

## HUD factions
The in-game HUD uses the art set of the player's ship race (`Navy`, `Pirate`, `Procyon`; civilians use Navy): sprites, main-bar part sizes, status gauge offsets and
the radar disk come from the matching entry of `hud/hud.json`. `game/src/classic/panels.rs` builds the main bar as a bottom-docked, top-aligned flex row, the toggles, ship bar and Fleet Commands window, and the radar, speed dial and weapon panel as anchored child trees. Gauges and gun slots update within their owning panels (behaviour and values: shared `panels.rs`); world labels, reticles and navigation are in `world_hud.rs`.
`content/data/hud/bar.json` defines bar part order and optional decorative backdrops; missing faction sprites/backdrops are skipped. Mods can replace that file without changing Rust layout code. New part names still need a behavior handler to become functional controls.
`tools/trim_hud_rects.py` preserves original atlas rectangles and generates isolated `hud/tex/bar_<skin>_<sprite>.png` art for bar parts. This avoids neighbouring atlas pixels showing as vertical lines at fractional UI scales. Navy keeps its shared-edge rectangles; Pirate/Procyon drop trailing transparent columns. HD atlas overlays fall back to scaled atlas rectangles when their sheet scale is not 1; generated bar PNGs can also be replaced directly.
Panels rebuild after faction selection on ship/faction/height changes, are removed with the battle scenario, and rebuild when re-entering the same scenario. Speed-dial clicks and weapon toggle use UI hit testing and block world commands; briefing-hidden panels ignore input.
Front-end screens always use Navy. PROVISIONAL: speed-dial segment geometry still uses Navy coordinates, and exact faction placement, very narrow windows and remaining baked-in art edges need retail comparison. Panic fires every gun at its own nearest target in range (`docs/re/gunnery.md`; Space also triggers it, provisional); the bottom-bar PAUSE button and the non-retail Pause key pause the simulation (`P` now promotes the selected ship in skirmish) (virtual time, provisional: no retail restrictions while paused) and the speed label reads "Paused". New scenarios (restart, next mission or battle entry) resume simulation without changing the chosen speed; `--script pause|pause-restart` checks freeze/resume at runtime. Fleet selection, secondary ship bar and formation commands now use shared runtime state in both views; see [fleet-controls.md](re/fleet-controls.md) for implementation/tests and explicit provisional limits. Chat and movement-mode buttons remain reserved artwork. No retail-parity comparison is claimed for the fleet implementation.
