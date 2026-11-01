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

The timeline is data, in two parts. Each ship's mesh scene carries one clip `ANIMATION00` (2-20 s) that moves the section layers
rigidly (translation up to 17 units, rotation up to 8.5 rad; values are offsets from the rest pose). It starts 0.14 s after the
kill and plays once; the ship is removed 0.14 s + the clip length after the kill. Each damage section's WOT association also
carries `destructFx = { effect start end ( a b c ) }`, e.g. the Cutter's bow `Final_Ship_Chunk 2.9 3.0`, stern `1.9 2.0`, upper mast
`0.9 1.0`, mid hulls `none 0 2`. When the clip time passes a section's `start` the section is hidden and its effect bursts
(`start` is the clip's end for that piece minus 0.1 s). The three numbers are **where the burst appears**, a point in the section's
own frame read in the order first, third, second (game x, y, z), not a velocity. Sections with `none` and start 0 stay until `end`
(PROVISIONAL; the earlier frames show the mid hulls drifting away, and a hide at start 0 would remove them at once). A mast that broke off earlier in the fight keeps
tumbling away for 6+ s: see "Section debris" below.

Implementation (`combat/mod.rs`, `clips.rs`): death marks Health destroyed and reports ShipLost at once, plays the class explosion
and the death sound, then plays the clip on the section mesh nodes (normal and damaged variants), hides and bursts each section as
above, and removes the presentation at the end (the logical ship stays for mission references). The exporter writes
`sections[].destruct` = {fx, start, end, point} (point in game axes, already reordered). Dev kills (K/J, `--script victory|defeat`)
zero the core and go through the same path. Classes without a clip hold 1 s.

### Break-apart animation clips (mesh scene animations)
`data/animations.json` holds every clip (`tools/export_animations.py`). Values are offsets added to the node's rest position and rest
yaw/pitch/roll. No ship clip returns to its start (0 of 41). Other scenes: the animals' swim cycles are rotations of their bones
(whale up to 0.6 rad, manta 1.0, dragon 1.4 plus up to one unit of translation); base windmills (clip `ENVIRONMENT`, also some mine
and shipyard machinery) loop from spawn, one full yaw (-2 pi) over 10-16.7 s (`clips::play_loops`); base cranes (`CRANES`, or `CRANES01`-`04` for the dock point of that number) run 10 s from their first frame while a ship sits docked at a matching point and stop on their end pose (`docking::play_cranes`). PROVISIONAL: that docking starts them is the owner's reading (the clips are matched to dock points by name); the exact start/stop moments in the dock routine are not measured. The smoothing value of a key picks the curve
(see "Animal skeletons").

### Animal skeletons (space dragon, whale)
The animals are single skinned meshes. Their mesh file keeps, after each face list, one influence map per bone (a list of
vertex index and weight pairs) and, after the texture table, a bone table: each bone's name and the inverse of its bind pose
as a 4x4 matrix (dragon 32 bones, whale 38 including one `NULL` bone the scene does not name). The mesh scene holds the bone
hierarchy with a rest position and yaw/pitch/roll per bone; a node's rotation is a turn about up (yaw), then about starboard
(pitch), then about forward (roll), which reproduces the stored bind matrices exactly for most dragon bones. The scene's rest
pose differs from the bind pose on the dragon's tail tip and wings (the rest pose is posed). Each animal scene has one clip
(dragon 1.67 s, a wing beat with tail, neck and leg motion and a 0.7-unit bob of the body); its keys start at zero on every
channel, so they are offsets from the rest values: position = rest + offset, and the offset angles are added to the rest yaw,
pitch and roll before the rotation is rebuilt. A channel's curve is picked by its segment's first key: 0 linear, 1 Hermite with the
stored tangents, 2 (nearly every key) Kochanek-Bartels from the key values with its tension/continuity/bias (almost always 0; the
incoming tangent is only the chord, so two-key channels are straight lines). The dragon's clip always loops from a random start
phase at 0.2 x its speed, held between 0.8 and 1.3; the whale's loops at 1 from a random phase but only while it moves (frozen
otherwise). Remake: `tools/skin.py` bakes the skeleton, weights and clip (sampled at 30 per second) into the animal's GLB;
`environment::play_clips` / `clip_rates` play it. PROVISIONAL: weights are normalised per vertex (stored values are small and do
not sum to one; most vertices have a single bone), at most four kept; the whale's on/off value is read as its speed; the manta ray
is not exported.

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
- **Scene-wide particle budget** (`fx::SCENE_LIVE_BUDGET`, 15,000 live particles): above it every effect emits proportionally less. The worst single sources by authored rate are a Gattling tracer (about 600 live particles per bullet), docking points (2,400 each) and the gravity-charge impact (capped per primitive at 4,000). Not retail; PROVISIONAL size.
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
Terrain_BlackHole_* WOTs name a render mesh, a radius, `Magnitude` (the pull, below), an ambient sound and a
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
* Pull: the WOT CUSTOMINFO `Magnitude` is 9000 / 15000 / 25000 for Small / Med / Big (radius R = 145 / 260 / 625). Measured
  on the Small hole (Zemyatin Anomaly skirmishes, 2026-10; Navy Cutter twice, Frigate, Man-O-War; ship positions and speeds
  logged every engine tick of ~0.033 s):
  - **Law.** The hole accelerates a ship by a(d) = A × min((R − d) / D, 1) at distance d from the centre, for R/4 < d ≤ R;
    nothing outside the radius (a Cutter at 2.6 radii and a Man-O-War at 1.4 radii for 6 minutes did not move) and nothing
    inside R/4. The ramp D fits 36-38 units, about R/4 (the remake uses D = R/4). The pull points at the centre turned about
    15° to one side (clockwise seen from above): sideways / inward = 0.27 in every log, whatever the ship's heading.
  - **Only ships that are not under way.** The pull acts only while the ship's own speed is zero (All Stop, or idle). A
    Man-O-War driven from 48 to 17 units moved exactly at its own speed throughout and was still afloat 17 s after passing
    R/4. A ship crossing the radius under way feels nothing.
  - **Resting ships creep.** At rest the engine snaps the velocity back to zero every tick, so the pull is cancelled each
    tick and the ship only creeps in at one tick's worth of it, about a × 0.033 units/s: Cutter 0.091 units/s at 23 units
    inside the edge, Frigate 0.040 and Man-O-War 0.033 at full depth (the Man-O-War the same at 105 and 48 units).
  - **Breaking free.** A ship that breaks free of the snap falls freely: its velocity follows v' = a − k v with the hull's
    own rate k = MaxThrust / (MaxSpeed × Mass) (Cutter 1.862/s), so it settles at A / k (Cutter 2.42 units/s, measured
    within 0.05 units over a 37 s fall). Inside R/4 the pull stops and the speed decays at the same rate. What triggers the
    break is not known (PROVISIONAL): both Cutter runs broke free ~21 s after stopping 122.5 units out, at 120.5 units where
    a ≈ 2.9-3.0 units/s²; a Frigate (a ≤ 1.26) resting 45 s and a Man-O-War (a ≤ 1.04) resting 200 s never did. Both Cutter
    runs started at the same spot, so time spent inside and depth cannot be told apart; the remake breaks free at
    a ≥ 2.9 units/s².
  - **Class.** Full-depth pull A: Cutter 4.50, Frigate 1.21-1.26, Man-O-War 0.99-1.04 units/s². A × Mass divided by the
    hull length (collision box MaxExtents.y − MinExtents.y: 13.5, 18, 31) gives 1333, 1344-1400 and 1290-1342, so
    A ≈ 1330 × length / Mass fits all three within ~4 % (1/√Mass does not). PROVISIONAL: three classes only. The scaling
    with Magnitude (A × Magnitude / 9000) is PROVISIONAL too: only the Small hole was measured.
  - **Kill.** A ship that is not under way is destroyed about 1.1 s after it comes inside R/4 (Cutter at 35.3 units, Frigate
    at 32.4; the pull ends at 36.5 ± 0.5). The hull then breaks up where it is (the distances logged earlier, 33.6 and ~21
    units, were where the wreck stopped). Not measured (PROVISIONAL): a ship that stops inside R/4 after an order; the remake
    starts its 1.1 s once it is at rest there.
  - Destruction by the hole credits no kill: the Game Summary showed the computer with 0 Ships Sunk, and our Damage Taken only
    counted the earlier gunfire.
  Remake (`blackhole.rs`): exactly the law above for every free, undocked ship, with A = 1330 × length / Mass × Magnitude /
  9000, D = R/4, the break-free threshold 2.9 units/s² and the 1.1 s kill inside R/4, credited to nobody in the Game Summary.
  The vortex's KILL_RADIUS only ends its ribbon points. Computer captains keep out of the radius (movement.md).
* Unknown: strip UV mapping/blend (ours: additive, v = 0.5 per segment), the funnel mesh
  (not drawn: the reference video shows only the glow, beams and ribbons).
