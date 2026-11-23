# Armed volcano islands (Island_Volcano_01..07)

Sources: the retail island world objects, the gun and bullet definitions they name, and the skirmish map files (Diablo Straight, Mousetrap) and missions 4, 7, 8 that place them.

- Each volcano has one to six lob mounts of the same gun (about one shot every 28 s, one shell per burst, muzzle speed 50, ranges 150 effective / 170 long / 200 maximum, wide yaw arc). Counts: volcano 1 has 6, 2 has 4, 7 has 4, 4 has 2, 3, 5 and 6 have 1.
- The shell is a lobbed magma bolt: lifetime 30 s, blast radius 40 growing over about 0.6 s, damage by material (wood 50-70, reinforced wood 40-60, iron 30-40, cloth 50-70, stone 1, dragon scale 5-15).
- The island has one 999-hit-point wooden section and an unset core strength (a placeholder, not a real value), so it behaves as indestructible in practice.
- The sound is `Volcano_Magma_FIRE01`; the island also hums a looping ambience (`docs/data-format.md`, island_sounds).

Remake (skirmish only for now): the islands are armed like the base towers and fire on every ship, whatever its side, and cannot be boarded. PROVISIONAL: which side the original gives them (none is authored), whether they ever stop firing, and the AI stance and gunnery level the mission setups give them (not read yet, issue on island gunnery level). Missions 4, 7 and 8 arm every volcano the map places (also those without a setup rule) and they fire on every visible ship, one shell per island at a time (PROVISIONAL), and the shell bursts where it comes down through the aimed height like a mortar shell.

Island accuracy (unchecked against play): a burst's yaw and pitch error are each up to 0.5 x D x F, D being the gun's deviation for the range band (volcano guns 10/20/30 degrees at 150/170/200). F is the island's gunnery factor: Green 0.7, Average 0.4, Elite 0.1; the "CREW SKILL LEVEL" placeholder 1.0; an island no `Setup Island` touches (every skirmish volcano) 0, perfect aim, as the retail saves show. Stances: Dummy never fires; Neutral returns fire, only at ships that target it (nearest within scan range); Aggressive (the default) fires at the nearest hostile visible ship or island. The AI re-decides every 0.5 s. PROVISIONAL: whether a click-attack counts as "targeting" (we treat the captain's attack target as it), and why the original's scan range differs per volcano (273-649 in saves; we use the exported sight 400).

## Armed asteroids (Asteroid_Large_Armed, Asteroid_Small_Armed)
Read from the retail data and code (not yet measured in play): the pirate asteroids of mission 11 (32 large ones along the "Armed Asteroid Belt") and
Convoy Raid (16 large ones) are island objects, not rocks: the island AI and custom info, the static-object collision, race pirate. A large one has two
guns, a 195-hit-point section and a core of 185; a small one has one gun and a 50-hit-point section. The importer now exports them like the bases
(`tools/export_bases.py`), so the setup rule of a mission (owner, gunnery level, stance, combat strength) arms them like any island. Mission 11 owns them
to the "Armed Asteroids" player, hides them until a speech line and allies that player with the human's ships for a while; Convoy Raid's belong to the
pirate base's player and fight for its side.
Remake: a base a rule has hidden does not fire (mission 11 hides the asteroids until a speech line and reveals them with "Set Group/Unit Visibility").
PROVISIONAL: whether the original's hidden island fires is not read. Gunnery level, stance and owner come from the setup rule like any island.
