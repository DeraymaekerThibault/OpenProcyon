# Basic controls and battle HUD

Sources: the game manual (`Play/Basic_Controls.htm`, `Play/Advanced_Controls.htm`, the annotated HUD image), the retail
`hud.hdt` sprite data, retail 1:1 screenshots. Fleet controls (ship bar, formations, command ship) are in [fleet-controls.md](fleet-controls.md).

## Manual-described controls

| Area | Original behaviour |
|---|---|
| View | 3D camera follows the ship, ship centred; right-drag horizontally rotates, vertically zooms, and the tilt follows the zoom (`camera.md`; the manual's "tilts" is the zoom's tilt); arrow keys adjust the view |
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
- Gauge fill position: each faction has its own offset per gauge (`MainBar_ShipStatus_{Health,Combat,SailPower}_Offset_From_MainBar_TopLeft`;
  Navy y 21, Pirate y 35, Procyon y 30, x 0-14). The offset is measured from the top-left of the gauge's bar part (its whole
  hud.hdt rectangle, which starts at the bar's top edge), not from where the part's visible art begins: the transparent rows
  cut off the part's top (3, 15 and 10-13 px for the three factions) are not added. Added by mistake they drop the fills by exactly
  that much, out of their tubes, most on the Pirate bar (checked against a retail 1:1 screenshot of the Navy bar and against the
  faction art for the other two: the fills then sit in the tubes' lit part).
- Ship portrait and docking icon: the class's portrait picture is placed at `MainBar_ShipIcon_Offset_From_Background_TopLeft` (Navy 24,16;
  Pirate 21,27; Procyon 10,26) from the top-left of the ship-frame part's whole rectangle, the same untrimmed origin as the gauge
  fills (the frame art's own transparent top rows, 2 on Pirate and 3 on Procyon, are not added). The check is the art: the portrait's
  centre then lands on the middle of the bar's height on Pirate and Procyon (46 of 92, 45 of 90) and on the middle of the oval that
  spans the panic button, frame and gauges on Procyon. The docking/boarding icon (32x33) is placed at `PRIMARY_SHIP_STATUS_ICON_OFFSET_FROM_PRIMARYSHIPICON_TOPLEFT`
  (Navy and Pirate 7,4; Procyon 8,4) from the top-left of the portrait picture's own untrimmed box (the fleet-bar slots use the
  same rule), which centres it on the portrait. Both were drawn too low and too far right by the trim of the node they hung on (frame
  art top 2 / 3 px, then the portrait art's own 2 left / 5 top margin for the docking icon). Not measured against the original's drawing:
  the origins rest on the data values and the art.
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

## Objectives and the Chat History (retail, 2026-11-22)

**Objectives are listed on the star map only.** The battle screen draws no objective text, in skirmish or in a campaign mission (the campaign case is unchecked against play). The task list is the "Objectives:" panel at the top left of the star map (M): each active task's own text. The remake's campaign star map
draws the same panel as the skirmish one, listing the mission's active tasks one per line (`StarmapContents::objective`); the campaign no longer draws a task line at the top of the battle
screen. Not yet read: how a complete or failed task looks in the panel (the remake still prefixes `[done]` / `[failed]`: PROVISIONAL).

**Chat History** (seen once in a skirmish after a fight; the rest unchecked against play). The speech-bubble button at the right end of the main bar toggles a framed
panel just above the bar, centred on it, about 440 x 170 px at 1440x1080 (blue nine-part frame, a scrollbar on its right, the thumb at the bottom while the list is at its end).
- Content: every text line of the battle so far, one entry per wrapped line, each in the colour of the event that produced it. Sources in the original: crew speech
  (`[ Speaker ] text`), the critical-system-hit message of the primary ship (red), chat and alliance messages, and mission speech that has text.
- A line is added when its speech event starts playing, not when it is queued. Lines are word-wrapped to the panel width and kept for the whole battle (no cap found); a new battle starts empty.
  New lines scroll the list to its end. The panel is empty at battle start.
- Opening it closes the fleet-commands bar; opening the star map closes it. Pressing the button again closes it.
- Remake: `panels::ChatLog` (lines + open flag), fed from `Script.started` (mission and skirmish speech with text, with the event's own text colour, default gold);
  `classic/panels.rs` `chat_panel` draws it with the retail `ChatBar_*` frame pieces and scrolls with the mouse wheel. The log is cleared with the battle (`scenario::reset_fleets`).
  Opening the fleet-commands bar also closes the panel (they share the strip above the bar; the original's rule for that direction was not read: PROVISIONAL).
- Not built: crew speech lines (the remake has the voice files but no text for them, so there is nothing to log), the critical-hit message (see below, wording unread), chat and alliance messages
  (no multiplayer chat), the blank line between entries, the open/close slide, and the scrollbar's drag and arrow behaviour (the thumb only follows the wheel).
- Not yet read: automatic opening of the panel by speech events flagged for it, and how complete or failed tasks look.

## Ship name banners (retail frame, 2026-11-01)
A ship's name ribbon is tinted with the colour of the map player that owns it: in a Diablo Strait skirmish our banner is green, the same green as our name in the
top-right roster (slots 1-3 green, navy blue, teal; `players[].color` in the map file gives all eight). The banner is fully opaque with the camera close to
the deck. A rule's banner type ('Banner BlueTeam' ...) still overrides in missions. Not observed: enemy and allied banners in a frame.

**When a banner shows** (unchecked against play, 2026-11-20). The profile option "Ship Name Banners" has four
values: 0 off, 1 "On When Close" (the default profile's value), 2 "On When Far", 3 always on. Every frame each ship's banner is opened or closed: with
"On When Close" it is open while the straight distance from the camera to the ship's centre is at most `Max Banner Visible World Distance - Near` (150 in
hud.hdt), with "On When Far" while it is at least that; the `- Far` value (also 150) is not read by this test. The banners toggle closes every banner.
The banner's alpha (ribbon and name text alike) then moves toward 1 when open and 0 when closed at 1.0 per second of real time, so a banner fades fully
in or out in one second, and it is drawn while its alpha is above 0. Remake: `scenario::PlayerColor`, `classic/world_hud.rs` `banner_tint` and
`banner_alpha` ("On When Close" only; there is no options entry for the other values yet).

## Critical hit text on the primary ship (unchecked against play, not built)
When a hit on the human's primary ship rolls a critical (movement.md, "Critical hits"; the roll names which of the section's critical systems was hit) and the HUD has nothing else open,
a text for that system is put on the HUD's message list. There are ten critical system kinds, each with its own string. The strings' wording was not read. Remake: no such text (crew voice lines only).
