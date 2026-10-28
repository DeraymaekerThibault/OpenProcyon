# Armed volcano islands (Island_Volcano_01..07)

Sources: the retail island world objects, the gun and bullet definitions they name, and the skirmish map files (Diablo Straight, Mousetrap) and missions 4, 7, 8 that place them.

- Each volcano has one to six lob mounts of the same gun (about one shot every 28 s, one shell per burst, muzzle speed 50, ranges 150 effective / 170 long / 200 maximum, wide yaw arc). Counts: volcano 1 has 6, 2 has 4, 7 has 4, 4 has 2, 3, 5 and 6 have 1.
- The shell is a lobbed magma bolt: lifetime 30 s, blast radius 40 growing over about 0.6 s, damage by material (wood 50-70, reinforced wood 40-60, iron 30-40, cloth 50-70, stone 1, dragon scale 5-15).
- The island has one 999-hit-point wooden section and an unset core strength (a placeholder, not a real value), so it behaves as indestructible in practice.
- The sound is `Volcano_Magma_FIRE01`; the island also hums a looping ambience (`docs/data-format.md`, island_sounds).

Remake (skirmish only for now): the islands are armed like the base towers and fire on every ship, whatever its side, and cannot be boarded. PROVISIONAL: which side the original gives them (none is authored), whether they ever stop firing, and the AI stance and gunnery level the mission setups give them (not read yet, issue on island gunnery level). Missions 4, 7 and 8 arm every volcano the map places (also those without a setup rule) and they fire on every visible ship, one shell per island at a time (PROVISIONAL), and the shell bursts where it comes down through the aimed height like a mortar shell.
