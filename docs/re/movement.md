# Ship movement: what the original exe does (own-words notes)

Source: observed behaviour of the original game and its retail data files, described in our own words. Only the numbers and relationships below are used.

## Ship physics data
`ShipPhysics` reads, in order: CenterOfMass, Mass, MaxThrust, MaxSpeed, RotationalFriction, MaxAngularAcceleration
(matches `Ship_Navy_Cutter.wot` lines 323-328).

## Verified in the code (turning)
Angular state: ω (rate), α (acceleration). Each frame, with speed v = |velocity|, vmax = WOT MaxSpeed:
1. α = −rudder-term + bias, where the rudder term is `(1 − r) · (1 − 1/(0.1·v + 1)) · input · (other field) · (cached field)`.
   Torque grows with speed (zero at rest). The field meanings of `input`, `r`, the two others are NOT verified;
   the game assumes input = steering in [−1, 1], r = 0, cached = MaxAngularAcceleration.
2. Speed bands: v > 0.67·vmax → α × 1/3; v < 0.33·vmax → α × 5/3; otherwise × 1.
3. Quadratic rotational drag: α −= sign(ω) · ω² · RotationalFriction.
4. α clamped to ±60. Then ω += α·dt; heading += ω·dt.
Consequence for the cutter (full rudder): top turn rate about 9°/s at full speed, about 12°/s mid-speed.

## Linear motion (structure verified, time scale not)
- The drag is not quadratic: it is per-axis drag with coefficient `(MaxThrust/Mass)/MaxSpeed`.
- Thrust: forward direction × `throttle · MaxThrust / (Mass + extra) · MaxThrust-side multipliers`, where four
  "1 − x" factors (damage-like) and two more unidentified multipliers apply.
- `SetThrottle` (slot) clamps the throttle to [0, 1]. "Current max speed" = MaxSpeed × four (1 − x) factors × two multipliers.
-  uses `ln`, i.e. exponential solutions: consistent with linear drag. Terminal speed = throttle × MaxSpeed (× multipliers).
- Lateral/vertical velocity is damped by extra terms (constants 0.5 and 0.3).
- NOT recovered: the engine-order → throttle mapping (SetShipSpeedCommand → AI → throttle), what the "Emergency" order changes
  (probably one of the multipliers), and the exe's fixed time step, so the absolute acceleration time. The game scales
  MaxThrust/Mass by a provisional 0.1 for running orders. Playtest feedback exposed that using this for All Stop
  made Comet coast for nearly a minute. All Stop now uses the unscaled WOT drag coefficient, exact exponential damping,
  and a 0.01 speed dead zone to reach true rest. This order-specific rate choice is **provisional**, not recovered
  engine-order behavior. At Comet's full speed it leaves ~0.19 units/s after five seconds and reaches zero within ten;
  original absolute stopping time still needs calibration. Running orders also use the exponential update rather
  than Euler integration; their rate and terminal-speed mapping are otherwise unchanged.

## Constants read from .rdata
0.33 / 1.0 / 5/3 / 1/3 (speed bands), 60.0 (α clamp), 0.1 (rudder-speed constant), 0.5 and 0.3 (lateral terms),
±0.01 (dead zone on |v|), 0.03 / 0.25 / 500 (bank/roll related, not understood).

## Damage → movement (verified, ship update, counter)
- Damage sections carry four flags at.. (vitalToShip, vitalToMaxVelocity, vitalToManeuverability, vitalToMission); HP int at.
- Every frame: physics = min(destroyed / total vitalToMaxVelocity sections, 0.9); = same for vitalToManeuverability.
   is one of the four "(1 − x)" factors on max speed/thrust; is the `r` in the rudder term (1 − r) (read at).
  Player speech fires when the sail loss crosses 25/50/75/100%.
- Not decoded: (1 − product of (1 − x) over some list, crew-like),, (0.6 × a timed progress, state-dependent), (turn multiplier from crew).

## Core damage (verified, hit handler, core routine, sum)
- After a hit is applied, if the hit section is vital-to-ship and at 0 HP (the killing hit and every later hit on it), the core loses
  a fresh roll of the bullet's damage for that material x (sum of `vitalSectionCoreDamagePercent` over ALL destroyed vital sections) x 0.01,
  clamped at 0. There is no lump sum on destruction, so keep shooting a dead vital section to finish the ship (rounding not traced).
- Section damage roll = round(uniform(lo, hi)). Damage-smoke level from remaining HP fraction: >75% none,
  50-75 Light, 25-50 Medium, 0-25 Heavy, 0 Destroyed: identical to the remake's thresholds.

### Provisional destroyed-section collision fix

Iron War's Navy torpedo boats have only 50 core HP, but their non-vital sail could continue intercepting shells after reaching zero HP: projectile collision used the original triangles regardless of section damage. The remake now skips destroyed non-vital sections for ship projectile collision, while retaining destroyed vital hull sections for the verified core-damage rule above. A clicked aim point inside a destroyed non-vital section's bounds falls back to the hull centre unless it also lies in vital-section bounds. Neither HP nor weapon damage was changed.

This is a provisional workaround, not recovered retail damaged-mesh collision: surviving spars/non-vital wreckage also become transparent to projectiles, and mouse picking still uses the original silhouette. Regression tests cover sail pass-through, retained vital collisions and aim fallback; tests and Iron War runtime acceptance are left to the owner.

## Critical hits (verified, section damage)
- Each hit on a section with a non-empty `criticals` list rolls once against the bullet's per-material `Chance of Critical Damage`;
  destroying the section always yields one. The system is picked uniformly from that section's list and its hit count increments.
- Hit counts are only read by the debug overlay; the gameplay effect is crew speech for the player's ship on section
  loss (by system type). No physics/gun effect found, so the remake does not model criticals.

## Path end behaviour (partial)
The ship movement routine (ctor, throttle update) defaults to "stop at end" (= 1, throttle clamp 0..1). Its state
(1 steer, 2 turn, 3 arrive) goes to 3 near the path end, where throttle ramps down, and it is 0 once the path is exhausted. So a retail
ship brakes to a halt at the last point of a `to end` path instead of sailing on.

Decoded follow-up (waypoint test = clamped linear interpolation):
- Throttle starts at the current speed fraction (|v| / max speed, 0..1). Heading error to the current point above pi/16 → turn state:
  throttle held at that fraction, at least 0.1. Otherwise full throttle (1.0) while the path runs.
- "Arrive" (state 3) is entered when the distance to the last point is <= the stopping-distance helper, or the path is done.
   builds `0.5·f·t²` style terms that cancel (result ~0 up to float rounding), so in practice arrive starts only when the path ends.
  In arrive the throttle ramps 1.0 → 0.1 between 2R and R from the last point while the path is live, and is 0 once it is done; then clamped 0..1.
- The last point of a `to end` path counts as reached inside 2·R (R = entity →, a per-ship radius; also maxed over groups);
  intermediate points use a passing (bisector-angle) test instead.
- Remake: `rules::mission_ai` drops the last point inside 2 × `class.radius` (PROVISIONAL stand-in for R) and sets All Stop; the existing
  drag then brakes the ship. The turn-state throttle hold is not modelled. A finished remake chase stops the same way (not retail-derived).
  The human's own move orders still coast on (`ship.rs`); the command ship is halted during the opening briefing (`briefing.rs`).
