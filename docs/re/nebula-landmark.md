# Nebulae and the Clarke landmark

Sources: `World/SinglePlayer/Mission*.twt` and `World/*.twt` (rule `*State Init* Setup Nebula`, point sets, polygons), `WorldObjects/Terrain_Nebula.wot`,
`Effects/Nebula.eft`, `BD_Meshes/OvalNebula_Layer1.mdb`, observed behaviour of the original game and the retail
`SavedGames/*.twt`. Already in the repo: `tools/export_nebulae.py` (per-map `nebulae`), `assets/fx/Nebula.json`, `assets/models/scenery/OvalNebula.glb`,
`game/src/nebula.rs` (mesh-shifting renderer), `game/src/environment.rs` (energy drain and sensor occlusion). Uncertain parts are PROVISIONAL.

## 1. How a nebula is defined

A nebula is **not** a placed mesh. Each map/mission contains:

1. A world object `Terrain_Nebula` (a `Terrain_Nebula.wot` entity: no render entity, null collision, a unit bounding sphere, and a
   `NebulaCustomInfoFactory` custom-info block). The WOT only holds: minimum lightning spawn distance from ships 50, minimum bolt length 100,
   max lightning spawn z amplitude 200, meteor hit chance modifier 80, three damage-potential tables (lightning, wind, meteor: per hull type
   chance of critical/fire, initial fire strength, damage bounds) and the three ambience sounds (`Ambience_Nebula`, `Ambience_SolarStorm_Loop`,
   `MeteorStorm_Inside`).
2. A rule action `*State Init* Setup Nebula` that binds the object id to a named configuration. Its fields (all exported verbatim to
   `assets/maps/<id>.json` -> `nebulae[]`): `New Nebula Name`, `Polygon Name` (a TWT *volume*: a 2-D polygon in game x,y), `Nebula Cloud Effect
   Name` (`Nebula` or `EFFECT` = none), `Nebula Cloud Point Set Name`, `Solar Storm Effect Name` / `Point Set`, `Meteor Shower Effect Name` / `Point Set`,
   `Lightning On/Off` + `Lightning Blast Recharge Time`, `Meteors On/Off` + `Meteor Strike Recharge Time`, `Rotational Winds On/Off` + `Wind Magnitude` +
   `Solar Storm Wind Damage Frequency`, `Nebula Cloud Energy Drain On/Off`, `Nebula Occlusion On/Off`, `Ambient sound max distance`.
   Other actions: `Make Nebula into a Meteor Shower`, `Set All Nebulas To Trap Objects`, `Set All Nebulas Fadeout Distance`; condition
   `Unit Is Within Any Nebula`.
3. A **point set** (`Nebula Cloud Point Set Name`): a list of `(position, magnitude)`. Each point spawns one cloud instance; **magnitude is the
   cloud radius in world units** (PROVISIONAL, matches the exe's use as the effect's radius variable `SET_MESH_RENDER_INFO_RADIUS`). Clarke has one point:
   centre (-0.42, -65.94, 4.0), magnitude 381.0, polygon `Clarke`, effect `Nebula`, energy drain TRUE, occlusion TRUE, lightning/meteors/winds off,
   sound `Ambience_Nebula` with 300 max distance.

Other missions using the same cloud effect: 4 (Calyan Nebula), 5, 7 (also solar storm + meteor storms), 8 (+ meteor storms), 9 (Big Nebula),
11 (two) and the skirmish maps Border Dispute (Boranaz) and Convoy Raid (darkwhorl). Solar storms (missions 3, 7) use the `SolarStorm` effect on the
`GeoSolar_Layer1` mesh; meteor storms use `MeteorStorm.eft`. `GeoNebula_Layer1.mdb` exists but no effect references it.

## 2. How it is drawn (retail)

`Effects/Nebula.eft` is one `GenericParticleSystem` primitive, "everlasting", with exactly one particle that is a *mesh particle*:
- Mesh `OvalNebula_Layer1` (387 vertices, bounding box +-(1, 0.7, 0.99), a squashed ovoid; texture `Nebula_alpha` -> our GLB material `Nebula_alpha.tga`).
  The effect variable `SET_MESH_RENDER_INFO_RADIUS` scales the unit mesh to the point-set magnitude.
- `USE_SHIFTING` true: the mesh vertices wander every tick. Parameters in the EFT (`CHUNK_DATA_SHIFT_*`, ...: noise 0.1, normal noise 0.8,
  attract 0.9, difference pull 0.1, constrain 3.5, scale limits 1.0..1.15, come-back distance/multiplier, neighbour distance, size multiply 100,
  texture spin 0.3). The exe scales the "noise" family by 0.001 on load (0.001). The update is already
  reproduced in `nebula.rs` ("breathing" scale per vertex pulled toward a random neighbour, small random offsets with a come-back phase, UV spin).
- `FADE_EDGES` true: a boolean stored in the render info and passed on to the mesh draw call. The
  actual shading rule (almost certainly alpha by normal-vs-view angle) was not recovered. PROVISIONAL: `nebula.rs` uses |dot(view, radial)|.
- Colour fader: RGB loops over 0..3.7 s (R 0.8->0.9->1.0->0.8, G 1.0->0.9->0.7->1.0, B 1.0->1.0->0.9->1.0), alpha constant 0.5.
- Blend: source ONE, destination ONE (pure additive), render pass 1 (a late/transparent pass; with unlit additive the texture gives the purple-white
  look seen in the reference video).
- Ambient loop: `Ambience_Nebula`, silent beyond `Ambient sound max distance` (provisional falloff).

The reference footage shows a large purple-white luminous cloud at Clarke, which is exactly this additive, textured, shifting ovoid.

## 3. Gameplay behaviour recovered from the exe (new)

`NebulaCustomInfoEntity` update, fields from `Terrain_Nebula` state: centre, radius,
"inside" list, "left" list, `IsTrapping`, `Update WorldObjects Within Nebula Timer`,
`Nebula Cloud Drain Sail Energy On/Off`, `Make Nebula into a Meteor Shower`.
- **Membership**: on the update timer the entity scans world objects of selected types (a small type list that includes ships, see the type
  filter in the function) and puts an object in the *inside* list when its 3-D distance to the centre is less than the radius **and** its (x,y)
  is inside the nebula's polygon (the exe calls its polygon test with x,y). Objects that left are dropped from the list unless `IsTrapping`
  is set, in which case they stay inside ("trap objects" action).
- **Slow-down factor**: every frame, each inside *ship* gets `Nebula Slow Down Factor = clamp(1 - distance_to_centre / radius, 0, 1)`; ships not in
  the list are reset to 0. The value is a ship field saved in the TWT (`Nebula Slow Down Factor`; observed non-zero saves 0.25-0.56) and the sail
  update copies it into the sail/movement state (the same step that raises crew sail-strength announcements at 25/50/75/100 %).
  So ships slow more the deeper they are; the exact use of the factor (probably a speed/accel multiplier of `1 - factor` or a sail-power
  drain) is PROVISIONAL.
- **Sail energy drain**: `Nebula Cloud Drain Sail Energy` is a flag only; the drain rate was not found. The current remake rate (0.12/s down to
  25 %) is a guess; prefer replacing it with the depth-based factor above. `Nebula Occlusion` blocks sensors/line of sight through the polygon
  (retail AI also treats `TYPEAVOID 'Nebula'` as an area to avoid on path planning; the remake only does the occlusion).
- **Camera effect**: the world saves `DATA_NEBULA_CAMERA_EFFECT`, `..._FADE_IN_TIMER`, `..._SPIN_TIMER`, `DATA_NEXT_NEBULA_CAMERA_EFFECT`:
  when the camera is inside a nebula the exe fades in a full-screen/spinning overlay. Not decoded. PROVISIONAL / not done.
- Lightning, winds and meteor strikes (solar storm / meteor shower variants) use separate WOT damage potentials.
  These are now exported into `data/nebula.json` and applied through the shared damage pipeline. Previously wind used full weapon-lightning
  damage and meteors used heavy-cannon damage. For cloth, weather wind is 5–7 damage versus lightning's 15–40; for wood, meteors are 5–10.
  Hit selection now follows the executable evidence below rather than guaranteed per-ship hits. Recharge floors (3 s lightning / 2 s meteors),
  endpoint sampling, wind force and membership/trap details remain provisional; critical/fire handling retains the existing environmental-hit limitations.
  The mission 3 `storm-m3-damage` smoke uses the normal 45% protection from its retail trigger rule (not forced invulnerability)
  and verifies carriage, release and survival.

### Weather hit-selection follow-up

Original game behaviour:
- recharge clocks belong to the nebula, not individual ships. A lightning pulse has a 50% chance to generate a bolt.
- generate two random points in the storm with height variation, rejecting endpoints near ships
  (WOT minimum 50 units). Enforce a 100-unit minimum bolt length and test the segment against the contained ships;
  only the closest intersection takes damage, on the struck section. Ambient bolts commonly miss every ship.
- each contained ship's meteor hit probability is its bounding-sphere diameter divided by the WOT modifier 80.
  On a successful roll, pick a section uniformly, not a point near the mast. The sloop's authored radius 8.75 gives a 21.875% chance per pulse.
- wind damages cloth sections only when the ship is outside the inner 5% of the nebula radius
  ( holds 0.05). Its clock is the configured wind-damage frequency.

The remake now uses storm-wide clocks, geometric lightning intersections via the existing ship hit test, size-based meteor rolls,
explicit section indices and the wind centre exclusion. It no longer guarantees repeated lightning hits near the sails.
Remaining differences: uniform rejection endpoint sampling instead of the native polygon sampler, conservative 3/2-second recharge floors
instead of the authored shorter lightning/meteor intervals, simplified membership/retention and wind force. These are not established retail parity.
Regression tests cover clocks, size probability, endpoint exclusion and a seeded 30-second storm with no ship intersections;
existing weather damage/protection tests now also reject invalid section indices. Runtime smoke `local/mission03-storm-hit-selection.log`
completed the ride with core 80/80 and sections 206.66/210, 180/180, 70.75/80 (sails). The previous smoke left sails at 31.64/80.
This is an isolated, deterministic wiring check, not a normal-play or retail side-by-side measurement.

## 4. Status in the remake / discrepancies found

- Clarke is already drawn: `game/src/nebula.rs` (`spawn` per map `nebulae`, additive unlit mesh with the vertex-shifting sim). `docs/re/campaign.md`
  ("Clarke and space whales") still describes the older static `environment.rs` rendering, and `docs/reference-video.md` still lists the cloud as
  missing; both are stale.
- **Bug**: `game/src/main.rs` lines 408-409 call `nebula::spawn(...)` twice in a row, so every cloud is drawn twice (doubled additive brightness,
  double simulation). Remove one call.
- Cloud size/placement is data-driven and correct for Clarke; the look (brightness, fade) still needs a side-by-side with the reference video frame
  at 8:25-9:23 (docs/reference-video.md). Radius = magnitude and FADE_EDGES are PROVISIONAL.

## Etherium currents

`*State Init* Setup Etherium Current` binds a mission path (missions 3, 4, 10).
The existing planar force uses `Terrain_Etherium_Current.wot` Magnitude 25000 and Radius 60, divided by ship mass in `collision.rs`;
force law/falloff remain provisional. The visual was incorrectly treated as particles, so nothing rendered.
`EtheriumRiver.eft` has two `River` primitives accepting `SET_POINTS` and `SET_WIDTH`, with the retail `Current` texture.
Community-exe RiverPrimitive at consumes path points; builds lateral pairs around that path using the supplied width;
 advances varying texture offsets. The remake now draws two continuous additive textured strips on the path in `environment.rs`,
with repeating UVs scrolling downstream. Currents flow from the first authored path point toward the last: in Mission 4, Nelson's fleet
leaves "to the frontier" on 'Nelson exit route', which runs inside 'Mucullough path' in the same order (an earlier reversal based on a
Mission 3 playtest was wrong). Missions 3 and 10 follow the same convention. Width = twice the force radius and the UV repeat/speeds remain provisional;
exact native spline sampling, oscillation and blending are not recovered. No particle emitter is spawned per path point.
The `current-m3` smoke confirms both mesh layers and changing UV offsets; `local/mission03-current.png` is the Classic visual check.
One mission 10 setup references absent path `ebend`; it remains empty rather than inventing a route.

## Implementation plan

1. `game/src/main.rs`: delete the duplicate `nebula::spawn` call (1 line). Add a regression test (cargo test in `nebula.rs`/`main.rs`) that the number of
   `Cloud` entities after setup equals the number of cloud points in the map JSON.
2. `game/src/environment.rs`: replace the time-based energy drain with the retail *depth factor*: inside = 3-D distance < magnitude AND (x,y) in the polygon
   (`volumes[Polygon Name]`); `factor = clamp(1 - d/r, 0, 1)`; use it as the sail-power/speed reduction (keep it separate from permanent damage; keep a
   clearly named PROVISIONAL constant for how it scales speed). Optional `IsTrapping` (`Set All Nebulas To Trap Objects` action) and a rule hook for
   `Unit Is Within Any Nebula` using the same membership test (shared with the sensor occlusion). Tests: membership, factor monotonic with depth, polygon
   edge case; keep the existing energy/occlusion tests adapting to the new function.
3. `game/src/nebula.rs`: move the FADE_EDGES alpha into a vertex-colour system parameter, then compare to a reference frame and tune only that
   one function; add the camera-inside overlay (full-screen tinted quad that fades in with the `NEBULA_CAMERA_EFFECT` timers) as a separate small
   system once a reference frame is available. No exporter change is needed (all data is already exported).
4. Later: lightning/meteors/winds for missions 2, 3, 7, 8 using `Terrain_Nebula.wot` damage tables (needs `tools/export_nebulae.py` to export those
   tables), AI path avoidance (`TYPEAVOID Nebula`).
5. Docs: update `docs/re/campaign.md` Clarke section and `docs/reference-video.md` to point here after the code lands.
