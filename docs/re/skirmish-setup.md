# Skirmish setup: lobby, fleet types, fleet assembly (original + data findings)

Sources: `World/*.twt` WorldInfo and player lists, `Screens/*.tst`, WOT ship properties and observed behaviour of the original game,
described in our own words.

## Map data (TWT)
- `MustAssembleFleet`: false = **historical** map, true = **open** map (the original also stores it with the lobby state).
- `Team List`: per team a name id, a **Race** and a **Race Lock** flag. Race values: 0 Navy, 2 Pirate, 3 Procyon, 4 none/other.
  Open team maps (Red Rover, Border Dispute, Convoy Raid, Mousetrap) have locked races; open free-for-all maps (Diablo Strait,
  Dragon's Nest, Shadow Dance, Zemyatin) have no team list and every player is race 4: the player must choose a fleet type.
  Some historical maps are unlocked (Ambush, Bayle's Stand, Iron War) and some locked (Locusts, Storm Front, The Maw).
- Player list: name, race, team index (-1 = alone), `IsPlayable`; non-playable entries are bases/asteroid owners.
- Historical maps pre-place ships (`Ship_*` world objects with a `PlayerIndex` = index in the player list). Open maps place none.
- Ship property `Ship Race` (Navy/Pirate/Procyon/Civilian) and `Victory Point Cost` (the economy; not used yet).

## Screens
- Skirmish Game Lobby (`Skirmish Game Lobby.tst`): Assemble Fleet, I'm Ready, Launch Game, Abort, Map Information, chat, a player
  list area (233,155)-(795,445) drawn by `LobbyPlayersList_Button` (draw), backdrop texture `GameLobby`.
- The player list draws one row per player with four cells: name, state (open/closed/computer/human), fleet type (string
  ids FLEET_NAVY/PIRATE/PROCYON, "Please Select A Fleet Type" when none) and team; the editable cells depend on the lobby mode and on
  host/ready flags. Column titles use the Players / Race / Team strings.
- Ship Selection Screen (`Ship Selection Screen.tst`, backdrop `ShipScreen`): tabs Ships/Crew/Arms, a ship viewport, Ship
  Information, Ship Victory Points, Ship Types Available (scrollable list), Add, Drop, Auto Assign, Fleet Select (the fleet being built),
  Load/Save Fleet. Launching without a fleet gives "You must select a fleet to continue."; losing/changing team resets the fleet.
- Ship Types Available (`ShipSelectionScrollableTextList`, populate): walks every registered entity type and lists the ship
  types whose `Ship Race` equals the player's fleet type. No other filter was found there.

## Measured on the original (2026-10, headless retail runs)

**Create Skirmish screen.** Victory Points slider: 50 to 1000 in steps of 10, default 300. Fleet Size Limit slider: 1 to 10, default
5. Each is a round arrow button either side of a short track with a knob; the label above reads "Victory Points : N" / "Fleet Size
Limit : N". The lobby repeats both numbers (left panel) and the Fleet screen's "Current Victory Points" starts at the limit. The budget
is the same for every side (the computer builds its fleet inside it, so a lower limit picks cheaper classes: issue #80).

**Team column on open team maps.** With two fleets per side (Border Dispute: Navy, Procyon, Navy, Procyon) the original shows the plain team
name on every row ("Navy", "Procyon"), no A / B letters, and gives each fleet its own flag. Slot order alternates the sides. Historical maps
keep the letters.

**Lobby on an open map.** You alone; the other seven slots start *Open*. Each slot's player cell is a pull-down: a Computer slot lists
Computer / Open / Close, an Open slot Open / Close / Computer (the current value first). A slot turned back to Open or Closed shows no
fleet or team. *Launch Game* and *I'm Ready* are greyed until the setup is valid (a computer opponent and an assembled fleet; the
exact trigger of each is PROVISIONAL).
- Free-for-all maps (Diablo Strait ...): your fleet type starts as Navy, team 1. The fleet cell is a pull-down (Navy / Pirate /
  Procyon) and the team cell a pull-down of Team 1 to Team 8, for you and for computers alike. A newly added computer takes the fleet type of its slot: slot 1 Pirate, 2 Procyon, 3 Navy and so on round (the player is
  slot 0, Navy), and the lowest free team number (owner observation; the first computer was seen as Pirate, Team 2).
  Fleets of one type cannot share a flag: a new fleet takes the lowest free flag of its type, a taken flag is dimmed in the picker. Changing a fleet type or team empties that
  player's fleet (the game's own text: the host changed your team, resetting your fleet).
- Team maps (Border Dispute ...): fleet and team cells have no pull-down for anyone; the team column shows the map's team name
  (Navy, Procyon ...) and the fleet type is the team's race.

**Fleet screen (Ship Selection Screen).** It opens blank: no ship info, no preview, *Add*, *Drop*, *Crew* and *Arms* greyed;
*Auto Assign*, *Load Fleet*, *Save Fleet* and the next arrow are live. Clicking a hull type in Available Ships shows its info and
the 3D preview and enables *Add*. Adding puts it in the next free fleet slot, selects that slot (info and preview now show the fleet
ship, the list highlight stays) and enables Drop, Crew and Arms. The slot bar shows five slots at a time with scroll arrows even when the
limit is higher. "Current Victory Points" falls by the ship's cost on Add. Leaving with points unspent asks "You have N victory points
remaining. Would you like to continue?" with Auto Assign / Cancel / Continue.
- *Auto Assign* (the same button on the Ships, Crew and Arms tabs) on an empty Navy fleet: 300 points / 5 ships gave a Heavy Scout (90),
  a Heavy Escort (62) and three Warsloops (40), the counter ending at 0 although the hulls cost 272; 50 points / 5 ships gave two Torpedo
  Boats (19 each), counter 0; 100 points / 1 ship a Cutter (58), counter 0; 1000 points / 10 ships began Fast Frigate 100, three
  Assault Cutters 70, Frigate 131 and left 1 point. So it fills the fleet limit with hulls that fit what is left, mixing classes
  (later picks get cheaper as the budget drains), then spends the remainder on named crew (the posts showed distinct portraits). The
  Weapons: from an empty fleet the added hulls came out with non-stock guns on several banks (Man-O-War, Minelayer, StarHammer
  samples), while Auto Assign on a hand-added StarHammer (any tab) only filled the crew and left every gun stock, with 507 of 1000 points
  unspent, so the leftover is not always spent (it was 0 only on small budgets). The pick among affordable hulls looked random: PROVISIONAL. The remake uses a fixed pseudo-random pick, a coin flip per weapon bank on the hulls it adds, and gives the leftover to the best
  affordable crew per post.

**Arms tab.** A top-down ship outline with one icon per selectable weapon bank (a mount of several guns is one bank). Clicking a bank
shows a tooltip "Heavy Mount / 4 Guns" and the list *Available Weapons* for that mount type, the fitted gun highlighted, with its name,
max range, best range, damage range and Point Cost. Dragging a weapon from the list onto the bank swaps it; the left panel then says
"Total Cost: N / Weapon Upgrade Successful". The change is the price difference times the number of guns in the bank: four heavy guns
from a 10-point to an 8-point weapon read -8 and the budget rose by 8. Load Arms / Save Arms and Auto Assign sit beside it.

**Crew tab.** A roster list (Sort By: name, victory points, leadership, gunnery, navigation, rigging, engineering, combat, spotting)
of named crew, each with a Point Cost and seven skills (Gunnery, Leadership, Navigation, Engineering, Rigging, Spotting, Combat; a
skill can read N/A). Dragging one onto a post spends its cost (a 9-point captain took the budget from 748 to 739). Empty posts are filled by *Generic Crew Member*: cost 0, every skill 1. The bottom bars
(Maneuverability, Speed, Repair Rate, Strength) summarise the posts. Load Crew / Save Crew exist.

## Remake (`menu.rs`: `Slot`, `new_slots`, `Skirmish::choices/choose/plan`; `main.rs`: `Group`)
- Create Skirmish: the retail sliders (`SliderRange`: `VICTORY_LIMIT`, `FLEET_SIZE`).
- Lobby: open maps start as above (`Kind::Open`), pull-downs through `Skirmish::dropdown` / `choices` / `choose`.
- Fleet screen: blank start, greyed Crew / Arms / Drop / Add (`shown_class`, `slot_filled`, `ship_chosen`). Crew and Arms still open
  the shared editor.
- Arms and Crew in skirmish charge or refund against the budget: `fitting_delta` for weapons, `crew_points` for named crew
  (`Slot::crew`, one post list per fleet entry; dropping a ship or changing fleet type returns its crew).
- Crew / Arms screens (`crew_arms.rs` `skirmish_screen`, drawn by `classic/crew_arms.rs` over the fleet screen): the Crew button opens
  the Crew tab, Arms the Arms tab; the roster is the race's named crew (`roster`) sorted by the Sort By pull-down (nine entries, `SORTS`; the list
  hangs under the box 25 units apart, the look is PROVISIONAL), minus anyone already posted; clicking a row (or pressing it) picks it and shows its info
  panel (portrait, cost, seven skills, N/A where none); a release over, or a later click on, a face / mount places it. A full post
  replaces its last named crew member (PROVISIONAL). Drop returns the selected post's last named crew member. Weapon placement
  follows the measured rules and uses the original's messages (wrong mount, not enough points, Total Cost).
- In battle the human fleet's named crew become `CrewPosts` (speed and rudder factors, boarding crews) through `SkirmishCrew`.
  Named gunners (posts) already drive the human ships' accuracy and gun reload (`combat/gunnery.rs`, docs/re/gunnery.md).
- Slot bar: five slots at a time with scroll arrows (`Skirmish::slot_scroll`), following the picked slot.
- Posted crew move by dragging their face onto another post (the points come back and are re-spent; a failed drop puts them back).
- Flag column: only your own flag is clickable; it opens a modal gold frame with your fleet type's ten flags (two rows of five); a pick sets
  the flag and closes it, a click outside does nothing (measured). Computers' flags cannot be changed. The remake draws the same
  (`Skirmish::flag_picker`, `Slot::flag`); the frame art is the generic dialog frame stretched (PROVISIONAL).
- Lobby pull-downs stay open only while the button is held on the original: press the cell, drag to an entry, release. The remake
  also takes that (release over an entry other than the first picks it) next to the click, click path.
- Computers build their fleets with the same routine as Auto Assign (`auto_build`, armed hulls only, same victory limit, weapon swaps but no named crew:
  they keep the measured computer crew factor). PROVISIONAL: the original's computer fleet was not read out of a battle.
- Launch Game is greyed until a computer opponent is in and (open maps) a fleet is assembled; I'm Ready is always greyed in single player
  (measured: fresh lobby, lobby with a computer and no fleet, and with a fleet).
- Every scrollbar (maps, ships, crew / weapons) shares one drag rule (`menu::scroll_drag`): press the track or thumb and drag. What a drag
  carries (crew portrait or gun icon) is drawn under the cursor.
- Not done: Load/Save Fleet/Crew/Arms, the ship-name drop-down, generic ("N/A") crews.

## Remake notes before the 2026-10 measurements (partly superseded above)
- Historical map: rows = playable players with their fixed fleet type/team and pre-placed fleets (exported to `ships` in the map
  JSON); you only choose which player you command (click the name). Players on your team are allies, all others hostile;
  different computer teams also fight each other (The Maw has three teams: Navy, Procyon and Pirates, a three-way battle).
  Assemble Fleet is unavailable.
- Open map: see the measured section; computer fleets are auto-built (issue #80).
- Arms: the fleet screen's Arms button opens the same shared weapon editor as campaign . Each fleet entry has its own bank fittings, including duplicate hulls; launch/restart carries these into turret models, firing and HUD state. Dropping/truncating a fleet or changing race removes the corresponding fittings. Campaign funds are untouched. Historical fleets remain fixed.
- Mount compatibility: exported gun `Mount Type` (whitespace-normalised) and `Exclusion Race` drive both editors; selectable same-size mounts permit cross-family weapons. `Non-Selectable` mounts and point defence are protected. Inferring a bank's type from its original guns is still provisional; mods missing metadata retain the older family fallback.
- Not done / PROVISIONAL: Auto Assign's rule; crew/load/save; the 3D ship preview; ready/chat/network;
  Race Lock on historical maps (we never change a historical fleet type); name filter that hides story/NIS/tutorial hulls (the original lists
  all ship types of the race); computer fleet choice; the lobby cell layout and states; hostile players do not fight each other (battle sides are only ours/enemy).

## Ship names
- Historical maps: the world rules' `*State Init* Setup Ship` action gives each pre-placed ship (by world object id) a script name and a
  localized name id; the exporter stores the English text as `ships[].name` in the map JSON. The banner shows it.
- Open maps: each ship type's WOT lists 12 `Available Unique Ship Name ID NN` strings (`data/ship_names.json`, `tools/export_ship_names.py`).
  Remake: a random unused name of the type's pool per ship (repeats only once the pool is used up). PROVISIONAL: the original's pick and uniqueness scope.
- Campaign ships keep their rule-set names (`MissionShip`).

## Arms acceptance (owner-run)
Compile-only checking includes regressions for mount compatibility, live campaign/skirmish equipment, independent duplicate-hull fittings,
and fleet drop/race-change cleanup; no tests or runtime checks were executed. Assemble an open-map fleet, select a ship, open Arms,
change a selectable bank to a different compatible weapon family, close, launch, verify model/icon/firing (including a lob weapon and
harpoon removal/addition), and restart. Check a second identical hull retains its own loadout.
Skirmish fittings currently have no budget charge or disk fleet-profile persistence; these are explicit limitations, not retail parity.

## Weapon bank recharge bar
hud.hdt has `WeaponBar_SlotBackground` (an empty vertical bar), `WeaponBar_RechargeStatus` (the same bar filled yellow), offsets
`Offset_WeaponBar_Recharge` (0,0) and `Offset_WeaponBar_GunIcon` (6,0). The remake draws the bar beside each bank icon with the fill growing
with the slowest live gun's reload progress, and dims the icon while the bank reloads. PROVISIONAL: the original's drawing code was not found
(the sprite names are only referenced from static initialisers); fill direction and bank aggregation are read from the artwork.

## Bases

Maps with bases list them as scenery owned by non-playable players ("navybase", "procbase", "Piratebase", "navy port"...); the map
data carries no side for them. Remake (PROVISIONAL): the owner's name (else the base's type: Grant, Montgomery and the shipyards and
ports are Navy, `Base_Pirate*` Pirate, `Base_Procyon*` Procyon) gives a fleet type, and the base fights for the first fleet of that
type in the battle, with that fleet's teammates. A base no fleet claims stays neutral and silent. A base fires on every ship of
another side it can see, can be shot by them (its sections have hit points), and computer ships with nothing in sight go for the
nearest hostile base (`ship::ai`). Not done: what destroying a base means for victory, base repair/docking by side, and bases
as part of the lobby (fleet type per base).

## Victory conditions (measured from the maps' own rules)

Every skirmish map ships its own rules that end the game; the engine has no built-in "destroy all enemies" outcome. Read from the
retail map files (2026-10-27), not from a played battle:

- **Maps with bases (Border Dispute, Red Rover, Bayles Stand, Mousetrap):** a team wins when the opposing base's mission-critical
  sections are all destroyed (the "vital to mission" damage of the base reaches 100%), or when it captures a ship from that base. The
  objective text is "Destroy enemy base" and the map's objective marker sits on the enemy base. Killing every enemy ship does not end
  Border Dispute by itself. Border Dispute and Mousetrap also end the game as lost for everyone when no human-controlled ship is left;
  Border Dispute also when no team has any ship (a draw). Mousetrap and Convoy Raid additionally give the other side the win when a
  team has no ships left.
- **Maps without bases (Diablo Strait, Dragon's Nest, Shadow Dance, The Maw, Zemyatin Anomaly, Iron War, Storm Front):** one rule ends
  the game on the engine conditions "Skirmish Game Complete" or "Team Game Complete" (the engine logs "Team Game Completed - 1 team
  remains"). PROVISIONAL meaning: the game ends when only one team still has ships; the exact skirmish variant is not decoded.
- **Historical maps** use their own goals: Ambush (reach the volume with the Mercy / capture it / destroy the three pirate groups),
  Convoy Raid (a team reaches 350 / 400 points, or the pirates have no ships), Locusts (the command fleet reaches the safety volume,
  or the privateers capture it, or lose all ships), Bayles Stand (destroy the Procyon group, or the Procyons wreck/capture the base).

Remake status: the importer exports each map's end rules as `victory` in `map.json` (with the world-object ids of the groups they name,
unit lookups by ship name, and volume polygons), the historical maps' ships carry their object `id`, and `victory.rs` plays the rules
for every map whose conditions it models: base damage, no ships, no human ships, team has no ships, game complete, group destroyed, unit
still present, group inside a volume, ship captured (a living ship of the group now sails for another side; the capturing team is not
checked, PROVISIONAL), ship destroyed from a group (the killer is not checked, PROVISIONAL). Capturing a base never fires yet (boarding
bases is unfinished). Ambush, Bayles Stand, Locusts, Mousetrap and the open maps use these. Convoy Raid needs scoring points through
its convoy scripts (not modelled): it, and any map without exported rules, keeps the plain "no enemy ships left" outcome. The map rule's
winner/loser message (`IDGS_TPINGAMEMESSAGE_*`, the human's side) is shown under the VICTORY!/DEFEAT! label of the Game Summary
(PROVISIONAL: retail position and timing unmeasured; "GAME STRING" means no custom message).
