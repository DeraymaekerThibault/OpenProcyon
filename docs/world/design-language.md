# Design language: how ships in this world look

This describes the *retail* look, for anyone who wants a ship to sit naturally beside the originals (the project's own
ships, and mods that aim for that). It is guidance, not a rule: mods may look like anything. It is written in our own words from measurements of the
retail models and the film's stated direction (`../remaster-style.md`). Numbers are measured; anything judged by eye or
inferred from material names is marked `PROVISIONAL`. Re-measure with the importer's output (`assets/ships/*/model.glb`) before trusting
a figure for a new decision.

## The guiding idea

A ship is a **sailing ship first, a spaceship second** (film direction: about 70% 18th-century, 30% sci-fi). Nothing is
streamlined for its own sake. Wood, brass, canvas and rope carry the silhouette; the sci-fi parts (engines, plasma, metal plating,
gun mounts) are bolted on and visibly *added*, not the hull's shape. A hull that could be a bare aircraft or rocket fails the rule.

## What every retail ship has (measured)

Sizes below are the model's bounding box in game units: length along y, width along x **including yards and sails**, height along
z **including masts**.

| Ship | Length | Width | Height | Materials | Triangles |
|---|---|---|---|---|---|
| Pirate Gunboat | 11.3 | 9.3 | 11.5 | 30 | 4.7k |
| Pirate Sloop | 11.1 | 10.1 | 11.7 | 26 | 4.8k |
| Pirate Fast Attack | 24.9 | 23.1 | 24.0 | 33 | 7.6k |
| Pirate Schooner | 17.6 | 15.4 | 12.2 | 39 | 6.9k |
| Pirate Barque | 21.1 | 12.3 | 18.9 | 40 | 11.2k |
| Pirate Galleon | 35.2 | 29.6 | 20.6 | 46 | 17.0k |
| Navy Cutter | 21.7 | 10.7 | 18.6 | 32 | 8.9k |
| Navy Man-o'-War | 35.3 | 19.5 | 28.0 | 45 | 20.5k |
| Procyon Cutter | 19.6 | 11.0 | 21.1 | 33 | 8.1k |
| Procyon Destroyer | 19.9 | 7.4 | 10.3 | 31 | 7.3k |

**Human scale: a person is about 1.0 unit tall** (1 unit is roughly 1.7 to 2 m, not 1 m). Measured against retail models, not stated
anywhere: the Navy lifeboat (3.4 long) has its thwarts about 0.5 apart, the Legacy's deck is about 6 wide with rises of 0.35 to 0.55,
the Man-o'-War (35 long) then comes out at a real ship of the line's length, and pedestal deck guns (light 0.74, medium 1.16, heavy 1.5
tall) put their trunnions at about a man's chest. Size doors, stairs, rails, hatches and seats to that: a door about 1.2 high, a rail
about 0.5, a stair step about 0.1 to 0.15. The remaster's deck crews (`render/deckhands.rs`) are drawn at this size. The big retail hulls
are not strict about it (the Merchant's deck rises a full unit at one step), so match their silhouette first and people second.

1. **Tall.** Sailing ships (Pirate, Navy, civilian) are roughly as tall as they are long: height / length is 0.6 to 1.1 for
   nearly every one (masts and rigging). Only barges, submersibles and the Procyon hulls sit lower (0.2 to 0.6).
2. **Wide when sailed.** Width with yards out is 0.8 to 1.0 of length for pirate ships; the Navy is narrower (0.4 to 0.6).
3. **Many materials, none metallic.** 22 to 58 distinct painted textures per ship, all with metallic = 0. Metal is *painted* to
   look like metal ("blue metal plate", "engine plating"); wood has its own tile per part (deck, hull, rail, cabin, keel).
4. **Part vocabulary** (from material names, common to most ships): hull planking, deck, railings (plain, ornate, gold),
   cabins with portholes or windows, stairs/doors, a stern with rudder, one or more masts with a mast ring and crow's nest,
   square sails, **engine plating plus a plasma exhaust**, gun mounts and cannons (`MountV2`/`MountV3`, medium and heavy
   cannon), a lamp, crates (`SY_box`). A ship missing most of these reads as foreign.
5. **Triangle budget** 4k (small) to 20k (capital). A 10-30 unit ship is 5k-10k triangles.
6. **Layers.** Every ship is split into numbered layers (`<Name>Ship_Layer1..N`) that map to damage sections (stern, bow,
   engine, and so on), plus matching `...ShipDamage_Layer*` meshes with the broken look. New ships reuse a retail
   geometry/section set; a mod ship can instead supply only its model (`docs/modding.md`, "A ship from a model").

## Factions

Colours, silhouettes and fleets of each faction are in [factions.md](factions.md); the one-line version: the **Navy** is tidy
timber with blue and yellow striped trim, gold rails and cream sails; **Pirates** are dark red-brown wood with patched rusty plate
and dull orange sails, cluttered and lopsided; **Procyon** are slender planked trimarans: a central hull and two outrigger pontoons with white bands, glowing cyan
engines and tall masts with huge pale blue-grey emblem sails. (Measured from the retail textures; the Navy's "blue" is trim, not the hull.)

## Checklist for a new ship

1. Pick the faction and an anchor retail ship of similar size; match its length, height / length and width / length ratios
   within roughly 20%.
2. Hull is wooden planking with a keel line, deck, rails and a stern with a rudder. Metal only as plating and mounts.
3. At least one mast with sails (faction colour) and a mast ring; the sails are what make the width and height above.
4. Engines are plating plus plasma exhaust at the stern, the way retail does it, not a nozzle that *is* the hull.
5. Use textured, non-metallic materials (painted wood, painted plate, canvas) in the retail count range for the size.
6. Guns sit on visible mounts; heavy weapons look bolted on.
7. Keep the damage layers and section names compatible so damage, debris and destruction work.
8. Test readability at game zoom: the team colour and the silhouette must identify it from far away.

## Still unknown (ask before inventing)

- Exact faction colour values (sample the retail textures rather than guess).
- Whether a faction's ships share keel and rail proportions (would need side views).
- How flags, names and emblems are placed on hulls.
