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

## Remake (`menu.rs`: `Slot`, `new_slots`, `offered`, `Skirmish::plan`; `main.rs`: `Group`)
- Historical map: rows = playable players with their fixed fleet type/team and pre-placed fleets (exported to `ships` in the map
  JSON); you only choose which player you command (click the name). Players on your team are allies, all others hostile;
  different computer teams also fight each other (The Maw has three teams: Navy, Procyon and Pirates, a three-way battle).
  Assemble Fleet is unavailable.
- Open map: you, one opponent, the other slots closed. Per slot: take it, open/close it, change fleet type (free-for-all maps)
  or team; on team maps the fleet type follows the team. Fleet size limit 1-8. Your fleet comes from the fleet screen; computer fleets are auto-built.
- Arms: the fleet screen's Arms button opens the same shared weapon editor as campaign . Each fleet entry has its own bank fittings, including duplicate hulls; launch/restart carries these into turret models, firing and HUD state. Dropping/truncating a fleet or changing race removes the corresponding fittings. Campaign funds are untouched. Historical fleets remain fixed.
- Mount compatibility: exported gun `Mount Type` (whitespace-normalised) and `Exclusion Race` drive both editors; selectable same-size mounts permit cross-family weapons. `Non-Selectable` mounts and point defence are protected. Inferring a bank's type from its original guns is still provisional; mods missing metadata retain the older family fallback.
- Not done / PROVISIONAL: victory points and Auto Assign; crew/load/save; the 3D ship preview; ready/chat/network;
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
