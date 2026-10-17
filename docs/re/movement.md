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
about every 0.034 s. Values are the cutter's; a Man-O-War checks them on a second class (below), the other classes are
assumed to scale the same way (PROVISIONAL). A Man-O-War run (2026-10) agrees: its approach after a click and "+" fits 4.687 = 1.125 x MaxSpeed (Emergency) with time constant 5.54 s
(WOT MaxSpeed x Mass / MaxThrust = 5.556), and All Stop snaps to 0 the tick its speed would fall below 1.0, as on the cutter.
A move click from All Stop with no key pressed runs the cutter at its Ahead Full (4.531).
- **Order speeds:** All Stop 0, Ahead Slow 2.266, Ahead Full 4.531, Emergency Speed 9.062 units/s, i.e. ½ / 1 / 2 × Full,
  and Full = 0.5625 × WOT MaxSpeed (Emergency = 1.125 × MaxSpeed). Emergency held its speed for the 15 s it was used.
- **Crew raise top speed and turning:** engineers scale Full, the helmsman scales rudder torque; see [crew.md](crew.md).
- **Time scale:** every order change approaches the new speed exponentially with time constant 0.53 s, matching the unscaled
  WOT rate above (0.537 s for the cutter). All Stop uses the same curve.
- **Coming to rest:** when the braking speed would drop through 1.0 it becomes 0 in one tick (1.05 → 0). Full to rest takes
  about 0.83 s. Whether a slow push (collision, tow) is also snapped is not observed; the remake snaps only braking.
- **Turning:** full rudder gives a steady 13.6°/s at both Slow and Full (turning circle about 19 units across at Full, 9.5 at
  Slow). The turn formula above predicts 12.8°/s; the measured rate needs its torque × 1.125 (the same factor as Emergency
  speed; its source is not identified, but it holds on two classes and three speeds, see the table). A click inside the
  turning circle makes the ship circle it, as in the remake.
- **Full-rudder turn rate, formula with torque × 1.125 vs measured** (Diablo Strait skirmish, no crew, steady state):

  | Ship | Order | Speed | Predicted | Measured |
  |---|---|---|---|---|
  | Cutter | Ahead Full | 4.525 | 13.62°/s | 13.62°/s |
  | Cutter | Emergency | 9.056 | 9.71°/s | 9.71°/s |
  | Man-O-War | Ahead Full | 2.341 | 4.16°/s | 4.16°/s |
  | Man-O-War | Emergency | 4.684 | 3.11°/s | 3.11°/s |

  Emergency turns slower because its speed falls in the top band (α × 1/3). The Man-O-War's Ahead Full is 2.341 (0.5625 ×
  MaxSpeed = 2.344), Emergency 4.684 (1.125 × MaxSpeed = 4.687).
- **Banking:** the hull rolls into the turn (starboard up in a port turn) up to 0.25 rad (14.3°) at 0.06 rad/s (about 4 s to full
  lean), holds it while the rudder is over and levels at the same rate. While a ship steers for a clicked point straight ahead
  its heading weaves: cutter ±2.2° (roll ±3.5°) with a 3.8 s period, Man-O-War ±0.45° with an 11.6 s period. Once it has
  passed its point and holds a course, the heading is dead steady on both (no weave in 30+ s). PROVISIONAL reading: the weave
  is the course controller hunting for the point, not a straight-course wobble. At Emergency Speed there is no roll. Remake: roll target = 0.25 × rudder, rate-limited; the weave
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

## Computer captains (measured, 2026-10)
Headless retail skirmishes, one computer ship against our idle or fleeing Navy Cutter / Man-O-War, every ship logged at ~30 Hz.
- **Crew:** computer skirmish ships run in at exactly 1.267 × Emergency Speed (Carrack 8.71, Barque 11.08 units/s). A save of
  the battle explains it: the Carrack's engineer and navigator factors are 1.425, our uncrewed Cutter's 1.125, and
  1.425 / 1.125 = 1.267 ([crew.md](crew.md#saved-crew-factors)). A patrolling Assault Cutter sailed at 8.56 (0.98 × Emergency).
- **Approach:** at that crewed Emergency Speed, aiming off the bow toward a tangent of the circle it will fly (we sat 17° off its
  bow at 330 units, 48° at 100, 66° at 60).
- **Circling:** from ~60-75 units it turns onto a circle around the target and holds it, the target on one beam (±90°, the side it
  approached on: port in two runs, starboard in one). Carrack 54 units (Man-O-War target; 36-70 spread while the target moved),
  Barque 55.7 (51-58) around an idle Cutter, turning a steady 8°/s; an Assault Cutter ~95 (60-120, with a Barque); a Procyon
  Minelayer a perfect 109-113 for 10 minutes; a Procyon Frigate kept us on its beam (55-125°) on a fixed 6°/s turn while we moved,
  so its distance swung 43-131. Radii do not follow a gun range (Barque and Minelayer both carry the 110-range medium cannon). Speed while circling: 0.66 (Carrack 5.74) and 0.74 (Barque
  8.2) of the run-in speed. A Carrack around an idle Cutter (Zemyatin, Easy, saved three times while circling): circle centred on
  us, radius 50 → 46 → 45 over two minutes, us at 89-90° on its port beam, 6.47 units/s, 8.23°/s.
- **What makes the circle (save + observation):** the circling ship runs a broadside routine (saved side flag "attack right" =
  false for the port-beam Carrack). Each tick it lays a short path from itself to a point R beside the target (the line of sight
  turned 90° toward the chosen beam, R units out from the target) and on to a point R beyond the target, R being the mean of one
  range value over the ship's guns. Steering for a point that far out (R ≫ the circle) pulls the ship inward until it turns as
  hard as it can, so it ends on a circle about its own tightest turn at that speed, centred on the target; the radius then
  drifts slowly. The approach bearings above fit it: R = d × tan(bearing off the bow) gives 101, 111 and 135 units. The saved
  random attack distance (Carrack 247.7, unchanged across the three saves) is not the radius.
- **Circling speed:** the path's saved minimum throttle × the crewed top speed: Carrack 0.745 × 8.71 = 6.49, measured 6.47; the
  Carrack and Barque ratios above (0.66, 0.74) lie in the saves' 0.65-0.75 range.
- **Computer turn rate:** the circling Carrack's 8.23°/s at 6.47 needs rudder torque × 1.605 on top of the uncrewed formula,
  the square of its saved navigator factor relative to ours (1.267² = 1.605 predicts 8.25°/s; × 1.267 would give 7.33).
  PROVISIONAL: one ship with its own save; why the computer's rudder takes the factor squared is not known.
- **Assault Cutter (Zemyatin, idle Cutter target):** it did not circle: it swung back and forth across our bow at 6.3 units/s
  between 66 and 117 units for 90 s (turn reversing every ~10-20 s), then closed at Emergency and rammed our Cutter, which sank
  (the save's frontal-attack routine holds a move-away vector and timer). PROVISIONAL reading: frontal attack.
- **Hazards:** with our ship inside a black hole's radius, a Carrack circled at a steady 157-165 units from the hole's centre
  (radius 145) for 13 minutes instead of closing.
- **Storms:** on Red Rover (solar-storm nebulae) idle ships of ours lost 5,400-6,300 hull points that no player was credited with, and a
  circling Carrack broke off at Emergency Speed shortly before; the save's avoid list names nebulae. PROVISIONAL reading: the
  storm drifted over the idle ship and the computer captain kept out of it.
- **Boarding:** Barques (Red Rover, two runs) boarded and captured a Man-O-War that was manoeuvring and being fired at (though rarely
  hit): after circling at ~60 units they dropped to ~3.5 units/s and made grapple passes to 12-18 units, broke away at speed to 40-110
  units, and came back; on the third pass in one run they held 12 units for ~12 s (grappled, winched) and the ship was taken within
  ~3 minutes. A Barque circling an idle Cutter for 200 s and Carracks circling an idle Man-O-War for 2 minutes made no passes.
  The rule (harpoons fired at random once the target is in a harpoon's arc and reach, boarding at random once grappled):
  [boarding.md](boarding.md) §5.
- **Target choice:** a computer ship takes the nearest hostile object within its scan range as its target, and chooses again
  every 6 s (keeping a farther target until then) or at once when the target is gone. It circles and fires at that target, so
  in a fleet battle each computer ship goes for whichever of our ships is nearest. PROVISIONAL: the remake counts only ships as
  candidates (the original also filters some object kinds; which non-ship hostiles qualify is not checked; bases do, see below).
  Checked in a fleet battle (Zemyatin, our Man-O-War + Cutter vs a computer Assault Cutter + Barque, six saves over 28 s): both
  computer ships took our Cutter, the nearer of our two ships by 45 units, from 2.8 s on and kept it; no spreading over targets.
  Remake:
  `ship::CaptainTargets` (steering and fire).
- **Nothing in sight: map exploration.** Retail saves give every computer ship an explore-map routine with a quadrant index 0-3
  (random per ship). Observed: a Carrack on the Diablo Strait (half-size 1625) ran from (322, -1127) on course (0.27, 0.96) for
  200 s, straight at the (+x, +y) quadrant centre (812, 812) (bearing (0.245, 0.97)); an Assault Cutter on the Zemyatin Anomaly
  (half-size 1250, sight 400) shuttled at Emergency between the two upper quadrant centres (±625, 625), turning ~220 units short of
  each. Other ships came straight at us from 740-1900 units (a Barque, a Procyon Minelayer), so some captains know where the enemy is
  or head for it; what decides that is unknown.
- **Why some ships head straight for us (save, 2026-10-17):** on Border Dispute a computer Procyon Minelayer's saved target is
  the Navy base (Base_Grant, ~2000 units away, far beyond its 562.5 scan range) from the first second of the battle and still
  60 s later. So with a hostile base on the map the computer goes for the base (which sits near our start) instead of
  exploring; on maps without bases it explores. Whether that is "nearest hostile base regardless of range" or a map/player
  setting is not checked (one map). The skirmish map's bases belong to non-playable players named "navybase" / "procbase";
  the Navy base fired on the Procyon ship (r5) and not on our idle Cutter. Remake: skirmish bases have no side and do not fire;
  computer ships explore instead (PROVISIONAL gap).
- **Scan range = WOT sight × the lookout post's factor:** the Minelayer (sight 500, empty lookout) saves 562.5 = 500 × 1.125; the
  Carrack (lookout rated 7) 712.5 = 500 × 1.425 (crew factors: [crew.md](crew.md#saved-crew-factors)).
- **Other routines in the save** (per computer ship): target selection with an attack timer; a simple attack routine with a random
  attack distance (about 100-250 units) and attack side; broadside / frontal (with a move-away vector and timer) / rear attack
  routines; dock (5 s dock time), board (with a grapple flag), tow, lay-mines (lay 10 s, pause 20 s; [gunnery.md](gunnery.md#mine-laying)) and fire-on-asteroids routines;
  collision avoidance against ships, asteroids, black holes, torpedoes, mines, dragons, islands and nebulae; waypoint following with
  a random minimum throttle 0.65-0.75; a scan range = WOT sight × the crew factor.
- **Fire:** see [gunnery.md](gunnery.md) (computer guns fire without clicks; the human command ship's do not).
- Remake (`ship::captain_steer`, shared by the skirmish AI and mission attack orders): the broadside aim point above, with R =
  `Class::reach` (mean LongRange over the gun banks: Carrack 150, Barque 128.6; which of the gun's range values the original
  averages is not confirmed, PROVISIONAL), Emergency run-in, throttle 1.4 (≈0.7 × Emergency) inside 0.6 × R (PROVISIONAL: the
  original's switch is not decoded; retail slowed 60-75 units out), keeping 15 units outside black holes and damaging nebulae
  (solar storm / meteors, as circles: `nebula::storm_circles`); computer skirmish ships get crew factor 1.267 on speed and
  1.267² on rudder. Simulated circles: Carrack 47.7 units (retail 45-50), Barque 55-66 (retail 51-58). Skirmish enemies with
  nothing in sight explore (`ship::Explore`: quadrant centre, then its x-neighbour, switching 220 units short). PROVISIONAL: the
  start quadrant and visiting order, and the ships that head straight for the enemy instead.
- **Helm (decoded):** the waypoint steering puts the rudder hard over when the heading error to the current point exceeds 20°
  and proportionally (error / 20°) inside it; a path point counts as reached within 2 × the ship's radius. Remake: `ship::HELM_FULL`
  for every ship.
- **Turning at speed (measured, Navy Cutter, no crew):** full rudder 13.6°/s at Ahead Full (4.53) and 9.70°/s at Emergency
  (9.06); speed does not drop in the turn. Our turn formula with its torque / 3 band above 0.67 × MaxSpeed predicts 9.70°/s, so
  the band holds. The computer's circling turn rates then need rudder torque × 1.72 (Barque) and × 1.87 (Carrack) under campaign-save
  navigator factors (1.31, 1.37) squared; the same-ship save above gives 1.605 = 1.267². Remake: `ship::COMPUTER_TURN` = 1.267².
