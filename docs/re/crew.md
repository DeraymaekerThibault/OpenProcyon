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
- **The factor:** c(1) = 1, c(5) = 1.178, c(9) = 1.334: the saved per-post factor (below) divided by the uncrewed 1.125.
- **Riggers** changed neither top speed nor turning nor the acceleration time constant (0.50 s for every crew), despite the
  manual's "acceleration" wording.
- Cross-check: an earlier skirmish with auto-assigned crew gave Full x1.267, i.e. c for an engineer of about 7 (crew not recorded).

## Remake
- `Ship::crew_speed` / `crew_turn` = mean over the post's slots of `crew_arms::skill_factor` (the per-crewman line above / 1.125,
  empty slots 1) for ships with campaign posts.
- Gunners: every ship's burst spread factor is `1 - 0.8 x gunnery` of the gun's own bank slot (a rating-1 hand when empty), and
  reload shortens by 10 x gunnery seconds on the player's ships ([gunnery.md](gunnery.md#gunners), measured).
- Computer ships use their crew for damage control, fire-fighting and gunnery (no flat stand-in): skirmish ships their bought crew,
  placed ships the generic crew of their level in every slot, with the usual leadership draw.

## Saved crew factors
Saves made mid-battle store per ship an engineer and a navigator factor. Measured (headless skirmishes, 2026-10):

| Ship | Post crew (rating) | Saved engineer / navigator | Measured |
|---|---|---|---|
| Navy Cutter, uncrewed | — | 1.125 / 1.125 | Full 0.5625 × MaxSpeed, turn torque × 1.125 |
| Man-O-War, helmsman only | Navigation 9 | 1.125 / 1.500 | speed unchanged (2.340); turn 4.80°/s Full, 3.59 Emergency (formula: 4.80, 3.60) |
| Man-O-War, engineers only | Engineering 9, 9, 8 | 1.491667 / 1.125 | Full 3.106, Emergency 6.209 (0.5 × 4.1667 × 1.4917 = 3.108) |
| computer Carrack | Engineering 7, Navigation 7 | 1.425 / 1.425 | Emergency 8.71 (6.111 × 1.425 = 8.708) |

- **Per crewman:** factor = min(1.075 + 0.05 × rating, 1.5): rating 1 (the placeholder hand that fills every empty slot) 1.125,
  5 → 1.325, 7 → 1.425, 8 → 1.475, 9 → 1.5 (capped). It reproduces crew.md's Cutter speeds (rating 5: 1.325 / 1.125 = 1.178).
- **Per post:** the mean of its slots' factors, empty slots included at 1.125 ((1.5 + 1.5 + 1.475) / 3 = 1.491667 exactly; the
  mean rating 8.67 would give 1.508).
- **Speed:** Ahead Full = 0.5 × WOT MaxSpeed × engineer factor; Emergency twice that. The speed bands of the turn formula use the
  uncrewed MaxSpeed (the engineers' Man-O-War at Full, 0.745 × MaxSpeed, falls in the top band: 2.68°/s measured and predicted).
- **Rudder:** torque × navigator factor, linear, the same for every ship, human or computer, at any difficulty (unchecked against
  play, 2026-10-31). The factor scales the rudder term only; the speed bands (on the uncrewed MaxSpeed) and the
  rotational drag follow, so a steady turn grows with the square root of the factor. The engineer factor scales thrust, so it
  multiplies top speed and the first acceleration alike; drag is not scaled, so the time constant does not change. Computer ships
  still turn harder: not the crew, but the computer helm's torque boost
  ([movement.md](movement.md#computer-helm-read-from-the-originals-code-2026-10-31)).
- **Leadership** (unchecked against play, 2026-10-31): each ship draws two chances in [0, 1) once when it is crewed. Every
  crewman but the captain and the first mate gains 0.1 skill (on the 0-1 scale, capped at 1) when the captain's Leadership / 10
  beats the first chance, and 0.05 when the first mate's beats the second. So a Leadership-7 captain lifts the crew 70 % of the
  time, fixed for the battle. The bonus counts wherever a crewman's skill does: gunnery accuracy, sail and hull repair,
  fire-fighting, boarding strength and spotting cloaked ships. The saved speed, rudder and sight factors are worked out before
  the draw, when both bonuses still apply, which is why they always equal min(1.075 + 0.05 x rating, 1.5) (rating/10 + 0.15
  through the 1 + 0.5 x skill rule) and why the Carrack's Leadership-7 captain changed nothing there.

## Computer fleets carry named crew; the missing-post rule (14 retail battle saves, 2026-10)
Saves taken seconds into skirmishes (seven maps; budgets and fleet limits varied) show every
computer ship with named crew from the roster (`Crew_Navy_*`, even for Pirate and Procyon hulls), bought with the same victory points as the hulls:
the first ship is crewed in full (gunners, captain, first mate, navigator, engineers, riggers, fighters, lookout) and later ships get what is left, so
the last ones are mostly empty posts (empty slot = the rating-1 hand). Each ship's saved engineer and navigator factor is reproduced exactly from its
crew (62 of 64 ships): the per-crewman value above, averaged over the post's slots, **and when the class has no such post at all (a Sloop has no engineers,
a Gunboat no navigator) the factor is 1 + 0.05 x a stand-in's rating in that skill** (no 0.075; no stand-in counts as rating 1: 1.05). The stand-in is
the first mate (failing that the captain) for the navigator, fighter and lookout posts, and the captain for the others (unchecked against play).
The two misses are Pirate ships under a Crag captain whose engineering is not listed in the data (the saved 1.30 reads as rating 6).
This is also where the generic levels' floors come from: a Green / Average / Elite captain is rated 3 / 6 / 8, so a postless factor is 1.15 / 1.30 / 1.40.
Scan range follows the same rule on the lookout post and spotting: 712.5 = 500 x 1.425 (lookout rated 7), 525 = 500 x 1.05 (no lookout post, empty captain).
Buying order (2026-10-31, budgets re-read): all hulls and weapons first, then crew ship by ship; 13 of 15 fleets spent their 300 or 340 points
to the point ([skirmish-setup.md](skirmish-setup.md)). Not resolved: the choice of crewman (257 of 319 picks rated exactly 7 in the post's skill, gunners 8-9).

## Crew losses and the crew-class gate (unchecked against play, 2026-11)
- **A battle takes no crew away.** Nothing that runs from a shell hit, a fire or a destroyed section adds, removes or replaces a ship's crewmen; the only runtime change of a ship's
  crew is a capture (a fresh generic crew with the old per-post counts, boarding.md section 4). Boarding casualties are strengths, not crewmen. Remake: same (posts are static).
- **Crew class per post.** The computer fleet's crew pick (skirmish-setup.md) only seats a man whose crew class may fill the post group. The table has 18 classes by 8 groups
  (Gunner, Captain, First Mate, Navigator, Engineer, Rigger, Fighter, Lookout; 1 = allowed):
  0: 01111101, 1: 00001101, 2: 10011111, 3: 00011001, 4: 01111001, 5: 10001010, 6: 01101001, 7: 10000111, 8: 01111111, 9: 11111111, 10: 11111111,
  11: 11110010, 12: 10001110, 13: 11110111, 14: 00001101, 15: 11111111, 16: 10000111, 17: 00000010 (class 17 may only be a fighter).
  The class is most likely the crewman's species (the retail crew data has exactly 18 species), but which species is which class number is **not established**, so the table cannot be
  applied yet. Remake: no gate (PROVISIONAL gap).

## Open
- Placed and computer ships' generic crews do get crew factors. Retail saves (missions 3 and 11) store per ship an engineer and a
  navigator factor: Green crews 1.15, Average 1.30-1.37, Elite 1.40-1.47, default (rating-1) hands 1.05-1.10, i.e. about
  1 + 0.05 × skill plus a small extra. Over 384 factors of generic-crew ships in 17 retail saves (2026-10) the generic crews
  (skill 3 / 6 / 8 in every skill, `Crew_General_*`) give values on a 0.025 grid between 1 + 0.05 × skill (the floor: 1.15 /
  1.30 / 1.40) and the floor + 0.075 (1.225 / 1.375 / 1.475, the per-crewman value above): means 1.163 / 1.326 / 1.441, Green
  mostly at the floor (61 of 90), Average 101 of 200, Elite split between floor and top. Engineer and navigator differ within
  a ship; which slots are filled (the spread) is not resolved. Measured speed factors are relative to the uncrewed ship (c(1) = 1), whose saved factor
  is 1.125, so speed follows the saved factor itself (see "Saved crew factors" above).
  Computer skirmish ships ran in at 1.267 × Emergency, between this curve's Average (1.219) and Elite (1.296).
  Remake: mission ships take the middle of their crew level's saved range / 1.125 for speed and rudder
  (`boarding::Skill::handling`); computer skirmish ships use the measured 1.267.
- Spotting range. (Repair Rate: refit-resupply.md 3.4; rate of fire: gunnery.md#gunners.)

## Engineers lower the critical-hit chance of their own ship (unchecked against play, 2026-11, not yet measured in play)
For every section a hit reaches, including the neighbours that take the 0.4 spill, the chance of a critical on that section is
(1 - E) x the bullet's per-material `Chance of Critical Damage`, held between 0 and 1, where E = the mean over the ship's engineer slots
of the per-crewman skill factor (the factor in "Per crewman" above, 1.125 for an empty slot up to 1.5) minus 1.0. A default crew gives E
of about 0.125 (chance x 0.875), a top engineer crew 0.5 (chance x 0.5). Criticals only feed a tally and crew speech (movement.md,
"Critical hits"), so the effect is on speech frequency. The factor belongs to the struck ship's own engineers, for any attacker, and
applies to scripted and nebula hits as well. Remake: criticals are not rolled (speech tags fire at section death, `crew.rs`), so there is
nothing to scale. Build: small once a critical roll exists (`combat/shells.rs`, `combat/gunnery.rs` hit rules); not built before then.
UNSURE: an engineer post with no slots (count 0) would give E = -1 (chance x 2, clamped); not read to the end.
