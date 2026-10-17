# Crew posts and what they change

Sources: the retail Crew Select screen (stat bars observed while moving one veteran between posts), the manual's crewing page,
the retail crew files (`Crew_*.wot`), and the running original sampled while sailing four Navy Cutters with different crews.

## Skills and posts
Seven skills, each 0-10: Leadership, Navigation, Spotting, Engineering, Rigging, Combat, Gunnery. A veteran has up to four or
more active skills; a default crew member is rated 1 in every skill. The generic crews named by a placed ship's
`Crew Skill Level` (`Crew_General_*`) are rated 3 / 6 / 8 in every skill but Combat (1 / 3 / 4) for Green / Average / Elite.

## Crew Select stat bars
Four bars: Maneuverability, Speed, Repair Rate, Strength. Each bar has a fixed part from the hull (green) and a crew part (red).
Moving one veteran (Leadership 5, Navigation 6, Engineering 7, Rigging 2, Combat 1, Gunnery 2) through the Cutter's posts:

| Post | Bar that moved |
|---|---|
| Helmsman (Navigator) | Maneuverability, from his Navigation |
| Engineer | Speed and Repair Rate, from his Engineering |
| Rigger | Repair Rate only, from his Rigging (Speed unchanged) |
| Fighter | none (his Combat equals a default hand's) |
| Captain, First Mate, Gunner | none |

So: Maneuverability = helmsman's Navigation; Speed = engineers' Engineering; Repair Rate = engineers' Engineering together with
riggers' Rigging; Strength = Combat. Leadership and Gunnery show on no bar.

## Measured effects in battle (Navy Cutter, Ahead Full, full rudder)

| Engineer and helmsman skill | Ahead Full | Full-rudder turn |
|---|---|---|
| 1 (default crew) | 4.530 | 13.62 deg/s at 4.53 |
| 5 | 5.336 | 15.61 deg/s at 5.33 |
| 9 | 6.041 | 9.99 deg/s at 6.04 (above 0.67 x MaxSpeed: the hull's high-speed band divides torque by 3) |
| 1, both riggers at Rigging 9 | 4.530 | 13.63 deg/s |

- **Top speed:** Ahead Full (and every order, which are fixed fractions of it) is multiplied by a crew factor `c` of the engineers'
  Engineering. Emergency Speed is still exactly twice Full (12.08 and 10.67 measured).
- **Turning:** the rudder torque is multiplied by the same factor `c` of the helmsman's Navigation (steady rate from the turn
  formula in [movement.md](movement.md)). In the test both posts had the same skill; the stat bars tie speed to the engineer and
  turning to the helmsman.
- **The factor:** c(1) = 1, c(5) = 1.178, c(9) = 1.334. Its square grows linearly with skill:
  `c = sqrt(1 + 0.972 x (s - 0.1))` with `s` = skill / 10. PROVISIONAL: shape fitted to three points.
- **Riggers** changed neither top speed nor turning nor the acceleration time constant (0.50 s for every crew), despite the
  manual's "acceleration" wording.
- Cross-check: an earlier skirmish with auto-assigned crew gave Full x1.267, i.e. c for an engineer of about 7 (crew not recorded).

## Remake
- `Ship::crew_speed` / `crew_turn` = c(mean skill of the post's slots, generic hands 0.1) for ships with campaign posts.
  PROVISIONAL: averaging several engineers is not measured (the Cutter has one of each).
- Gunners: accuracy `1 - 0.8 x gunnery` as before, and reload shortened by 10 x gunnery seconds on the player's ships
  ([gunnery.md](gunnery.md#gunners), measured).

## Saved crew factors
A save made mid-battle (Zemyatin skirmish, our uncrewed Navy Cutter against a computer Carrack) stores per ship an engineer and a
navigator factor: our Cutter 1.125 / 1.125, the Carrack 1.425 / 1.425.
- **Speed:** Ahead Full = 0.5 × WOT MaxSpeed × engineer factor. Uncrewed: 0.5 × 1.125 = 0.5625 × MaxSpeed, as measured on the
  Cutter and the Man-O-War ([movement.md](movement.md)); the Carrack: Emergency 2 × 0.5 × 6.111 × 1.425 = 8.71, as measured. So
  the remake's relative factors are the saved factor / 1.125 (computer skirmish ships 1.267).
- **Rudder:** the uncrewed turn needs torque × 1.125 (= the navigator factor) on the decoded formula. The computer Carrack's
  circling needs (1.425 / 1.125)² on top of that ([movement.md](movement.md#computer-captains-measured-2026-10)).
- PROVISIONAL: the skill → factor curve for player crews in absolute terms (c(5) = 1.178 relative would be 1.325 saved).

## Open
- Placed and computer ships' generic crews do get crew factors. Retail saves (missions 3 and 11) store per ship an engineer and a
  navigator factor: Green crews 1.15, Average 1.30-1.37, Elite 1.40-1.47, default (rating-1) hands 1.05-1.10, i.e. about
  1 + 0.05 × skill plus a small extra. Measured speed factors are relative to the uncrewed ship (c(1) = 1), whose saved factor
  is 1.125, so speed follows the saved factor itself (see "Saved crew factors" above).
  Computer skirmish ships ran in at 1.267 × Emergency, between this curve's Average (1.219) and Elite (1.296).
  Remake: mission ships take `skill_factor` at their crew level's skill (Green 3, Average 6, Elite 8) for speed and rudder
  (`boarding::Skill::handling`); computer skirmish ships use the measured 1.267. PROVISIONAL: the navigator's (rudder) share
  for crewed player ships.
- Leadership (manual: "improves the effectiveness of your entire crew"): no bar shows it; not measured.
- Spotting range. (Repair Rate: refit-resupply.md 3.4; rate of fire: gunnery.md#gunners.)
