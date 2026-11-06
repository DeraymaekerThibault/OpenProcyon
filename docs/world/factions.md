# Factions

Who sails what, and how to tell them apart at a glance. Colours are the average of the retail painted textures (`#rrggbb`), so
they are the *feel* of a material, not a palette to copy pixel for pixel. Numbers come from `ships.md`. Anything judged by eye is
`PROVISIONAL`. Written in our own words.

The four fleet types in the data are `Navy`, `Pirate`, `Procyon` and `Civilian` (civilians borrow the Navy's HUD art). A fleet's team
colour (the red and blue ribbons over ships, the radar dots) is separate from the hulls and says only whose side a ship is on.

## Navy (the Royal Navy, ships named `RLS ...`)

- **Look:** formal, tidy, symmetrical. Timber hulls (`#684b29` to `#806950`) with blue and yellow striped trim bands, gold rails
  (`#d7a35b`) and ornate railings, cabins with windows and portholes, a crow's nest. The blue is a stripe and a trim, not the
  whole hull (the hull textures average brown). My earlier note that Navy hulls are blue was wrong.
- **Sails:** cream and gold, `#d3b25e` to `#e5e0d7`: the brightest sails in the game.
- **Metal:** painted grey plate (`#828281`), neat and unrusted.
- **Feel:** an 18th-century warship with engines added; discipline over improvisation.
- **Fields:** the widest spread. Torpedo boat (19 points, 40 km/h) up to the capital ships Man-o'-War and Star Hammer (250 and 292).
  Cutters and frigates carry many medium cannons; escorts add lancers and an energy net; tenders dock and repair.
- **Engine:** `NavyEngine_*` plume, engine sounds `Engine_Navy_*`.

## Pirates

- **Look:** the same wooden-ship bones, rougher. Dark red-brown hulls (`#653b14` for the red hull, `#613f30` patched), rust
  (`#716b65`), mismatched plates and bolted-on extras: buzz saws, blades, grapples, gatling guns on mismatched mounts.
- **Sails:** muted terracotta orange, `#b5775a` to `#bd7b5b`: warm and dull next to the Navy's bright cream.
- **Metal:** grey plate again, but patched and rusted.
- **Feel:** improvised, asymmetrical, crowded; a recognisable wooden hull under clutter. Long John Silver's own ship is the
  exception: larger and cleaner, with its own window and door set.
- **Fields:** fast, fragile hulls (Fast Attack and Gunboat at 40 km/h or more with 100 and 30 core HP), boarding raiders with
  harpoons and an energy net, and one monster Galleon (230 points, 29 guns, the most hull of any ship). Fewer engineers aboard than
  the Navy: many pirate ships carry none.
- **Engine:** `PirateEngine_*`, sounds `Engine_Pirate_*`.

## Procyon

- **Look (seen in the retail models and textures):** slender **planked wooden hulls**, usually a central hull between two long
  outrigger pontoons joined by struts (a trimaran or catamaran), with white metal bands around the hulls, a scalloped
  wave frieze along the gunwale, riveted grey plating and round hatches that carry the faction's emblem (a blue, three-lobed
  figure). Engines are blue-teal housings with a glowing cyan disc at the stern of each hull. The big ships (cruiser, dreadnought,
  destroyer) are the exception: lower, more industrial, with more grey plating and smokestacks.
- **Sails:** tall masts carrying very large, billowing **pale blue-grey sails** (`#c4ccd8`, `#95a5bb`) that wear the blue emblem, with
  small green pennants. The most spectacular rigging of any fleet.
- **Colours:** hull wood `#806151` to `#9a6346`, white and silver bands, grey plate `#78797b`, teal engines `#4f656e`, cyan glow
  `#72cbd3`.
- **Feel:** elegant, light, almost Polynesian: a fast outrigger rather than a galleon. It is the most advanced fleet (energy weapons,
  cloaking, plasma) wrapped in wood and cloth, which is the whole point of the world.
- **Fields:** plasma and lancers on the big hulls, gravity charges on the cruiser, torpedoes on the destroyer, a submersible that
  cloaks. Ironclad capital ships have no riggers at all and 4 to 8 engineers; heavy hulls (Dreadnought 350 points) turn slowly
  (0.3 to 0.9), the small ones quickly (Gunboat 8.3).
- **Engine:** `ProcEngine_*`; the ironclads sound `Engine_Ironclad_*`, the smaller hulls `Engine_Proc_*`.

## Civilian

- **Look:** Navy-like wooden trade ships (barque, galleon, packet, schooner, sloop, merchant) with grey-brown hulls and
  plain sails (`#c7beaf`), little ornament, few or no guns. Not offered in skirmish.

## Telling them apart at game zoom

1. **Sail colour first:** cream and gold (Navy), dull orange (Pirate), pale blue-grey on the side (Procyon).
2. **Silhouette second:** symmetrical and tall (Navy), lopsided and cluttered (Pirate), long slender hulls side by side under a big pale sail (Procyon).
3. **Surface third:** clean plate and trim (Navy), rust and patches (Pirate), planked hulls with white bands and a cyan engine glow (Procyon).

## Not yet known

- Faction emblems, flags and how names and crests are painted on hulls.
- Whether pirate clutter follows rules (where the saws and grapples go) or is free.
- The in-fiction reasons Procyon ships look the way they do (needs the owner's knowledge of the game's story).
- What the emblem stands for, and whether other emblems exist for the other fleets.
