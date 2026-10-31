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
- On the player's crewed ships each gun waits **TimeBetweenBursts - 10 x its gunner's Gunnery** (Gunnery 0-10 read as 0..1,
  so a gunnery-7 gunner takes 7 s off), never below 0, before its next burst. The in-burst spacing is unchanged, and other
  per-gun modifiers multiply the wait (1 normally). Computer ships ignore crew here: they fire every TimeBetweenBursts.
- Measured on the original (headless 1-vs-1 skirmish, Navy Man-O-War with nine gunners of Gunnery 7-9 vs a Galleon, the
  Galleon's section HP at 20 Hz): one gun's hits arrived every 3.5-3.9 s for 90 s, against at least ~13 s between heavy
  hits with the default crew. 12 s - 8.5 = 3.5 s fits a heavy cannon with a gunnery-8.5 gunner.
- Each gun reads the gunner of its own bank, not a ship-wide average (owner-confirmed; the crew screen's gunner slots sit at the
  selectable banks, in the same order). PROVISIONAL: the point-defence banks have no visible slot and count as a generic hand
  (0.1); which slot a gun reads was inferred from the screen, not decoded. The ship's leadership bonus ([crew.md](crew.md)) adds to the
  gunner's skill before the 1 - 0.8 x skill factor.

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
  data); a ring from a shell ~50 units ahead reached the firing ship. Ring growth timing not measured.

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

## Shots and land (read from the original's code, 2026-10-31)
Every projectile (cannon shells, lob shells, harpoons, nets, torpedoes, mines and lancer bolts alike) has each step of its flight
swept as a 3-D segment against the collision triangles of islands, bases and armed asteroids, so nothing behind the first land it
meets can be hit. The firer's own island or base is exempt. Only scenery with no collision at all (nebulae, black holes, wrecks,
dock buoys) lets shots through. A lob shell clears an island only if its arc passes above the triangles. On land with no damage
section the shot is spent with its impact (its impact sound and effect; an explosive or lobbed shell bursts there); a gun tower's
section takes the hit as before. Remake: `combat::shells` with `collision::Obstacle::segment_hit`. PROVISIONAL (not decoded): the
island hit effect the original shows half the time near the player, the ricochet roll on a section hit, the exact removal.

## Destroyed base part (PROVISIONAL)

When a mission-vital part of an armed base (building, defence ring) drops to 0 hit points, the remake plays that part's own destruct effect (from the retail section data) at the centre of the part, plus a ship-explosion sound. The part then swaps to its damaged mesh and stops blocking shells. Retail's actual effect and sound for a destroyed base part have not been measured; every headless attempt lost to the base's guns first (#97).
