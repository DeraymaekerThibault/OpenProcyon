# Star map (radar click)

The original's star map is its `MapBackdrop` overlay. Implemented in `game/src/starmap.rs`.

- The radar disk (`Compass_Background`, which has CursorOver/CursorDown art on sheet 640hud_31) is a button; clicking opens the map.
- Backdrop: the world's `World Map` block `Backdrop Texture Name` (e.g. `MapMission01`, 2x2 tiles of 256 px = 512 px) drawn over the
  playable square. Verified against island positions: half-extent = `World Size - Max` minus the `World Buffer Size` (Mission 1:
  2000 - 500 = 1500). Exported as `world_half` in `assets/maps/*.json`; backdrops as `assets/maps/<id>/backdrop.png`.
- Open animation `Animation Speed MapBackdrop` = 1 s; hud.json `Map Display`: blue background and world lines fade 0..1, 10 alien-text strips
  (`MapBackdrop_RandomText0`) scroll 10..100 px/s and fade in 0.8..1.
- Icons (sheet 640hud_21): player = `MAPBACKDROP_ICON_PRIMARYSHIP_FRAME0..3`, objective point = `MapBackdrop_Icon_ObjectivePoint_Frame0..3`
  (0.1 s/frame), enemy/allied ship, whale, tender, base/town static. Each is drawn at its world position turned by its orientation.
- Map text points (`MapText System`: Name, DisplayedText, Position, Visible) are exported as `map_text`; rules
  `Set Map Text Visibility` and `Set Current Objective Point Visible On Starmap` switch them (`Script::map_text`, `point_on_map`).
- Sounds: `HUD_Map_Open`, `HUD_Map_Close`, `HUD_Map_BackgroundLoop` (while open), `HUD_Map_MouseOver` (radar hover).
- Opening also pauses the game when the world's "pause when starmap opens" setting is on (tutorial rule `Pause when starmap opens`, gate at
  ). Not modelled.

PROVISIONAL in the remake: window layout (centred square + mirrored world-line quadrants), close controls (click/Esc/M), `M` to open,
label colour, icon rotation sign, tender/base icons, default objective-point visibility. `--starmap` opens it on the first frame (screenshots).
