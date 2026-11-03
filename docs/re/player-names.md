# Player list window (top right of a skirmish battle)

Sources: retail `hud.hdt` layout (`Player Names Window` settings, `PLAYER_NAMES_WINDOW_*`, `PLAYER_ALLIANCE_STATUS_*` sprites), the
launch/roster code and the alliance command as read statically, a reference recording of a team-map skirmish. Implemented in
`game/src/alliance.rs` (state, rules, input) and `game/src/classic/player_names.rs` (drawing).

## Rows
- One row per lobby slot that maps to a world player (human and computer slots), added at launch, in slot order. No sorting, no scrolling.
  Map-only owners (bases, asteroid owners that are no slot) get no row. The window shows only when at least one row exists (battles
  started with `--map` have none; campaign missions are undecoded and show none).
- A row: the player's flag (29x21), then the name in Univers10 in the player's own colour, 2 px right of the flag and centred on it.
  The human shows the lobby name, computers show "Computer".
- A player counts as alive while it has a ship afloat (retail: a non-empty fleet list). A dead player's name is struck through
  with a 2 px red line (PROVISIONAL: colour from the footage), the row stays, the icon is still drawn but is no longer clickable.
  PROVISIONAL: when exactly captured ships, lifeboats and bases keep a player alive.
- Layout (reference px): window width = the top-detail sprite (196); rows 23 apart (21 + 2); flag 8 from the left edge; icon 8 from the
  right edge, centred on the row; first row `PLAYERLISTING_OFFSET_Y` below the window top (Navy 8, Pirate 14, Procyon 13); height =
  top detail + bottom detail + rows x 23 minus `VERTICALCOMPRESSION_OFFSET_Y` (Navy 8, Pirate 6, Procyon 12). Anchored at the screen's top
  right plus `OFFSET_FROM_TOPLEFT`. The stretched-centre sprite fills between the details. PROVISIONAL: +-1 px.

## Alliance icon
- Shown on every row except the local player's. Allied means both directions are set (teams set from the map or lobby show as allied);
  anything else, a one-sided offer included, is "not allied". Green joined chain = allied, red broken chain = not allied.
- With `Is Alliance Change Allowed` false (team maps, campaign) the plain sprites are drawn and there is no hover or click. With it true
  (the free-for-all maps) the `_CHANGE` sprites are drawn: brighter on hover, yellow while pressed.

## Clicking the icon
Only when the map allows changes, the row is not the local player's and the local player still has a ship. Per row, by state:
1. Allied: both directions are cleared at once (no message).
2. Only we had offered: the offer is withdrawn.
3. Only they had offered: both directions are set (accepted).
4. Neither: our offer is set.
Computers never offer, accept or answer: nothing in the computer AI touches alliances, so against computers only breaking an existing
(lobby team) alliance does anything; an offer stays red. Forming an alliance needs the other side to press back, which only another
human could. The three in-game messages ("<name> has Requested an Alliance / Accepted your Alliance Request / Removed their Alliance
Request", strings `IDGS_TPINGAMEMESSAGE_ALLIANCE_*`) go to the target player only, so a lone human sees none. No sound, no tooltip.
Alliances also end battles: on a free-for-all map the battle is over when every surviving player is mutually allied (docs/re/skirmish-end).

Implementation: a break moves the player's ships to a new side (hostile to everyone, the human included). PROVISIONAL: hostility in
the engine is per side, so breaking with one member of a lobby team also makes it hostile to the other members of that team, where
retail only clears the pair. Ships already sailing alongside keep their orders until their own AI re-evaluates.

## Clicking a name or flag
Works when the local player has no ship left or is allied with that player, and the row has a ship afloat: the camera goes to that
player's first ship.

## Not decoded
Campaign rows; the strike-through draw colour; the alliance-formed listener; the exact fleet-list events for captures.
