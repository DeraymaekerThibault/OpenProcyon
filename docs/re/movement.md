# Ship movement: what the original original does (own-words notes)

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
Consequence for the cutter (full rudder) at the measured order speeds: about 12.8°/s at both Slow and Full; measured 13.6°/s (below).

## Linear motion
- Drag is linear (not quadratic) with rate `(MaxThrust/Mass)/MaxSpeed`; terminal speed = throttle × current max speed.
- Lateral/vertical velocity is damped separately (constants 0.5 and 0.3).

## Measured on the running original (Navy Cutter, skirmish, no crew)
Sampled ~60 times a second from the running retail game (Steam/Proton) while issuing each order; the game itself updates
about every 0.034 s. Values are the cutter's; other classes are assumed to scale the same way (PROVISIONAL).
- **Order speeds:** All Stop 0, Ahead Slow 2.266, Ahead Full 4.531, Emergency Speed 9.062 units/s, i.e. ½ / 1 / 2 × Full,
  and Full = 0.5625 × WOT MaxSpeed (Emergency = 1.125 × MaxSpeed). Emergency held its speed for the 15 s it was used.
- **Crew raise top speed and turning:** engineers scale Full, the helmsman scales rudder torque; see [crew.md](crew.md).
- **Time scale:** every order change approaches the new speed exponentially with time constant 0.53 s, matching the unscaled
  WOT rate above (0.537 s for the cutter). All Stop uses the same curve.
- **Coming to rest:** when the braking speed would drop through 1.0 it becomes 0 in one tick (1.05 → 0). Full to rest takes
  about 0.83 s. Whether a slow push (collision, tow) is also snapped is not observed; the remake snaps only braking.
- **Turning:** full rudder gives a steady 13.6°/s at both Slow and Full (turning circle about 19 units across at Full, 9.5 at
  Slow). The turn formula above predicts 12.8°/s; the measured rate needs its torque × 1.125 (PROVISIONAL: the same factor as
  Emergency speed, source not identified). A click inside the turning circle makes the ship circle it, as in the remake.
- **Banking:** the hull rolls into the turn (starboard up in a port turn) up to 0.25 rad (14.3°) at 0.06 rad/s (about 4 s to full
  lean), holds it while the rudder is over and levels at the same rate. On a straight course the heading weaves ±2° and the roll
  ±3.5° with a ~3.8 s period; at Emergency Speed there is no roll. Remake: roll target = 0.25 × rudder, rate-limited; the weave
  and the Emergency exception are not modelled.

## Other constants
0.33 / 1.0 / 5/3 / 1/3 (speed bands), 60.0 (α clamp), 0.1 (rudder-speed constant), 0.5 and 0.3 (lateral terms); 0.25 is the
measured bank limit above.

## Damage → movement (verified, ship update, counter)
- Damage sections carry four flags at.. (vitalToShip, vitalToMaxVelocity, vitalToManeuverability, vitalToMission); HP int at.
- Every frame: physics = min(destroyed / total vitalToMaxVelocity sections, 0.9); = same for vitalToManeuverability.
   is one of the four "(1 − x)" factors on max speed/thrust; is the `r` in the rudder term (1 − r) (read at).
  Player speech fires when the sail loss crosses 25/50/75/100%.
- Unknown: (1 − product of (1 − x) over some list, crew-like),, (0.6 × a timed progress, state-dependent), (turn multiplier from crew).

## Core damage (verified, hit handler, core routine, sum)
- After a hit is applied, if the hit section is vital-to-ship and at 0 HP (the killing hit and every later hit on it), the core loses
  a fresh roll of the bullet's damage for that material x (sum of `vitalSectionCoreDamagePercent` over ALL destroyed vital sections) x 0.01,
  clamped at 0. There is no lump sum on destruction, so keep shooting a dead vital section to finish the ship (rounding unknown).
- Section damage roll = round(uniform(lo, hi)). Damage-smoke level from remaining HP fraction: >75% none,
  50-75 Light, 25-50 Medium, 0-25 Heavy, 0 Destroyed: identical to the remake's thresholds.

## Player vs computer damage factor (measured, retail headless skirmishes 2026-10-16)
- 1-vs-1 skirmishes on The Diablo Strait (Navy Man-O-War vs one computer Pirate Barque or Carrack), one per difficulty
  level, section HP logged at 20 Hz on both ships. Drops match the bullet table scaled by side and the Single Player
  Difficulty Level (the retail default is Normal):

  | Difficulty | player shells on computer ships | computer shells on the player's |
  |---|---|---|
  | Easy | x 4 (two hits only: heavy cannon on reinforced wood, table 35-45, dropped 155 and 156) | x 0.25 (wood 3-13) |
  | Normal | x 2 (medium cannon on cloth, table 14-15, dropped 28-29 five times; heavy on reinforced 72-89) | x 0.5 (gattling on wood, table 4-7, dropped 2 or 3) |
  | Hard | x 1 (cloth 5-7 / 14-19, reinforced 28, 43) | x 1 (cannon on wood 30-48, cloth 15) |
- The scaling applies before rounding (Normal gattling drops of 2 and 3 come about 1:2, as round(0.5 x roll) gives).
- PROVISIONAL: Easy x 4 rests on two hits. Computer-vs-computer and allied hits are not measured (the engine uses 1.0).
  Whether the core bleed roll is scaled too is not measured (the engine scales it).
- Volcano meteors (The Diablo Strait) hit every section of a ship at once for 10-20 each; they are not shells.
- Damage control mends hulls only through engineers and sails through riggers (refit-resupply.md 3.4).

### Provisional destroyed-section collision fix

Iron War's Navy torpedo boats have only 50 core HP, but their non-vital sail could continue intercepting shells after reaching zero HP: projectile collision used the original triangles regardless of section damage. The remake now skips destroyed non-vital sections for ship projectile collision, while retaining destroyed vital hull sections for the verified core-damage rule above. A clicked aim point inside a destroyed non-vital section's bounds falls back to the hull centre unless it also lies in vital-section bounds. Neither HP nor weapon damage was changed.

This is a provisional workaround, not recovered retail damaged-mesh collision: surviving spars/non-vital wreckage also become transparent to projectiles, and mouse picking still uses the original silhouette. Regression tests cover sail pass-through, retained vital collisions and aim fallback; tests and Iron War runtime acceptance are left to the owner.

## Critical hits (verified, section damage)
- Each hit on a section with a non-empty `criticals` list rolls once against the bullet's per-material `Chance of Critical Damage`;
  destroying the section always yields one. The system is picked uniformly from that section's list and its hit count increments.
- Hit counts are only read by the debug overlay; the gameplay effect is crew speech for the player's ship on section
  loss (by system type). No physics/gun effect found, so the remake does not model criticals.

## Path end behaviour (partial)
Ship path following defaults to "stop at end" (= 1, throttle clamp 0..1). Its state
(1 steer, 2 turn, 3 arrive) goes to 3 near the path end, where throttle ramps down, and it is 0 once the path is exhausted. So a retail
ship brakes to a halt at the last point of a `to end` path instead of sailing on.

Follow-up (waypoint test = clamped linear interpolation):
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
