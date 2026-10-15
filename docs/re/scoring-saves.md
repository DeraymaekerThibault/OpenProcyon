# Campaign scoring, stats screen and save games

Sources: observed behaviour of the original game, the retail
`SavedGames/*.twt` files (text chunks, 15 of them: one per mission plus a few user snapshots), `UserProfiles/*.pft`, `Screens/*.tst`,
`hud.hdt`, `Strings/English_GameStrings.txt`, and `assets/maps/mission*/mission.json` (exported rules). Described in our own words.
Anything not marked otherwise is read directly from those files. Uncertain parts are marked PROVISIONAL.

## 1. Victory points (the campaign "score")

- The rule action `Add Victory Points for SinglePlayer` (field `Victory points to be added`, an Int) adds to a single
  campaign-wide counter, `SinglePlayerVictoryPoints`. The exe's log text for it is "Add %i victory points to SinglePlayer's total",
  so the total changes at the moment the rule fires, not at the end of the mission.
- Values per mission are plain constants in the rules (exported in `assets/maps/missionNN/mission.json`). Examples: mission 1 awards
  1, 2, 3, 1, 2 over its stages; mission 2 awards 4, 4, 4, 5; mission 10 awards 30/60/90 and 30; mission 6 up to 50 per event.
- The counter is a *currency*, not just a score. It is spent between missions on gun upgrades and crew: every gun WOT has
  `Victory Point Cost Int` (e.g. 3, 19) and the exe has the error text "You do not have enough victory points to upgrade all the guns
  in that bank". Evidence from the retail mission-start saves (frame 1 of each mission):

  | save (start of) | total points | crew list | arms list |
  |---|---|---|---|
  | Mission 02 | 10 | 0 | 0 |
  | Mission 03 | 29 | 11 | 11 |
  | Mission 04 | 27 | 0 | 0 |
  | Mission 05 | 94 | 21 | 12 |
  | Mission 06 | 77 | 0 | 0 |
  | Mission 07 | 154 | 38 | 19 |
  | Mission 08 | 114 | 0 | 0 |
  | Mission 09 | 169 | 71 | 15 |
  | Mission 10 | 112 | 0 | 0 |
  | Mission 11 | 234 | 91 | 16 |
  | Mission 12 | 27 | 0 | 0 |

  Totals go down between some missions, i.e. points were spent. The "crew list" / "arms list" are lists of unlocked crew
  (`Crew_Arc_Navy_JEmdall`, ...) and unlocked guns (`Gun_CannonBall_Heavy`, `Gun_Carronade_Light`, `Gun_Gattling_Light`, ...). The alternating
  0/non-0 pattern is probably an artefact of how those test saves were made (only some were saved after visiting the Arms/Crew screens);
  PROVISIONAL.
- The retail rules also contain `Open Crew and Arms Screens` (a mission action, now handled by `crew_arms.rs`; see §7). Skirmish
  victory points (fleet budget, `Victory Limit`, `Ship Victory Points` buttons) are a separate feature of the lobby screens.
- Difficulty: the player profile stores `Difficulty Level Int` (retail strings Easy / Normal / Hard; the campaign menu has a
  "Toggle Difficulty Button"). What difficulty changes in the simulation is not recovered. PROVISIONAL / not done.

## 2. End-of-game flow

World/game fields serialized by the exe (in the `GameImpl` chunk, see section 4): `Game Complete` (bool) + `Game Complete Timer`,
`Custom Game Complete` + `Custom Game Complete String`, `Game Winner Players` (list), `Show Stats Screen`, `Goto Next Level Enabled`,
`Next Level World Name`, `Next Level Display Timer`, `Is Campaign Game`, `User Interface Disabled`.

Rule actions:
- `End Game` (fields `Use Custom Message`, `Winner - Custom Message String ID`, `Loser - Custom Message String ID`, `Show Stats Screen`).
  The exe's log line shows all four. Every mission's defeat path uses it with `Show Stats Screen = TRUE` (missions 1-12);
  mission 13 uses `IDGS_TPINGAMEMESSAGE_DEMO_COMPLETE` with stats FALSE. Winner/loser strings are ids in the game strings
  (already resolved by `tools/export_campaign_media.py` into `*_media.json` `messages`).
- `Goto Next Level` (`World`: next TWT such as `Mission02.twt`; `Display Loading String`), present in every mission 1-12.
  Missions 3, 9 and 12 also have an `End Game` with the default message (`Use Custom Message FALSE` / `GAME STRING`) and stats TRUE on
  their win path; the other missions' `End Game` actions are defeat paths. Whether the stats screen is shown before loading the next
  mission in the other missions is not recovered (PROVISIONAL). `Game Complete Timer` / `Next Level Display Timer` imply a delay
  before the stats screen / loading.
- The World header flag `PlayEndMovie Bool` is `True` only in `Mission12.twt` (28 other worlds: False). See `movies.md`.
- Mission 1's failure message is `IDGS_TPINGAMEMESSAGE_GAME_M01_FAILED_GRAD_EXERCISES`; missions 2/4/5/6/7/10/11 reuse the generic
  `..._GAME_LOST`; 8 has `..._M08_IC_DEFEAT`; 12 has `..._M12_FAILED_QUEEN`.

## 3. The stats screen (what the exe shows)

The screen is the in-game-menu style screen `Multiplayer Stats Screen.tst` (screen name "InGameMenu Screen", backdrop "Game Lobby
Backdrop", an "Open Next Arrow Button" at 350,500-450,600 in the 800x600 reference space, a centred "Game Summary" title label at
y 68-112, and one custom element `Game Stats - Player Kills by Object Type` filling the whole screen). It is shown at game end
when `Show Stats Screen` is true and from the in-game menu ("Summary" button, `IDGS_TPFRONTENDTEXT02_BUTTON_SCORE`). The campaign
and skirmish both use it.

Strings (English): `VICTORY!` / `DEFEAT!` (the "victory status" label), `Ships Left`, `Ships Sunk`, `Damage Inflicted`, `Damage Taken`,
`Victory Points Earned`, `Total Victory Points`, `Player Kill Count by World Object Type`, button `Exit Stats Screen`.

Layout/colour constants from `hud.hdt` (800x600 reference, same for Navy/Pirate skins): victory-status label at (230,120) from the window
top-left, stats border backdrop at (35,160), column width 130, gap between columns 10, player-name width 110, row height 30; fonts
`UniversLightBold14` (victory label) and `Univers10` (column headings); colours: bar `(0.207,0.566,0.265)`, everything else white.
So: one row per player (name, then columns of counts as horizontal bars), the kill-count-by-object-type table, a points panel for
campaign games. The exact column order, which object types are listed and how bars scale are not recovered. PROVISIONAL.

Implemented so far: only the lifeboat table (`rules::Script::lifeboats_saved`, `PlayerLifeBoatSavedInfo`: collector, lifeboat owner, count) and the live
`score` that lifeboat pickups add to; no `GameStats` resource, stats screen or save serialisation yet (mission-start saves restart the mission, so a
mission's table is not persisted).

Data the screen reads, serialized as `GameStats` (see saves): `PlayerKillInfo`, `PlayerDockInfo`, `PlayerLifeBoatSavedInfo`,
`FleetSizeInfo` (per player `Starting Size`, `Ending Size`), `PlayerDamageInfo` (per player `Damage Inflicted`, `Damage Received`) and
`SinglePlayer Points` (points earned in *this* mission; the campaign total is the separate `SinglePlayerVictoryPoints`).
Ships Left = Ending Size; Ships Sunk presumably = kills from `PlayerKillInfo` (PROVISIONAL).

## 4. Save games

- Location: `SavedGames/` (directory key `SAVED GAMES DIRECTORY` in `AppSettings.ini`), extension `.twt`, one file per save. The
  in-game menu (`InGameMenu_Default.tst`) has Save Game / Load Game / Options / Quit / Summary / Resume; the Load screen is a scrolling list
  of files with Load / Remove (delete confirm dialog) and a text entry for the new name (columns "Filename"). The save list title is
  "Saved Campaigns"; main menu entry "Load Campaign". `steam_autocloud.vdf` in that folder is Steam bookkeeping, not game data.
- Format: the same text-chunk format as World TWT files (`docs/wot-format.md`). A save is a **complete world snapshot**, not a
  checkpoint record. Top-level chunks, in order:
  1. `WorldInfo`: `IsMultiplayerMap`, `MustAssembleFleet`, `World Description`, `WorldNameID` (e.g. the mission journal title), `Object Count`,
     `Team List`, `Number of Players` with `PlayerInfo - Player Name/TeamIndex`, `IsCampaign`, custom world name/description flags.
  2. `Game`: `Time` (`Game Tick`, `Game Time` double), `Frame`, `Paused`, `ActivePlayerIndex`.
  3. `World`: `WorldName` (`Mission02`; note the retail `Mission 05.twt` says `Mission05Test`), `Random Seed`, `World Size`, the player list
     (colour, start point, race, `Points`, formation, `FleetAI` state with its timers), every world object (ship transform/velocity,
     hull sections, crew, guns, sails, fires, docks, AI, per-ship fields such as `Is Mission Essential`, `Repair Time`, `Nebula Slow Down
     Factor`), the complete rule list with each rule's timers and fired state, objectives (`Completed` bools), map text, `Journal Music
     Name`, `PlayEndMovie`, `IsCampaign`.
  4. `GameImpl`: `SelectedWorldObjectID`, `GameStats` (above), `SinglePlayerVictoryPoints`, `SinglePlayerCrewList`, `SinglePlayerArmsList`,
     `HUD` (NIS bars open, speech queue/timers), the end-of-game fields from section 2, `Already Created Game`.
- The retail files here were produced at frame 1 of a mission (`Game Tick 1`) by the community/owner, so they double as "mission start"
  states carrying the campaign counters (points, unlocked arms/crew) into that mission. In-mission saves hold the same structure at a later
  tick. Which options the exe offers during the Mission 1 NIS or after the end is not recovered.
- `UserProfiles/<code>.pft` is **not** a campaign save. It holds `Player Profile` (name, a player code, buddy list, `Difficulty Level`,
  `Display Banners`), `Sound Options`, `Render Options` and the input mappings. No campaign progress is stored in it; progress lives only in
  the saved game files. `GameFleetProfiles/M01JimTBoat.ckb` is the mission-1 crew preset for Jim's boat (`Crew Initialized`, gunners, captain,
  first mate lists), not a save.
- Compatibility: the remake's own save format does not need to be retail-loadable. A remake save should store, at minimum: mission id,
  the campaign counters (total points, unlocked arms/crew, difficulty) and either a mission-start marker or a snapshot of ship/rule state.

### Original game findings (+ `UserProfiles/default.pft`)
- Screens/classes: `Load Campaign Screen` (front end), `InGameMenu_Default` (Save Game / Load Game / Options / Quit Game / Summary / Resume),
  `InGameMenu_Load_Campaign` (list, Load, Remove, Cancel, `Campaign Text Entry` for the file name), a `Save Game Dialog Screen`,
  `Open Confirm Saved Game Overwrite/Delete Dialog` buttons, `SavedCampaignScrollableTextList`, `LoadOrSaveCampaignButton`, `SaveCampaign_TextEntry`,
  `DeleteSavedGame_Button`. Save-game directory key `SAVED GAMES DIRECTORY`. The `.tst` rectangles are exported by `tools/export_maps.py`.
- Input actions `Quick Save` / `Quick Load` exist (`GameUserInput` classes); default profile maps them to DirectInput scancodes 0x3d (F3) and 0x3e (F4).
- Separate from saves: "Save Recorded Game" / `RecordedGameSaveLog.txt` (replay recording), fleet/arms/crew profile saves (`.ckb`-style, `Load Or Save Fleet/Arms/CrewKeeper` buttons).
- Not yet known: the exact chunk writer/reader order, what is skipped on load, overwrite/delete dialog flow, whether Quick Save uses a fixed file name.

## 5. What the remake does today (saves)

- Esc opens the in-game menu (overlay on the paused battle; same retail layouts). Save writes `CampaignSave` = mission id + the campaign total **at the
  start of this mission** + crew/arms; Load/F4 restarts the saved mission with those counters. Main menu > Single Player > Load Campaign lists the 13
  shipped mission-start saves (retail counters) plus the player's saves. No delete/overwrite confirmation, no Options/Summary pages yet.
- Mid-mission snapshots (ship state, rule timers) are not implemented; retail saves them (section 4).

## 5b. What the remake did before saves (`game/src/rules/`, `campaign.rs`)

- `Add Victory Points` adds to `Script.score` (per mission, reset on load). End overlay shows "Mission complete."/failure text plus
  "Victory points: N", with `N` next mission / `R` restart. No stats table, no persistent total, no arms/crew spending, no saves.
- `End Game` stores the failure message from `*_media.json`; `Goto Next Level` stores "Mission complete." (continues to `campaign::next`).

## Implementation plan

1. **Campaign state resource** (`game/src/campaign.rs`): `CampaignState { total_points, arms: Vec<String>, crew: Vec<String>, difficulty,
   next_mission }`, persisted as pretty JSON in a user save directory (not `assets/`; use a `saves/` folder next to the working dir or an XDG
   path; document in `docs/data-format.md`). `rules/` `Add Victory Points` adds to both the mission earned counter and the campaign total.
   Regression test: points accumulate across `load_id` of two missions and survive JSON round trip.
2. **Mission stats collector** (`combat/` messages already exist: `Hit`, destruction): record per-player kills by object type, damage
   inflicted/received, dock count, lifeboats saved, starting/ending fleet size into a `GameStats` resource (same field names as retail
   `GameStats`). Add unit tests with synthetic message streams.
3. **Stats screen** (new `game/src/stats.rs`, owned by `docs/development.md`'s table; spawned when `Script.ended` with `Show Stats Screen`
   true, and from the in-game menu): headline `VICTORY!`/`DEFEAT!`, the ships left/sunk/damage rows per player as bars (colour `0.207,0.566,0.265`),
   a points panel (`Victory Points Earned` = mission earned, `Total Victory Points`), the kill-by-type table, `Exit Stats Screen`. Use the
   `hud.hdt` STATSSCREEN offsets (export them in `tools/export_hud.py` so mods can change them). Mark the column set PROVISIONAL.
   Replaces the plain `Ui::End` text in `rules/`. Runtime smoke test via `--script victory` screenshot.
4. **Between missions**: honour `Open Crew and Arms Screens` by showing a placeholder list now; full spending UI later
   (`SinglePlayerArmsList` / `CrewList`, gun `Victory Point Cost`). Needs a new exporter field for each gun's cost
   (`tools/export_combat.py` reading `Victory Point Cost Int`).
5. **Saves**: start with "campaign save" = `CampaignState` + mission id (+ stats), written when a mission is won and from the menu; a Load Campaign
   list on the front end mirroring the retail screen (`menu.rs`). Defer full mid-mission world snapshots (large; the retail format shows what
   would be needed: per-object state and rule timers) until the rest of the simulation state is stable.
6. Open RE questions: difficulty effects, exact stats column set and bar scaling, whether `Goto Next Level` shows the stats screen
   before loading in missions 1-11, how kill points map to `SinglePlayer Points`, arms/crew unlock flow.

## 7. Functional campaign outfitting (provisional)

`crew_arms.rs` now owns transactions and the shared screen model; `classic/crew_arms.rs` draws it.
The mission action opens a paused, paged Crew/Arms screen. Previous/next ship and post/bank navigate the own fleet; Enter/Continue
closes it, restoring the pre-screen pause state. World/fleet commands and the context menu are gated while it is open.

Evidence-backed inputs: crew prices and active skills in `data/crew.json`; eight post capacities in `data/boarding.json` (WOT maxima,
role order in boarding.md); per-gun `Victory Point Cost`, `Exclusion Race` and `Mount Type`; §1's whole-bank insufficient-points message.
Only these three metadata fields were added to committed weapon JSON, preserving every existing weapon statistic. They were read from
the **currently modded local installation**, so prices/exclusions are not established unmodded-retail values. Exporters retain them
on future conversion; no full regeneration was run and no original program was run/modified.

Implemented behaviour:
- Availability lists (`CampaignPoints::crew/arms`) remain separate from bought crew (`outfit.hired`) and ship equipment.
  Recruit charges the crew's price once. Assign moves a hired member between ships/posts without duplication and enforces capacity;
  Return to reserve removes that assignment without selling the member. Purchases and assignments persist atomically; a write error
  rolls back the transaction and appears on screen.
- Gunners affect only their assigned ship's accuracy, with `1 - 0.8 * skill`; engineers affect that ship's crew-repair odds using the
  existing provisional repair mapping. Assigned named members replace generic hands' combat contribution in boarding strength.
  Unfilled posts retain generic sailors. Re-applying an unchanged outfit does not heal boarding casualties. Older saves without
  assignments retain the former roster-best gunnery/engineering fallback until an outfit is created for that ship.
- A whole-bank purchase checks compatibility/race, prices all mounts, refuses insufficient funds/already equipped, replaces each
  turret hierarchy, and switches firing/reload data to the equipped definition. Both HUDs rebuild icons and use the equipped gun's
  recharge timing; refit re-arms the same equipment. Authored class mounts and other ships are never mutated.
- Automatic campaign economy storage now includes outfits and availability (legacy bare point totals still load). Checkpoint saves
  also carry outfits. If this mission's earned points funded purchases beyond its starting balance, `replay_debt` offsets the
  replayed awards instead of giving those upgrades back for free. This is a remake checkpoint accounting rule, not a recovered retail
  rule: retail snapshots do not need it.

Explicit fallbacks / remaining probes:
- An empty availability list offers the hull race's catalogue / compatible loaded guns; non-empty lists restrict it. The actual
  campaign unlock flow is unresolved. One hire per named ID is provisional; no sale/refund mechanism was invented.
- Assigned post skill is the mean of named members with that active skill; generic members are not included in this skill mean.
  Captain/first-mate bonuses and navigation/rigging/spotting effects are not invented; their assignment currently affects combat only.
- Bank changes initially used name-derived weapon families; the shared skirmish batch replaced that fallback with exported `Mount Type`.
  Same-size selectable mounts allow cross-family weapons, with race exclusions; fixed/point-defence mounts remain protected. Missing
  metadata in legacy/mod definitions retains the old family fallback. The destination's full price per mount is charged, no trade-in.
  Native allowed-upgrade sets, mixed/two-deck banks and cost differences/refunds need investigation. AI engagement distance remains
  the authored class distance, not recalculated for upgraded guns.
- Outfits are keyed by hull class + script name (world object ID if unnamed). They survive restart/save and same-identity progression;
  a replacement hull/name needs reassignment. Purchased reserve crew remain available, but automatic transfer on promotion/hull change
  is unresolved. This is not a claim of faithful native crew-keeper serialisation.
- The functional layout is not the native ship-selection screen. Skirmish weapon fitting now shares the editor (see skirmish-setup.md),
  but skirmish crew assignment/budgets, difficulty and fleet profile files remain open.

Compile-only check (`cargo check --tests`) passed. Regression cases added for spending/duplicates/capacity/transfers, outfit round trip,
bank compatibility, live turret/post application, casualty preservation and checkpoint replay accounting; **not executed**.
Owner acceptance: open Mission 3/5/7/9/11's screen; recruit/assign/reserve/move crew, buy a bank upgrade, reject an unaffordable purchase,
resume, verify weapon icons/firing and boarding/repair changes, save/reload/restart, and continue into the next mission.
