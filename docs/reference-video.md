# Reference footage notes

## Earlier gameplay reference (~4:10-5:30)

Observed in the original game (used as a visual target; the clip itself is not stored in the repo):

* **Ship name labels**: thin ribbon banners above each ship (`ShipNameBackdrop_*_WhiteBanner`, tinted: red for the player's
  side, blue for the other side), small white text. Implemented in `game/src/world_hud.rs`.
* **Bottom bar**: compact strip; besides the gold ship-status centre it has buttons: pause hourglass, fleet formations,
  movement type, chat, waypoint tool, red panic button. Not yet built (sprites exist: `MAINBAR_IS_*PAUSED_BUTTON`,
  `MainBar_FleetFormationsButton`, `MainBar_ChatButton`, `Main Bar_*MovementTypeButton`, stretched details).
* **HUD scale**: HUD art looks fixed-size in pixels rather than scaled to the window height. We currently scale by height/480.
* **Radar**: small round gold-framed device at the bottom-left; speed control a round gold knob at the bottom-right
  (opens into the arc dial).
* **Weapon bar**: blue ship-diagram panel at the left edge with gun icons.
* **Heading ring**: cyan compass ring (N/E/S/W) with a yellow arrow under the player's ship; course drawn as dashed
  yellow tiles ending in an arrow (waypoint path textures live outside BD_GUITextures).
* **Engine exhaust**: long bright orange-yellow tapered jet (additive). **Wakes**: long pale smoke streaks behind every
  ship (`Wake_Medium`, referenced by the ship WOT, not yet built).
* **Backdrop**: orange nebula sky with floating islands; flags on ships; sails semi-transparent.

## Mission 1, part 1 — user-supplied reference

Video: https://www.youtube.com/watch?v=rRFD6niorDM (approximately 9:24).
Directly inspected extracted frames: an overview sample across the clip, eight
frames from 2:10–4:05 and five from 8:25–9:23. This is a **sampled visual review**,
not continuous playback or an audio/transcript review. Transcript retrieval failed.
Timestamp ranges below bracket observed frames, not exact edit/control boundaries.
No video or reference images are committed.

### Sequence and useful checkpoints

| Video time | Directly visible | Remake implication |
|---|---|---|
| 0:51, 1:43, 2:10–2:34 | Cinematic fleet scenes; at 2:26 Jim's small boat passes a densely built floating settlement | This is the pre-rendered campaign introduction, not a Mission01 camera path to imitate in-engine |
| 2:43 | Game title card | Separate the movie/title transition from the mission's own opening NIS |
| 2:59 | Letterboxed camera view over the archipelago, no regular battle HUD | Mission begins with an environmental camera tour, not immediate player control |
| 3:16 | Close view of Amelia's ship, portrait and amber text in the top black bar | Opening briefing needs speaker portraits, subtitles and camera focus changes |
| 3:25 | Fleet overview; visible instruction to use dummy weapon loads | Supports the training setup, but does not measure damage/invulnerability semantics |
| 3:32 | Three blue-flag fast frigates are framed as the first exercise's targets | Blue flags are a distinct fleet texture, not just blue labels |
| 3:49 | Camera frames red fleet; subtitle instructs a southern swing below the archipelago, then attack through the small islands | Narrative explains the route and should gate the player handoff |
| 4:05 | Regular battle HUD, RLS Comet label, compass disc and a bottom dialogue panel with Jim's portrait | Player handoff has occurred by this sampled frame; precise handoff lies between 3:49 and 4:05 |
| 4:16–5:08 | Ship-following views, dashed thick yellow course tiles, large cyan compass disc, bright exhaust and long blue-grey wake | Replace our thin course line/empty rings; compare camera at matched zoom rather than the wide test screenshot |
| 5:59–7:41 | First mock engagement; blue frigate labels, targeting markers, luminous impacts/projectile effects | Useful combat references; isolated frames cannot establish the 95-second timer or firing cadence |
| 8:25 | Comet approaches a prominent purple-white cloud; Amelia's visible instruction says to wait at the nebula if ahead | Our rendezvous should account for other fleets, not only Jim's proximity |
| 8:40 | Letterboxed fleet briefing with Amelia's portrait; visible text assigns red ships west and blue ships east, then engagement at the northern tip | There is another in-engine briefing before routes split around Clarke |
| 8:54–9:23 | Normal HUD resumes; red fleet alongside the cloud, then a zoomed-out view | This clip reaches the start of the nebula circuit, not tender refit or pirate rescue |

**Movie cross-check:** read-only ffprobe of `Movies/SinglePlayer_NIS.bik` reports
640×480 Bink video with audio, duration 161.793 seconds. A locally decoded frame at
movie time 145 seconds matches the settlement/boat composition near video 2:26.
The early cinematic can therefore use the owner's existing movie rather than a
newly authored camera sequence. Converted playback is not implemented yet.

### Comparison with `local/mission01-course.png`

- Correct broad asset identities: Comet's boat, the three named blue frigates,
  red/blue label ribbons and grassy floating-island meshes.
- Missing the largest environmental landmark: the luminous purple Clarke nebula.
  Our sky also looks smoother/less visibly star-speckled in this screenshot;
  investigate sky UVs/orientation/material and matched viewpoint before substituting
  textures or adding an invented star layer.
- Original has a filled compass disc with cardinal letters and a yellow heading
  arrow; remake has thin wire rings. Original course uses chunky dashed yellow tiles,
  not our thin continuous prediction line and green cross alone.
- Original flags distinguish blue fleet and later red exercise flags. The remake's
  first-Navy-flag shortcut is not sufficient for campaign state.
- Original provides letterboxed speaker portraits/subtitles and a compact in-game
  dialogue panel. Our persistent large debug objective text is not equivalent.
- Original bottom-right round device/portrait and speed-control presentation are
  absent/different in our view. The bottom bar and left weapon panel already share
  recognizable art, but panel icons/charge bars need checking. The reference also
  uses the generic ship-outline panel for Comet: **do not replace it with a torpedo
  silhouette merely because it looks generic**.
- The reference includes very close user-controlled camera views as well as a
  distant view at 9:23. Ship screen size in our 130-unit screenshot is not evidence
  of a world-scale bug or the original default camera distance.
- Long pale wakes, tapered engine jets and visible hit effects are comparison
  targets, not measured emission rates, weapon physics or collision rules.
- White uploader captions (for example the large text over the 3:25 frame and
  '*BUMP*') are **not game UI**. A caption/overlap frame alone does not establish
  the native collision response.

### Next implementation batch

1. Campaign movie playback from the owner's Bink conversion, kept separate from NIS.
2. Retail mission opening NIS paths/focus changes, speaker portraits, subtitle/speech
   sequencing and player-control handoff. The existing exported rules already name
   these events: use them rather than scheduling by YouTube wall-clock time.
3. Clarke's cloud rendering and fleet rendezvous gating; then the nebula briefing
   and red-west/blue-east circuit.
4. Compass disc, course tiles, campaign flag selection and dialogue HUD; compare
   against fixed timestamps and matched camera views before adjusting physics.

Unverified: spoken audio, frame-exact cue timings, native NIS interpolation and
ship motion/firing/formation equations. This clip cannot validate later rescue
behavior or full mission completion.
