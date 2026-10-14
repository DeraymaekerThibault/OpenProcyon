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
- Gunners: accuracy `1 - 0.8 x gunnery` as before ([gunnery.md](gunnery.md)); the manual also promises a faster rate of fire,
  not measured.

## Open
- Whether placed ships' generic crews (Green/Average/Elite, skill 3/6/8) get the same factor; the remake applies none yet.
- Leadership (manual: "improves the effectiveness of your entire crew"): no bar shows it; not measured.
- Repair Rate in battle (only the bar mapping is known), rate of fire, Spotting range.
