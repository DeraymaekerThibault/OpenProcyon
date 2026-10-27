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

### Hover decision — one intent per frame from the object `o` under the cursor
Tests in order:
1. no object → nothing. In lob-target mode (probably) → focus.
2. `o` is **hostile to the selection** → attack: with the primary when the primary is selected, with the selected secondary otherwise;
   a base/island with no attackable part under the cursor → "can't do anything".
3. otherwise: `o` is the **selected ship** → focus the camera on it; `o` is the **primary ship** (while another ship is selected) → select
   the primary; `o` is another own selectable ship → select it; **if the selected ship can dock with `o` and is in range → dock**; else
   "can't do anything".
Dock-ability test: the two players are **allied**, `o` is dockable (every ship and island can be), its dock points are enabled
("Can Be Docked With") and it has at least one, and its owner is not destroyed. For a base the cursor must also be on the part that
carries a dock point.
**Defaults**: a ship is dockable *and* repairs ("Repair When
Docked With") only when its WOT `Is Tender` is true; islands/bases always. "Set Group/Unit Dockable" writes only the dockable flag.
**There is no dock range.** The range check is a *sight* test (also used to enable the menu's Focus item): the AI entity's `Default SightRange` (400-600
for most ships; crew multiplier applied elsewhere). It passes when the surface distance (centre distance minus both
collision radii) from the selected ship, or any ship of its group, is below that ship's sight range and the object is not hidden.

### Clicks
**Left click** on an object: an enemy → attack order; camera-focus click → focus; an own ship → select it; a dockable ally →
**dock order for (selected ship, object)**. So docking is a plain left click on the ally.
The [fleet-control notes](fleet-controls.md) distinguish the selected ship from the primary.
Do not substitute primary ownership for the selected actor in world-click commands.
**Right click**: open the options menu for the object under the cursor. Camera drag with the right button is separate
[? click vs drag threshold not checked; the repo already rotates on right-drag per the manual].

## 2. Options menu (builders, layout)
A small panel at the cursor drawn from four corner sprites plus icon buttons (hud.hdt `OPTIONS_MENU_*`):

| item | HUD sprite | order given on click |
|---|---|---|
| Dock | `OPTIONS_MENU_BUTTON_DOCK` | dock with the object |
| Tow start / stop | `…TOW_START` / `…TOW_STOP` | tow it / break the tow (the same order as docking, in another mode) |
| Grapple start / stop | `…GRAPPLE_START` / `…GRAPPLE_STOP` | grapple / release |
| Board start / stop | `…BOARD_START` / `…BOARD_STOP` | board / call off the boarding |
| Focus | `…FOCUS` | camera focus |
| Hold position | `…AI_LOCK_POSITION` / `…AI_NOT_LOCK_POSITION` (chosen by the ship's current hold state) | set / clear hold position |
| Stop | `…STOP` | cancel the ship's current actions |
| Cloak start / stop | `…CLOAK_START` / `…CLOAK_STOP` | cloak / decloak |

Three variants, chosen when the menu opens (background corner sprites in brackets):
- **Enemy ship** (hostile) [`OPTIONS_MENU_ENEMY_SHIP_BACKGROUND_*`]: Focus (always), Board start/stop, Grapple start/stop.
  - Board shows *stop* when a boarding is under way between them, otherwise *start*, enabled by:
    not allied, no fight yet, both objects can take part in boarding, the *target's* Can Be Boarded flag is on, the boarder is not a base, and a grapple
    holds (ship target) / the boarder is in range of a boarding point (base target); see [boarding.md](boarding.md) §3.
  - Grapple shows *stop* when a grapple holds them, otherwise *start*, enabled when the primary has a grapple gun whose
    live grapples are fewer than its limit.
- **Own primary ship** [`…SELECTED_SHIP_BACKGROUND_*`]: Focus, Stop, Hold-position toggle (enabled for ships whose AI can hold), Cloak start/stop
  (cloak items only when the ship's cloak state allows it).
- **Any other ship/base** (friendly or neutral) [`…FRIENDLY_SHIP_BACKGROUND_*`]: Focus, Tow start/stop (stop when a tow link between them exists;
  start enabled when the target is towable and no link exists), **Dock** (enabled by the dock test above).
Strings for these items are icons only (no text). Selecting an item closes the menu; clicking elsewhere closes it.
Sounds: `HUD_Options_Open` when the menu opens; the click handler plays
`HUD_Options_Focus` / `_Stop` / `_Dock` / `_TowStart` / `_TowStop` / `_GrappleStart|Stop` / `_BoardStart|Stop` / `_CloakStart|Stop` for the item
and `HUD_Options_NoSelection` when no item is under the cursor; `HUD_Options_MouseOver` plays on hovering a button [? exactly when]. `HUD_Options_Dock` is also played by the dock routine when a ship docks.

## 3. Orders and ship activities
A dock order names the docker, the dockee and a mode: **tow**, **break tow** or **dock**. Giving it puts the ship under explicit
orders and starts the matching activity: towing, or docking; break tow drops the tow link (and ends the towing activity if it was
running). Grapple, Board and Stop likewise start grappling-then-boarding, boarding, or an idle activity. A ship runs one activity at a
time: dock, dock-for-repairs, tow, board, grapple-and-board.
Rule actions `Dock Ships`, `Tow Ships`, `Board Ships`, `Break Tow`, `Set Dock Time` and conditions `Group/Unit Is Docked`, `Group/Unit Has Docked`,
`Is Ship In Tow` use one shared list of links between two objects (kind, docker, dockee; kinds: tow, grapple, dock/board).

## 4. Dock routine phases
None → waiting to dock → approach → pre-docked → docked → moving away from the dock → turning to return → returning. Behaviour:
- **0 NONE**: remember the ship's current position (the return point), go to 1.
- every tick (states 1-5): if the reserved point is not the nearest point (free or not) and the nearest is free, the reservation is
  dropped and the routine goes back to 1.
- **1 waiting / approach**: with a reservation or a free point: reserve the nearest *free* point; within **10 units** →
  state 3 directly, else plan the approach (below) and go to 2. With every point taken: compare own distance to the nearest point with its
  holder's distance (both divided by the *docker's own* speed, 999 when it is ~0, so a stopped ship never wins); if nearer, take the point
  over and send the holder back to waiting (1); otherwise hold (path cleared) inside **80 units** of the point, or path toward it.
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
(`ToweePoint = front_tow`, `TowerPoint = back_tow` mesh nodes). The remake now uses these nodes for approach and a shared mass-weighted cable constraint; retail spring/damping remain PROVISIONAL.

### Repair on docking
Per-ship WOT field **`Repair Effect Name`** picks the size class: `Repair_Small` → small; `Repair_Medium`, `Repair_IC_Small` →
medium; `Repair_Large`, `Repair_IC_Medium` → large; `Repair_IC_Large` → large ironclad; `Repair_Tender` sets nothing. `hud.hdt`
"Repair Speed Info" gives times (seconds, repair / effect): small solarsail 4 / 7, medium 10 / 15, large 20 / 25, large ironclad 30 / 35;
effect fade-out distance 600. The effect objects are attached to the docker (one per repair job for ship dockees, one for bases);
the effect file is presumably the ship's `Repair Effect Name` (`Effects/Repair_*.eft` exist with matching lengths) [? not confirmed]. Docking creates one repair job per damaged section and starts the repair sound.
The base dock time is the larger of 5 s and that repair time. Whether every section ends at full health and what torpedoes/ammo get refilled
is **[? not confirmed]**; the rule action `Toggle Island Repair When Docked` flips a per-dockee flag, so islands/bases repair only when enabled
(ships that are tenders repair [? player-setting exception unverified]). Mesh nodes on dockees: `Port_DockPoint` / `Star_DockPoint`
(ships), `DockPoint1/2` or `Dock_Point` with an association name such as `CRANES` (bases/islands), described by WOT `MeshAttributes` entries with `AttributeName 'DockPoint'`.

### Computer ships docking for repair
Computer players' ships (not the human's, not ones under explicit orders) decide on their own to dock for repair: once **at
least two of their damage sections are at or below half their hit points** (destroyed sections do not count), they pick the
nearest dockable object of their own or an allied player and run the dock routine above on it (AI ships then leave and sail
back to where they started). Overall hull share is not the test, which is why a skirmish computer ship at 80 % hull with its damage
spread thin never docked ([movement.md](movement.md#computer-captains-measured-2026-10)). PROVISIONAL: no distance limit, and
the choice among friendly docks taken as the nearest; not yet seen in play. Remake: `docking::ai_repair_dock`.

### Dock-point markers
While an object is dockable every dock point carries an effect on its node; when it stops being dockable they are removed. The
effect is taken to be `Effects/Docking_Point.eft` [? not confirmed].

A dock point's position is the node's own position, which is already in the model's frame. Unlike gun mounts, the parent layer's
offset must not be added: doing so put the lights tens to hundreds of units from the piers on almost every base (checked against the
model geometry: every base's dock points now lie within about 25 units of the model; Dolinor's were 200-300 off). Re-import after
updating.

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
PROVISIONAL in the remake: scenery-clear corner routing for bases and simple detour legs for ship dockees instead of the original path planner, approach-leg
tolerance (docker length or lined up), the docked ship settling onto the point parallel to its axis (the original's arrival steering is
unknown), dock-point yaw sign, the docked sound only for player-side dockers, one repair effect at the ship centre, sight test by centre
distance minus our hull radii and the player's ship only (no group), the relation is also registered for base dockees.
Verified in Mission 1 (`--mission 1 --script dock|dock-click|dock-menu`: `Script::skip_to_tender` jumps to Objective 4 and puts the player,
half damaged, next to RLS Bounty; `dock` sends the order like the D key, `dock-click` / `dock-menu` drive the real left click / right-click
menu with synthetic mouse input): order accepted, approach ends on a dock point, Docked, repair to full, "Jim Has Docked With Tender" →
M01AMELIA12 → Objective 6 and the small ships leave for the asteroids, the player is released. On Wayland the synthetic cursor warp logs a
harmless winit error (the in-app cursor position is still set).
Tow (added): the Friendly menu's Tow start / stop (the tow / break-tow modes of the dock order) makes the player's ship steer for the towee and
registers the rules' tow link inside 50 units of surface distance; Tow stop breaks the links of the player's ship. Approach aligns the
tower stern node with the cargo bow node. `collision.rs` replaces rigid following with a mass/torque-aware slack cable between those
points, preserving independent hull motion. PROVISIONAL: registration-distance cable length and inelastic tension instead of the
retail springs; every friendly ship counts as towable (the towable flag is not modelled); campaign-only links live in the rules' `Script`.
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
