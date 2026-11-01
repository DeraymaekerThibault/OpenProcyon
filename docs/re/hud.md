# Basic controls and battle HUD

Sources: the game manual (`Play/Basic_Controls.htm`, `Play/Advanced_Controls.htm`, the annotated HUD image), the retail
`hud.hdt` sprite data, retail 1:1 screenshots. Fleet controls (ship bar, formations, command ship) are in [fleet-controls.md](fleet-controls.md).

## Manual-described controls

| Area | Original behaviour |
|---|---|
| View | 3D camera follows the ship, ship centred; right-drag horizontally rotates, vertically tilts; arrow keys adjust the view |
| Movement | Left-click a destination: green marker, dotted yellow curved course |
| Engine orders | All Stop, Ahead Slow, Ahead Full, Emergency Speed; +/- steps the order |
| Manoeuvring | Speed, ship type/size and damage affect turning; the course preview updates with speed |
| Multiple waypoints | Plot a sequence, then accept (check mark) or cancel (X) |
| Firing | Left-click a hostile ship or section; charged banks in range and arc fire; a bank icon fires only that bank |
| Damage | Aim at sails, hull, rudder, ...; status shows ship, sails and crew separately |

The manual gives no numbers: speeds, turning, timing and damage come from data and measurement ([movement.md](movement.md), [gunnery.md](gunnery.md)).

## Main bar

- Built from `hud.hdt` sprite rects per faction art set (`Navy`, `Pirate`, `Procyon`; civilians use Navy). Rects, icon offsets and
  gauge-fill offsets are retail data. Part order is authored in `content/data/hud/bar.json` (PROVISIONAL: inferred from the sheet).
- Parts share their top row (gauge offsets are measured from the bar's top-left; rect heights differ only in bottom rows) and dock with
  the shortest part's bottom on the screen edge.
- Gauges: health = core / max core; combat = guns ready / total (PROVISIONAL); sail = speed factor (PROVISIONAL). Fills crop from the bottom.
- Weapon and ship-bar toggles stand on their `*_ToggleDetail` arms: arm bottom `Move_Toggle*_Detail_Down` px behind the compass /
  talking head, button at `Toggle*_Button_Offset_From_Detail_TopLeft` (checked on a retail 1:1 screenshot). Ship icons use the class's WOT icon name.
- A selected secondary shows its name on the `ShipName_HUDComponent_Backdrop_*` plate at the top (PROVISIONAL placement).

## Speed dial

`SpeedControl_CurrActiveN_NextActiveN` sprites, four lever positions: 0 Stop (red), 1 Slow (yellow), 2 Full (light green),
3 Emergency (dark green). A click selects the nearest coloured segment of the curved sprite. Exact retail hit regions unverified.

## Compass and radar

Brass frame `COMPASS_OVERLAY` (128x123) over the radar disk (offset 18,12) and a 10-frame sweep (0.1 s per frame, offset 17,13).
Contacts are ship-relative with forward up: hostile ships `HUDRADAR_ICON_ENEMYSHIP_*`, friendly ships `HUDRADAR_ICON_FRIENDLYSHIP_*`.
Measured on a 27-minute retail recording (2026-10-31, 1080p, compass drawn 1:1): 0.297 px per world unit on the 48 px disk; ships out
to ~153 units are drawn and ships further out are not drawn at all (not pinned to the rim); icons blink. PROVISIONAL: screen
placement; asteroid/torpedo/animal contact types and animated icon frames not done.

## Target marker

Four `ShipTargeted_Icon_Enemy` corner pointers on the diagonals around the target, each turned so its tip points at the ship. Retail
indicator data: 20x20 units, radius offset 0.8, z-plane offset -2; ours is screen-space with PROVISIONAL size/offset. A click stores the
clicked spot in ship-local space and guns aim there.

## Navigation arrow in a skirmish

Retail footage (headless runs, Border Dispute and The Diablo Strait): the compass disc under the selected ship is always drawn. The
gold arrow on it appears only when the map has a base to destroy. On Border Dispute it points at the enemy base from the first frame
(no waypoint set) and keeps pointing there; on The Diablo Strait (end rule: last ships standing) there is no arrow. Whether it also
follows waypoints, and what it shows on maps with a group objective (convoy, capture), is not measured: PROVISIONAL. The remake points
at the local team's current objective point (see "Skirmish team objective") and hides the arrow when there is none.

What marks a targeted base is not known. A lone Man-O-War cannot beat a Procyon base on Easy (26 guns; it was sunk four times at 137-266 units, and its clicks on the base never
made it fire at the base): the destruction of a base part has not been filmed yet (issue #97).

## Skirmish team objective (rule action `Setup Team Objective`)

Observed on retail, Border Dispute on Easy: during the battle the skirmish HUD shows **no objective text** (no panel, the chat history is empty, the bar buttons are pause,
fleet commands, chat history, waypoint mode, plus the weapon-bar and ship-bar toggles). The objective cue there is the compass arrow on the ship's ring (gold pointer,
the same art as the campaign's objective arrow), which the remake draws towards the team's objective point.

**Decoded rules** (static analysis): `Setup Team Objective` acts only for the local human's team. Its point name, when found in the map's points, becomes the one current objective point; `NO OBJECTIVE POINT` clears it (no arrow, no ring); any other unknown name leaves it unchanged. Its task name sets that task active; nothing deactivates older tasks, and the panel lists each active task's own text. The star-map ring and the ship-ring arrow both read the same current point (the arrow's own routine was not located: PROVISIONAL, but it is the only objective data path, and it fits Border Dispute, whose point lies beside the enemy base, and the maps with no arrow). Untested prediction: Ambush's Navy side gets an arrow towards its docking point. Remake: `Script::team_objectives`, `classic/world_hud.rs` `navigation`.

**The star map (M) shows it** (second retail check, same map): a blue panel with a gold border at the top left reads "Objectives:" over the team's task text
("Destroy Enemy Base."), the team's objective point is drawn with the objective-point icon (a target ring on the enemy base, Starkeep Cerebrus), and the map's place
names are written in yellow-gold (Starkeep Cerebrus, Orpheus Cove, Outpost Perseus, The Mourning Nebula). Remake: the skirmish star map draws all three from the map
script (`Setup Team Objective`, `tasks`, `map_text`). Still PROVISIONAL: a team whose rule says `NO OBJECTIVE POINT` (e.g. the Pirates on Ambush) shows no ring
(its panel text is drawn if it has a task); only the Navy side's map was observed.

## Ship name banners (retail frame, 2026-11-01)
A ship's name ribbon is tinted with the colour of the map player that owns it: in a Diablo Strait skirmish our banner is green, the same green as our name in the
top-right roster (slots 1-3 green, navy blue, teal; `players[].color` in the map file gives all eight). The banner is fully opaque with the camera close to
the deck (no fade). A rule's banner type ('Banner BlueTeam' ...) still overrides in missions. Not observed: enemy and allied banners in a frame, and what
`Max Banner Visible World Distance` (150) limits. Remake: `scenario::PlayerColor`, `classic/world_hud.rs` `banner_tint`; the fade we had was a guess.
