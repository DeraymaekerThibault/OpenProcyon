# Battle camera

Sources: the original's camera logic (read, not measured), and the camera state the retail saves store (`SavedGames/*.twt`,
`Game Camera` block, `SwoopCameraController`), which confirms the height rule below: at a distance of 100 it holds the camera
41.02 above and 100 behind its target (the rule gives 41.01).

## Retail: the pitch follows the zoom

The battle camera has no pitch control of its own. It keeps a back distance d (the zoom) and a turn about the vertical:

- The camera sits d behind the target point (horizontally, along the turn) and `0.065 x d^1.4` above it, and looks at it with
  world up. So it looks down at `atan(0.065 x d^0.4)`: about 8 degrees at d = 7.7, 12 at 20, 22 at 100, 33 at 307.
- Target point: the focused object's origin raised by 2 (a ship; the original takes a quarter of a hull value below 4, unread,
  PROVISIONAL), and when the focus changes the target glides to the new one, an eighth of the way more each second.
- The wanted distance changes in steps; the shown distance eases toward it, closing `6.5 x dt` of the gap each update.
- A step outward adds its size to the wanted distance; a step inward takes twice its size off. Steps:
  - mouse wheel: 10 per notch (one notch in = 20 closer, out = 10 farther);
  - right-drag up and down: 1 per pixel (drag down zooms out, PROVISIONAL direction); right-drag sideways turns the camera by
    0.005 rad per pixel (the cursor stays where the drag started);
  - zoom keys: 3 per update, so 90 a second out and 180 a second in at 30 updates a second;
  - rotate keys: 0.04 rad per update (1.2 rad a second).
- Limits, set whenever the camera takes a new focus: the nearest distance is 1.2 x the focus's bounding radius, never under 2
  (half that radius for one object class, unread); the farthest is the nearest plus 300. The wanted distance is clamped to them.
- Start: the wanted distance starts at 20 (raised to the focus's nearest). Missions set distance and angle with the Focus
  Camera On Group rule (100 in the stored mission saves).
- Field of view 45 degrees, near plane 0.5, far plane 3000.

Here (`view.rs`): the retail camera is the default. The camera runs on real time, so it keeps working while paused, and the
per-update steps are scaled to 30 updates a second (PROVISIONAL: the original steps it with the world tick, and how it acts
while paused is not read). The bounding radius is the ship's collision sphere (the original uses its mesh bounds, PROVISIONAL);
any other focus counts as radius 0. The glide to a new target is not modelled (the camera follows a new focus at once).

## Not retail: the free camera

The bundled `free-camera` mod (on by default, `data/camera.json`) gives the camera a free pitch instead: right-drag up and
down tilts it between the file's `pitch` limits (default 0.05..1.5 rad), the wheel zooms 3 a notch and the zoom keys 60 a
second between 10 and 300, with no easing. Switch the mod off for the retail camera.
