# Gunnery: aim lead and accuracy (original findings)

Source: observed behaviour of the original game and its retail data files, described in our own words.

## Units
WOT angles (accuracy deviations, rotation speeds, vertical limits, lob angle) are radians. The gun entity's
property-bag reader/writer converts deg<->rad, which is the editor view, not the file.

## Aim point with lead
Each update the ship's targeting block gets an aim point (normally the target's chosen point). Flight time is
the straight distance from the gun to that point divided by the gun's MuzzleSpeed (one step, no gravity term);
the aim point is moved by target velocity * flight time. For some targets (flag bit 8 on the target, probably
crewed/player ships) the lead is only applied while the target is turning and adds an extra physics vector; not modelled.

## Deviation (run once per burst, or per shot if CalculateAccuracyDeviationDuringBurst)
- Band by distance: <= EffectiveRange -> Effective deviation, <= LongRange -> Long, else Maximum.
- Half of it, times an accuracy factor: 0.5 for plain AI ships (0.8 or a per-ship value for some ship flags);
  crewed ships use 1 - 0.8 * gunnery skill (+0.1 / +0.05 random bonuses, clamped to [0,1]); NIS mode uses 0.1.
- The final angle is uniform in [-a, +a].

## Firing
Per muzzle the angle is applied twice to the muzzle orientation, about two axes, each with a random sign
(so yaw and pitch error have the same magnitude). Speed is MuzzleSpeed.

## In the remake (`combat/`)
Lead and deviation follow the above. PROVISIONAL: factor 0.5 for every ship (no crews); the two rotation axes
taken as world up and the muzzle's right. No shell inherits the shooter's velocity (measured, see below), so the lead uses
the target's velocity alone.
Measured in an earlier scripted skirmish: hit rate 4% before, 60% after (not remeasured after the changes below).

## Shell gravity
Measured on the running original game (2026-10): two skirmishes on The Diablo Strait (a Navy Man-O-War and Cutter against
pirate fleets), every shell within 250 units of the Man-O-War sampled about 30 times a second, each track fitted as a
ballistic arc and attributed to the ship it started from.

- **Each gun has its own shell gravity: g = MuzzleSpeed² / (√2 · MaximumRange).** Put differently, a shell fired at 45° would
  carry √2 times the gun's maximum range. It holds for every gun seen (median of repeated shells):

| Gun | MuzzleSpeed / MaximumRange | Predicted g | Measured g | Shells |
|---|---|---|---|---|
| Star mortar | 25 / 350 | 1.263 | 1.263 | 2 |
| Heavy cannon | 40 / 350 | 3.232 | 3.231 | 14 |
| Medium cannon | 40 / 270 | 4.190 | 4.184 | 25 |
| Medium carronade | 30 / 130 | 4.895 | 4.908 | 12 |
| Light cannon | 40 / 180 | 6.285 | 6.269 | 51 |
| Light carronade | 30 / 100 | 6.364 | 6.355 | 15 |
| Gattling | 70 / 120 | 28.87 | 28.5 | 547 |

- Guns not flagged BulletAffectedByGravity (lancers) fly straight. Gravity charge and nova mortar shells follow the same
  formula in the remake but were not measured (PROVISIONAL).
- **No velocity inheritance:** the launch speed equals MuzzleSpeed in the world frame (40.0-40.4 for cannons fired from ships
  moving 7-11 units/s, 70.2 for gattlings, 30.3-30.5 for carronades). No drag: horizontal speed stays constant.
- Shells that hit nothing keep falling below the flight plane until their lifetime ends (only lob shells burst there).

## Burst timing
- TimeBetweenBursts is in seconds and runs from the **last** shot of a burst: a medium cannon fired 8.96-8.99 s apart
  (TimeBetweenBursts 9), a light carronade 4.6-5.0 s (5); gattling bursts (8 shots, DuringBurstReloadTime 0.15) began every
  3.98-4.17 s = 3 + 7 × 0.15 s plus aiming. So one gun's burst-to-burst cycle is TimeBetweenBursts + (BulletsPerBurst - 1) ×
  DuringBurstReloadTime.

## Gunners
- **Only torpedo launchers** (the gun flag `Is TorpedoLauncher Gun`; in retail data only `Gun_Torpedo`, TimeBetweenBursts 35) have their wait
  shortened by crew (decoded from the original's logic): wait = max(0, TimeBetweenBursts - 10 x the raw Gunnery (0..1) of the gunner at the
  gun's bank), with no leadership bonus, no difficulty term and no human/computer distinction (a computer torpedo ship uses its own generic
  crew's gunnery; PROVISIONAL, remake: computer ships fire at the authored time). Every other gun (cannons, carronades, Gattlings, lancers,
  mortars, mines, point defence) reloads at its authored TimeBetweenBursts whoever mans it. The in-burst spacing never changes. The wait is
  compared against the current skill at every readiness check, so a crew change takes effect at once (remake: latched when the burst starts).
- Earlier measurement (Navy Man-O-War, nine gunners of 7-9, one gun's hits every 3.5-3.9 s) was read as a cannon crew effect; it more likely
  was several heavy guns hitting in turn, since the schooner's medium-cannon spacing shows no crew effect. Retail check still open: a torpedo
  ship with gunnery 0 vs 9 should show about 35 s vs about 26 s between torpedoes; a cannon ship no difference.
- Each gun reads the gunner of its own bank (bank index, shared by the bank's guns); the remake's slots follow the weapon banks including
  fixed ones. PROVISIONAL: the original may also count point-defence banks in that index. The ship's leadership bonus ([crew.md](crew.md))
  applies to accuracy only, not to reload.

## Lob shells
Measured on the running original game (2026-10): a Navy Man-O-War's bow Star Mortar fired at open etherium in a
skirmish, the shell position sampled about 30 times a second.

| Ship | Range | Flight | Launch speed | Elevation |
|---|---|---|---|---|
| stopped | 126.9 | 15.75 s | 12.6 | 50° |
| moving 2.4 u/s | 47.9 | 8.9 s | 7.5 | 44° |

- **Gravity 1.263 units/s²** on the shell (both shots agree within 0.1%), as the per-gun rule above gives; no drag.
- The launch speed is solved so the shell comes down on the aim point at about the gun's LobAngle (45°): speed ≈ √(g·range).
  MuzzleSpeed is not the launch speed (at 45° it would carry √2 × 350 = 495 units). Whether it caps the speed is not
  observed. The measured elevations
  (44-50°) scatter around LobAngle; the cause is unknown (PROVISIONAL: the remake fires at exactly LobAngle).
- The gun fires as soon as it is aimed (0.08 s after the click when already pointing that way, 0.65 s after a turn).
- A shell that hits nothing bursts where it reaches the flight plane.
- Look: in flight a small pink-white spark with a short trail. On impact a white flash with long rays, then about a second
  later a wide flat pink-orange ring spreading over the flight plane (blast MaxRadius 60 over Duration 3 in the bullet
  data); a ring from a shell ~50 units ahead reached the firing ship.

## Mortar blasts (read from the original's code, 2026-10-31)
- Star-class blasts (Star mortar, Nova secondaries, the volcano's magma bolt, mission 9's dark matter) are a ring spreading from the
  burst at MaxRadius / Duration (Star: 60 over 3 s, 20 units/s). A Star shell bursts where it comes down to the flight plane; one that
  strikes a hull or tower first deals its normal hit (unless it ricochets), then bursts there. One that times out just vanishes.
- The ring strikes everything once, when it reaches its centre: every ship and base section whose centre it passes (not just the
  nearest one), rocks, and shells in flight. Damage = the bullet's roll for the material x max(0.1, 1 - distance / MaxRadius): full at
  the centre, half at 30 units, a tenth from 54 (Star). Ship sections take the difficulty factor and spill; no fires or criticals start.
  Nobody is spared: the shooter's own ships are struck too. Rocks take the Stone roll, shells in flight the Iron roll off their hit
  points (a torpedo within about 32 units of the centre dies). Star-class shells are immune.
- A Nova mortar splits at the top of its arc (with a flash and its detonate sound) into five `Bullet_NovaMortar_Secondary` shells at
  its speed: one straight on and four turned 0.1 rad either way about its up and right axes, landing roughly 25 units apart. Each
  is a Star blast (radius 60 over 3 s). A Nova that strikes a hull before its apex is spent.
- Ordinary shells have no splash.
- Remake: `combat::lob::blasts`, `nova_split`. PROVISIONAL: the push a blast gives everything inside it (the bullet's `Magnitude`,
  constant over the radius) and the gravity well's pull (`Magnitude` 40000, zero at the centre and rim, peak at half the radius; it
  does not freeze ships) are not ported: their scale to ship motion is unmeasured, so the well still slows ships as before. The Nova
  pattern is not rolled with the shell's spin; lifeboats are not struck. The remake bursts lob shells at their aim point's height,
  not at the flight plane.

## Playtest fixes: arcs and torpedoes

- Fixed a runtime units bug: mount arcs were converted from degrees to radians in the loader, then converted again
  during aiming. A nominal 45-degree torpedo launcher had less than a one-degree total firing arc. Runtime now uses
  the loaded radians directly. This affects every gun, not just torpedoes.
- Torpedoes now render the authored `LaserTorpedoBullet` scene's root mesh (`LaserTorpedo_Layer1`) rather than a shell bead,
  retaining the existing retail trail and impact effects. The scene and MDB layer have different names.
- Torpedo launch no longer adds the firing ship's velocity. Aim and launch now consistently use target world velocity and
  the torpedo's own forward speed. The flat launch direction is normalized after deviation, keeping launch speed at 22 units/s.
  This removes artificial sideways drift/speed changes caused by the firing ship's motion. Ordinary shells were later measured to
  behave the same way (see Shell gravity).
- Launch aim still uses a horizontal intercept, falling back to current target position if unreachable. Flight now has guidance:
  0.5-second launch coast, authored ship homing point (other targets use position), speed-based lead inside 100 units, retail
  20-degree rudder saturation and bounded altitude adjustment. Exported bullet physics drives thrust/speed and angular response.
  The planar integrator, exact retail tracking-invalidation timer and altitude scheduling remain provisional; arming/point defence
  are not implemented.
- Tests cover stationary/crossing/receding/unreachable intercepts, the retail launcher arc/body asset, and schooner
  triangle collision. A GPU arena run with a stationary schooner 80 units ahead and 15 units to starboard logged an
  actual torpedo hit on its stern. Moving-target hit rate and original torpedo behavior are not yet measured.
- Training fleets remain invulnerable: impact effects do not imply hull damage during mission one's first drill.
- Follow-up regression tests cover crossing targets with moving shooters, unreachable-target fallback, constant horizontal
  torpedo launch speed. Two stationary-target GPU arena runs logged torpedo impacts.
  `local/torpedo-polish-oblique.png` shows the flame meeting the projectile at an oblique camera angle; this is not a moving-target
  Mission 1 validation.

### Retail torpedo findings (follow-up)

A torpedo starts with its muzzle speed as forward velocity (on the common physics time scale); launching does not add the
firing ship's velocity. Ordinary shells do not add it either (measured, see Shell gravity).

A torpedo is not a straight projectile: it has a 0.5-second initial phase, then a homing phase,
then clears its steering command when tracking ends or the target is absent. Ships supply their `TorpedoHomingPoint`; other
targets use their world position. Inside 100 units it adds a speed-based lead before issuing the common turn-to-point controller
. It also adjusts altitude in small steps, subject to height bounds. Exact phase timers, steering/propulsion integration,
and target invalidation still need a complete specification for exact fidelity. Follow-up work established the turn
controller's saturation at 20 degrees, altitude steps of 0.025 with a +/-3-unit bound, and the 0.5-second initial phase. Runtime now
uses these findings with substepped planar integration and approximate 0.5-unit/s height adjustment. Invalid/destroyed/hidden targets
stop tracking rather than being reacquired. A regression intersects the authored schooner hull moving sideways at 4 units/s;
GPU Mission 1 smoke logs homing hits on the moving, invulnerable training fleet.

### Torpedo flame alignment

Pseudo-billboard streaks now retain their long axis in world space; only their width turns toward the camera. Previously the
long axis was projected into the camera plane and normalized, lifting a level torpedo flame above its body at oblique angles
and preventing natural foreshortening. This shared fix applies to fixed-axis and velocity-axis streaks. Ordinary camera-facing
particles and the authored EFT brightness/size/rectangle curves are unchanged. The precise retail quad builder is still unknown.

## Turret tracking (horizontal, vertical, engage test)
- Horizontal: each update the base turns toward the aim point by HorizontalRotationSpeed x 0.033 s (the original's fixed tick),
  never past the error, and the resulting yaw (relative to the mount's rest direction) is clamped to +-half the mount arc.
  This runs whatever the range: guns follow a target they cannot reach, stopping at the arc edge. Inverted mounts flip the sign.
- Aligned means the remaining angle is under 1 degree (pi/180, set).
- Vertical runs only once the base is aligned, and only if the horizontal distance is within the gun's range; it steps the
  barrel at VerticalRotationSpeed toward the ballistic (or lob) elevation, clamped to the vertical limits; 1 degree tolerance.
- Engage test: horizontal distance <= range and the target bearing within the half-arc.
- Unknown: what a gun does with no target (the remake returns it to rest).
- Measured (2026-10, two skirmishes, Normal): an idle command ship with no clicked target and panic fire off never fires, even
  with an enemy Carrack 40 units away for over a minute; the Game Summary shows 0 Damage Inflicted for us both times. So the
  command ship's direct-fire guns need a clicked target or panic fire (point defence not checked).

## Panic button
- The hotkey/HUD button works only while the player ship is not disabled. It is sent like any order (so it works in multiplayer).
- Every qualifying gun bank (some gun classes are skipped; which is unknown) switches to fire-when-possible in panic mode: each
  gun picks its own target, the best candidate within its own range (nearest-style).
- Remake: `GroundFire::panic` set by the bar button; each non-indirect player gun fires at the nearest enemy ship in its
  range and arc (fallback: the normal target). Cleared on any retarget (click or C). PROVISIONAL: persistence, selection score, flag filter.

## Mine laying
Mine guns (the star-mine dropper on the Navy and Procyon Minelayers and the weapon barge) sit on non-selectable mounts: neither
the player nor the gun's own aim decides when they fire. Every ship's saved AI block holds a lay-mines routine with a laying
time of 10 s and a no-laying time of 20 s (retail saves: `LAYMINESMAXTIME` 10, `DONOTLAYMINESMAXTIME` 20, `LAYINGMINES`).
- The routine only runs for a ship that carries a mine gun, and only while at least one hostile ship exists.
- A laying spell lasts 10 s. During it every mine gun fires whenever it has reloaded (Gun_StarMine_Med: one mine per 10 s), the
  mine leaving along the mount's rest direction at MuzzleSpeed and drifting.
- A new spell starts when the ship moves faster than 1 unit/s and 20 s have passed since the last spell ended (or since the
  ship appeared), or at once whenever the nearest enemy in sight is closer than 50 units. PROVISIONAL: the 1 unit/s and
  50-unit thresholds, and that the human player's minelayers run it too, are not yet observed in play.
- Remake: `combat::gunnery::MineLaying`; mine guns never aim at a target.

### Placed minefields
Border Dispute (186 mines) and Red Rover (135) place `Bullet_StarMine_Med` world objects in the map. The script's `Set Group/Unit Owner` hands each
field (a group of object ids) to a player; the mines then fight for that player's side with the same homing as a laid mine (sight 50,
speed 20) and rest where they were put. Until given an owner a mine is hostile to every ship. Remake: `combat::minefield`
(`maps/<id>/map.json` `mines`). The mine's mesh is the scene's first mesh element (`StarmineV2_Layer1`), now exported so mines are visible.
PROVISIONAL: whether an unowned mine really is hostile to all (the scripts always assign every field at world start).

## Shots and land (read from the original's code, 2026-10-31)
Every projectile (cannon shells, lob shells, harpoons, nets, torpedoes, mines and lancer bolts alike) has each step of its flight
swept as a 3-D segment against the collision triangles of islands, bases and armed asteroids, so nothing behind the first land it
meets can be hit. The firer's own island or base is exempt. Only scenery with no collision at all (nebulae, black holes, wrecks,
dock buoys) lets shots through. A lob shell clears an island only if its arc passes above the triangles. On land with no damage
section the shot is spent with its impact (its impact sound and effect; an explosive or lobbed shell bursts there); a gun tower's
section takes the hit as before. Remake: `combat::shells` with `collision::Obstacle::segment_hit`. PROVISIONAL (not decoded): the
exact removal.

## Base and island hit feedback (decoded)
Bases are islands and take hits like ship sections. A shell that is not ricocheted and whose upper damage on the material is over 10 makes a wood or
reinforced-wood section play `Splintering_Wood` and a stone section `Rock_Chunks`, 50 % of the time and only within 300 units of the
camera (iron, cloth, dragon: nothing; island mesh between sections: only the shell's own impact). Smoke per section from the remaining
share r of its HP: over 0.75 none, 0.5 up to 0.75 Light, 0.25 up to 0.5 Medium, under 0.25 Heavy, none left Destroyed (exactly 0.75 falls
to Heavy), swapped when the state changes, no sight test. A destroyed base section plays each of its destruct effects at once at its
point (or `ShipExplosion_Medium` at its centre when it has none), one explosion sound, and drops its debris piece. There is no health bar
or section readout for an enemy (PROVISIONAL: inferred from the absent HUD resources); guns aim at the clicked point, stored in the
target's frame. Remake: `combat::damage::section_smoke` serves ships and bases; `shells::base_parts_burst`; the 300-unit camera gate is
not applied. Not decoded: how a click picks a point on a base mesh.

Ships take the same splinter/rock rule, and a destroyed ship section also bursts into one `ShipExplosion_Medium` at its place. Hull scars
(decals): every hit that is not a ricochet leaves one unless the target is fully invulnerable, even with no HP lost. The scar is a box
of edge 0.25 / 0.5 / 1.0 (Small / Medium / Large, the bullet's decal size) on the struck hull. A new scar meets each older one closer than
the sum of their radii (the same edges): overlap % = (R^2 - d^2) / R^2 x 100, R the radius sum. The older scar is merged away if its class
is smaller or the overlap is under T[new][old] (new Small 50/20/0, Medium 75/10/0, Large 99/70/0 for old Small/Medium/Large); otherwise
the new scar is refused. If something was merged and the new class is not larger, the new scar takes the next class up (two nearby small
hits become a medium). No new scar once a ship carries over 150; scars never fade and go only when merged or with their destroyed
section's mesh swap. Remake: `decals.rs` (a camera-lit quad instead of a clipped box; the repair clear-out is ours). Damage smoke (decoded, applied): each section's effect is attached at the hull origin, ship-local, and every primitive that
declares the mesh-vertex variable adds a random vertex of the section's mesh to each spawn position (the invisible source primitive keeps
10 particles for Light and 2 for the others, re-rolled when the state changes; the smoke follows them; the third primitive is a flame
spawning at a fresh vertex). Fires (decoded, applied): several per section allowed; they change state only on the 6 s ticks: a fire that is
out (no strength or its section gone) stops emitting on the tick that finds it, is cut loose from the ship (keeping its world pose) on
the next and deleted on the one after. Also applied: hit effects on a ship (the bullet's own and the splinters) are attached to the hull and ride with it; a new fire takes the
shell's orientation, so it tilts for steep shots (PROVISIONAL: the original's conversion of that basis was not decoded); decal textures
are filed by the original's table (five in every group, six in wood and cloth, the rest by name; engine_part, rudder and hole_damage60
are never used); the 15 % smoke thinning is gone. Not applied yet: freezing effects that are off-screen (a unit-radius frustum test;
spawn gate 700 with 50 hysteresis).

## Ricochet (read from the original's code, 2026-10-31)
Every shot that strikes a damage section (a hull, a gun tower, a volcano) first rolls a ricochet:
- Chance = (F + M + angle) / 100, clamped to 0..1, one uniform draw per strike.
- F is the firing gun's ricochet factor for the range band it fired in (`EffectiveRange_`, `LongRange_`, `MaximumRange_RicochetFactor`,
  picked by the distance to its target like the accuracy bands). Vanilla: cannonballs -60/-50/-40 (effective/long/maximum), gattling
  -10/0/0, plasma, harpoons and lancers far below zero, torpedoes and mines -1000 (never), mortars 0.
- M by the section's material: wood 0, reinforced 25, iron 50, stone 50, cloth -100 (sails never deflect), dragon 0.
- angle: degrees between the shot's flight relative to the target, reversed, and the struck face's normal: 0 head-on, 90 grazing.
- A ricochet does no damage, no spill, no fire and no impact effect. The shot mirrors off the face at 0.6 of its speed and keeps
  flying (it can hit something else), and the bullet's `RicochetSound` for the material plays at the strike.
- Remake: `combat::shells::ricochet`. PROVISIONAL: a small extra effect the original also plays is not identified; the sound's range
  is the warning shot's 600 units; warning shots keep their harmless glance; mines' forced ricochet off friendly hulls is not ported
  (our mines do not strike friendly hulls at all). Rocks never deflect a shot (their material term is -100).

## Destroyed base part (PROVISIONAL)

When a mission-vital part of an armed base (building, defence ring) drops to 0 hit points, the remake plays that part's own destruct effect (from the retail section data) at the centre of the part, plus a ship-explosion sound. The part then swaps to its damaged mesh and stops blocking shells. Retail's actual effect and sound for a destroyed base part have not been measured; every headless attempt lost to the base's guns first (#97).
