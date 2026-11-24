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

## Where a computer captain's guns aim on a ship
When a computer captain chooses a target (its first target, a new choice after the 6 s hold, or a fleet mate's target taken over),
it also fixes the point its guns aim at: the centre of the target's damage section nearest to the captain's own ship, by straight
3-D distance, the first section on a tie, destroyed sections included. The point is kept in the target's own frame, so it moves
and turns with the target, and it stays the same until the next choice; it is not re-picked as the ship circles in between. A
target with no damage sections is aimed at its origin. So shots from one attacker land around one part of the hull at a time
(bow when it comes from ahead, a side when abeam, the stern from astern), and several attackers spread their fire over the hull.

Checked in the original (a computer Barque against an idle Man-O-War): the stored point was 12.1 units forward of the
target's origin while the Barque was ahead, 4.9 to port when it was abeam to port, 16.0 aft when astern and 4.8 to starboard when
abeam to starboard, and it changed only at target choices, 6.3 and 12.7 s apart.

Remake: `ship::attack::aim_local` picks the section (its box centre, the same point the attack courses steer by), stored with the
target in `ship::CaptainTargets` and used by `combat::gunnery`. PROVISIONAL: only skirmish captains; a base target keeps the roof
point of the section nearest its centre (see "Gun tower aim point").

## Deviation (run once per burst, or per shot if CalculateAccuracyDeviationDuringBurst)
- Band by distance: <= EffectiveRange -> Effective deviation, <= LongRange -> Long, else Maximum.
- Half of it, times an accuracy factor that depends on what the gun sits on. **Every ship, computer or not, uses its crew**:
  1 - 0.8 x the gunnery of the gunner in the gun's own bank slot, after the ship's leadership bonuses (+0.1 / +0.05, drawn once
  per ship from the captain's and first mate's Leadership, [crew.md](crew.md)) and clamped to [0, 1]. A slot nobody holds is a
  rating-1 hand (0.1, so a factor of 0.92 before bonuses). The flat 0.5 belongs to objects that are neither ships, bases nor dragons;
  a base reads a per-base value (the mission's gunnery level for its guns, `IslandGunnery`), a dragon's breath 0.8 (the remake gives
  dragon breath no spread yet). NIS mode uses 0.1 (the deviation is scaled, not removed).
- The final angle is uniform in [-a, +a].
- Remake: the cinematic factor is 0.1. The gun's `CalculateAccuracyDeviationDuringBurst` flag (set on a few retail guns) makes the angle be
  drawn for every shot instead of once per burst (`GunDef::dev_per_shot`; the exporter writes the flag, so re-import assets).

## Firing
Per muzzle the angle is applied twice to the muzzle orientation, about two axes, each with a random sign
(so yaw and pitch error have the same magnitude). Speed is MuzzleSpeed.

## Muzzles
A gun with several muzzle points (the Procyon dual plasma and lancer mounts have two) launches one shell from **every** muzzle on each
shot, in the same tick; a burst of 5 shots on a dual mount is 5 pairs. Each shell takes its own deviation turn (two random signs per
shell; the angle size is the burst's). Each muzzle also plays its own fire sound. Read from the original and confirmed on a Procyon
cruiser: one shot of a two-muzzle gun counted two shells launched, one shot of a one-muzzle gun counted one. The remake used to fire one
muzzle per shot, alternating them (OpenProcyon #469). Not yet checked: the sound per muzzle, and the deviation turn per shell.

## In the remake (`combat/`)
Lead and deviation follow the above. Ships use the crew rule above: named crew (skirmish, campaign outfit) or, for a placed ship
off the player's side, its level's generic crew in every slot (3 / 6 / 8 of 10); the player's side with no crew record at all keeps
the campaign roster's best gunner (human fleet) or a rating-1 hand. PROVISIONAL: the two rotation axes
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
  shortened by crew (unchecked against play): wait = max(0, TimeBetweenBursts - 10 x the raw Gunnery (0..1) of the gunner at the
  gun's bank), with no leadership bonus, no difficulty term and no human/computer distinction (a computer torpedo ship uses its own generic
  crew's gunnery; PROVISIONAL, remake: computer ships fire at the authored time). Every other gun (cannons, carronades, Gattlings, lancers,
  mortars, mines, point defence) reloads at its authored TimeBetweenBursts whoever mans it. The in-burst spacing never changes. The wait is
  compared against the current skill at every readiness check, so a crew change takes effect at once (remake: latched when the burst starts).
- Earlier measurement (Navy Man-O-War, nine gunners of 7-9, one gun's hits every 3.5-3.9 s) was read as a cannon crew effect; it more likely
  was several heavy guns hitting in turn, since the schooner's medium-cannon spacing shows no crew effect. Retail check still open: a torpedo
  ship with gunnery 0 vs 9 should show about 35 s vs about 26 s between torpedoes; a cannon ship no difference.
- Each gun reads the gunner of its own bank, for accuracy (every ship) and for the torpedo reload: the bank index is the order in which
  banks first appear in the ship's gun list, shared by all the bank's guns. The original counts point-defence banks in that index (a
  class's gunner slots equal its bank count: Navy Cutter 5, Navy Man-O-War 10), while the remake's slots follow the banks the crew
  screen lists, which skip point defence. The two agree unless a point-defence bank comes before a listed one (most Navy hulls, e.g.
  the Cutter's fourth bank); whether the retail crew screen shifts its slots there is not known, so the remake keeps the screen's
  order. PROVISIONAL. The ship's leadership bonus ([crew.md](crew.md)) applies to accuracy only, not to reload.

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
- The gun fires as soon as it is aimed (0.08 s after the click when already pointing that way, 0.65 s after a turn, fixed however far it turns).
  Remake: "already pointing" is the gun's ordinary aligned test (base within 1° of the aim and barrel within 1° of its elevation), taken before
  the first step's turning.
- A shell that hits nothing bursts where it reaches the flight plane.
- Look: in flight a small pink-white spark with a short trail. On impact a white flash with long rays, then about a second
  later a wide flat pink-orange ring spreading over the flight plane (blast MaxRadius 60 over Duration 3 in the bullet
  data); a ring from a shell ~50 units ahead reached the firing ship.

## Mortar blasts (unchecked against play, 2026-10-31)
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
  constant over the radius) is not ported: its scale to ship motion is unmeasured. The Nova
  pattern is not rolled with the shell's spin; lifeboats are not struck. The remake bursts lob shells at their aim point's height,
  not at the flight plane.
- **Gravity charge well** (unchecked against play): for 17 s after it lands (Radius 40) everything within 40 units (flat
  distance, no friend/foe filter, the shooter's ships included) is pulled straight at the centre with force Magnitude (40000) x
  4 s² for s < 0.5, 4 (1 - s)² beyond, s = (40 - d) / 40: zero at the centre and at the rim, the full 40000 at d = 20, a quarter of it
  at 10 and 30. It does no damage. Remake: `combat::lob::GravWell::force`; ships get it through the black hole's pull
  (`blackhole::pull`: the same force-to-acceleration scale, only ships not under way move, the rest snap and break-free rule),
  so a resting Cutter is dragged in near the peak and a Man-O-War barely moves. PROVISIONAL: the scale is the black hole's,
  and that ships under way are unmoved by a well is assumed from it.

- **Star mortar impact glow** (`StarMortar_Heavy_Impact`): both particle systems start at rest within 1.5-4 units of the centre;
  the attractor (effects.md: constant acceleration along the unit direction) goes from +5 to -20 / -27 over 0.5 s, so they then
  speed up outward at 20 and 27 units/s^2. Checked on the original's running game: median radius 72 and 96.5 units at 2.97 s,
  when the 3 s effect ends. Nothing removes them early and nothing needs to.

## Who each gun shoots at, and lob aim points of computer ships (unchecked against play)
- **Panic fire** (the player's button, refused while the ship is docked) goes through every gun bank in two passes. First pass: banks
  whose guns are not lobbing, not point-defence, not mine-laying and not torpedo launchers. Second pass: torpedo-launcher banks. So
  every class fires except lob, point-defence and mine guns, which keep their own routines. Each gun that fires then finds its own
  target: the nearest hostile ship in sight within its maximum range, and keeps it. The finder was read testing range, not mount arc.
  Remake: `gunnery::panic_fires` (the class rule) and `panic_target` (nearest, PROVISIONAL: it also requires the mount arc, and falls
  back to the ship's own target when nothing qualifies; the pending request lapses after 2 s or ends with the gun's first shot, see the fire request time-out).
- A gun's own target choice happens only in panic fire; the "fire at will" state is set by the point-defence routine alone (which
  picks incoming torpedoes and mines, as above). Every other gun fires at the ship's target.
- **Computer ships' lob guns** (star mortars, nova mortars, gravity charges: the guns flagged as lobbing) do not aim at the target
  itself. About every think (0.5 s, PROVISIONAL) a ship that has a target computes one ground point: a base is taken as it stands;
  any other target gives its position + its velocity x k (k uniform 5..10 s) + a flat random offset of length uniform 0..(|velocity| x k).
  The point is adopted only if it is farther from the shooter than a distance drawn uniformly from 50..70; otherwise the guns keep
  their previous point. Each lob bank then aims at that point plus a fresh random offset of at most one unit (flat), on the flight
  plane. Non-lob banks are unaffected. Remake: `gunnery::ground_point`, `bank_point`, `GroundAim`. PROVISIONAL: a lob gun that has
  never had a point stays idle (the original leaves it aimed at the map origin); the think is skipped while a friendly ship or
  scenery is in the line of fire (see below).

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
uses these findings with substepped planar integration. Remake: the height step is built as read: once per battle tick a homing
torpedo moves 0.025 toward the target's height when they differ by more than 0.05, bounded to +/-3 units. Invalid/destroyed/hidden targets
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

## Unchecked against play, not yet measured in play (2026-11)
- **Idle guns.** A direct-fire gun with no target never runs its tracking, so it stays where it was; hold fire (a fleet order or a script)
  freezes the turrets the same way, it does not swing them back to rest. A lob gun keeps tracking its stored ground point.
- **Torpedo that loses its target** (destroyed, released or fully cloaked): it cuts its thrust and slows to rest, and can still hit;
  one launched with no target is not affected.
- **Energy net on a ship that is cloaking or cloaked** orders it to decloak.
- **Ignition.** One ship, the mission 9 lifeboat, never catches fire.

## Line of fire (computer captains)
Unchecked against play, not yet measured in play. A computer-run ship (never the human's command ship) fires nothing in a
think while the straight segment from its centre to the aim point is blocked: no bank fire, no ground-fire point, no harpoon roll.
Every attack routine's firing step has the test (simple, ram, broadside, frontal, rear, formation, dock for repairs) and so does the
harpoon decision.
- **Friend in the line:** the segment (parameter 0 to 1) crosses the bounding sphere of any other ship that is friendly: a lifeboat class on
  either side, or an allied ship. The sphere's own radius, no margin. Dead and cloaked ships block too. A segment lying wholly inside a
  sphere does not cross it and does not block.
- **Scenery in the line:** islands and bases are tested against their collision triangles; any other obstacle by its bounding sphere,
  blocking when the segment crosses it or an end lies inside it. Only candidates whose surface distance from the ship is at most the
  segment length are tested, and the aimed-at object never blocks its own shot.
- Remake: `line_of_fire.rs` (pure predicates, unit-tested), applied in `combat::gunnery::fire` (every gun of a computer ship except point-defence and mine-laying guns holds, as those have their own routines; turrets
  keep tracking; no new ground-fire points) and `boarding::ai_boarding` (harpoon roll skipped). Allied = same side, or allied mission
  players, and not hostile. PROVISIONAL: the aim point (the target's aim point for every routine; the original's broadside, frontal
  and rear runs use their own points), asteroids and other non-mesh obstacles by their bounds sphere, bases tested only if they carry
  a collision mesh, and the segment starts at the hull centre.

## Panic button
- The hotkey/HUD button works only while the player ship is not disabled. It is sent like any order (so it works in multiplayer).
- Every qualifying gun bank (some gun classes are skipped; which is unknown) switches to fire-when-possible in panic mode: each
  gun picks its own target, the best candidate within its own range (nearest-style).
- Remake: `GroundFire::panic` set by the bar button; each non-indirect player gun fires at the nearest enemy ship in its
  range and arc (fallback: the normal target). Cleared on any retarget (click or C), or when no gun holds its request any more (2 s, or the gun's first shot). PROVISIONAL: persistence, selection score, flag filter.

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

### Mine target pick
Unchecked against play (the 5 s constant read from its data; not yet measured in play): a mine does not look for a target every
frame. It picks the nearest hostile ship in sight only every 5 s, the first time 5 s after it was laid, keeps chasing that ship in
between while it is alive and not cloaked, and coasts to rest when it has none. A ship that sails into sight is therefore noticed
up to 5 s late, and one that leaves sight is still chased until the next pick.

### Placed minefields
Border Dispute (186 mines) and Red Rover (135) place `Bullet_StarMine_Med` world objects in the map. The script's `Set Group/Unit Owner` hands each
field (a group of object ids) to a player; the mines then fight for that player's side with the same homing as a laid mine (sight 50,
speed 20) and rest where they were put. Until given an owner a mine is hostile to every ship. Remake: `combat::minefield`
(`maps/<id>/map.json` `mines`). The mine's mesh is the scene's first mesh element (`StarmineV2_Layer1`), now exported so mines are visible.
PROVISIONAL: whether an unowned mine really is hostile to all (the scripts always assign every field at world start).

## Shots and land (unchecked against play, 2026-10-31)
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

### Gun tower aim point
Owner playtest of mission 2 (two island guns): most player shells hit the island, not the towers. Cause, measured against the exported
collision mesh: the remake aimed at the centre of the tower's bounding box, which is inside the tower body and behind terrain from most
approaches (flight plane at the tower's mid height; from 50 units out, only 5 of 72 compass bearings had a clear line to the centre of tower 2,
31 of 72 for tower 1; the roof was clear from 48 and 34, any face from 61 and 56). Now the aim is the clicked point on the tower mesh
(what the original does), and the roof centre when no click is available (computer-less fallback, PROVISIONAL): the top centre of the
section's box dropped straight onto its mesh, or the nearest triangle centre when no mesh lies below it. Mission 6's stockade gate (#353)
is a sprawling section whose box top centre is air; aimed there, every shell flew over it. The remaining blocked
bearings are real terrain occlusion, which the original's swept collision also applies. Remake: `combat::hit_test::base_aim_point`.

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

## Ricochet (unchecked against play, 2026-10-31)
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
  is the warning shot's 600 units; warning shots keep their harmless glance; a mine's forced ricochet off allied hulls is the `force` argument of
  `ricochet`. Rocks never deflect a shot (their material term is -100).

## Destroyed base part (PROVISIONAL)

When a mission-vital part of an armed base (building, defence ring) drops to 0 hit points, the remake plays that part's own destruct effect (from the retail section data) at the centre of the part, plus a ship-explosion sound. The part then swaps to its damaged mesh and stops blocking shells. Retail's actual effect and sound for a destroyed base part have not been measured; every headless attempt lost to the base's guns first (#97).

## Lob targeting cursor
Source: unchecked against play unless noted; **[?]** = inferred, not seen in play. Remake: `combat::LobAim`/`lob_solution`, `options_menu::context_cursor`, `view::input`, `classic/world_hud.rs` (PROVISIONAL where marked).
- **Arming**: a left click on a weapon-bar bank whose first gun is a lob gun arms it for the *primary* ship (no ammo/reload check); the weapon type
  comes from the bank's bullet (gravity charge, nova mortar, star mortar). Other banks get the ordinary fire toggle.
- **Leaving**: a right click while armed cancels it, plays `HUD_LobWeapon_Cancel` and opens no menu. A left click with a ground point under the cursor
  sends the lob order and clears the mode (one shot at a time); a left click with no ground point keeps it armed. No Esc, no cancel on reselecting a ship [?].
- **Ground point**: the camera ray through the cursor meets the sea plane (z = 0), whatever is under the cursor; none over the HUD.
- **Cursor**: while armed with a ground point, the lob cursor of the weapon type shows over ships, bases and open sea alike [?: priority over the hover
  cursors]. The `_HAS_NO_FIRING_SOLUTION` variant shows when no live gun of the bank has the point within its maximum horizontal range and mount arc
  (the engage test above). Minimum range, height, reload, crew and line of sight do not count. A click is accepted either way.
- **Cursor art**: hud.hdt gives all six lob cursors the same art as the plain move arrow (the Pirate set uses one other arrow), so the cursor *shape* does not change; only the marker below tells the states apart.
- **Ground marker**: not a HUD sprite (the `LobTarget_Icon_*` rectangles in hud.hdt are unused placeholders cut from HUD panels). The game draws one of six
  small flat meshes (per weapon, plus a "no firing solution" twin) on the ground point each frame, and removes it with the mode: no marker while shells fly.
  Data: `LobIcon WorldSpace Radius` 10, `Z Plane Offset: LobIcon` -3; the draw call ignores both, so size and height are PROVISIONAL (remake: the mesh's own size, on the sea plane).
- **Sounds**: `HUD_LobWeapon_Accept` / `_Reject` on the click for gravity charges only (solution / none); star and nova clicks play neither [? looks odd, confirm in play].

### Section damage roll

One shell hit on a section rolls uniformly between the bullet's low and high value for the section's material (after the
difficulty factor has scaled both) and rounds the roll to a whole number of hit points; a light carronade takes about 17 at Normal.
Remake: the section roll is rounded to the nearest whole number. PROVISIONAL: the original's rounding mode is not confirmed
(nearest assumed; exact halves are measure-zero for a continuous roll). The spill to neighbours still reuses the rounded roll
x 0.4 rather than a fresh rounded roll per level, and base parts and lob blasts still take unrounded rolls.

## Unchecked against play: shell fuses, crew, requests, torpedoes, mines, rocks and counter-fire (2026-11, not yet measured in play)

Unchecked against play: nothing below has been checked against the running game. Where the earlier
inventory wording was wrong the correction is stated.

### Shell lifetime (no fuse)
Every shell, mine, net and torpedo carries a `Lifetime` (seconds, from its WOT) and is removed on the first tick whose age is more
than that value (strict "more than", game time, so one 0.033 s tick of slack). Nothing bursts, rolls or credits on expiry: the
shell just disappears. Values: cannon 7 / 9 / 12 (light / medium / heavy), carronade 6 / 8 / 9, plasma 7 / 9 / 12, gattling 2.2,
harpoon and grappling harpoon 3, lancer 2 / 3 / 4, Arcturian fire 8, dragon breath 10, energy net 12, torpedo 25, mine 1,000,000
(never), star / nova / magma lob shells 30, dark matter 120. The one exception is the torpedo: when it expires it plays its impact
effect (no damage), and its impact sound if the listener is within the bullet's sound distance (1500 for the torpedo). There is no
floor under the sea: a shell that falls below the water keeps falling until its lifetime ends.
Remake: `combat/shells.rs` (age > lifetime) is the same for shells. Differences: it also removes a shell below y = -300, and a
torpedo's expiry plays nothing. Build: small, `combat/shells.rs`.

### Guns do not need crew
No crew presence, crew count or alert state stops a gun firing. A gun's "Firing Crew Alert" is the id of a speech line, not a
condition. When a gun of the fleet's primary ship fires (not a point-defence gun, and only when the id is set) the crew speaks that
line (41 guns: gunner fire; the torpedo launcher: torpedo-fire line; the mine layer: none). What does gate a gun: the hold-fire
flags, the cloak hold (a gun that cannot fire cloaked), turret tracking, the reload time (a torpedo launcher's reload is shortened by
its gunners' skill) and a live section. Remake: nothing gates fire on crew; already the same. No build; the earlier "crew alert"
wording in `refit-resupply.md` means this speech cue.

### Fire request time-out
A "fire when possible" request on a gun (a fire order, panic fire, or a bank fired by a computer routine) stays pending until the gun
has fired once, or until more than **2.0 s** have passed since the request was last made for a direct-fire gun, **5.0 s** for a lob
gun (a gun with the lobbing flag). The time is counted from the latest request (a repeated request restarts it). A shot clears the
request and its panic mark; the time-out clears only the request. Point-defence guns fire continuously and have no request.
Remake: the panic order is the pending request (`gunnery::FireRequest`, one per gun): each gun that panic fire reaches holds a request
that ends with its first shot or after the lapse, and the order is over once no gun holds one and no gun is still in the burst it
fired (the rest of a burst follows the gun's own target: the request ends with the burst's first shot, the burst does not); a repeated press while some gun still
holds a request does not restart it (PROVISIONAL). The player's click-shot is still dropped once fired or when out of reach (see the
cursor section), with no time-out.

### Torpedoes: no arming, launch record, fixed-aim launchers
- A torpedo has no arming time or minimum travel. It can hit anything except its firer from the first tick. The firer is on the shell's
  rejection list and is released after 1.0 s from creation, so the firer's own ship becomes hittable after 1 s. The 0.5 s of straight
  flight before steering starts is a separate rule (torpedo guidance above). Remake: same as far as hits go; no arming to build.
- At launch a torpedo records its **launch point** (its own position at the moment it is created at the muzzle) and its **launcher**
  (the firing object's handle). Both are kept for the torpedo's life. The launcher also becomes the torpedo's target handle for
  homing. Counter-fire at torpedo launchers (below) reads the launch point.
- One case records no target: when the launching object is a space animal of hull class 34, the torpedo gets no target handle. A
  torpedo with no target leaves its 0.5 s straight flight, switches thrust off, and coasts on at its launch velocity. No shipped
  data qualifies (no animal carries a torpedo launcher), so this is unreachable in vanilla; the meaning of class 34 is not resolved.
  Spec only, no build.

### Mines
- **No blast.** A mine strikes as a point projectile against ships and scenery: one hit on the section found from the hit mesh,
  through the normal hit path (difficulty factor, 0.4 spill to neighbours when the section reaches 0, fire and critical chances of
  the table, a decal), then the mine is spent. Damage by material (low-high; crit, fire chance, fire strength): wood 60-120 (0.8, 0.7,
  1), reinforced 50-100 (0.6, 0.5, 1), iron 30-60 (0.5, 0.2, 1), cloth 0, stone 1, dragon 0.5. A mine has 20 hit points and no
  radius, so nothing else near it is hurt. Its ricochet factor is -1000 (the roll never ricochets it).
- **Ricochet off allies.** When the roll does not ricochet a mine, a mine still bounces off any ship whose owner is in mutual
  alliance with the mine's owner (own side included, matrix value 3): no damage, the mine stays live and rebounds at 0.6 of its speed
  (the normal ricochet motion). Every other ship is hit. Remake (`combat/shells.rs`): the same test (`boarding::allied_pair`: the owners'
  alliance, else the same side), then the normal ricochet motion; the roll is still drawn. Every other ship is hit.
- **Mine cap.** When a mine that has an owner object is created, the game reads the world's list of mines (list number 2 of the 13
  per-category object lists; "mines" for that list is inferred from point defence taking list 2 as its second target kind). If that
  list then has more than 100 entries, the **first** (oldest) is removed. This is one removal per creation, **world-wide, not per
  owner**: map-placed minefields (Border Dispute 186, Red Rover 135) are in the same list. Whether the new mine is already in the
  list at that moment, and whether map-placed mines have an owner object, are not read. UNCHECKED: the earlier wording "an owner
  keeps at most 100" is wrong on the code. Not built.

### Computer captains shoot rocks on their course
Once per second (1.0 s), every ship steered by the computer: take the ship's speed v; candidates are asteroids (not other scenery)
within 10 x v units that the ship's hull box would meet on a straight run of up to 10 s at its present velocity (at rest only a rock it
already overlaps counts). Take the nearest by surface distance; use it only if that distance is within the captain's scan range.
Then, unless a friendly ship is in the line of fire to it or scenery blocks it (the rock itself excepted), **every gun bank fires at
the rock** (normal range and arc tests per gun, any gun class) and the captain is marked busy for this think: the attack routines
skip their own firing then. The captain's target is not changed. No difficulty or stance term. Remake (`combat/gunnery.rs`, `collision::avoid::rock_in_the_way`):
the look runs every 1.0 s for every ship that is not the player's command ship (the captain's scan range is its sight times its crew
factor; ships on hold fire do not look); the pick stands until the next look and while it does every gun except point-defence and
mine guns aims at the rock and fires once, and no gun fires at the ship's target. Guns that cannot bear at the rock wait for it,
until the next look. PROVISIONAL: the reach test uses the hull's bounding sphere for the surface distance.

### Counter-fire at torpedo launch points ("hunt cloaked subs")
Every think of a computer-steered ship that has at least one lob bank: take torpedoes (not mines) whose launcher passes the **relation
test used in the code: the launcher is in mutual alliance with the ship (own side included), or either is a lifeboat**. The
torpedo with the nearest centre within the captain's scan range is taken; the aim point is that torpedo's **launch point**. If no friendly ship
or scenery blocks the line to the point, every lob bank aims at the point and fires. The cloak state of anyone is never tested, a torpedo
without a recorded launcher is skipped, and the point is where the torpedo was created, not where the launcher is now. UNCHECKED
AGAINST THE RUNNING GAME and surprising: the relation test reads as "allied", not "hostile" (hostile is the other mode of the same
filter), so the original's lob ships would be shelling the launch points of their own side's torpedoes. An earlier reading took it as
hostile; do not build until one value from the running original settles which. Remake: no such routine. Build: medium, shell
record gets `launch_point` and `launcher` (`combat/shells.rs`), the routine goes in `combat/gunnery.rs` next to the computer's lob
aim points.
