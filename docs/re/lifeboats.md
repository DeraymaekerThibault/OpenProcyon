# Lifeboats: creation, movement, pickup, statistics, mission rules

Implemented in `game/src/lifeboats.rs` (status in §9); this file stays the evidence record. Source: observed behaviour of the original game and its retail data files (WOTs, mission-start saves), described in our own words.

- Evidence levels: **[C]** confirmed by a traced handler; **[I]** inference from traced code plus data; **[?]** unresolved.

## 1. Summary

1. A lifeboat is an ordinary ship entity of a dedicated class (`Is Lifeboat = True`, collision response type `Lifeboat`). Lifeboats are **created when a ship's destruction sequence starts exploding**, only if that ship's per-ship **Create Lifeboats** flag is set. The flag defaults to false and only the rule action `Set Lifeboat Creation State` (and a save load) changes it. The number comes from the ship class (`Number of lifeboats`).
2. Each lifeboat belongs to the **same player as the dead ship**, has the race-matched lifeboat class, starts at the ship's position and sails off slowly in a random direction. Nothing was found that expires it.
3. Every lifeboat's AI tick checks whether any live non-lifeboat ship (any owner, AI or human, ally or enemy) touches it (bounding spheres). The nearest such ship collects it: the lifeboat is removed, the collector's `(collector player, lifeboat owner)` counter in `PlayerLifeBoatSavedInfo` goes up, a UI sound plays, and if the **local player** collected an **enemy** lifeboat, +2 victory points. No crew transfer exists in the handler.
4. `Player Vs Player capture count` reads exactly that table (not boarding). `Player has no lifeboats` counts lifeboats owned by the named player. Mission 2's pickup objective is the player (or the allied schooner Lady J) collecting a **Pirates**-owned lifeboat.

## 2. Confirmed findings

### 2.1 Rule action `Set Lifeboat Creation State`
- Fields `Group Name`, `Boolean`, editor text "Set Group/Unit %s Lifeboat creation State = %s". [C]
- Execute resolves the name list with the shared group/unit resolver, then for each resolved entity that is a ship writes the `Boolean` into that ship's **Create Lifeboats** flag. Non-ship entities are skipped. It affects ships that exist when the rule runs, once; it is not a group property. [C] (Resolver identity with other group actions is [I].)
- Meaning: "this ship will leave lifeboats when destroyed". `TRUE` sets, `FALSE` clears.
- The flag is a per-ship field named `Create Lifeboats` in the ship's custom-info serialiser (load, save), so it persists in saves. The ship-info constructor starts it at **false**; no other writer was found. Retail mission-start saves agree: in `Mission 01.twt` only the ships of the groups named in the init rules carry `Create Lifeboats Bool True`; e.g. Big Ships (Blue Team) stay `False`. [C]
- So skirmish/MP battles (no TWT rules) create **no** lifeboats unless another writer exists. [I]

### 2.2 Creation, called once from the ship destruction sequence
- Ship destruction is a state machine: when the pre-explosion delay elapses it enters the explosion state, fires the death effects and the "ship lost" speech, then runs lifeboat creation exactly once. Removal of the hull follows later via the world's deferred-removal list. [C]
- Preconditions: dying entity is a ship, `Create Lifeboats` set, class `Number of lifeboats` > 0. Count = that class value (one lifeboat per loop turn, independent random draws). [C]
- Lifeboat class by the dying ship's **race** (race order Navy 0, Civilian 1, Pirate 2, Procyon 3): Navy → `Ship_Navy_Lifeboat`, Civilian → `Ship_Civilian_Lifeboat`, Pirate → `Ship_Pirate_Lifeboat`, Procyon → `Ship_Procyon_Lifeboat`, any other value → Civilian. [C]
- Created through the world's ship factory at the dying ship's world position. [C] Orientation/placement detail: see 2.3 and §4.
- **Owner**: the new ship's player index is set to the dying ship's *current* owner, which also notifies the info object. So a ship handed over by `Set Group/Unit Owner` leaves lifeboats for its new owner. [C]
- The dying ship is added to the lifeboat's **collision exclusion list** (the save format shows it as `IsInclusionList False / Object IDS`), so the lifeboat does not collide with its parent. [C for the call; "collision exclusion" meaning from the save field names, I]
- Initial state: hull health fraction 1.0, `Localized Ship Name StringID` = 48803. [C; the text of string 48803 is not resolved]

### 2.3 Movement and lifetime
- Two uniform random numbers in [-100, 100) (world LCG, multiplier `0x19660D`, increment `0x3C6EF35F`, float from the low 23 bits) give a direction; the lifeboat is given a **one-waypoint path at ship position + 10000 × that offset** (so effectively "sail straight that way forever"), and its ordered throttle is a further uniform draw mapped to **0.2 + 0.8·u**, clamped to [0, 1]. Path-active flag set on its AI entity. [C for constants; "faces and sails along the same direction" is I]
- Class speeds (WOT `MaxSpeed`, same units as `ship.json`): Navy 0.556, Civilian 1.056, Pirate 0.839, Procyon 0.5 (Cutter is 8.06). So drift is 0.1–1.1 units/s: lifeboats stay near the wreck for a long time. Hull section hp is 999, not vital. [C, from WOTs]
- **No lifetime timer, off-map cull or fade** was found in creation, the pickup tick or the (few) other lifeboat-flag readers. They persist until collected, destroyed, or the mission ends. [I: absence of evidence; a generic world-bounds rule was not ruled out]
- Lifeboat AI: the ship AI update runs the pickup test first every tick, then the normal AI update only for ships whose owner is unset or flagged as AI-controlled. Whether human-owned lifeboats get steering is [?].

### 2.4 Collision, targeting, rosters
- Lifeboat WOTs use `ResponseType = Lifeboat` (not `Ship`), bounding-box detection; the Civilian/Navy lifeboats have an authored sphere (radius 2.0 at local (0,0,1)), Pirate/Procyon use a mesh-derived sphere. `Distance to Start Sphere Picking = 18`, icon `ShipBar_ShipIcon_Navy_LifeBoat`. [C]
- Response table (16-wide pair matrix): the class `Ship_vs_Lifeboat_ResponseFunctor` fills exactly the (0,7) and (7,0) cells, and its only method returns false: **a ship touching a lifeboat gets no physical response**. No other cell involves type 7, so e.g. bullet-vs-lifeboat has no registered functor. [C for the table; "shells pass through" is I because the dispatcher's handling of empty cells was not traced]
- The shared relation filter (used by the AI target selection / group queries) treats a pair containing a lifeboat as **never an enemy and always friendly** (mode 1 returns not-hostile, mode 2 returns friendly; mode names are inferred). Lifeboats are therefore never auto-targeted as enemies. [C for the predicate; I for mode naming]
- Lifeboats are excluded from the starting-fleet roster builder, the ship-lost/primary-ship speech dispatcher, and the pickup candidate list. [C]
- Whether the player can select/target one by click is [?] (picking radius and ship-bar icon exist). If the player's primary ship *is* a lifeboat the game raises speech event `0x57` and sets a flag; meaning unresolved.
- `Ship_Navy_Lifeboat_Mission9` is **not** a lifeboat (`Is Lifeboat False`, `ResponseType Ship`, speed 16.7, vital hull). Never match lifeboats by class name; use the flag. [C]

### 2.5 Pickup (at the top of the AI update, every tick, for every ship AI)
1. Return unless this entity's class has `Is Lifeboat`. [C]
2. Candidates = the world's category-0 entity list (all ships in practice [I]), then three filters: drop the entity itself, drop every lifeboat, keep only entities with entity-flag bit 0 whose damage-sections state field is 0 (alive/not destroyed [I]). So **any alive non-lifeboat ship qualifies: human or AI, own, allied, neutral or enemy**. [C]
3. Take the candidate with the smallest **surface distance** (mode 1 = centre distance between the two bounding spheres' world centres minus the sum of their radii; spheres from the collision entity). [C]
4. The pick happens when that nearest distance is within the AI entity's range field **and** the second test, surface distance ≤ 0 (spheres overlap), passes. With a non-negative range the second test is the effective rule: **contact of bounding spheres**. [C for the tests; value of the range field is [?], see §4]
5. On success, in this order: (a) lifeboat queued for removal from the world; (b) increments the **lifeboats-saved record (collector owner, lifeboat owner)** unless a session flag is set; (c) a UI sound `HUD_Lifeboat_Pickup` is started through the audio object, unconditionally of who collected; (d) **only if the collector's owner is the local player** **and** the alliance matrix does not report the two owners as mutually allied (value 3): live points +2 and the stats counter `SinglePlayer Points` +2. The same arithmetic is the rule action `Add Victory Points for SinglePlayer` (live counter + stats counter, both only when the session flag is clear). [C]
6. No crew, ship repair, banner, speech or objective call is in the handler. **Pickup does not change the collector's crew**; "Grange and his men" is flavour. [C by absence]
- Only the nearest candidate is tested each tick (a farther touching ship does not collect). One collector per lifeboat; the lifeboat is gone next frame. [C]

### 2.6 Statistics — `PlayerLifeBoatSavedInfo`
- Record: `Player`, `# Lifeboats Saved`, `Player Index of Lifeboats`, saved as `PlayerLifeBoatSavedInfo - Size Int N` in `GameStats` (the retail mission-start saves have size 0).
- **Only writer**: the pickup handler (find `(player, lifeboat-owner)`, add 1, or append with 1). **Only reader**: `Player Vs Player capture count`. [C]
- So the stats screen's "lifeboats saved" and the PvP capture count are the same data, keyed by collector and lifeboat owner, no ally sharing.

### 2.7 Conditions
- **`Player Vs Player capture count`** (text "Player %s has captured %s %d from Player %s"). Fields `Player Name A`, `Player Name B`, `Equivalence` (`Greater Than` / `Less Than` / `Equal To`), `Number Of Captures`. Resolves both names (index in the world's player-name list, last match, -1 if absent); if either is unknown the condition is **false**; otherwise compares the stored count for (A as collector, B as lifeboat owner), 0 if no record, with the number. An unrecognised equivalence string is false. [C]
- **`Player has no lifeboats`**. Field `Player Name`. Unknown player name → **true**. Otherwise it scans the ship list and returns **false iff some entity whose owner index equals that player has `Is Lifeboat`**. No liveness test; an entity queued for removal still counts until the world flushes it. It does not look at groups, only the owner. [C]
- `Team has captured a ship from Group/Unit` is a different condition (it counts boarding captures) and does not use this table. [C]

## 3. Mission usage (exported rules, all 13 missions)

`Set Lifeboat Creation State` (TRUE everywhere; no FALSE found) appears in missions 1-12; none in 13. "Approx. count" multiplies the class `Number of lifeboats` of the ships whose group/owner/name the rule names; player ships are not counted when the name cannot be matched from the export.

| Mission | Groups set TRUE (besides Player0) | Approx. enemy/NPC lifeboats | Uses of the two conditions |
|---|---|---|---|
| 1 | Small Ships (Red Team), Pirate Schooner/Warrior, Navy Tender in Distress/RLS Providence, Player ship | Pirate ×2 (Warrior), Navy ×4 + ×3 | `Player has no lifeboats` (Pirate Schooner) in two "Go To Next Level" rules |
| 2 | PirateSloop/Ravage, Schooner1, Schooner2, Player0 | Pirate ×2 (Ravage), Civilian ×1 each schooner | `PvP capture count` ×2 (objective below) |
| 3, 8, 9, 11 | only Player0 | none | none |
| 4 | Pirates, asteroid ships (BHpathing rule); Player0 | Pirate ×3, Navy ×3 (`navy3`) | none; **3 pre-placed `Ship_Navy_Lifeboat` (ids 220-222, owner `navy`)** next to RLS Nightingale |
| 5 | Player0, Pirate Ships @ Base, Evil Pirates | Pirate ×7 | none |
| 6 | Pirates, PirateRunSloop, PirateWimp, PirateBigBrother | Pirate ×11 | `PvP capture count PlayerStart vs PirateRunner == 1` (alt to the boarding rule `Team has captured`) |
| 7 | Navy Rescue Fleet, Pirate Fleet | Navy ×6, Pirate ×8 | none |
| 9 | Player0 only | none; `Ship_Navy_Lifeboat_Mission9` (id 303) is a combat story ship, not a lifeboat | none |
| 10 | Merchant Convoy, pirate grp 1/2/3 | Civilian ×24, Pirate ×11 | none |
| 12 | Pirates, RoyalGuardOne, Player0 | Pirate ×10, Navy ×4 | `PvP capture count Procs vs RoyalPalace == 1` (the defeat) |
| 13 | none | none | none |

### 3.1 Mission 1
- "Initial Vulnerability & Lifeboats" and "Pirates & Lifeboat Buffer Zone Rule" (world-init) enable creation for the Navy small ships, Navy tenders in distress, the player's command ship and the Pirate Schooner (Warrior).
- When group `Pirate Schooner` has < 1 member: objective *Save Tender in Distress* completes and *Collect Pirate Lifeboat* activates (text "M0109COLLECTLONGBOATS"); it is never completed by a rule, only superseded by the level change.
- Exit paths: (a) `Group has X members < 1`, **`Player has no lifeboats` (Pirate Schooner)** and speech `M01AMELIA17` (player alone) or `M01AMELIA15` (Amelia helped) already played → music + `Goto Next Level Mission02` immediately; (b) fallback: victory timer 55 s after the schooner died, then +45 s. "Player Takes Too Long To Pick Up Lifeboats" fires 25 s after the second timer starts if still in the level. So collecting both Pirate-owned lifeboats ends mission 1 early; otherwise ~100 s after the kill.
- The border-zone rule `Set Group/Unit Border Zone 'Pirate Schooner'` is a ship leash; no evidence it applies to the lifeboats (they are not group members).

### 3.2 Mission 2 pickup objective (`PickUpLifeboats`, text "Move Your Ship Over Any Lifeboats to Pickup Grange and His Men.")
1. World-init: creation TRUE for `PirateSloop,Ravage` (id 163, a `Ship_Pirate_Schooner`, owner `Pirates`, 2 Pirate lifeboats), Player0 Group, Schooner1 (SS Coventry), Schooner2 (SS Lady J).
2. `Victory1`: group `PirateSloop` destroyed → speech `M02ONYX18`, objective *DefeatGrange* complete, `PickUpLifeboats` active, +5 points.
3. `Victory1.5`: after that speech, if flag `LifeboatsPickedUp` is FALSE → reminder `M02JIM22`.
4. `Victory2`: after `M02ONYX18`, `PvP capture count (PlayerStart, Pirates) > 0` → start `FinalTimer`, complete `PickUpLifeboats`, set `LifeboatsPickedUp`.
5. `Victory-LadyJCapturesPirates`: `PvP capture count (Schooner2, Pirates) > 0` → start `FinalTimer`, **deactivate** (not complete) the objective, set the flag. So the allied AI schooner collecting the boats ends the mission without credit; this proves AI ships collect and that counts are not shared with allies.
6. `Victory3`: `FinalTimer > 3 s` → `Goto Next Level Mission03`.
- Consequence for the port: the player must be able to reach the lifeboats and no other ship (including Pirates-allied) may remove them first; a pirate ship nearby would delete them and leave the objective unreachable, as in retail.

## 4. Inferences and unresolved questions

Unresolved (impact in brackets):
1. [?] Value and provenance of the pickup range field (not set by the constructor). [low: the touch test dominates if the range is ≥ 0; use sphere overlap]
2. [?] Exact spawn pose: position taken as the dying ship's position (z kept), heading toward the random direction. [low]
3. [?] The pre-explosion delay and which destruction paths reach it (combat death certainly; scripted `Destroy Group/Unit`, collisions, boarding capture not verified). [medium: Mission 1's Warrior and Mission 2's Ravage die in combat]
4. [?] Whether human-owned lifeboats are steered; world-bounds handling; hit/targeting by shells and by clicks; speech `0x57` for a lifeboat primary ship.
5. [?] Meaning of the session flag (suppresses stats and points) and of the alliance matrix cell (value 3 assumed mutual alliance; a ship's own cell assumed allied so own-lifeboats give no points). [low]
6. [?] Text of name string 48803; entity category 0 contents (ships assumed).
7. [?] Whether skirmish/MP sets the creation flag by another route (none found).
8. [?] How Mission 12's `Procs vs RoyalPalace` and Mission 6's `PlayerStart vs PirateRunner` can ever be true in retail, given the table is lifeboat-only. The runtime currently feeds boarding captures into the same table (`rules/eval.rs::record_capture`, allies share credit) so mission 12's defeat fires; making `PvP capture count` lifeboat-only would turn that into dead code. Keep a marked PROVISIONAL bridge for those two rule names until the defeat trigger is re-derived.

## 5. Gaps in the current repo

| Area | Today | Needed |
|---|---|---|
| `tools/export_combat.py` (ship JSON) / `pack_data.py` | no lifeboat fields | export `Number of lifeboats`, `Is Lifeboat`, `Ship Race` (present), `ResponseType`, bounding sphere (`User defined sphere size`, `Radius`, `LocalPosition`; mesh-derived when not user-defined: Pirate/Procyon lifeboats, Weapon Barge, StarHammer wreck, RefuelingStation) |
| `Ship_Civilian_Lifeboat` | not exported (`export_ship` demands Min/MaxExtents; the WOT has none) | tolerate missing extents (sphere/section bounds); export the model; needed because Civilian is also the default race |
| `data.rs` | no lifeboat/sphere fields | add fields to the ship class, keep mod inheritance |
| `rules/eval.rs` | `Set Lifeboat Creation State` ignored (`act`), `Player has no lifeboats` always true, `PvP capture count` shared with boarding | implement §2.1/§2.7; separate lifeboat counter |
| `campaign.rs::MissionShip` | has `owner` | add per-ship `create_lifeboats` (default false, saved) |
| Death path (`combat/`, `scenario.rs`) | ships hide/despawn on destruction, no spawn | spawn hook at explosion start |
| Stats | no `GameStats` resource yet (`docs/re/scoring-saves.md` plan) | minimal `lifeboats_saved: Vec<(collector, owner, n)>` that the future stats screen can adopt |
| Audio | `assets/sound/ui/hud_lifeboat_pickup.ogg` exists | play `Sfx::ui("HUD_Lifeboat_Pickup")` on pickup |
| Menu | `NOT_OFFERED` matches the name `Lifeboat` | fine for the fleet picker; do not reuse for gameplay (Mission9 trap) |

## 6. Implementation checklist (ordered)

1. **Export**: add class fields and spheres (above); fix Civilian lifeboat; regenerate assets (`tools/build_assets.sh`); extend the asset tests to assert the four lifeboat classes (`is_lifeboat`), `count` for Schooner 2 / Sloop 1 / Procyon Dreadnought 0, and sphere radii (Navy lifeboat 2.0, Cutter 12).
2. **Data**: parse the new fields in `data.rs`; unit-test defaults for mods that omit them (count 0, not a lifeboat).
3. **Model**: `Lifeboat` marker component (+ owner as `MissionShip.owner`), per-ship `CreateLifeboats(bool)`; consider a feature module `lifeboats.rs` like `boarding.rs` (needs a row in `docs/development.md` at merge).
4. **Rules**: `Set Lifeboat Creation State` (comma lists, ships only, TRUE/FALSE); `Player has no lifeboats` (unknown name → true; any lifeboat owned by the player → false); `PvP capture count` reading only the lifeboat table with GT/LT/EQ; move the boarding feed to a PROVISIONAL, clearly named bridge for missions 6/12.
5. **Spawn**: on explosion start, if flag && count > 0 && ship: choose class by race (Navy/Civilian/Pirate/Procyon, default Civilian); owner = current owner; position = ship position; random direction (uniform square) and throttle `0.2 + 0.8u`; straight far waypoint; no collision with the parent; hull hp as exported; inject the RNG for tests.
6. **Collision/targeting**: lifeboat vs ship: no response; exclude lifeboats from enemy target scans, fleet rosters and ship-lost speech; keep shells from damaging them until the dispatcher is traced (provisional).
7. **Pickup**: each tick per lifeboat: nearest alive non-lifeboat ship by sphere-surface distance; collect when ≤ 0; one collector; remove the lifeboat; update the lifeboat table; play the sound; +2 points iff collector is the local player and not allied with the lifeboat owner (also add to the mission points counter used by `CampaignPoints`).
8. **Mission wiring**: run Mission 1 and 2 end-to-end (see §7); then 4 (pre-placed boats) and 5-7, 10, 12 smoke.
9. **Docs/backlog** at merge: update `fidelity-backlog.md` (M1-03, CAM-01, rule table), `campaign.md` "Still missing", `boarding.md` Mission usage line, `scoring-saves.md`; this file stays the evidence record.

## 7. Acceptance scenarios

1. **Flag defaults**: a ship never named by the rule leaves no lifeboat when destroyed; after the rule with TRUE it does; with FALSE again it does not; the flag survives a save/load round trip.
2. **Counts and class**: Pirate Schooner → 2 Pirate lifeboats; Pirate Sloop → 1; Civilian Merchant → 6 Civilian; Procyon Dreadnought (0) → none; a Navy tender handed to another owner → lifeboats of that owner and Navy class.
3. **Motion**: after spawn each lifeboat's speed ≤ class `MaxSpeed` × 0.2..1.0, heading constant, nothing deletes it for several minutes; it does not collide with its parent.
4. **Pickup rule**: a live ship overlapping a lifeboat's sphere collects it (surface distance ≤ 0); one just outside does not; two ships: only the nearest; dead ships and other lifeboats never collect; AI and enemy ships collect.
5. **Condition semantics**: PvP count GT/LT/EQ against the table, unknown player false, no ally sharing, boarding does not increment it. `Player has no lifeboats`: unknown name true; true before any lifeboat exists; false while one exists; true after the last is collected.
6. **Mission 1**: kill Warrior → 2 Pirate lifeboats; both collected after the NIS speech → next level at once; if ignored → fallback ~100 s; objective text *Collect Pirate Lifeboat* shown after the schooner dies.
7. **Mission 2**: kill Ravage → `PickUpLifeboats` active, `M02JIM22` reminder if idle; player collects one → objective complete → 3 s → Mission 3. Variant: Lady J collects first → objective deactivated, flag set, same 3 s exit. A pirate-side ship overlapping the boats removes them (retail quirk).
8. **Points/statistics**: local player collecting an enemy lifeboat: +2 once (live counter and stats counter); collecting an allied/own one, or an AI collecting: no points but the table still increments; sound plays once per pickup.
9. **Mission 4** pre-placed lifeboats are real lifeboats (checked: at start the nearest ships are 31-39 units away, outside sphere contact, so nothing is collected at start); Mission 9's `Ship_Navy_Lifeboat_Mission9` is never treated as one.
10. **Smoke**: the headless screenshot run of Mission 1/2 with a short script that destroys the schooner and collects the boats (ECS wiring is not covered by unit tests alone).

## 8. Evidence index

Retail data read: WOTs (`Number of lifeboats`, `Is Lifeboat`, `ResponseType`, sphere), `SavedGames/Mission NN.twt` (`Create Lifeboats`, `PlayerLifeBoatSavedInfo`).

## 9. Implementation and validation status

Implemented (checklist §6, in order): exporter fields (`lifeboats`, `is_lifeboat`, `response`, `sphere`; extents optional so `Ship_Civilian_Lifeboat` exports; the ship JSONs and the new `assets/ships/Ship_Civilian_Lifeboat/` regenerated together; unrelated asset drift from a full `build_assets.sh` run was discarded) → `data.rs` class fields → `lifeboats.rs` (`Lifeboat`, `ShipLost`; per-ship flag is `MissionShip.create_lifeboats`) → rules (`Set Lifeboat Creation State` → `Fx::Lifeboats`; `Player has no lifeboats`; lifeboat-only `Player Vs Player capture count`) → spawn → collision/targeting → pickup → Mission 1/2/4/9 wiring.

Decisions and deviations (each provisional unless noted):
- **Flag only**: every test is `Class::is_lifeboat`; no class-name match, no mission-name exception. Mission 9's `Ship_Navy_Lifeboat_Mission9` stays an ordinary combat ship (unit + `lifeboat-m9` smoke).
- **Spawn trigger**: when the combat death handler (`combat::deaths`: core hit points gone, from shells or fire) or the dev kill keys report `ShipLost`. Scripted `Destroy Group/Unit` does not spawn boats (§4.3 unverified). The exe's pre-explosion delay is not modelled: boats appear on the frame after the loss.
- **Boats** are ordinary `Ship` entities (class by dying ship's race, owner = the ship's current `MissionShip.owner`, `Side::Player` always, no `PlayerShip`/`Boardable`/`FleetMember`), one far waypoint along a random direction and `throttle = 0.2 + 0.8u` of class `MaxSpeed`; no lifetime. Spawned boats have no `MissionShip` (rules address groups/owners, not boats: a Pirates-owned boat does not become a member of group `Pirates`); pre-placed boats (mission 4, ids 220-222, group `navy`) keep theirs and get a `Lifeboat` marker, and are not in any fleet roster nor mission AI.
- **Collision/targeting**: `collision::sail` skips ship-ship response when either is a boat (scenery still blocks them); shells skip boats (PROVISIONAL); boats are `Side::Player` and absent from the rule AI target pool, so they are never auto-targeted, never picked by click as an enemy, never in the roster.
- **Pickup**: each frame, per boat, the nearest live non-lifeboat ship by sphere surface distance (`Class::sphere`; for Pirate/Procyon boats a sphere derived from the authored box, PROVISIONAL); contact (≤ 0) collects. Both boats of one wreck start together and drift ≤ ~1 unit/s, so a ship usually collects both on one visit (retail-faithful: no one-per-ship limit). Effects: despawn, `HUD_Lifeboat_Pickup` UI sound, `Script::lifeboat_saved(collector owner, boat owner)` (table + `+2` to `Script.score` only for the local player collecting a boat of a non-allied owner), the owner list refreshed for `Player has no lifeboats`.
- **Statistics**: the table is `Script::lifeboats_saved` (`PlayerLifeBoatSavedInfo`), exposed via `lifeboats_saved_by`; no `GameStats` framework. Saves restart the mission, so nothing extra is persisted: the creation flag is re-applied by the world-init rules (§7.1's "survives a save/load" is therefore trivially true and not separately tested).
- **Missions 6/12 ambiguity (§4.8)**: reviewed, not changed in outcome. Mission 6's `PlayerStart vs PirateRunner == 1` is the *lifeboat* route (The Little Thief, group `PirateRunner`, has creation TRUE): it now works natively, while its boarding route stays the separate `Team has captured` rule. Mission 12's `Procs vs RoyalPalace == 1` (the defeat) has no lifeboat source (the palace is a base and none of its group owners is flagged), so a narrowly scoped PROVISIONAL bridge remains: boarding captures **of bases only** (not ships) still feed a separate `base_captures` table, with allied credit as before, and the condition reads lifeboat table + bridge. It is keyed by entity type, not by mission or rule name; remove it once the defeat trigger is re-derived. Ship boardings never touch the lifeboat table (`rules::tests::boarding_feeds_team_capture_not_the_lifeboat_capture_count`).
- Unknown player: `PvP capture count` is false and `Player has no lifeboats` true, as in the exe.

Validation actually run (all scripted: ships killed through `Health.core`, collectors teleported; none is a normal-combat replay):
- Unit/ECS (`cargo test`): `data::lifeboat_tests`, `lifeboats::tests` (flag default/true, counts, race class, owner of the moment, drift ranges, straight motion, no ship-boat response, nearest-only pickup, dead ships/other boats never collect, AI collects, one sound per pickup, placed boats by flag), `rules::tests` (conditions, points/ally rule, action targeting, boarding vs table, mission 12 bridge), `tests/test_lifeboats.py` (assets).
- Smoke (`docs/development.md`): `lifeboat-m1` (both Warrior boats collected → `Go To Next Level (Alternate Condition)` immediately, the NIS speech event pre-marked played), `lifeboat-m1-fallback` (no pickup: next level at t+~100 s, the "takes too long" speech at +25 s of timer 2), `lifeboat-m2` (player collects → `PickUpLifeboats` completed, 3 s, exit), `lifeboat-m2-ladyj` (Lady J collects → objective deactivated, same exit, no points), `lifeboat-m4`, `lifeboat-m9`, `lifeboat-kill` on missions 5, 6, 7, 10, 12 (right class/count/owner).
- Not validated: real combat deaths in a play-through, the early-exit timing against the real NIS speech (the player is throttle-locked during it), whether the retail boat orientation/placement matches, banner text for boats (string 48803 unresolved; the class name is shown), and the minimap/HUD look of boats.
