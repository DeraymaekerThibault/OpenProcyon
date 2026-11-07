# Ship catalogue

The retail skirmish fleet, from the imported ship data. Points are the skirmish cost, speed is the game's km/h figure (max speed
x 3.6), turn is the maximum angular acceleration, HP is core plus every section, guns counts mounts. Roles are my reading of
the weapons and crew (`PROVISIONAL`); the data has no role field. Use it to place a new ship among the originals: pick the class
nearest the one you want and stay near its numbers unless you mean to break them.

Rough scale: a ship of **15 to 60 points** is a raider you lose without much pain, **90 to 130** a line ship, **220 to 290** a
capital ship. HP runs from 300 (torpedo boat) to about 6400 (Pirate Galleon).

### Navy

| Class | Role | Points | Speed | Turn | HP | Guns |
|---|---|---|---|---|---|---|
| Torpedo | torpedo boat | 19 | 40 | 7.5 | 350 | 3 |
| Tug | tug (harpoons, towing) | 30 | 20 | 2.4 | 1110 | 3 |
| Sloop | scout / raider | 40 | 32 | 4.4 | 550 | 5 |
| Weapon Barge | static gun platform | 50 | 0 | 0.0 | 1150 | 12 |
| Cutter | light warship | 58 | 29 | 3.2 | 960 | 10 |
| Heavy Escort | escort (lancers, energy net) | 62 | 24 | 2.0 | 1560 | 14 |
| Assault Cutter | assault cutter | 70 | 22 | 2.0 | 1260 | 10 |
| Legacy | broadside frigate | 90 | 25 | 1.2 | 1660 | 18 |
| FastFrigate | fast frigate | 100 | 30 | 2.0 | 1930 | 10 |
| Tender | tender (docks and repairs) | 110 | 17 | 0.6 | 2800 | 5 |
| Minelayer | mine layer | 120 | 18 | 1.2 | 2000 | 13 |
| Frigate | frigate | 131 | 21 | 0.6 | 3150 | 21 |
| ManOWar | capital ship | 250 | 15 | 0.5 | 4940 | 28 |
| StarHammer | capital ship | 292 | 14 | 0.4 | 4820 | 22 |

### Pirate

| Class | Role | Points | Speed | Turn | HP | Guns |
|---|---|---|---|---|---|---|
| Sloop | light raider | 15 | 34 | 5.5 | 580 | 7 |
| Gunboat | gunboat | 24 | 43 | 8.8 | 520 | 3 |
| FastAttack | fast attack | 30 | 40 | 5.5 | 950 | 7 |
| Schooner | broadside raider | 40 | 29 | 2.9 | 1010 | 10 |
| Weapon Barge | static gun platform | 45 | 0 | 0.0 | 500 | 12 |
| Assault Cutter | boarding raider (harpoons, energy net) | 68 | 28 | 2.4 | 1320 | 14 |
| Barque | barque | 86 | 28 | 1.4 | 1680 | 13 |
| Carrack | carrack | 120 | 22 | 1.8 | 2810 | 18 |
| Tender | tender | 120 | 18 | 0.6 | 2845 | 7 |
| Galleon | capital ship | 230 | 15 | 0.4 | 6980 | 29 |

### Procyon

| Class | Role | Points | Speed | Turn | HP | Guns |
|---|---|---|---|---|---|---|
| Gunboat | gunboat | 22 | 45 | 8.3 | 510 | 3 |
| Sloop | light warship | 28 | 35 | 5.5 | 800 | 6 |
| Cutter | cutter | 59 | 28 | 3.8 | 1120 | 7 |
| Submersible | submersible (torpedoes, cloaks) | 71 | 20 | 0.7 | 360 | 3 |
| Tender | tender | 100 | 16 | 0.9 | 3080 | 9 |
| Frigate | frigate | 116 | 21 | 0.9 | 2990 | 16 |
| Minelayer | mine layer | 124 | 19 | 0.7 | 1980 | 13 |
| Destroyer | destroyer (plasma, torpedoes) | 170 | 26 | 0.6 | 1450 | 9 |
| Cruiser | cruiser (plasma, grav charge) | 219 | 24 | 0.5 | 1620 | 8 |
| ManOWar | capital ship | 270 | 16 | 0.7 | 5340 | 23 |
| IceFist | capital ship | 275 | 15 | 0.7 | 4740 | 19 |
| Dreadnought | ironclad capital ship | 350 | 18 | 0.3 | 3700 | 12 |

## Patterns worth knowing

- **Speed trades against everything.** The fast hulls (40 km/h and up) are the cheapest and have the least HP: Torpedo, Fast Attack,
  Gunboats. The capital ships crawl at 14 to 16 km/h and turn at 0.3 to 0.7.
- **Pirates buy hull per point; the Navy buys discipline.** Pirate Galleon: 230 points for about 6400 HP; Navy Man-o'-War: 250 for 4440.
- **Weapon families.** Navy: cannonballs (light, medium, heavy), carronades, gatlings, lancers, harpoons, mines, the star mortar. Pirates:
  cannonballs, harpoons, energy net, a fire weapon (`ArcturianFire`). Procyon: plasma and lancer, gravity charges, torpedoes,
  mines, and cannons on the smaller hulls.
- **Crew follows role.** Every ship has a captain; engineers cluster on tenders and ironclads; riggers follow sails; fighters
  (boarders) follow the big and the piratical.
- **Tenders dock and repair** (`Repair_Small/Medium/Large` by size); they are the slowest per point.

## Where the Hunter sits

The Hunter mod (`mods/hunter`, a Procyon trimaran): 85 points, 50 km/h, turn 5 (as the data states it), 120 core HP plus its sections, one
heavy railgun built into the bow (16 degree arc, so the ship turns to aim; a slow, heavy slug that drops with range and
reloads in 8 s). It is faster than every retail ship (the fastest is 45 km/h) and has one gun against 3 to 7 on the fast raiders: a
sniper by design, a role the retail fleet does not have. That is fine for a mod; it is also why it must read as *different* on screen
while still looking Procyon (`design-language.md`, audit).
