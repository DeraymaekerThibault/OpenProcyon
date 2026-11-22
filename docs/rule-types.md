# Mission rule Types

Reference of every condition and action `Type` a `mission.json` rule can use, for mod authors. How rules fit together (folder
layout, the keys of `mission.json`, a worked example) is in `docs/missions.md`; the engine-added extensions are introduced in
`docs/modding.md`. The tables below are generated from the rule schema (`--dump-rule-schema`, `game/src/rules/eval/schema.rs`) by
`tools/gen_rule_types.py`: do not edit between the markers, edit `tools/rule_types_notes.json` and re-run the script.

## Conventions

- A rule is `{"name", "Run Once", "Is Active", "conditions": [...], "actions": [...]}`. All conditions must hold; actions run in order.
  Every condition or action is an object with a `"Type"` and the fields listed below. Field names are exact, spaces, odd capitals
  and typos included.
- **Booleans are the strings `"TRUE"` / `"FALSE"`.** A JSON `true` reads as FALSE. Only the rule field `Is Active` and the task keys
  take JSON booleans.
- **Numbers must be JSON numbers.** A number written as a string reads as 0. Fields marked as a fraction (0..1) are fractions:
  damage, throttle, vulnerability and volume are `0.5`, never `50`.
- A missing field reads as its default. A field marked required is a mistake when absent: its default is a silent no-op or a condition
  that never holds. `--check-mission` reports it.
- A group spec is `Group` (a group of the mission, or an owner when no group has the name) or `Group,Unit` (one named ship).
  A misspelled name has no members: conditions on it are false, actions on it do nothing. `Player0 Group` always means the human player.
- `Equivalence` is `Greater Than`, `Less Than` or `Equal To`; anything else is false. `Equal To` on a measured number is rarely useful.
- Event conditions (kills, captures, entering a volume once per unit) count events, and are true when **more than** the quota has
  been seen since the rule last fired. A quota of 0 therefore means "at least one".
- An unknown `Type` is logged once, a condition is then false and an action does nothing.
- Origin: **retail** Types come from the original game's rule editor. **engine** Types were added by OpenProcyon
  and start with `OP `; the original ignores or rejects them.
- "ignored" in a field's description means the retail editor exports it but the engine never reads it.
- Things written PROVISIONAL are not verified against the original.

<!-- generated:begin -->
## Mission flow, timers and flags

Rule plumbing: run once at start, remember state in flags, measure time, end the mission.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `World Initialize` | condition | retail | none | True on the first evaluation of the world only, then never again. The usual trigger for setup rules. |
| `Flag Condition` | condition | retail | `Flag Name` (flag; required)<br>`Boolean Value` (bool; required; default `FALSE`) | Holds when the flag equals `Boolean Value`. A flag that was never set reads FALSE, so `TRUE` needs a prior `Set Flag Action`. |
| `Set Flag Action` | action | retail | `Flag Name` (flag; required)<br>`Boolean Value` (bool; required; default `FALSE`) | Sets the flag to `Boolean Value`. Flags are created on first use. |
| `Timer Condition` | condition | retail | `Timer Name` (timer; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Time in seconds` (number; required; default `0`) | Compares the timer's seconds with `Time in seconds`. A timer that was never started is false for every comparison. `Equal To` allows a tiny tolerance and is rarely useful; use `Greater Than`. |
| `Start Timer` | action | retail | `Timer Name` (timer; required) | Resets the timer to 0 and runs it; starting a running timer restarts it. Timers stand still during the opening-briefing pause. |
| `Stop Timer` | action | retail | `Timer Name` (timer; required) | Stops the timer advancing; it keeps its last reading (PROVISIONAL: reading after a stop is as the code stores it, not checked against the original). |
| `Goto Next Level` | action (campaign only) | retail | `World` (text; required)<br>`Display Loading String` (bool; ignored; default `FALSE`) | Ends the mission as a victory. `World` is only used to build the end text (the digits give "Mission N complete."); which mission follows is decided elsewhere (PROVISIONAL: see `docs/missions.md`). Only the first end action takes effect. |
| `End Game` | action | retail | `Loser - Custom Message String ID` (message; required)<br>`Winner - Custom Message String ID` (message; ignored)<br>`Use Custom Message` (bool; ignored; default `FALSE`)<br>`Show Stats Screen` (bool; ignored; default `FALSE`) | Ends the mission as a defeat. `Loser - Custom Message String ID` is looked up in `media.json` `messages`; an unknown id shows "Mission failed". The winner message and the stats flag are accepted but ignored. |
| `Add Victory Points for SinglePlayer` | action (campaign only) | retail | `Victory points to be added` (number; required; default `0`) | Adds to the mission's score. Must be a whole non-negative JSON number. |

## Objectives and the star map

Tasks and the objective point are declared in `mission.json` (`tasks`, `points`); these Types change their state. Star map labels are `map_text`.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Set Objective Task Active State` | action | retail | `Objective Task` (task; required)<br>`Active State` (bool; required; default `FALSE`) | Shows or hides the task. A name that is not in `tasks` does nothing. |
| `Set Objective Task Failed State` | action | retail | `Objective Task` (task; required)<br>`Failed State` (bool; required; default `FALSE`) | Marks the task failed (or clears it). A name not in `tasks` does nothing. |
| `Set Objective Task Complete State` | action | retail | `Objective Task` (task; required)<br>`Complete State` (bool; required; default `FALSE`) | Marks the task complete (or clears it). A name not in `tasks` does nothing. |
| `Set Current Objective Point` | action (campaign only) | retail | `Objective Point` (point; required) | Selects the objective point by `Name` from `points`; `NO OBJECTIVE POINT` clears it. Also drops any point attached to a ship. |
| `Set Current Objective Point On Ship` | action (campaign only) | retail | `Group Name` (group; required) | Attaches the objective marker to the ship(s) of the group instead of a fixed point. |
| `Set Current Objective Point Visible On Starmap` | action (campaign only) | retail | `Visible On Starmap` (bool; required; default `FALSE`) | Shows or hides the objective marker on the star map. |
| `Set Map Text Visibility` | action | retail | `Map Text Name` (map_text; required)<br>`Visible On Starmap` (bool; required; default `FALSE`) | Shows or hides one `map_text` label on the star map. |
| `Is Starmap Open` | condition | retail | none | Holds while the star map is open. |
| `TUTORIAL - Pause when starmap opens?` | action (campaign only) | retail | `Pause when starmap opens` (bool; required; default `FALSE`) | Tutorial helper: when TRUE the battle pauses whenever the star map opens. |

## Triggers, positions and counting events

Conditions that look at where ships are, how often they were hit or fired, and what the player has killed or captured.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Is Group In Volume` | condition | retail | `Group Name` (group; required)<br>`Volume Name` (volume; required)<br>`Entire Group` (bool; default `FALSE`) | Level condition: true while any visible, living member is inside the volume. With `Entire Group` TRUE every visible member must be inside. Hidden and dead ships do not count; an empty group is false. (`Entire Group` is read only here, not by `Enter Volume`.) |
| `Enter Volume` | condition | retail | `Group Name` (group; required)<br>`Volume Name` (volume; required) | Edge condition: true when a member comes in. In a run-once rule it stays true once the group has entered and is still inside (level-triggered, PROVISIONAL); in a repeating rule it is true for one frame per entry. A ship that was already inside at start does not count until it has left once. |
| `Exit Volume` | condition | retail | `Group Name` (group; required)<br>`Volume Name` (volume; required) | True for one frame when the group goes from having a visible member inside to having none inside. |
| `All Units from Group have entered trigger volume Once` | condition | retail | `Group Name` (group; required)<br>`Volume Name` (volume; required)<br>`Player Name` (player; ignored)<br>`Object that have already entered - Size` (number; ignored; default `0`) | True once every living member has been inside the volume at some time (not necessarily together). The extra exported fields are ignored. |
| `Unit from Group enters trigger volume ( Once per Unit )` | condition | retail | `Group Name` (group; required)<br>`Volume Name` (volume; required)<br>`Team Name` (team; required)<br>`Number of objects entered, waiting to report` (number; ignored; default `0`)<br>`Object that have already entered - Size` (number; ignored; default `0`) | Event: each unit counts the first time it is inside; true when at least one new unit has entered since the rule last fired. `Team Name` is read as the team credited, not a filter on the ships (PROVISIONAL). The other exported fields are ignored. |
| `Group to Group Distance` | condition | retail | `GroupA Name` (group; required)<br>`GroupB Name` (group; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Distance` (number; required; default `0`) | Compares the smallest distance between any visible living member of A and of B, in game units, with `Distance`. False if either group has no such member. |
| `Unit is Within any Nebula` | condition | retail | `Unit Name` (group; required) | True while any living member is inside any nebula polygon of the mission. |
| `Group Destroyed` | condition | retail | `Group Name` (group; required) | True when the group has members and all of them are dead. A misspelled or empty group is never destroyed. A ship that left the group by a transfer is no longer counted. |
| `Group has X members` | condition | retail | `Group Name` (group; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`number` (number; required; default `0`) | Compares the number of living members with `number` (the field really is named `number`). |
| `Does Group Contain Unit Name` | condition | retail | `Unit Name` (group; required)<br>`Exists?` (bool; required; default `FALSE`) | `Unit Name` is `Group,Unit`. With `Exists?` TRUE, true while that living unit is owned by that group; with FALSE, true when it is not. |
| `Group/Unit contains no mission essential ships` | condition | retail | `Group Name` (group; required) | True when the owner named has no living primary ship. This is the loss rule for the player's fleet. |
| `Group Under Attack` | condition | retail | `Group Name` (group; required) | True when any member has been hit since its hit count was last reset (PROVISIONAL). Pair with `Reset Hit Count`. |
| `Group/Unit Hit at least x Times` | condition | retail | `Group/Unit Name` (group; required)<br>`Number Of Times Hit` (number; required; default `0`) | True when the members' total hit count reaches `Number Of Times Hit` (at least, not more than). Counted until `Reset Hit Count`. |
| `Group/Unit Hit at least x Times by Player ( with eqivalence )` | condition | retail | `Group/Unit Name` (group; required)<br>`Player Name` (player; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Number Of Times Hit` (number; required; default `0`) | Hits on the members by one player, compared with `Number Of Times Hit` using `Equivalence`. The retail name has the typo `eqivalence`; keep it. |
| `Player has hit Group/Unit at least x Times` | condition | retail | `Player Name` (player; required)<br>`Group/Unit Name` (group; required)<br>`Number Of Times Hit` (number; required; default `0`) | Hits the player landed on the group, at least `Number Of Times Hit`. |
| `Reset Hit Count` | action | retail | `Group/Unit Name` (group; required) | Clears the hit counts the hit conditions read. |
| `Group/Unit Fired X Shots` | condition | retail | `Group/Unit Name` (group; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Number Of Rounds Fired` (number; required; default `0`) | Compares the members' total shots fired since the last `Reset Shots Fired Count` with `Number Of Rounds Fired`. |
| `Reset Shots Fired Count` | action | retail | `Group/Unit Name` (group; required) | Zeroes the shot counter of the group's ships. |
| `Group/Unit has >,<,= X damage` | condition | retail | `Group/Unit Name` (group; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Damage Percent` (number; fraction 0..1; required; default `0`) | Compares the most damaged member's damage with `Damage Percent`. Damage is a fraction 0..1 (0.5 is half), despite the name. False for an empty group. |
| `Group/Unit Vital Section has >,<,= X damage` | condition | retail | `Group/Unit Name` (group; required)<br>`Vital Section` (text; ignored)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Damage Percent` (number; fraction 0..1; required; default `0`) | Vital sections are not modelled: the whole-ship damage of the worst member stands in (PROVISIONAL). `Vital Section` is ignored. |
| `Unit Flag Texture` | condition | retail | `Unit Name` (group; required)<br>`Flag Type` (text; required)<br>`Boolean` (bool; required; default `FALSE`) | With `Boolean` TRUE, true when every member flies the flag texture `Flag Type`; with FALSE, the negation. The group may include ships that left it. |
| `Player Killed A Object` | condition | retail | `Player Name` (player; required)<br>`World Object Type` (one of `Asteroid`, `Ship`; required) | Event per object the player destroyed: `World Object Type` is `Ship` or `Asteroid`. In a repeating rule it fires once per kill. |
| `Player Vs Player capture count` | condition | retail | `Player Name A` (player; required)<br>`Player Name B` (player; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Number Of Captures` (number; required; default `0`) | Compares the lifeboats and bases the first player saved or captured from the second with `Number Of Captures`. False when either player is unknown. |
| `Player has no lifeboats` | condition | retail | `Player Name` (player; required) | True when no lifeboat is owned by the player, or the player is unknown. |

## Groups and ships: spawn, ownership, appearance

Actions that change what a ship is: owner, name, visibility, whether it exists. A group spec is `Group` (a TWT group or an owner) or `Group,Unit` (one named ship).

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Destroy Group/Unit` | action | retail | `Group Name` (group; required) | Kills the group at once (core to 0). |
| `Transfer Group/Unit to Group` | action | retail | `Group/Unit to transfer` (group; required)<br>`Target Group` (group; required) | Hands the ships to the target owner. |
| `Set Group/Unit Owner` | action | retail | `Group/Unit Name` (group; required)<br>`New Owner` (player; required) | Same effect as the transfer action, with a player name. `NO PLAYER` is a retail placeholder for unset. |
| `Set Ship Name` | action | retail | `Group/Unit Name` (group; required)<br>`New Ship Name` (ship_name; required) | `New Ship Name` is an id in `media.json` `ship_names`; an unknown id logs a warning and leaves the name. |
| `Set Ships Flag Texture` | action | retail | `Group Name` (group; required)<br>`Flag Texture` (text; required) | Changes the flag texture of the ships. |
| `Set Ship Banner Type` | action | retail | `Group/Unit Name` (group; required)<br>`Banner Type` (text; required) | Changes the banner type of the ships. |
| `Set Group/Unit Visibility` | action | retail | `Group/Unit Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Hides or shows the ships. Hidden ships do not trigger volume or distance conditions. |
| `Set Group/Unit Cloak State` | action | retail | `Group Name` (group; required)<br>`Cloak State` (bool; required; default `FALSE`)<br>`AI Can Override` (bool; required; default `FALSE`) | Cloaks or uncloaks. `AI Can Override` lets the computer captain uncloak on its own. |
| `Teleport Group/Unit` | action | retail | `Group/Unit Name` (group; required)<br>`Point Set Name` (point_set; required) | Moves the group to the first position of the point set; the others are ignored. |
| `Set Group/Unit Mission Essential` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Accepted and ignored: essential ships are the primary ships (see `Group/Unit contains no mission essential ships`). |
| `Set Group/Unit Movable` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Allows or forbids the ships to move. |
| `SetCollidable` | action | retail | `Group/Unit Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Switches collisions for the ships. |
| `Set Lifeboat Creation State` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Allows or forbids the ships to release lifeboats when destroyed. |

## Orders, movement and AI

Scripted orders given to computer ships. The human's own course and throttle are left alone by most of these.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Group to follow path` | action | retail | `Group Name` (group; required)<br>`Path Name` (path; required)<br>`Find Closest Point` (bool; required; default `FALSE`)<br>`Follow Mode` (one of `to end`, `loop`, `teleport loop`, `end to end`; required) | Makes the group follow `Path Name`. `Follow Mode` is `to end`, `loop`, `teleport loop` or `end to end`; only `loop` loops a ship, the others differ for meteor groups. `Find Closest Point` starts at the nearest point instead of the first. |
| `Set Group Throttle Percent` | action | retail | `Group Name` (group; required)<br>`Throttle Percent` (number; fraction 0..1; required; default `0`) | Throttle as a fraction 0..1. Not applied to the human's ship (a value of 0 stops the player's scripted course only). |
| `Set Max Throttle Percent (Max user settable)` | action | retail | `Group Name` (group; required)<br>`Throttle Percent` (number; fraction 0..1; required; default `0`) | Caps the throttle as a fraction 0..1, also for the human. |
| `Set Ship Top Speed Percentage` | action | retail | `Group Name` (group; required)<br>`Throttle Speed` (number; fraction 0..1; required; default `0`) | Treated as the same cap as the max throttle (PROVISIONAL), fraction 0..1. |
| `GroupA to attack GroupB` | action | retail | `Attack Group` (group; required)<br>`Target Group` (group; required) | Adds the target group to the attackers' orders. |
| `GroupA to Ram GroupB` | action | retail | `Group Name` (group; required)<br>`Group Unit Target` (group; required) | Attack order flown bow-on at emergency speed; contact damage comes from the normal collision rules. |
| `Is Group A attacking Group B` | condition | retail | `GroupA Name` (group; required)<br>`GroupB Name` (group; required) | True when a member of A has B in its orders, or fired at B within the last 6 seconds. |
| `Clear all AI commands` | action | retail | `Group/Unit Name` (group; required) | Drops the ships' scripted orders. The human's own course and throttle stay. |
| `Set Group/Unit Hold Position` | action | retail | `Group/Unit Name` (group; required)<br>`Boolean` (bool; default `TRUE`) | Holds (clears the waypoints). A missing `Boolean` means TRUE, unlike most boolean fields. |
| `Set Group/Unit AI Stance` | action | retail | `Group/Unit Name` (group; required)<br>`AI Stance` (one of `Stance_Dummy`, `Stance_Neutral`, `Stance_Defensive`, `Stance_Aggressive`, `Stance Persistant`, `Stance Persistent`, `AI STANCE`; required) | Stances: `Stance_Neutral`, `Stance_Defensive`, `Stance_Aggressive`, `Stance Persistant` (sic) and others listed. |
| `Set Group/Unit AI Captain` | action (campaign only) | retail | `Group/Unit Name` (group; required)<br>`AI Stance` (one of `Captain_Human`; required) | Only `Captain_Human` is meaningful: it gives the human control of the ships and makes them the primary. Other captains keep the stance AI. |
| `Set Group/Unit Warning Shot Mode` | action | retail | `GroupUnit Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Makes the ships fire warning shots. |
| `Set Group/Unit Border Zone` | action | retail | `Group/Unit Name` (group; required)<br>`Polygon Name` (volume; required) | Keeps the ships inside a polygon (a `volume`); see the return-point action. |
| `Clear Group/Unit Border Zone` | action | retail | `Group/Unit Name` (group; required) | Removes the border zone. |
| `Set Group/Unit Border Zone Return Point` | action | retail | `Group/Unit Name` (group; required)<br>`Pointset Name` (point_set; required) | Where a ship that leaves its border zone is sent back to: the first position of the point set. |

## Damage and vulnerability

Fractions, not percentages: `0.5` is half. A JSON number is required.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Damage Group/Unit by X percent` | action | retail | `Group Name` (group; required)<br>`Percent` (number; fraction 0..1; required; default `0`) | `Percent` is a fraction 0..1 (1 is 100 percent), not 0..100. Each hull section loses that share of its current hit points through the normal hit routine, so it respects vulnerability and a fraction below 1 does not destroy the core by itself. |
| `Set Group/Unit Vulnerability` | action | retail | `Group Name` (group; required)<br>`Invulnerable Percent` (number; fraction 0..1; required; default `0`) | `Invulnerable Percent` is a fraction 0..1 of incoming damage the ship ignores; 1 is invulnerable. |
| `Dragon - Set AI Stance` | action | retail | `Group/Unit Name` (group; required)<br>`AI Stance` (one of `Stance_Dummy`, `Stance_Neutral`, `Stance_Defensive`, `Stance_Aggressive`, `Stance Persistant`, `Stance Persistent`, `AI STANCE`; required) | Stance for the dragon (space animal) boss, same values as the ship stance. |
| `Dragon - Set Damage Threshold` | action | retail | `Group Name` (group; required)<br>`Damage Threshold` (number; fraction 0..1; required; default `0`) | Damage fraction 0..1 at which the dragon changes behaviour (PROVISIONAL). |

## Docking, towing and boarding

Docking itself is simulated by `docking.rs`; these Types request it or toggle what is allowed.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Dock Ships` | action | retail | `Docking Group` (group; required)<br>`Target to Dock to` (group; required) | Orders the first group to dock with the second. |
| `Group/Unit Docked` | condition | retail | `Dockers Group/Unit Name` (group; required)<br>`Targets Group/Unit Name` (group; required) | True when any docker is docked at any target. |
| `Group/Unit Is Docked` | condition | retail | `Dockers Group/Unit Name` (group; required)<br>`Targets Group/Unit Name` (group; required) | Same as `Group/Unit Docked`. |
| `Set Group/Unit Dockable` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Allows or forbids docking at the ships. |
| `Set Group/Unit Boardable` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Allows or forbids boarding the ships. |
| `Toggle Island Repair When Docked` | action | retail | `Group Name` (group; required)<br>`Repair when docked` (bool; required; default `FALSE`) | Island (base) repairs ships docked to it when TRUE. |
| `Set Dock Time` | action | retail | `Docking GroupUnit` (group; required)<br>`Dock Time` (number; required; default `0`) | How long docking takes, in seconds. |
| `Tow Ship` | action | retail | `Docking Group` (group; required)<br>`Target to Tow` (group; required) | Starts a tow: the first group tows the second. |
| `Break Tow` | action | retail | `TowerGroupA` (group; required)<br>`TowerGroupB` (group; required) | Ends a tow between the two groups, whichever one is the tower. |
| `Is Ship In Tow` | condition | retail | `Towing Group` (group; required)<br>`Target to Tow` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | With `Boolean` TRUE, true while a member of the first group tows a member of the second; FALSE negates. |
| `Set Group/Unit Towable` | action | retail | `Group Name` (group; required)<br>`Boolean` (bool; required; default `FALSE`) | Allows or forbids towing the ships. |

## Camera and NIS cutscenes

A cutscene (NIS) is `NIS Start`, camera Types, then `NIS End`. Use the engine-added `OP Wait` between steps to pace it. Camera motion is smoothed by the engine, so the exact feel is PROVISIONAL.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `NIS Start` | action (campaign only) | retail | `All Objects Visible` (bool; ignored; default `FALSE`)<br>`Open NIS bars instantly` (bool; ignored; default `FALSE`) | Begins a cutscene: letterbox bars on, camera under rule control. The exported visibility and instant-bars fields are ignored. |
| `NIS End` | action (campaign only) | retail | none | Ends the cutscene, returns the camera and makes all objects visible again. |
| `NIS Set Camera Path` | action (campaign only) | retail | `Path Name` (path; required)<br>`Jump to start` (bool; ignored; default `FALSE`) | Camera follows `Path Name` (speed default 60 until a speed action). `Jump to start` is ignored. |
| `NIS Set Camera Speed` | action (campaign only) | retail | `Max Velocity` (number; required; default `0`)<br>`Acceleration` (number; ignored; default `0`) | Sets the speed along the camera path. `Acceleration` is ignored. |
| `NIS Set Transition Camera Speed` | action (campaign only) | retail | `Max Velocity` (number; required; default `0`)<br>`Acceleration` (number; ignored; default `0`) | Same arm as `NIS Set Camera Speed`. |
| `NIS Focus camera on Group/Unit` | action (campaign only) | retail | `GroupUnit Name` (group; required) | The camera looks at the unit; clears a point focus and an attached camera. |
| `NIS Focus camera on Point` | action (campaign only) | retail | `Point Set Name` (point_set; required) | The camera looks at the point set. |
| `NIS Attach Camera` | action (campaign only) | retail | `Group/Unit Name` (group; required)<br>`Distance` (number; required; default `0`)<br>`Angle YZ` (number; required; default `0`)<br>`Angle XY` (number; required; default `0`) | Places the camera at `Distance` and the two angles relative to the unit and keeps it there. |
| `NIS Position Camera Relative to Object` | action (campaign only) | retail | `Unit Name` (group; required)<br>`Distance` (number; required; default `0`)<br>`Angle YZ` (number; required; default `0`)<br>`Angle XY` (number; required; default `0`)<br>`Jump to point` (bool; ignored; default `FALSE`) | A fixed offset from the unit, shown like an attached camera (PROVISIONAL). `Jump to point` is ignored. |
| `NIS Focus on Main Ship` | action (campaign only) | retail | none | Focus on the human's flagship. |
| `NIS Zoom` | action (campaign only) | retail | `FOV` (number; required; default `0`)<br>`Speed` (number; required; default `0`) | Field of view (radians, vertical, PROVISIONAL) and how fast it changes per second. |
| `NIS Toggle NIS mode Gun Accuracy` | action (campaign only) | retail | `Use NIS Gun Accuracy` (bool; required; default `FALSE`) | Uses the cutscene gun accuracy while in a NIS. |
| `NIS Toggle All Objects Visibility` | action (campaign only) | retail | `All Objects Visible` (bool; required; default `FALSE`) | Shows or hides every object during a cutscene. |
| `Focus Camera On Group` | action (campaign only) | retail | `GroupUnit Name` (group; required)<br>`Distance` (number; required; default `0`)<br>`Relative Angle` (number; required; default `0`)<br>`Use Transition` (bool; ignored; default `FALSE`) | Orbit camera around a group at a distance and relative angle. `Use Transition` is ignored. |

## Sound, speech, music and HUD

Speech events come from `media.json` `speech` (or the mission's own `opening_speech`).

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Play speech event` | action | retail | `Speech Event Name` (speech_event; required) | Queues a speech event from `media.json` `speech`. Lines play one after another. |
| `Speech Event Played Once` | condition | retail | `Speech Event Name` (speech_event; required) | True after the event has played. |
| `Speech Event Not Played Yet` | condition | retail | `Speech Event Name` (speech_event; required) | True while the event has neither played nor been queued nor is playing. |
| `Play Music Track` | action | retail | `File Name` (text; required)<br>`Crossfade transition` (bool; ignored; default `FALSE`)<br>`Fade In Time ( secs )` (number; required; default `0`)<br>`Fade Out Time ( secs )` (number; required; default `0`)<br>`New Volume ( 0 to 1 )` (number; fraction 0..1; required; default `0`) | Plays `File Name` with fades. Volume is a fraction 0..1. `Crossfade transition` is ignored. |
| `Play Special Effect` | action | retail | `File Name` (text; required)<br>`Play As Dialog` (bool; ignored; default `FALSE`) | Plays a sound file by name. `Play As Dialog` is ignored. |
| `Crew Speech - Toggle On Off` | action (campaign only) | retail | `Crew Speech State State` (bool; required; default `FALSE`) | Turns the crew's spoken warnings on or off (the field name has `State State`, as exported). |
| `Crew Speech - Helm Off Course` | action (campaign only) | retail | none | Triggers the helmsman's off-course line. |
| `Open Crew and Arms Screens` | action (campaign only) | retail | none | Opens the crew and arms screens (tutorial). |
| `Open HUD Texture Overlay` | action (campaign only) | retail | `Texture Base Name` (text; required) | Shows a HUD overlay texture by base name. |
| `Close HUD Texture Overlay` | action (campaign only) | retail | none | Hides it. |
| `Open Weapon Bar` | action (campaign only) | retail | `OpenState` (bool; required; default `FALSE`) | Opens or closes the weapon bar (`OpenState`). |
| `Set Radar Active State` | action (campaign only) | retail | `Radar Active` (bool; required; default `FALSE`) | Turns the radar on or off. |

## Environment, nebula, animals and asteroids

Scenery and hazards. Nebula and current setups are read from the mission's `environment` data.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Setup Space Animal Flock` | action | retail | `Group Name` (group; required)<br>`Path Name` (path; required)<br>`Find Closest Point` (bool; required; default `FALSE`)<br>`Follow Mode` (one of `to end`, `loop`, `teleport loop`, `end to end`; required)<br>`Velocity` (number; required; default `0`) | Gives the flock group a path and a speed. Fields are as for `Group to follow path` plus `Velocity`. |
| `Setup Asteroid Belt` | action | retail | `Group Name` (group; required)<br>`Path Name` (path; required)<br>`Find Closest Point` (bool; required; default `FALSE`)<br>`Follow Mode` (one of `to end`, `loop`, `teleport loop`, `end to end`; required)<br>`Velocity Lower m/sec` (number; required; default `0`)<br>`Velocity Upper m/sec` (number; required; default `0`)<br>`Tumble Lower Rads/sec` (number; required; default `0`)<br>`Tumble Upper Rads/sec` (number; required; default `0`) | Turns the group into a moving belt along the path, with a random speed in `Velocity Lower/Upper m/sec` and tumble in `Tumble Lower/Upper Rads/sec`. |
| `Set Group Space Objects velocity` | action | retail | `Group Name` (group; required)<br>`Velocity` (number; required; default `0`) | Changes the speed of a group of moving space objects (rocks, animals). |
| `Set Nebula Fadeout Distance` | action (campaign only) | retail | `Fadeout Distance` (number; required; default `0`) | Distance over which the nebula fades out. |
| `Set Nebula to Lock Objects` | action (campaign only) | retail | `Trap` (bool; required; default `FALSE`) | When TRUE ships cannot leave the nebula (`Trap`). |
| `Set Is In Calyan Abyss` | action (campaign only) | retail | `Boolean Value` (bool; required; default `FALSE`) | Sets a mission-wide "in the Calyan Abyss" state that other engine systems read. PROVISIONAL: what exactly changes is not described here. |
| `Create/Release Event Effect` | action | retail | `Point Set` (point_set; required)<br>`Effect Name` (text; required)<br>`Boolean State` (bool; required; default `FALSE`) | Creates or releases a named effect at a point set. `Effect Name` is a free text that must match an engine effect. |
| `Mission 9 - Do Dark Matter Explosion` | action | retail | `Affected Area` (volume; required) | Mission 9 only: the dark-matter blast in the area. |
| `Mission 9 - Teleport Longboat` | action (campaign only) | retail | `Longboat Group Name` (group; required) | Mission 9 only: moves the longboat group. |
| `Mission 9 - If Mortar explodes within Area` | condition (campaign only) | retail | `Volume Name` (volume; required) | Mission 9 only: true after a mortar burst inside the volume, and stays true afterwards. |

## Fleets, alliances and formations

Player names are the owners listed in `players`; a leading `Player0 Group` always means the human player.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Set FleetFormation Type` | action | retail | `Player Name` (player; required)<br>`Formation Type` (one of `None`, `Custom`, `LineAbreast`, `EchelonRight`, `EchelonLeft`, `Column`, `Diamond`, `Convoy`; required)<br>`Hold Formation` (bool; required; default `FALSE`) | Sets the fleet's formation (None, Custom, LineAbreast, EchelonRight, EchelonLeft, Column, Diamond, Convoy) and whether it is held. |
| `Set Fleet Hold Formation` | action | retail | `Player Name` (player; required)<br>`Hold Formation` (bool; required; default `FALSE`) | Only changes whether the formation is held. |
| `Set Fleet Primary Ship` | action (campaign only) | retail | `Group/Unit Name` (group; required) | Makes the unit the fleet's primary ship. |
| `Set Fleet Hold Fire` | action | retail | `PLAYER FLEET` (player; required)<br>`HOLD FIRE` (bool; required; default `FALSE`) | Fleet holds or releases fire. |
| `Set alliance between PlayerA and PlayerB to TRUE/FALSE` | action | retail | `PLAYER A` (player; required)<br>`PLAYER B` (player; required)<br>`SET ALLIANCE TO` (bool; required; default `FALSE`) | TRUE allies the two players, FALSE makes them hostile (both directions). |

## Skirmish and teams

Used by skirmish maps. The four end conditions are not read by the rule interpreter but by `victory.rs` (the handler is `victory`) and belong in a map's `victory` list.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `Team Member Enters Volume` | condition | retail | `Team Name` (team; required)<br>`Volume Name` (volume; required) | True while a living visible ship of the team is inside the volume (level, not an edge). |
| `Team has captured a ship from Group/Unit` | condition | retail | `Team Name` (team; required)<br>`Group/Unit` (group; required)<br>`Number Of Captures` (number; required; default `0`) | Event quota: true when more than `Number Of Captures` of the group's ships were captured by the team since the rule last fired. So 0 means "at least one". |
| `Team has destroyed a ship from Group/Unit` | condition | retail | `Team Name` (team; required)<br>`Group/Unit` (group; required)<br>`Number Of Ships Destroyed` (number; required; default `0`) | Event quota like the capture one: more than `Number Of Ships Destroyed`. |
| `Team has X points` | condition | retail | `Team Name` (team; required)<br>`Points` (number; required; default `0`) | True once the team's points reach `Points` (at least). |
| `Grant Team X Points` | action | retail | `Team Name` (team; required)<br>`Points` (number; required; default `0`) | Adds points to the team (truncated to an integer). |
| `Setup Team Objective` | action | retail | `Team Name` (team; required)<br>`Objective Point` (point; required)<br>`Objective Task` (task) | Skirmish: sets the team's objective point and task text for the star map. The winner is decided by the map's end rules, not by this. |
| `Team X Wins` | action | retail | `Team Name` (team) | Accepted and does nothing in the interpreter (the end rules of the map decide). |
| `Remaining Team Wins` | action | retail | none | Accepted and does nothing in the interpreter. |
| `No Human Controlled Ships Remain` | condition (end rule) | retail | none | Skirmish end condition read by `victory.rs`: no human ship is left. |
| `No Team Has Ships` | condition (end rule) | retail | none | Skirmish end condition read by `victory.rs`. |
| `Skirmish Game Complete` | condition (end rule) | retail | none | Skirmish end condition read by `victory.rs`: only one side (allies count as one) has ships, or no human ship is left. |
| `Team Game Complete` | condition (end rule) | retail | none | Skirmish end condition read by `victory.rs`. |
| `Team X has no ships` | condition (end rule) | retail | `Team Name` (team; required) | Skirmish end condition read by `victory.rs`. |

## State-init setup actions

Retail missions place their world with these actions at load time. Most of them are applied while the mission spawns (`campaign.rs`, `environment.rs`) rather than when a rule fires. A hand-written mission normally puts ships, nebulae and currents in `ships` and `environment` instead.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `*State Init* Setup Ship` | action | retail | `World Object ID` (number; required; default `0`)<br>`Ship Name` (text)<br>`Ship Path` (path)<br>`Follow Mode` (one of `to end`, `loop`, `teleport loop`, `end to end`)<br>`AI Stance` (one of `Stance_Dummy`, `Stance_Neutral`, `Stance_Defensive`, `Stance_Aggressive`, `Stance Persistant`, `Stance Persistent`, `AI STANCE`)<br>`Player/Owner` (player)<br>`Primary Ship` (bool; default `FALSE`)<br>`Crew Skill Level` (text)<br>`Boardable` (bool; default `FALSE`)<br>`Localized Ship Name` (text) | Placement-time AI stance and path of the ship with `World Object ID`. The name, owner, primary and boardable fields are applied at spawn, not here. |
| `*State Init* Setup Island` | action | retail | `World Object ID` (number; required; default `0`)<br>`Combat Strength` (number; default `0`)<br>`Player/Owner` (player)<br>`Gunnery Level` (one of `CREW SKILL LEVEL`, `Green`, `Average`, `Elite`)<br>`AI Stance` (one of `Stance_Dummy`, `Stance_Neutral`, `Stance_Defensive`, `Stance_Aggressive`, `Stance Persistant`, `Stance Persistent`, `AI STANCE`) | Applied at spawn (owner, combat strength); the action itself does nothing when a rule runs it. |
| `*State Init* Setup Nebula` | action | retail | `World Object ID` (number; ignored; default `0`)<br>`New Nebula Name` (text)<br>`Polygon Name` (volume)<br>`Lightning On/Off` (bool; default `FALSE`)<br>`Lightning Blast Recharge Time` (number; default `0`)<br>`Meteors On/Off` (bool; default `FALSE`)<br>`Meteor Strike Recharge Time` (number; default `0`)<br>`Nebula Cloud Effect Name` (text)<br>`Solar Storm Effect Name` (text)<br>`Meteor Shower Effect Name` (text)<br>`Nebula Cloud Point Set Name` (point_set)<br>`Solar Storm Point Set Name` (point_set)<br>`Meteor Shower Point Set Name` (point_set)<br>`Rotational Winds On/Off` (bool; default `FALSE`)<br>`Wind Magnitude` (number; default `0`)<br>`Nebula Cloud Energy Drain On/Off` (bool; default `FALSE`)<br>`Nebula Occlusion On/Off` (bool; default `FALSE`)<br>`Solar Storm Wind Damage Frequency` (number; default `0`)<br>`Ambient sound max distance` (number; default `0`) | Applied at spawn by the environment code; running the action does nothing. Read PROVISIONAL for the weather fields' units. |
| `*State Init* Setup Etherium Current` | action | retail | `World Object ID` (number; ignored; default `0`)<br>`Etherium Name` (text; ignored)<br>`Etherium Path` (path; required; a broken reference is only a warning) | Adds a current along `Etherium Path`. A path the mission does not have only logs a debug message (a retail mission has such a break). |

## Engine-added OP types

Not in the retail game. Every name starts with `OP `. See `docs/modding.md`, "Mission rule extensions", for sequences, rule activation and self-completing tasks.

| Type | Kind | Origin | Fields | What it does |
|---|---|---|---|---|
| `OP Any Of` | condition | engine | `Conditions` (condition_list; required; default `[]`) | True if any condition in `Conditions` is true; an empty list is false. Every member is evaluated. |
| `OP Not` | condition | engine | `Condition` (condition; required) | Inverts the inner condition's answer of this frame. Not an "is not inside" test for edge conditions: use a level condition. |
| `OP Counter Condition` | condition | engine | `Counter Name` (counter; required)<br>`Equivalence` (one of `Greater Than`, `Less Than`, `Equal To`; required)<br>`Value` (number; required; default `0`) | Compares a counter (0 when never set) with `Value`. |
| `OP Set Counter` | action | engine | `Counter Name` (counter; required)<br>`Value` (number; required; default `0`) | Sets a counter to `Value`. |
| `OP Add To Counter` | action | engine | `Counter Name` (counter; required)<br>`Value` (number; required; default `0`) | Adds `Value` (may be negative) to a counter. |
| `OP Wait` | action | engine | `Seconds` (number; required; default `0`) | Pauses the rest of this rule's action list for `Seconds`. Outside a rule's action list it does nothing. |
| `OP Wait Until Speech Done` | action | engine | `Speech Event Name` (speech_event) | Pauses until the speech event is neither queued nor playing; an empty name waits for all speech. Put it after the `Play speech event` it waits for. |
| `OP Set Rule Active` | action | engine | `Rule Name` (rule; required)<br>`Boolean` (bool; required; default `FALSE`) | Activates or deactivates every rule of that name. A deactivated rule stops a sequence that is waiting. |
<!-- generated:end -->
