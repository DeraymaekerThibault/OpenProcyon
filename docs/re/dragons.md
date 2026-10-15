# Dragons (Animal_SpaceDragon, `Dragon - *` rules)  -- DRAFT 1 (data-derived; exe pass pending)

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

(more to follow: AI state machine, mission walk-through, implementation plan)
