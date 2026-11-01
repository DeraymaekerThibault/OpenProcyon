# Ship and scenery collision

Source: observed behaviour of the original game, WOT collision definitions and the converted meshes, described in our own words.

## Recovered

- Ship WOTs select `BoundingRenderEntity` detection and `Ship` response, with explicit `MinExtents` / `MaxExtents`. These are distinct from damage-section bounds and the large broad-phase sphere (which includes sails). Cutter's box in game axes is (-2.25, -7, -1.5) to (2.25, 6.5, 2); its sphere is radius 12 centred at (0, 0, 5.5).
- Islands and most bases select `OBBTree` detection and `Static Object` response, deriving their bounds from the render mesh. This is triangle collision, not one enormous sphere around the island.
- Unarmed asteroids select `BoundingRenderEntity` / `Asteroid` with authored boxes; armed asteroids select `OBBTree` / `Static Object`.
- Dock buoys select `Null` detection/response; Base_Cresentia has no collision entity. Neither should block ships.
- External pushes on a ship are divided by its WOT mass and accumulated as acceleration, which the physics step integrates into velocity. This confirms inverse-mass impulse/force handling, not its caller's complete impulse formula.
- Helper uses a contact point's displacement from the transformed centre of mass and collision-box dimensions to accumulate angular acceleration, clamped to ±60. The exact torque/sign relationship is not recovered well enough to port.
- Debug strings include `Collision LOD Level Index` and `Valid Collision Triangle Indices`. The exact collision LOD, triangle rejection rules, response dispatcher, restitution, sound trigger and damage calculation remain unresolved.

## Ships and rocks (read from the original's code, 2026-10-31)

- **Contact shapes:** ship against ship tests the two physics boxes (the WOT `MinExtents` / `MaxExtents`, no padding) once their
  collision spheres overlap, by separating axes (the box faces and the cross products of their edges); the axis of least
  penetration is the normal and the contact point is the centre of the touching corners.
- **Ship against ship**, checked every physics tick (1/30 s) while the hulls overlap: the two are pushed apart along the contact
  normal in proportion to the other's mass (the heavier one moves less; an idle ship is no exception). Neither ship's velocity
  changes: no bounce, restitution or friction. Each gets a yaw kick at the contact point: a force of 0.1 x its mass x the
  difference between its velocity and the pair's mass-weighted common velocity, per tick, over its box inertia (mass x (width² +
  length²) / 12, width and length twice the box's starboard and forward extents), at the contact point taken at deck height; the
  angular acceleration is capped at ±60. Its own mass cancels, so a Man-O-War barely turns and a Cutter swings.
- **Ram damage:** a ship takes 0.0004 x the *other* ship's mass x the closing speed, on its one section nearest the contact,
  through the normal hit path. At most once per
  3.5 s per ship, no speed threshold, no material or ram-bow factor. None between allies. Scaled by side and difficulty like
  shells (Easy: the human's ships x0.25, computer ships rammed by the human's x3; Normal x0.5 / x2; Hard x1). Example: a Cutter
  rammed by an enemy Man-O-War at 9 units/s loses 144; the Man-O-War 14.4.
- **Ship against rock:** pushed apart by mass, the ship's speed and turn untouched; the rock is shoved away from the ship at
  0.25 x the offset between their centres per second. Every tick both take 0.0002 x the rock's mass x the closing speed (the
  ship's not scaled by difficulty, no cooldown, no ally test); the rock breaks at 0.
- **Spill (every hit, shells too):** a hit that leaves its section at 0 hit points (already dead or not) passes 0.4 x its damage
  on to each of the section's neighbours (the WOT section's `adjacent` list), and on again from any neighbour left at 0, each
  section at most once per hit. Base parts (towers, defence rings) take shells through the same routine, so they spill too.
- **Sound:** each tick a touching pair has a 7 % chance of a collision sound: a coin picks one of the two hulls, and its section
  nearest the contact plays `Ship_Collision_Wood` or `_Metal` by material (cloth, stone and others are silent), heard to 700 units.
  A damaging ram plays the struck section's `Damage_<material>` sound at the contact (once when both struck sections share a
  material). Lifeboats pass through ships.
- Remake: `collision::sail`, `combat::lob::ram_damage`, `combat::shells::apply_ship_hit` and `apply_base_hit` (spill). PROVISIONAL:
  a rock touch plays the ship's collision sound (the rock's own impact sound is not read); shells on bases take no difficulty factor
  (the original applies one on the same routine, but its value for bases is not measured).

## Land and bases (read from the original's code, 2026-10-31)

Contact with an island or a base pushes the ship's hull straight out along the horizontal normal, and nothing else: its speed,
throttle and turn are untouched, it takes no damage and makes no sound. A ship pressing into a shore therefore keeps its (avoider-
capped) speed and slides along it; one wedged bow-first is freed by the avoider's turn in place ([movement.md](movement.md)).
Remake: `collision::sail` (the old impulse response for land is gone; ships and rocks keep it until their rules are ported).

## Measured on retail (2026-10, one glancing ram between our own ships)

Since read from the code: ships of one alliance never damage each other by contact, so the damage below was not the ram; it
matches the Diablo Strait volcano meteors, which hit every section at once.

Headless Diablo Strait skirmish on Easy, our Navy Cutter driven past our own idle Man-O-War; both ships logged at 30 Hz and saved
before and after (section HP from the saves).
- **Not a contact either:** the Cutter (9.07 units/s, Emergency) passed the Man-O-War's port side 12.7-12.9 units from its centre
  line, slowing to 6.2 units/s and turning ~30°. Replayed against the physics boxes the two never overlapped (closest 2.5 units
  apart): the slow-down and turn were the Cutter's collision avoider, not contact.
- **Damage:** both ships lost hit points on every section during the run (Cutter −75, Man-O-War −86): meteors, not the contact.
- A head-on Emergency ram missed (passed 24 units off) and did nothing.

## Implementation

`tools/export_collision.py` exports `assets/data/collision.json` keyed by retail entity type. Scenery/asteroid boxes come directly from the collision definition, mapped to Bevy axes. Ship boxes reuse the existing `data/ships/<id>.json` physics `MinExtents` / `MaxExtents`, so inherited mods keep their collision shape; custom ships without those fields provisionally use section bounds. Static objects use indexed triangles extracted from the already converted GLB, **provisionally model/LOD 0**. No raw WOT/MDB or executable is committed. Regenerate after mesh/exporter changes:

```sh
set -a; source .env; set +a
python3 -m tools.export_collision "$PROCYON_GAME_ROOT" assets
```

`game/src/collision.rs` owns movement/contact scheduling; `main.rs::step` remains the free-motion step. Contacts use oriented ship boxes, vertical overlap, and separating axes in the sailing plane. Mesh triangles are clipped to the ship's vertical slab before projection; high scenery does not block a ship below it. Scenario creation builds static world-space obstacle bounds/triangles once; moving/tumbling asteroid shapes are rebuilt before ship movement. Asteroid boxes use transformed 3D faces, with all eight corners included in the bounds (see [asteroids.md](asteroids.md)). Destroyed ships and hidden scenery do not collide.

Adaptive substeps limit translation to roughly half a unit (and time to 1/60 s), followed by contact separation and a normal impulse weighted by inverse WOT mass. Lateral contact velocity is retained separately from engine speed. Tests cover rotated boxes, walls, vertical clearance, momentum, non-approaching contacts, and ECS ships driving into one another/a wall. Mission 1 screenshot startup also exercises the resource/query wiring.

## Owner feedback / next validation

The owner reports that collisions do not account for weight as in the original. The current ship/ship solver does use inverse
WOT mass for both separation and linear impulse, but that does not establish faithful collision behaviour. Reproduce light/heavy
ship contacts at equal closing speeds, glancing contacts and ship/asteroid contacts; inspect linear transfer, angular response and
post-contact motion. The first implementation batch replaces immovable ordinary asteroids with finite-mass contact response
using their authored mass and relative velocity. The comprehensive follow-up adds contact-point velocity and lever-arm torque,
including angular effective mass in the impulse denominator, for ships and ordinary asteroids. Box inertia and the single-point
support-interval contact manifold are provisional. Coplanar mesh triangles are combined so centred hits do not acquire artificial
turning from one half of a triangulated face. This addresses concrete gaps, not proof of exact retail weight/handling.

Ordinary rocks now carry the same `Drift` component used for ship contact motion, separately from their rule/path-controlled velocity.
The planar inelastic impulse and separation are shared with the existing mass-weighted ship solver. Rock transforms and collider
points/triangle bounds translate together during substeps; collision movement does not rebuild or clone the mesh. A subsequent
`Setup Asteroid Belt` replaces the prior contact velocity; ordinary path ticks do not erase it. Breakup includes contact drift in
the parent's provisional inherited fragment velocity. Armed asteroid scenery remains static, as specified by its response type.

Regression coverage: the old solver failed the light-rock test by stopping the ship without moving the rock. New tests check
light/heavy rocks, relative velocity/equal opposing momentum, momentum conservation, next-frame motion, translated tumbling faces,
retail cutter/torpedo/tug mass ratios, rule-setup drift reset, and drift inheritance through breakup. Static walls/islands retain
zero inverse mass. This is still a provisional inelastic planar response, not a recovered retail asteroid response formula.

Validation: `tools/check.sh` passed (91 Rust tests, Python suite with six skips). The `--script asteroid` runtime smoke produced
`local/collision-weight-breakup.png` without ECS query conflicts. A full Mission 1 autopilot run (`--speed 16`, autokill enabled,
scratch `XDG_DATA_HOME`) completed and wrote `local/collision-weight-end.png`; frame time stayed about 16.7 ms near the ending.
This checks mission progression and runtime wiring, not manual combat or retail collision feel. Log: `local/collision-weight-mission.log`.

## Tow constraint

The rule interpreter still owns tow relationships; movement substeps now own the physics. Ship models provide the tower's stern
`TowerPoint` and cargo's bow `ToweePoint`, with hull-end fallbacks for custom models. Each link captures its initial horizontal node
separation as cable length. Slack links exert no force; extended links correct position by inverse mass and apply a tension-only
impulse at the attachment points, turning both hulls independently. No ship position, heading or speed is copied from its partner.
Broken links discard their stored length; missing/destroyed partners exert no force. Retail rope/spring constants are unknown:
this is a provisional inelastic cable, not a literal port. Regression covers off-axis pulling/turning and release without teleporting.

## Explicitly provisional / unfinished

- Inelastic response (zero restitution), positional separation, lateral damping, and the planar solver are stand-ins, not a recovered retail response. Contact torque is implemented with provisional box inertia; impact damage and collision sound are not.
- Ordinary asteroid contacts are now finite-mass, but their retail restitution/torque response still needs tracing. The original never resolves rock/rock contacts (belt rocks pass through each other, docs/re/asteroids.md); rock/static push-out is not ported. Existing path motion occurs before the ship substeps, so fast moving belts still lack continuous collision detection. Whales/mines are not movement colliders in this pass.
- Docking ignores only the assigned dockee during approach/docked/departure so repairs are not broken; the original collision rejection-list rules are unknown.
- Obstacle avoidance for AI ships: the original's avoider, see [movement.md](movement.md#collision-avoidance-read-from-the-originals-code-2026-10-31).
- Course preview (`course.rs`) is clipped at the first pose whose box touches visible scenery (`collision::clear_prefix`). It still uses free motion (no drift, no steer-away) and 0.1 s samples, so very thin walls can be stepped over in the preview.
- Collision sound: not wired. `audio.rs` has only the generic `Sfx` message and no collision event; the `Ship_Collision_*` pools stay unused until the response/sound trigger is recovered.
- This is discrete contact, not continuous collision detection. Extreme modded speeds/very thin modded boxes and ships spawned completely inside a closed island are not validated. Large fleet/contact stress tests and all 13 missions' changed routes still need playtesting.
