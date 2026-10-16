# World clicks, the options menu and docking

Source: observed behaviour of the original game and its retail data (`hud.hdt`, WOTs), described in our own words; **[?]** marks inferences not yet confirmed.

## 1. There is no "right-click menu" in the sense of a text popup
The retail game has **context cursors** (left click) and an **icon "options menu"** (right click on a ship). Both live in the HUD
class family whose data is in `hud.hdt` (`Cursor_*`, `OPTIONS_MENU_*`).

### Cursors (hud.hdt components)
Enum order: 0 DoNothing, 1 MOVEMENT_OUT_OF_BOUNDS, 2 PrimaryShip_Select, 3 PrimaryShip_AttackShip,
4 PrimaryShip_SimpleMove, 5 PrimaryShip_WaypointMove, 6–8 SetLobTarget (GravCharge/StarMortar/NovaMortar), 9–11 the same
`_HAS_NO_FIRING_SOLUTION`, 12 SecondaryShip_Select, 13 SecondaryShip_AttackShip, 14 SecondaryShip_Move,
15 Cant_Do_Anything_To_WorldObject_Under_Cursor, 16 Dock_With_WorldObject_Under_Cursor, 17 Focus_On_WorldObject_Under_Cursor.
Each has an `Offset_Cursor_*` hot-spot in hud.hdt. Further HUD sprites registered next to them: `Primary/Secondary_ShipSelected_Icon(+_West/East/North/South)`,
`ShipTargeted_Icon_Enemy/Friendly`, `ShipNameBackdrop_*`.

### Hover decision — one intent code per frame from the object under the cursor
Intent codes (not the cursor enum): 0 none, 0xE nothing possible, 0xF attack with primary, 0x10 attack with the selected secondary,
0x11 focus camera on the already selected ship, 0x12 select the primary, 0x13 select another own ship, 0x14 **dock**.
Order of tests, with the cursor object `o`:
1. no object → 0. A HUD mode flag (settings, probably the lob-target mode) → 0x11.
2. `o` is **hostile to the selection** (relation test) → attack: 0xF when the selected ship is the primary, 0x10 when it is a
   secondary; for a base/island (bit 15) with no attackable part under the cursor → 0xE.
3. otherwise: `o` is the **selected ship** → 0x11; `o` is the **primary ship** (while another ship is selected) → 0x12; `o` is another own selectable ship → 0x13;
   **if the selected ship can dock with `o` and is in range → 0x14**; else 0xE.
Dock-ability test: relation between the two players is **allied (value 3 of the 2-bit matrix)**, `o` has the
*dockable* object flag (bit 2; the ship and island custom-info constructors set it on every ship and island),
and the target's dock-point keeper is enabled ("Can Be Docked With"), has at least one dock point, its owner is not
destroyed and the keeper is not disabled. For a base (bit 15) the cursor must also be on the part that carries a dock point.
**Keeper defaults**: a ship's keeper is dockable *and* repairs ("Repair When
Docked With") only when its WOT `Is Tender` is true; islands/bases always. "Set Group/Unit Dockable" writes only the dockable flag.
**There is no dock range.** The range check is a *sight* test (also used to enable the menu's Focus item): the AI entity's `Default SightRange` (400-600
for most ships; crew multiplier applied elsewhere). It passes when the surface distance (centre distance minus both
collision radii) from the selected ship, or any ship of its group, is below that ship's sight range and the object is not hidden.

### Click handler
Event kind 0 (**left**): intent 0xF/0x10 → attack command (command id 0x10); 0x11 → camera focus;
0x12/0x13 → select ship; **0x14 → `DockCommand` mode 2 with (selected ship, object)**. So docking is a plain left click on the ally.
The [fleet-control notes](fleet-controls.md) distinguish the selected ship from the primary.
Do not substitute primary ownership for the selected actor in world-click commands.
Event kind 1 (**right**): open the options menu for the object under the cursor. Camera drag with the right button is separate
[? click vs drag threshold not checked; the repo already rotates on right-drag per the manual].

## 2. Options menu (builders, layout)
A small panel at the cursor drawn from four corner sprites plus icon buttons; the HUD sprite order gives the item ↔ icon mapping
(item kind → hud component 0x38..0x44):

| kind | HUD component | command created on click |
|---|---|---|
| 7 | `OPTIONS_MENU_BUTTON_DOCK` | `DockCommand` mode 2 |
| 5 / 6 | `…TOW_START` / `…TOW_STOP` | `DockCommand` mode 0 / 1 (same class, mode = tow / break tow / dock) |
| 3 / 4 | `…GRAPPLE_START` / `…GRAPPLE_STOP` | `GrappleShipCommand` (id 0x1C) mode 0 / 1 |
| 8 / 9 | `…BOARD_START` / `…BOARD_STOP` | `BoardShipCommand` (id 0x1B) mode 0 / 1 |
| 1 | `…FOCUS` | camera focus |
| 0 | `…AI_LOCK_POSITION` / `…AI_NOT_LOCK_POSITION` (chosen by the ship's current hold state) | `ChangeHold` command (id 0x14, bool) |
| 2 | `…STOP` | cancel-ship-actions command (id 0x12) |
| 10 / 11 | `…CLOAK_START` / `…CLOAK_STOP` | `CloakShipCommand` (id 0x1D) mode 0 / 1 |

Three variants, chosen when the menu opens (background corner sprites in brackets):
- **Enemy ship** (says hostile, HUD flag 0x95D clear) [`OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_*`]: Focus (always), Board start/stop, Grapple start/stop.
  - Board shows *stop* (kind 9) when a boarding relation (docker/dockee/type 1) exists, otherwise *start* (kind 8), enabled by:
    not allied, no fight yet, both objects carry the boardable flag (bit 3), the *target's* Can Be Boarded flag is on, the boarder is not a base, and a grapple
    holds (ship target) / the boarder is in range of a boarding point (base target); see [boarding.md](boarding.md) §3.
  - Grapple shows *stop* (kind 4) when a grapple relation exists, otherwise *start* (kind 3), enabled when the primary has a grapple gun whose
    live grapples are fewer than its limit.
- **Own primary ship** [`…SELECTED_SHIP_BACKGROUND_*`]: Focus, Stop, Hold-position toggle (enabled for ships whose AI can hold), Cloak start/stop
  (cloak items only when the ship's cloak state allows it).
- **Any other ship/base** (friendly or neutral) [`…FRIENDLY_SHIP_BACKGROUND_*`]: Focus, Tow start/stop (stop when a tow link between them exists;
  start enabled when the target is towable (flag bit 8 and a towable flag) and no link exists), **Dock** (enabled by the dock test above).
Strings for these items are icons only (no text). Selecting an item closes the menu; clicking elsewhere closes it.
Sounds: `HUD_Options_Open` when the menu opens; the click handler plays
`HUD_Options_Focus` / `_Stop` / `_Dock` / `_TowStart` / `_TowStop` / `_GrappleStart|Stop` / `_BoardStart|Stop` / `_CloakStart|Stop` for the item
and `HUD_Options_NoSelection` when no item is under the cursor (case -1); `HUD_Options_MouseOver` is played from the generic button code
[? exactly when]. `HUD_Options_Dock` is also played by the dock routine when a ship docks.

## 3. Commands and ship AI tasks
`DockCommand` fields: docker WO id, dockee WO id, mode. Executing it first sets the ship's movement-AI
state to 4, then pushes an AI task: **mode 0 → task 8 (tow)**, **mode 2 → task 6 (dock)**, mode 1 breaks the tow link and (if the tow routine was running) pushes task 0.
Grapple pushes task 5, Board pushes task 7, Stop pushes task 0xB. All tasks are routines owned by one ship-AI object:
`ShipDockRoutine`, `ShipDockForRepairsRoutine`, `ShipTowRoutine`, `ShipBoardRoutine`, `ShipGrappleAndBoardRoutine`.
Rule actions `Dock Ships`, `Tow Ships`, `Board Ships`, `Break Tow`, `Set Dock Time` (class `DockTime`) and conditions `Group/Unit Is Docked`, `Group/Unit Has Docked`,
`Is Ship In Tow` use the same relation list (entries: type, docker id, dockee id; type 0 = tow, 1 = grapple, others = dock/board).

## 4. Dock routine state machine (`ShipDockRoutine`)
State names are the exe's own (`DOCKSTATE_*`): NONE → WAITING_TO_DOCK → DOCK_APPROACH → PREDOCKED → DOCKED →
MOVINGAWAYFROMDOCK → TURNINGTORETURN → RETURNING. Observed behaviour:
- **0 NONE**: remember the ship's current position (the return point), go to 1.
- every tick (states 1-5): if the reserved point is not the nearest point (free or not) and the nearest is free, the reservation is
  dropped and the routine goes back to 1.
- **1 waiting / approach**: with a reservation or a free point: reserve the nearest *free* point; within **10 units** →
  state 3 directly, else plan the approach (below) and go to 2. With every point taken: compare own distance to the nearest point with its
  holder's distance (both divided by the *docker's own* speed, 999 when it is ~0, so a stopped ship never wins); if nearer, take the point
  over and reset the holder's routine to 1; otherwise hold (path cleared) inside **80 units** of the point, or path toward it.
- **Approach point**: on the dock-point node's own forward axis, on the side the docker is on (axis flipped when the docker is
  ≥ 90° off it), **1.5 × dockee length** out for a ship dockee, **1 × docker length** for a base (lengths = collision-box y extents).
- **2 dock approach**: within **10 units** → 3. While the path runs, the movement controller's target is the dock point itself; each tick
  the angle seen from the dockee's centre between the docker and the approach point is checked: above **0.6π** (ship dockees only) the path is
  dropped; in both cases the approach is re-planned (the dockee may move).
- **3 docked (entry)**: stamps the start time, plays `HUD_Options_Dock`; for repairing dockees runs the repair setup (below);
  registers the dock relation (docker, dockee, point index; **not for base dockees**) so rules see "is docked"; the dock
  duration is `max(dock time (5.0 s default, 'Set Dock Time'), repair time)`.
- **4 docked**: wait for the duration, then release: free the dock slot, remove the relation and clear dock state. AI ships
  go to 5 and then 6/7; the **human ship just stays where it is** (no return trip).
- **5 moving away**: ship dockees only (bases → 6 at once): target = dock point + node axis × ⅔·(dockee length + docker length), axis
  flipped when the docker faces > 90° away from it; done within 5 units or after 10 s. → 6.
- **6 turning to return**: controller turn-to mode toward the start; done within 0.1 rad or after 10 s, then path via a point 100 units
  toward the start. **7 returning**: follow the path until it ends → 0.
Tow: the tower paths to the towee's back/front tow nodes; **inside 50 units of surface distance the tow link is registered**
(`ToweePoint = front_tow`, `TowerPoint = back_tow` mesh nodes). The remake now uses these nodes for approach and a shared mass-weighted cable constraint; native spring/damping remain PROVISIONAL.

### Repair on docking
Per-ship WOT field **`Repair Effect Name`** picks the size class: `Repair_Small` → small; `Repair_Medium`, `Repair_IC_Small` →
medium; `Repair_Large`, `Repair_IC_Medium` → large; `Repair_IC_Large` → large ironclad; `Repair_Tender` sets nothing. `hud.hdt`
"Repair Speed Info" gives times (seconds, repair / effect): small solarsail 4 / 7, medium 10 / 15, large 20 / 25, large ironclad 30 / 35;
effect fade-out distance 600. The effect objects are attached to the docker (one per repair job for ship dockees, one for bases);
the effect file is presumably the ship's `Repair Effect Name` (`Effects/Repair_*.eft` exist with matching lengths) [? name not traced]. creates one repair job per damaged section (jobs 0x54 bytes each) and starts the repair sound.
The base dock time is the larger of 5 s and that repair time. Whether every section ends at full health and what torpedoes/ammo get refilled
is **[? not confirmed]**; the rule action `Toggle Island Repair When Docked` flips a per-dockee flag, so islands/bases repair only when enabled
(ships whose dockee flag bit 8 repair unless the player settings byte is on [?]). Mesh nodes on dockees: `Port_DockPoint` / `Star_DockPoint`
(ships), `DockPoint1/2` or `Dock_Point` with an association name such as `CRANES` (bases/islands), described by WOT `MeshAttributes` entries with `AttributeName 'DockPoint'`.

### Dock-point markers
`DockPointEffectKeeper` (sync): while the keeper is dockable every dock point carries an effect
object on its node; when it stops being dockable they are removed. The effect is taken to be `Effects/Docking_Point.eft` (its name
is registered next to the keeper's effect table) [? name lookup not traced].

In the remake every map object and ship type with exported dock points gets them when it spawns, on every map type
(skirmish, historical and campaign). Bases no rule assigns an owner to (all skirmish bases) are dockable by every side **[PROVISIONAL]**.

## 5. Not reversed yet
Boarding fight / grapple mechanics (reversed, see [boarding.md](boarding.md)), the path planner and the arrival
controller used by the dock legs, where `ShipStatus_Icon_Docking_*` (animated docking status icon) is drawn, cloak effect,
hold-position semantics, waypoint-mode click behaviour, and what the two `RepairVitalSection` rule actions do per section.

## Mission 2 base-approach correction

Owner playtesting found the ship sailing through the abandoned island to its dock. The shared routine now caches a clear
base approach route around visible solid scenery instead of replacing it with straight legs each tick; reservation changes
invalidate that route. `collision::route_clear` uses conservative swept hull boxes and padded scenery-bound corners to find
a shortest clear route. An unreachable approach stops the ship. Dockee contact is allowed only for the final approach,
docked hold and departure/clearance, never while waiting or travelling the detour. This is a provisional planner, not the
recovered; it does not continuously re-plan around moving obstacles.
Regression simulates the Mission 2 torpedo ship's full approach around the actual abandoned-island collider into the dock;
synthetic checks cover clear, blocked and unreachable routes. See [campaign.md](campaign.md#mission-2-owner-playtest-fixes-islands-and-gun-towers).

## Implementation status
Done: `tools/export_docks.py` → `assets/data/docks.json` (MSB `DockPoint` nodes with yaw, `tender`); ship JSON `sight` / `repair_fx`;
`docking.rs` with the routine above (wait / take-over / approach leg / final approach / docked / move away / turn / return), tender-and-base
default dockability, repair over the class repair time with the `Repair_*` effect, `HUD_Options_Dock` on docking, `Docking_Point` markers,
rules `Dock Ships`, `Set Dock Time`, `Set Group/Unit Dockable`, `Toggle Island Repair When Docked`, `Group/Unit Is Docked`; mission bases
addressed by rules (`MissionBase`); left-click / menu dock gated by the sight test (no range constant); `options_menu.rs` with the three
variants (any friendly ship gets the friendly menu), retail icons/backgrounds/slot offsets and menu sounds, Focus / Stop / Dock live and the
rest greyed; context cursors. Camera: right-drag still rotates, a short right click opens the menu.
PROVISIONAL in the remake: scenery-clear corner routing for bases and simple detour legs for ship dockees instead of the native path planner, approach-leg
tolerance (docker length or lined up), the docked ship settling onto the point parallel to its axis (the exe's arrival controller is not
decoded), dock-point yaw sign, the docked sound only for player-side dockers, one repair effect at the ship centre, sight test by centre
distance minus our hull radii and the player's ship only (no group), the relation is also registered for base dockees.
Verified in Mission 1 (`--mission 1 --script dock|dock-click|dock-menu`: `Script::skip_to_tender` jumps to Objective 4 and puts the player,
half damaged, next to RLS Bounty; `dock` sends the order like the D key, `dock-click` / `dock-menu` drive the real left click / right-click
menu with synthetic mouse input): order accepted, approach ends on a dock point, Docked, repair to full, "Jim Has Docked With Tender" →
M01AMELIA12 → Objective 6 and the small ships leave for the asteroids, the player is released. On Wayland the synthetic cursor warp logs a
harmless winit error (the in-app cursor position is still set).
Tow (added): the Friendly menu's Tow start / stop (`DockCommand` mode 0 / 1 semantics) makes the player's ship steer for the towee and
registers the rules' tow link inside 50 units of surface distance; Tow stop breaks the links of the player's ship. Approach aligns the
tower stern node with the cargo bow node. `collision.rs` replaces rigid following with a mass/torque-aware slack cable between those
points, preserving independent hull motion. PROVISIONAL: registration-distance cable length and inelastic tension instead of decoded
native springs; every friendly ship counts as towable (no flag decoded); campaign-only links live in the rules' `Script`.
Docking status: the primary ship's looping `ShipStatus_Icon_Docking_Frame0..5` sits on its bottom-bar portrait, using
`PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT`. Placement follows a retail playtest observation;
8 fps remains PROVISIONAL and the `_Start` / `_Stop` icons are unused. It shows while waiting, approaching and docked, inherits
cinematic hiding and scenario cleanup from the panel tree, and is no longer projected above world ship labels.
Automatic docking routes hide both the course dashes and waypoint stars. Automatic speed changes produce neither helmsman
acknowledgements nor speed-dial / movement-order UI sounds; their state is still tracked so undocking does not replay a stale order.
Runtime smoke: `--mission 1 --script dock-click --shot-frame 180 --late-frame 0`, screenshot `local/mission1-docking-polish.png`.
Repair scaffold morphs are now rendered from six converted endpoints by the existing FX module, using each EFT's deploy/hold/retract
weight curve. Ironclad effects deliberately hold at fractional weights (0.5/0.6). GPU dock smoke reaches the repair phase and shows
scaffolding in `local/feedback-scaffold.png`; effect children and handles follow the existing FX/scenario cleanup.
Board and Grapple items: done (boarding.rs). Not done: cloak/hold items, path planning, the base dock-part hover rule, repair-effect fade distance. `Repair Vital Section`
is not done: no mission in the exported data uses that rule action and the doc has no per-section semantics for it.

## Original plan
1. Exporter: dock-point node positions per ship/base (MSB/WOT `DockPoint` attributes) and `Repair Effect Name`, into the ship/map JSON.
2. `combat/`/new `docking.rs`: relation list + dock routine states above; rule conditions `Group/Unit Is Docked`, actions `Dock Ships`, `Set Dock Time`, `Toggle Island Repair When Docked`; replace the mission-1 `D`-key dock.
3. `options_menu.rs` (+ `classic/options_menu.rs` art): right-click options menu (three variants, icons from `OPTIONS_MENU_*`), dock cursor/left-click dock; hover cursor set.
4. Tests: pure routine/state tests, rule tests, plus a runtime smoke test.
