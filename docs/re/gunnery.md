# Gunnery: aim lead and accuracy (exe findings)

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
taken as world up and the muzzle's right. Ordinary shells inherit the shooter's velocity, so their lead uses
target minus shooter velocity (native ordinary-shell inheritance remains unverified). Torpedoes do not inherit it; see below.
Measured in an earlier scripted skirmish: hit rate 4% before, 60% after (not remeasured after the changes below).

## Lob shells
Measured on the running original game (2026-10): a Navy Man-O-War's bow Star Mortar fired at open etherium in a
skirmish, the shell position sampled about 30 times a second.

| Ship | Range | Flight | Launch speed | Elevation |
|---|---|---|---|---|
| stopped | 126.9 | 15.75 s | 12.6 | 50° |
| moving 2.4 u/s | 47.9 | 8.9 s | 7.5 | 44° |

- **Gravity 1.263 units/s²** on the shell (both shots agree within 0.1%); no drag (horizontal speed constant).
- The launch speed is solved so the shell comes down on the aim point at about the gun's LobAngle (45°): speed ≈ √(g·range).
  MuzzleSpeed is not the launch speed. With this gravity, MuzzleSpeed 25 at 45° would carry 25²/1.263 ≈ 495 units, about
  the authored maximum ranges (350 star, 500 nova). Whether it caps the speed is not observed. The measured elevations
  (44-50°) scatter around LobAngle; the cause is unknown (PROVISIONAL: the remake fires at exactly LobAngle).
- The gun fires as soon as it is aimed (0.08 s after the click when already pointing that way, 0.65 s after a turn).
- A shell that hits nothing bursts where it reaches the flight plane.
- Look: in flight a small pink-white spark with a short trail. On impact a white flash with long rays, then about a second
  later a wide flat pink-orange ring spreading over the flight plane (blast MaxRadius 60 over Duration 3 in the bullet
  data); a ring from a shell ~50 units ahead reached the firing ship. Ring growth timing not measured.
- PROVISIONAL: direct-fire shells flagged BulletAffectedByGravity are assumed to fall under the same gravity (not measured).

## Playtest fixes: arcs and torpedoes

- Fixed a runtime units bug: mount arcs were converted from degrees to radians in the loader, then converted again
  during aiming. A nominal 45-degree torpedo launcher had less than a one-degree total firing arc. Runtime now uses
  the loaded radians directly. This affects every gun, not just torpedoes.
- Torpedoes now render the authored `LaserTorpedoBullet` scene's root mesh (`LaserTorpedo_Layer1`) rather than a shell bead,
  retaining the existing retail trail and impact effects. The scene and MDB layer have different names.
- Torpedo launch no longer adds the firing ship's velocity. Aim and launch now consistently use target world velocity and
  the torpedo's own forward speed. The flat launch direction is normalized after deviation, keeping launch speed at 22 units/s.
  This removes artificial sideways drift/speed changes caused by the firing ship's motion. Ordinary-shell behaviour is unchanged.
- Launch aim still uses a horizontal intercept, falling back to current target position if unreachable. Flight now has guidance:
  0.5-second launch coast, authored ship homing point (other targets use position), speed-based lead inside 100 units, native
  20-degree rudder saturation and bounded altitude adjustment. Exported bullet physics drives thrust/speed and angular response.
  The planar integrator, exact native tracking-invalidation timer and altitude scheduling remain provisional; arming/point defence
  are not implemented.
- Tests cover stationary/crossing/receding/unreachable intercepts, the retail launcher arc/body asset, and schooner
  triangle collision. A GPU arena run with a stationary schooner 80 units ahead and 15 units to starboard logged an
  actual torpedo hit on its stern. Moving-target hit rate and original torpedo behavior are not yet measured.
- Training fleets remain invulnerable: impact effects do not imply hull damage during mission one's first drill.
- Follow-up regression tests cover crossing targets with moving shooters, unreachable-target fallback, constant horizontal
  torpedo launch speed and unchanged ordinary-shell inheritance. Two stationary-target GPU arena runs logged torpedo impacts.
  `local/torpedo-polish-oblique.png` shows the flame meeting the projectile at an oblique camera angle; this is not a moving-target
  Mission 1 validation.

### Native torpedo findings (follow-up)

`TorpedoPhysics` exposes the muzzle-speed setter at; its initialization at sets local forward velocity from
that speed using the common physics time scale. The launch/custom-info path does not add
firing-ship velocity. This is distinct from the remake's still-provisional ordinary-shell inheritance.

The torpedo custom-info update is not a straight projectile: it has a 0.5-second initial phase, then a homing phase,
then clears its steering command when tracking ends or the target is absent. Ships supply their `TorpedoHomingPoint`; other
targets use their world position. Inside 100 units it adds a speed-based lead before issuing the common turn-to-point controller
. It also adjusts altitude in small steps, subject to height bounds. Exact phase timers, steering/propulsion integration,
and target invalidation still need a complete specification for exact fidelity. Follow-up constant decoding established the turn
controller's saturation at 20 degrees, altitude steps of 0.025 with a +/-3-unit bound, and the 0.5-second initial phase. Runtime now
uses these findings with substepped planar integration and approximate 0.5-unit/s height adjustment. Invalid/destroyed/hidden targets
stop tracking rather than being reacquired. A regression intersects the authored schooner hull moving sideways at 4 units/s;
GPU Mission 1 smoke logs homing hits on the moving, invulnerable training fleet.

### Torpedo flame alignment

Pseudo-billboard streaks now retain their long axis in world space; only their width turns toward the camera. Previously the
long axis was projected into the camera plane and normalized, lifting a level torpedo flame above its body at oblique angles
and preventing natural foreshortening. This shared fix applies to fixed-axis and velocity-axis streaks. Ordinary camera-facing
particles and the authored EFT brightness/size/rectangle curves are unchanged. The precise native quad builder is still not decoded.

## Turret tracking (horizontal, vertical, engage test)
- Horizontal: each update the base turns toward the aim point by HorizontalRotationSpeed x 0.033 s (the exe's fixed tick),
  never past the error, and the resulting yaw (relative to the mount's rest direction) is clamped to +-half the mount arc.
  This runs whatever the range: guns follow a target they cannot reach, stopping at the arc edge. Inverted mounts flip the sign.
- Aligned means the remaining angle is under 1 degree (pi/180, set).
- Vertical runs only once the base is aligned, and only if the horizontal distance is within the gun's range; it steps the
  barrel at VerticalRotationSpeed toward the ballistic (or lob) elevation, clamped to the vertical limits; 1 degree tolerance.
- Engage test: horizontal distance <= range and the target bearing within the half-arc.
- Not decoded: what a gun does with no target (the remake returns it to rest).

## Panic button (PanicFireCommand, execute; `GameUserInput::PanicFire`)
- Hotkey/HUD action runs only if the player ship is not in a disabled state; the command is skipped when the ship's
  state value (of its controller) is 4. Network-serialisable (one id), so it is a replicated command.
- It walks the ship's gun-bank list and calls the per-bank routine with panic=1 for banks
  whose gun class flags qualify: first pass banks with class flags 4 and 6 clear (and 5, 7 clear on the first gun); second
  pass banks with flag 7 set. The flag meanings are not decoded.
- Per gun: sets FireWhenPossible and the panic flag; with panic it asks the gun's own target finder (
  candidates within the gun's range, nearest-style scoring) and stores that target id, so each gun picks its own target.
- Remake: `GroundFire::panic` set by the bar button; each non-indirect player gun fires at the nearest enemy ship in its
  range and arc (fallback: the normal target). Cleared on any retarget (click or C). PROVISIONAL: persistence, selection score, flag filter.
