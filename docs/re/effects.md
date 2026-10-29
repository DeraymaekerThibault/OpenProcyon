# Effects runtime: what the original does

Source: observed behaviour of the original game and its retail effect files (EFT), described in our own words.

Class names (namespace `Effects`): `GenericParticleSystem`, `ParticleSystem`, `Particle`, `Emitter`, `AngleEmitter`,
`ParticleSystemSourceEmitter`, movement infos (`Attractor`, `AttractorLine`, `Friction`, `Gravity`, `Expiration`, `HitGround`,
`Jitter`, `ParticleSystemSourceAttractor`) and render infos (`ColorFader`, `ParticleSize`, `PseudoBillBoard`, `SimpleTexture`, ...).

## Time
* Engine tick constant = **0.033 s (30 Hz)**. Per tick every particle does `pos += vel`, where `vel` is stored
  per tick. A new particle's velocity (units/second in the files) is multiplied by 0.033 as the last step of
  its initialisation.
* Movement infos run only when more than **0.1 s** has passed since their last run, with the elapsed
  time as `dt`.

Remake presentation: particles still simulate at 30 Hz, but position and visual-curve age are sampled forward by the unused
tick remainder when rendered. This removes visible 30 Hz stepping without changing the emission rate or movement-info cadence.
FX simulation/rendering runs after Bevy transform propagation, so attached effects use this frame's ship, gun, shell and camera
transforms. Catch-up ticks interpolate the emitter transform over the frame's movement instead of emitting all particles at
one endpoint. These are remake smoothing choices, not a claim about retail render interpolation.
Only primitives referenced by source emitters have their particle positions copied for spawning; ordinary engines/projectile
trails no longer copy all particle positions every tick. The initial 60-second aggregate growth test was too permissive:
allowing almost twice the particles missed linear growth from the docking marker. It is now tightened to 10% growth, with
a separate five-minute docking-marker bound and start-only/explicit-expiration regressions (see the profiling findings below).
Runtime smoke: `--script fxtest --shot-frame 180 --late-frame 0`, screenshot `local/mission1-effects-polish.png`.

## Mission 1 particle-growth profiling

A full runtime profile reproduced the reported slowdown: entity/mesh counts stayed roughly stable, but particle counts grew
with mission time. `Docking_Point` primitive 8 continuously emits a non-fading flare without an `ExpirationChunk`. Its rate
settles at four particles per tick. The remake's fallback gave each flare an infinite lifetime, so the two tender markers
added about 240 permanent particles per simulated second, even when out of sight (`LowDetailParticleRateMultiply = 1`).

The fallback now uses the primitive's authored `Duration` (20 s for that flare) only when it emits continuously and has neither
explicit expiry nor time-varying visual curves. Explicit expiration remains authoritative; start-only permanent particles,
including the invisible damage-smoke source pools, remain permanent. No global particle cap, engine brightness adjustment or
reduction of authored emission rates was introduced. **PROVISIONAL:** the original no-expiration lifetime is not fully known;
this is a bounded, data-derived fallback, not a claim of exact retail lifetime semantics. Retail initialization
clears the dead flag and timestamps a particle; explicit expiration marks it dead. Neither establishes the missing-expiry default.

Same-machine GPU runs (AMD Radeon 780M, vsync, Mission 1 autopilot, `--speed 16`, camera distance 60/pitch 0.5) near mission time
1075 s / the pirate-ending scene:

| Measurement | Before | After |
|---|---:|---:|
| Live particles | ~261,000 | ~8,000 |
| Rolling real frame time | ~45 ms (~22 FPS) | ~16.7 ms (~60 FPS, vsync ceiling) |
| FX mesh-building CPU time, 300-frame average | ~25 ms | ~0.42 ms |

Collision stayed about 1.1 ms and course rendering was small; they were not changed. CPU timers were temporary instrumentation
and removed after comparison. Logs: `local/profile-before.log`, `local/profile-after.log`. The baseline hit the five-minute
wall-clock timeout near the ending; the corrected run completed and wrote `local/profile-after-end.png`. Both used
`PROCYON_AUTOKILL=1`, so this validates sustained runtime load and ending presentation, not normal pirate-fight combat.
The new marker regression failed before the fix (4,472 particles at 30 s) and passes across five simulated minutes afterward.

`PROCYON_PROFILE=1` enables optional five-real-second logs of frame time, mission/objective, entity/ship/rock/effect/particle,
mesh-asset and audio-player counts; it also logs mission-end state changes. It is disabled by default and has no system timers.
For personal profiling runs, use a scratch `XDG_DATA_HOME` if campaign points must not touch the player's save.

## Comprehensive feedback follow-up: meshes and blending

`tools/export_effects.py` now converts both endpoints of authored `UseMorph` mesh primitives through the existing MDB→GLB exporter.
Six `RepairMesh_{Small,Medium,Large}_{Down,up}_Layer1` GLBs serve the five available solar/ironclad repair effects. The shared FX
renderer interpolates matching vertex topology using `MeshWeightInterpolator`, retains UVs/texture, and evaluates the EFT colour
and deploy/hold/retract timing. Ironclad weights intentionally peak below 1. Each mesh is a child of its existing FX root, so
lifetime and scenario cleanup also release its mesh/material handles. Other unsupported mesh effects are not newly implemented.
The shared converted-mesh reader moved to `data.rs`, with checked buffer/chunk bounds and finite/index validation.

Bevy's `AlphaMode::Add` multiplies RGB by texture/vertex alpha in `pbr_functions.wesl::premultiply_alpha`. The EFT additive mode is
ONE/ONE; the new shared material keeps unpremultiplied RGB and specializes the actual blend state to ONE/ONE. Nebula edge fade now
modulates RGB instead of relying on alpha; its authored tint is unchanged. Every ONE/ONE primitive uses that material, flares
included. The converted `flare` (and `cannonballpseudo00`) textures have an all-zero alpha channel; ONE/ONE ignores it, but
Bevy's premultiplying `AlphaMode::Add` drew them black, which hid the dock lights, repair sparkles, SOS beacon flare and debris
flares (PT-11). With the bounded flare lifetime above, the Docking_Point markers no longer overbrighten.

Wakes share the corrected material, bake their existing age/speed fade into RGB, drop the guessed 0.7 brightness multiplier, and
render a live stern endpoint between distance-sampled points to avoid trailing-end jumps. Width/lifetime remain provisional.
GPU smoke reaches the docked repair phase and shows scaffolding (`local/feedback-scaffold.png`); the oblique torpedo arena and
Mission 1 training-fleet impacts exercise guidance/attached effects. Geometry, morph curves, malformed GLBs and export endpoint
regressions are included in `tools/check.sh` (98 Rust tests, 61 Python tests, six Python skips), including a live wake-head ECS test.

## Spawning (`GenericParticleSystem` update)
* Each tick: `count = trunc(rate(t) * detailFactor)`, where `t = now - start - delay` and `detailFactor = 1` at full detail
  (`D - D*L + L`, `L` = LowDetailParticleRateMultiply).
* So **the rate is particles per tick, not per second** (x30 per second). If the rate is below 1, one particle
  spawns with probability = rate (LCG random in [0,1) < p).
* `NumberParticlesAtStart` is added on the first tick.
* **PROVISIONAL particle budget (ours, not retail):** a primitive whose steady live count (peak rate x 30 x lifetime) would
  exceed 4000 emits proportionally fewer particles. Only `GravCharge_Medium_Impact` trips it (180 per tick for 18 s, about
  97000 additive particles, more than 30x any other effect): it rendered as a saturated blob and stalled the frame. The
  original's pool limit, if any, is unknown.

## Distance detail (per effect, spawn count)
* Each effect gets detail D = (1 - distance to camera / EffectsLowestDetailDistance) clamped to 0..1, times EffectsMaximumDetailPercentage
  (profile `.pft`, retail defaults 350 and 1.0; `EffectsAlwaysUseFullDetail` False). Spawn count = rate x (D - D*L + L), L = the primitive's
  `LowDetailParticleRateMultiply` (0.1 for the damage smoke). Far effects therefore emit down to 10%; the remake used full rate before.
  Not modelled: an off-screen test in the same routine that forces D = 0.
* Rectangle2D render info: per particle it stores four floats evaluated at the particle's age (default -1, 1, 1, -1) and exposes them
  through a getter (default infos return false). The consumer that builds the quad was not found, so the remake still
  multiplies them by the size curve. The Damaged Section smoke texture is 32x128, matching its tall rectangle (about 1:4), so the tall plume itself is probably right.

## Particle initialisation
Position and velocity are sums of: position emitter, angle emitter, **source emitter** (adds only a *position*
taken from a particle of the source primitive; velocity untouched), optional world transform, then velocity x 0.033.

### Mission 2 SOS bottle-rocket beacon

Owner reached the first SOS message but could not see its beacon. All three textures (`flare`, `ring`,
`testparticletexturebw`) were present. The effect starts a fixed pool of nine particles, with zero continuous emission,
no explicit expiration and size/colour/rectangle curves repeating over 0–2.5 seconds. `tools/eft.py` discarded `LoopInfo`;
the runtime then expired the initial particles at the end of the first visual curve (2.5 seconds).

The exporter now retains active curve loops as `loop: {StartLoopingT, EndLoopingT, NumberLoops}`. The shared curve sampler
repeats that interval and resumes the curve after finite repetitions; the exact terminal count convention is provisional.
Start-only visual loops keep their fixed particles until primitive/event release, while explicit expiration and bounded
continuously emitted docking particles remain unchanged. Only `BottleRocket_Light_Impact.json` was regenerated, and stripping
its new loop metadata reproduced the previous committed data exactly; no installation/mod tuning was imported.

Gate: 191 Rust / 75 Python tests, seven skips. Tests cover active/disabled export loops, interval boundaries, finite-loop
completion, 60 seconds with exactly nine beacon particles and existing texture files, and both Mission 2's original
pickup rules (enter the volume, release effect, pickup sound and story flags, run once). Classic isolated smoke
`local/m2-sos-visible.png` shows the beacon near the player; startup NIS was removed only in the ignored scratch mod.
Pickup is automatic on sailing into its volume, not click-targeting. Full owner-path rescue/tow continuation remains unvalidated.
Other converted effects have not been regenerated to restore their previously discarded loops.

### Mission 2 meteor-shower direction correction

Owner playtesting found the shower travelling horizontally instead of falling like rain. `MeteorStorm` combines a near-horizontal
position cone (88–89 degrees around game +Z) with a target-emitter position above the storm and velocity along game -Z.
The runtime incorrectly replaced both target contributions with the position cone and used that cone for velocity.
`fx.rs::spawn_particle` now adds the enabled target/angle contributions separately and honours each emitter's
use-position-for-velocity flag; otherwise the angle velocity uses its own authored cone. No asset values were changed.
Regressions sample all four MeteorStorm primitives: the two streak layers spawn above the emitter and fall vertically,
while the splash layers do not inherit cone velocity. A muzzle regression checks reused position versus independent angle velocity.
`tools/check.sh` passed (182 Rust / 72 Python, six skips). Classic runtime smoke relocated Mission 2's shower to the player
on Red Rover with an ignored scratch mod (`local/meteor-smoke-mod`); `local/meteor-vertical.png` shows vertical streaks.
This checks the effect, not Mission 2's full gameplay or other affected muzzle/repair effects against retail.

Follow-up: play-testing showed that the splash rings still faced the camera instead of lying flat. MeteorStorm's ring primitive
already contains a `Flat` render chunk, which the runtime ignored. The parser now retains it and the renderer uses the effect's
game XY plane (Bevy XZ) rather than camera axes; streak and spark primitives retain their authored modes. A regression checks
horizontal ring geometry across camera pitches. The full gate passed (183 Rust / 72 Python, six skips), and Classic smoke
`local/meteor-flat.png` shows flat splash rings. Other effects containing `Flat` also use this mode; their retail orientation
has not been independently compared.

Second follow-up: play-testing showed rain passing below the splash/ship plane. Independent authored spawn heights, randomized fall
speeds and 1.5-second lifetimes left the two streak layers ending at widely different heights. `MeteorStorm` now starts each
falling particle at `surface - vertical_velocity * lifetime`; splash particles start exactly at the storm's flight plane.
Horizontal spread, velocity, lifetime, flat orientation and fade curves stay authored. Drawing skips particles already expired
in the interpolated frame, preventing a last frame below the endpoint. This is provisional alignment from owner feedback, not
recovered retail meteor behavior; rings remain independently emitted, not one ring per falling particle.
Regression samples all four primitives at two surface heights and proves streaks reach the plane at expiry and splashes start
there. Gate passed (188 Rust / 74 Python, seven skips); Classic isolated smoke `local/meteor-flight-plane.png` shows the shower
above the ship's plane. Other effects retain their existing emitters and positions.

## Attractor (movement info slot 4)
`vel += (target - pos) * k * dt * 0.033` (in per-tick units; i.e. per second `vel += (target-pos)*k*dt`),
with `k = Const + WithDist/d + WithDistSquared/d^2`, `d = |target - pos|`. The chunk's
`PositionInterpolator3Chunk` is the **target point**, not an acceleration.

## Gravity / friction
Gravity: `vel += g * dt * 0.033`. Friction: scales velocity by a factor, zero once speed falls below a threshold.

## Pseudo-billboard
Two `SimpleTexture` entries: side and front (see "Visual refinements" below for how they are blended). Flag clear: a fixed axis from three curves (default (0,1,0) if all zero). Flag set: the axis is the particle's velocity
direction (`vel / 0.033`), (0,1,0) if the velocity is zero.

## Still open
Expiration/fade interplay (what keeps a particle without an Expiration chunk alive), the `0x24`-byte batch object
the system creates every 50 ticks, `HitGround`, `Jitter`, `AttractorLine`, and the world-space frame of attractor targets.

## Emitters
* Position/velocity emitter: `vec = normalize(Target - Radius * u) * (Length + e)`, `u` a uniform random point on the
  unit sphere, `e` uniform between 0 and `LengthError` (one-sided; negative errors go below). With Target = 0 the
  direction is uniformly random; with a Target the particles move toward it. (First read as `rand - Target`;
  wrong sign, found from the footage: engine exhaust went forward. The engine mount faces the stern and its
  velocity target is +Y, the shell-smoke position target is behind the shell.)
* Angle emitter: direction in a cone around the axis: polar angle uniform in [min, max], azimuth uniform in
  [0, 2pi]; magnitude `Length + e` as above.
* Random numbers: LCG `x = x * 0x19660d f35f`, float = mantissa bits | 1.0 - 1.0.

## Consequence for movement
The same engine tick (1/30 s) is probably the ship-physics step too, which may fix the provisional x0.1 thrust time
scale in `docs/re/movement.md` (not yet checked).

## Damage mesh swap (setup, on destruction)
A swappable damage section whose layer mesh is "<A>_<B>" gets the swap mesh "<A>Damage_<B>" (split at the first '_').
When the section is destroyed the layer's mesh is exchanged for it. Extra Damage_ layers beyond the section count (the
cutter has Damage_Layer7-10) are not referenced by this rule. Destruction also spawns the section's debrisMeshScene
(not done in the remake).

## Ship sinking and final destruction

Retail (headless run, our Navy Cutter sunk by a computer Sloop, frames every ~0.18 s): there is no sinking dive. The hull blows
apart where it is: the class `Explosion Effect Name` (fireballs, white flash, smoke) plays at once, the sections fly off as separate
pieces (planks, the stern with its wings, the masts), each piece bursts in its own fireball within the next ~3 s, then only smoke
remains. The ship's physics object does not move at all through this; it is purely presentation.

The timeline is data: every damage section's WOT association carries `destructFx = { effect start end ( vx vy vz ) }`, e.g. the
Cutter's bow `Final_Ship_Chunk 2.9 3.0 ( 8.8 4.1 11.1 )`, stern `Final_Ship_Chunk_large 1.9 2.0`, upper mast `0.9 1.0`, mid hulls
`none 0 2 ( ±4 ... )`. Reading (matches the frames): the section's mesh flies at the velocity (ship axes, units/s) from the moment of
death and bursts with the effect at a time inside [start, end]; `none` sections just drift and are gone at `end`. The stern piece
vanished ~2 s in, as its 1.9-2.0 window says. A mast that broke off earlier in the fight (section destroyed before death) keeps tumbling away for
6+ s: see "Section debris" below.

Implementation (`combat/mod.rs`): death marks Health destroyed and reports ShipLost at once, plays the class explosion and the
death sound, then moves each section's mesh nodes (normal and damaged variants) along its `destruct.vel`, bursts its effect at a
random moment of its window and hides it; after the last burst the presentation is removed (the logical ship stays for mission
references). The exporter writes `sections[].destruct` = {fx, start, end, vel}. Dev kills (K/J, `--script victory|defeat`) zero the
core and go through the same path. PROVISIONAL: no tumbling of the pieces, linear flight (no gravity or drag), the burst moment
picked uniformly, sections without data hold 1 s.

### Break-apart animation clips (mesh scene animations)
Every ship's mesh scene carries one clip (`ANIMATION00`, 2-20 s; exported in `data/animations.json`). Decoded on the Navy Cutter (3.0 s,
eight motions, two linear keys each): it is the one-shot break-apart motion, not an idle loop. No ship clip returns to its start (0 of
41), translations reach 3.8-16.8 units and rotations 0.7-8.5 rad. The end times per piece match the `destructFx` windows above: bow 3.0 s,
stern layers 2.0 s, masts 1.0 s, and layer 6 at 2.5 s. The displacements do not match the WOT velocities (bow clip totals 5.9 / 6.6 /
7.4 units against velocities 8.8 / 4.1 / 11.1 per second), so whether retail uses the clip, the velocities or both is open. The clip
supplies the rotation (the tumble) that the implementation above lacks. Open: whether values are offsets from the rest pose or absolute
poses (layer values start at 0, which suggests offsets, but the constants on helper nodes look absolute); the meaning of the keyframe
smoothing value (0, 1 or 2; 2 on almost all keys; tangents are per segment, linear for two-key segments).
Other scenes: the animals' swim cycles are rotations of their bones (whale up to 0.6 rad, manta 1.0, dragon 1.4 plus up to one unit of
translation); base windmills turn one full yaw (-2 pi) over 10-16.7 s and loop; base cranes run 10 s and mostly return to start; one base
(the iron-clad mines) has a 43 s clip with up to 67 keys.

### Section debris
A section destroyed mid-fight swaps to its damaged mesh and, when its WOT association names a `debrisMeshScene`, drops that piece
(a torn-off mast, the bow, the engine block): the scene holds one `...Damage_LayerN` mesh in the hull's own model space, its node
position being the piece's pivot; `debrisFx` (Falling_Debris: three everlasting smoke systems) trails it. Retail frames show a
torn-off Cutter mast still drifting and turning 6+ s later. Remake (`combat::damage::spawn_debris`, `debris`): the piece starts in
place, keeps the ship's velocity at the moment it broke, sinks at 1.5 units/s, tumbles at 0.25-0.75 rad/s about a random
horizontal axis through its pivot, and is removed after 10 s (all PROVISIONAL: no retail numbers). The exporter writes the meshes
to `models/debris/` and `sections[].debris` = {model, pivot}.

### Visual refinements that depart from retail (2026-10, requested by the owner)
- **Damage and debris smoke is thinned** to 15% of the authored rate (`fx::SMOKE_THINNING`, effects `Damaged Section_*` and `Falling_Debris`). Retail's additive
  smoke emits one particle per tick per system, each living ten seconds, several systems per section; here that saturates to a white blob on a damaged ship and
  is the main particle load in a long fight.
- **A debris piece's smoke trail stops after 3 s** (`DEBRIS_SMOKE_TIME`); smoke already out fades on its own.
- **Shell sprites use both of their textures.** A pseudo-billboard effect lists a side texture and a front texture (cannonballs, lancer shots, the Gattling
  tracer, meteors). Retail shows the side streak when seen from the side and a round glow when seen along the shell's flight line, which is what gives
  its shells a 3D feel; we drew only the side texture. The streak and the camera-facing front quad are now drawn together, weighted by cos^2 of the angle between
  the view and the axis (PROVISIONAL blend). A cannonball is a glowing fireball with a flame tail, not an iron ball.
  To look at every shell type at once: `tools/shot.sh local/x.png --script shells --shot-frame 45 --dist 80 --pitch 89` (also `--yaw`; the log lists the order,
  two rows of up to twelve, left to right).

## Fire (ignite, update, fire-fighting)
- Ignite on a hit: if the bullet's Chance of Fire for the section material is > 0 and a uniform roll is <= it (and an
  extra check passes, unknown), a fire starts on the section with the bullet's Initial Fire Strength.
- Every 6 s: the ship's fire-fighting capacity is spent on the strongest burning fires first, subtracting from their
  intensity; a fire at 0 goes out, and what is left of the capacity moves on to the next strongest.
- Fire-fighting capacity = the sum, over the eight crew posts, of every member's Leadership (rating / 10). Members outside
  the captain and first-mate posts get the same captain/first-mate leadership bonuses as damage control (+0.1 / +0.05
  above a per-ship threshold), each member clamped to 0..1. A gunner, engineer or rigger post with nobody in it counts
  the first-mate post's members instead; other empty posts add nothing. A default Cutter (12 hands rated 1) gets 1.2. Each burning fire then damages its section by its
  intensity (a damage potential with lower = upper = intensity, no criticals). Fires on a destroyed section die.
  A dead fire stops emitting after 3 s and is removed after 5 more. Burning fires then double in intensity.
- Effect FireOnShip, its rate/length multipliers = intensity / 20 clamped to [0.1, 1].
- Measured (retail saves of our default-crewed Navy Cutter under fire, Easy, 2026-10): fires start at powers of two (1-64
  seen) and untouched ones double exactly every tick (4 -> 64 in four ticks); the strongest fire grew 11.2 -> 273.8 over five
  ticks, which fits 2 x (intensity - c) with c = 1.36 per tick (another fire lost 1.1 in one tick), against 1.2 from the
  crew rule above. Fires kill: that Cutter burnt to 0 HP on every section. A section can carry several fires at once
  (five on one section in one save).
- Remake (`combat::damage::fire_fighting`): the crew rule above, with the same crew slots as damage control (computer
  crews 0.75 per slot, the player's empty slots generic hands at 0.1). PROVISIONAL: the leadership bonuses are not
  modelled (thresholds unknown, as for damage control); at most one fire per section (retail allows several).

## Black holes (Vortex primitive, spawn, point update; Trail)
Terrain_BlackHole_* WOTs name a render mesh, a radius, `Magnitude` (pull, not implemented), an ambient sound and a
`VortexEffectName` (BlackHoleRibbons_*). That effect = six ordinary particle systems (the vertical beams, glow, black
core: run by fx.rs) plus a `Vortex` primitive, implemented in `game/src/blackhole.rs`:
* Per 30 Hz tick a point spawns with probability SPAWN_PROBABILITY (detail clamp = 1) at a random angle, START_RADIUS from
  the centre, CENTER_Z_OFFSET below it. Each point owns a Wake_Hole Trail effect.
* Point tick: angle step += ANGLE_DELTA_CHANGE; radius -= RADIUS_DELTA; angle += step; xy = centre + r(cos, sin);
  dropping speed += Z_DELTA_DELTA_ADD x 0.001 (the load scales it) and the point sinks by the running sum, so z falls
  cubically (~50 units in a Small hole's life). Below KILL_RADIUS the point dies and its trail vanishes at once.
* Trail frame: forward = back toward the previous position; up = (toward centre xy, f x HIGHT_POINT + UP_CHANGE) with
  f = 1 - min(depth below centre, UP_CHANGE)/UP_CHANGE; right = forward x up.
* Trail segment every 15 ticks, life 300 ticks. Edges start at pos +- right*OutDrift*InitialWidth, then each tick move
  +-right*OutDrift and both +forward*BackDrift, plus per-edge random jitter (+-0.07, +-0.07, +-0.02). Alpha rises 1/60 per
  tick for 60 ticks, then falls 1/240 per tick. A trail is drawn only with more than two segments.
* Pull: the WOT CUSTOMINFO `Magnitude` is 9000 / 15000 / 25000 for Small / Med / Big (radius 145 / 260 / 625). The original's pull law is
  unknown; the remake applies PROVISIONAL inverse-square pull on free ships, softened at the radius, gain 25 (`blackhole.rs`).
  Measured on the Small hole (Zemyatin Anomaly skirmishes, ships at All Stop, 2026-10):
  - Outside the radius nothing moves: a Cutter at 372 units (2.6 radii) for 22 s and a Man-O-War at 200-203 units (1.4 radii)
    for 6 minutes had zero displacement. The remake's law gives 1.6 and 5.6 units/s² there.
  - Inside the radius a resting ship creeps toward the centre at a constant speed, independent of distance: Man-O-War 0.033
    units/s at both 105 and 48 units, Cutter 0.093 units/s at 122 units. The motion is mostly radial with a sideways part of
    about a quarter (a ~15° inward spiral). The ship's own velocity stays 0 (it keeps its rest snap), so the hole moves the
    position directly rather than accelerating the hull.
  - A Cutter that rested ~21 s inside (at ~121 units) then moved straight in at a constant 2.32 units/s with no order given
    (twice, identical), and was destroyed 33.6 units from the centre. A Man-O-War resting 200 s at 105 units never did this;
    driven to the centre it was destroyed somewhere inside 17 units.
  - Destruction by the hole credits no kill: the Game Summary showed the computer with 0 Ships Sunk, and our Damage Taken only
    counted the earlier gunfire (2139 of the Man-O-War's 4940 hull and core HP).
  - Navy Frigate (mass 20000, half the Man-O-War's; Easy, 2026-10-17): rest drift 0.0046 units/s at 140.8 units (0.97 radii),
    0.0342 at 113.5 (0.78), 0.0396 at 53.9 (0.37), sideways part 0.27 of the inward one. Near the edge the drift grows with the
    depth inside the radius (0.0046 × 31.5 / 4.2 = 0.0345, as measured at 113.5), deeper it levels off: the Frigate creeps about
    as fast as the Man-O-War, not the √2 faster a 1/√mass law gives.
  - **The pull is an acceleration, hidden by the rest snap.** A save taken while the Frigate "rested" holds a physics velocity of
    1.26 units/s, mostly tangential (the logged speed field, which the earlier notes read as "velocity 0", is a different
    value). Once the ship moved faster than the snap's 1 unit/s it fell freely: from 67 units it spiralled in at up to 5 units/s
    (inward ≈ 1.1-1.4 units/s² between 66 and 40 units after the hull's own drag rate 0.214/s is added back, tangential ≈ 0.3),
    turning 75° around the centre, slowing again inside ~35 units, and was destroyed at about 21 units. Reading (PROVISIONAL):
    at All Stop each tick's pull is mostly cancelled by the snap, so the creep is roughly one tick's worth of acceleration
    (1.2 units/s² × 0.034 s ≈ 0.04 units/s), and anything that lifts the speed above 1 unit/s (a ram, a push, the hole's own
    swirl) starts a free fall: this is likely the Cutter's "plunge".
  - Destroy distance: Cutter 33.6, Frigate ~21, Man-O-War < 17 units (larger hull → closer?). PROVISIONAL.
  So the hole is a trap at its centre, not a long-range pull. Unknown (PROVISIONAL): the drift's dependence on class (mass,
  thrust or size), what starts the cutter's plunge (time inside, distance, class), the destroy radius per class, and whether a
  moving ship feels any extra pull (a Man-O-War crossing the radius at Emergency showed none clearly).
  Remake (`blackhole.rs`): no pull outside the radius; inside, every free ship's position moves toward the centre at
  6.2 × (Magnitude / 9000) / √Mass units/s turned 14° clockwise (fits both measured classes within 6 %; the mass law and the
  Magnitude scaling are PROVISIONAL), and a ship closer than the vortex's KILL_RADIUS (10 / 100 / 290 for Small / Med / Big;
  PROVISIONAL as the destroy distance) loses its core and sinks, credited to nobody in the Game Summary. The Cutter's plunge is
  not modelled.
* Unknown: strip UV mapping/blend (ours: additive, v = 0.5 per segment), the funnel mesh
  (not drawn: the reference video shows only the glow, beams and ribbons).
