# Dragons (Animal_SpaceDragon, `Dragon - *` rules)  -- DRAFT 2 (data plus a static decode of the AI)

Sources: retail `WorldObjects/Animal_SpaceDragon.wot`, `Gun_DragonBreath.wot`, `Bullet_DragonBreath.wot`, mission TWT files, rule JSON in
`assets/maps/mission0{6,7,8}/mission.json`. Mission 13 only has *Island_Dragon_01* scenery (no dragon creatures); its `BTL_Dragon02_FullFast` is just music.

## Where dragons are
The campaign exporter (`tools/export_campaign.py`) only exports `Ship_*` and `Animal_SpaceWhale` objects, so **dragons are missing from the mission JSON**.
They are `Animal_SpaceDragon` objects in the TWT (id, position incl. z 16..40, orientation, PlayerIndex -1), grouped by the TWT `Group` blocks:

| Mission | Group -> object ids (position x,y,z) |
|---|---|
| 6 | `SpaceDragons1` 256 (-1135,-80,17), 551 (-1129,-178,19); `SpaceDragon2` 473 (-1031,-210,19), 552 (-1048,-85,16); `SpaceDragonsNIS` 260 (-716,-91,19); `NewDragons` 593 (1233,459,28), 594 (1270,517,28) |
| 7 | `Dragons 1` 389 (-149,472,40); `Dragons 2` 390 (118,463,29); `Dragons 3` 405 (-135,-424,32); `Dragons 4` 406 (-117,-50,25) |
| 8 | `Dragons` 365 (-15,1026,33), 366 (215,495,29) |

PLAN item: extend the exporter to emit `environment.dragons` like `environment.animals`, with group names.

## Unit data (Animal_SpaceDragon.wot)
AI factory `DragonAI` (default sight range 500), physics `DragonPhysics`: mass 1000, max thrust 15000, **max speed 12**, rotational friction 20, max angular accel 6.
Render scene `AnimalSpaceDragon` (mesh `SpaceDragon_Layer1`), one mouth gun mount `MouthGunMount` -> `Gun_DragonBreath` (45, Bank00).
Single damage section: material `Dragon Scale`, **500 hit points**, not vital to anything. Collision: bounding render entity, response type Ship.
Custom info: roar sounds `Animal_SpaceDragon_01..06` every 7..13 s random (range 700); reports spotting type `SPOTTER_DRAGONS` (crew line) ; selected indicator 0.4 of radius.

### Gun_DragonBreath
Heavy cannon scene mount, bullet `Bullet_DragonBreath`, muzzle speed 50, one shot per burst, **6 s between bursts**, gravity-affected projectile, h/v traverse 1.134 rad/s,
elevation +-60 deg (1.047 rad), ranges effective 150 / long 270 / max 350, accuracy deviation 0.052 / 0.070 / 0.087 rad, "can fire while cloaked" no, victory point cost 10, sound `Animal_SpaceDragon_Fireball`.
### Bullet_DragonBreath
Bullet lifetime 10 s, effects `ArcturianFire_Heavy_Bullet(_Impact)`, "Damage HitPoints 4". Per-material damage (chance crit / chance fire / initial fire strength / damage min-max):
Wood 0.14/0.99/4/30-40; Reinforced 0/0.8/3/20-35; Iron 0/0.1/1/5-10; Cloth (sails) 0.5/1.0/5/40-50; Stone 1/1/1/10-15; Dragon 0.5/0.5/0.5/10-15.
Impact sound `Animal_SpaceDragon_Impact`.

## Rules (all in `rules/` currently warned/ignored)
Actions: `Dragon - Set AI Stance` (group/unit, stance string), `Dragon - Set Damage Threshold` (group, float 0..1), plus generic ones that dragons rely on:
`Set Group/Unit Border Zone` (polygon), `Set Group/Unit Border Zone Return Point` (point set), `Set Group/Unit Owner`, `Set Group/Unit Visibility`, `SetCollidable`,
`Teleport Group/Unit`, `Group to follow path`, `Set Group/Unit Hold Position`, `GroupA to attack GroupB`, `Set alliance between PlayerA and PlayerB`.
Conditions: `All Units from Group have entered trigger volume Once` (Group, Volume, Player Name, "Object that have already entered - Size"), `Is Group A attacking Group B`,
`Player has hit Group/Unit at least x Times`, `Group to Group Distance`, `Is Group In Volume`, `Exit Volume`.

Numeric parameters used (damage threshold): m6 0.75 (SpaceDragons1/2/NIS), 0.65 (NewDragons); m7 0.25 (Dragons 1/2), 0.85 when attacking pirates, 0.65 (3/4); m8 0.2.
Stances used: `Stance Persistant`, `Stance_Dummy`, `Stance_Aggressive`, `Stance_Neutral`, `Stance_Defensive`.


## Behaviour (decoded from the original's AI; implemented in `game/src/dragons.rs`)
**Damage.** Every bullet type has a `Dragon` material column, so any gun hurts it. Heavy/medium cannonballs and carronades do 20-60 per hit,
plasma 25-45, harpoon 50-80, lancers 5-16, gattling 5-7. Light cannonball, torpedo, mines, grav charge, energy net and dark matter carry 0.5 in
every field (an unset default: effectively harmless). HP lost = round(roll in lo..hi) x the difficulty factor of a player shell on a computer
victim (Easy x3, Normal x2, Hard x1; not measured on a dragon). Ramming hurts it like any ship. Anything not allied to the dragon's player is hostile
(missions toggle the alliance: mission 7 after docking, mission 8 "Dragons Go Friendly").

**Damage threshold.** `Set Damage Threshold` is the fraction of HP *lost*: the dragon breaks off while `(max - hp) / max >= threshold`
(default 1.0). The test is redone every tick: no latch, no healing, no memory, so a rule that raises the threshold puts a fleeing dragon back in combat
at once (mission 7: 0.25, later 0.85). While past it the dragon never attacks (Dummy aside). It flies at full speed (12) to the border-zone
return point plus a random planar offset under 10 units, re-drawn every tick, so it loiters there and never arrives; the border-zone clamp does not apply.
Nothing despawns it: mission 6's drive-off works because rules hide dragons that enter a removal volume.

**Stances.** The default is Aggressive. Dummy idles and drops its target. Defensive (and any unknown name) never scans: it wanders and attacks only a
target a rule gave it. Aggressive, Persistent and Neutral scan every tick for the nearest hostile live ship by surface distance within the sight range
(500), replacing the current target; with nothing in range the old target stays. Persistent keeps following its scripted path and never uses the attack
routine. Nothing reacts to being hit: a dragon does not turn on whoever shot it. With no target it follows a scripted path, else wanders (new waypoint
100-200 units away in a random direction).

**Attack routine.** No stand-off: it flies to the target's current position (through it), then to a point 150 units beyond, and starts again after 20 s or
once the surface distance is 150 or more.

**PROVISIONAL / unknown.** Death at 0 HP (no death code found; we still explode and hide it, and the flee test keeps a 0 HP dragon fleeing at the default
threshold); whether its core HP enters the fraction; breath firing conditions (we fire when in range and facing); what the extra Persistent/Neutral target
filter and object categories 9-10 are; the flee goal when no return point is set; player click/auto-target rules; the direction of the point 150 beyond
(we use the target's heading); the ship radius in the surface distance (we assume 8).
