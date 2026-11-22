# Sight range, visibility and line of sight

## Status

**Implemented, with explicit fidelity limits:** shared range/group/contact checks now drive radar, star map, world labels/reticles, world picking, context menus, Focus, docking and ship/base target acquisition. An unseen ship's own effects stop drawing (§5); its hull is not hidden by sight, as in the original. Existing script visibility and cloak rendering remain separate.

Camera visibility, gun range, script visibility, sight range and nebula occlusion are different things. Detection is not a camera-distance or radar-disk test.

Source: observed behaviour of the original game and its retail data files, described in our own words. Related: [docking-and-orders.md](docking-and-orders.md), [nebula-landmark.md](nebula-landmark.md), [starmap.md](starmap.md), [cloaking.md](cloaking.md).

## 1. Sight test

The Dock/Focus "in sight" test and the list of objects the player's UI may show use one sight test.

For the current ship, then each ship in its group:

1. Read the observer's AI `Default SightRange` (→ AI).
2. Measure **surface distance**: the distance between the two collision bounding-sphere centres minus both radii.
3. Require distance **strictly less than** that observer's range.
4. Require that no occluder (§2) blocks the line between them.
5. Succeed if any observer qualifies. With no current ship, the test passes.

Thus a large object's centre can lie beyond the sight range while its surface remains in range. This is 3D distance, so altitude matters. Sharing is established for the current group, **not an all-allied-fleets intelligence network**. Last-known-contact positions are not established.

Retail ranges vary: the inspected ship catalogue includes 300, 400, 450, 500, 600, 800, 1000, 2000, 5000 and 10000 (including special/variant objects). AbandonedCamp and SmugglersCamp each specify 3000. Do not substitute one global range.

### Separate scenery policy

Islands are treated separately by the UI list: they carry their own distance/occlusion rule rather than the ship sight test, and its constants are unknown.

The remake therefore retains scenery as known UI objects with nebula gating, rather than applying an unverified ship-range disappearance rule. Dock/Focus actions still require the ship group's sight. Context selection and the context cursor also use the sight test.

## 2. Occlusion: authored polygons, not island triangle raycasts

Occluders are islands and nebulae; each tests whether a line between two points is clear. Islands are skipped for lines that start or end at scenery.

### Shipped island polygons are disabled

An island tests the horizontal game X/Y line against its authored occlusion polygon (WOT `Occlusion Polygon Point Count` and points, placed by the object's orientation and position); no polygon means clear. This is authored polygon geometry, **not the rendered mesh**.

- All **77 map-referenced scenery WOTs carrying `Occlusion Polygon Point Count`** have count **0**.
- The wider installed WOT catalogue scan also found no positive count among files carrying that field.
- Nonzero candidate point coordinates on AbandonedCamp/SmugglersCamp are unused because their count is zero.

No island mesh-based sensor blocking has been added. Physical collision/projectile blocking does not establish detection occlusion. Positive-count mod polygons are not yet exported/supported as a sensing feature.

### Nebula gate

A nebula blocks when occlusion is enabled and the two endpoints are not both inside (or both outside) the same cloud, or the line crosses its boundary polygon. Boundary and overlapping-cloud details are unverified.

The shared nebula gate now:

- Obeys `Nebula Occlusion On/Off`.
- Rejects inside/outside pairs and segments crossing the boundary.
- Allows a segment wholly inside the same cloud; concave-cloud segments that exit/re-enter still block.
- Has **no unverified 40-unit nebula bypass**. The existing 40-unit *cloak* exception remains a separate provisional rule.

Moving storms' sensor polygons still use the authored mission volumes rather than the live storm displacement. That and nested/overlapping clouds remain provisional; cloud energy/rule volumes and rendering are separate consumers.

## 3. Human-player visibility spheres

The UI first gathers candidate objects inside per-group visibility spheres, then applies the sight test (§1) to them. A group's sphere grows from each member's AI sight times a modifier plus the member's distance from the group centre. Retail saves store each computer ship's **scan range = WOT sight × its crew factor** (a Barque's 500 as 575 / 687.5 / 737.5 with Green / Average / Elite generic crews; the factors equal the ships' engineer/navigator factors), so the modifier is the crew's. Remake: `environment::ScanFactor` multiplies the sight used by the sight test (mission ships by crew level 1.15 / 1.30 / 1.40; every skirmish ship and the campaign player's ships by their lookout post: sight x 1.125 with an empty post, x (1.075 + 0.05 x the lookout's spotting rating) with a lookout, a class with no lookout post rated on its captain, see crew.md; scripted-map computer ships without crew data 1.35, PROVISIONAL). The owner confirms that the spotter crew changes sight range for the player too. Candidate bounds and final detection are distinct; the remake does not invent a final range multiplier.

## 4. Shared runtime implementation

`environment.rs::Sensors` is a **PreUpdate frame-boundary snapshot**, refreshed after scripted fleet-smoke positioning and before HUD input. It collects ship and gameplay-object positions, collision spheres, class sight, group membership, script/life state and cloak state. It avoids reading mutable ship queries from AI/input systems and keeps view-specific code out of the sensing rules. Scenario reset clears it; each refresh discards despawned contacts.

- `in_sight(observer, target, mission)` uses the original's strict surface distance and live/script-visible observers in the same shared fleet group (mission owner fallback for units without fleet membership), plus the target's script/life state, cloak gate and nebula clear-line test.
- Ships use `Class::sphere`, including its local centre transformed into world space. Some class spheres are exporter-derived fallbacks rather than authored spheres. Unarmed scenery without an exported sphere uses its world collision bounds (provisional), **not offshore dock-point distance**. Collider-free mod docks retain the previous dock-point-bound fallback so they remain usable; all shipped dockable scenery has a collider.
- `player_sees` uses the primary ship's group. Its own group remains known, script-hidden/dead targets do not qualify, and scenery retains its separate policy. Missing-primary and cinematic reveal exceptions are explicit. Blanket cinematic reveal of scripted-visible units remains provisional pending comparison with the original.
- Radar and star map contacts, names, reticles, left/right picking and context cursors use the same result. Stale ship menus close after contact loss; Focus/Dock are checked again when clicked. Focus is greyed without group sight, and the camera falls back from an unseen ship/animal focus rather than tracking its hidden coordinates. Projectile debug focus still works.
- Automatic skirmish/mission ship acquisition and ship/base gun target selection use sensing separately from gun range/arc. Explicit fleet/script attack orders may retain a destination outside sight so units can approach it; firing still needs detection. AI stance/path/contact-loss fidelity remains provisional, as before.
- Crew enemy-spotted speech now consumes shared detection, rather than an independent centre-distance heuristic. Its destruction callout requires a previously detected enemy. Dragon brains and homing/point-defence projectiles retain their specialized existing acquisition rules.
- Rules' actual unit/script visibility is **not overwritten by player detection**. Mission conditions must still know about distant units.

Snapshot changes become visible next frame; what the player's view shows follows the once-a-second re-test and fade of §5. No last-known-position/contact-memory system was invented. No new mesh range-culling or cloak timing/AI-override rules were added. Camera fallback on contact loss and the application of the recovered UI gate to automatic ship/base acquisition are coherent remake policies, not proof of every retail consumer's exact behaviour.

## 5. What is drawn of an unseen ship

### The player's view: re-tested once a second, with a fade

The original keeps, for every object the interface may show, a **seen** flag and a **fade** value between 0 and 1. Both run on real
time (they go on while the game is paused or sped up).

- **Re-test.** On the first frame more than **1.0 s** after the previous re-test, every object the view holds loses its seen flag,
  then every candidate (§3) that passes the sight test (§1) is seen again. Objects that are neither seen nor fading are dropped from
  the view. A flag can force a re-test on the next frame.
- **Appearing.** An object with a collision sphere that comes into the world already in sight is seen at once with fade **1** (no
  fade-in, no wait for the re-test). An object made visible by a script starts again from fade 0.
- **Fade.** Every drawn frame the fade moves toward 1 while the object is seen and toward 0 while it is not, by **0.8 per real
  second**, held to [0, 1]. So it takes 1.25 s from 0 to 1, and as long from 1 to 0 after a loss. On a re-test frame an object that
  was in the view, or is seen again, takes no step that frame (its fade clock restarts).
- **Shown** = (seen, or fade above 0) and not script-hidden. An object therefore appears at the first re-test after it comes into
  sight (up to 1 s late) and stays shown for up to 1 s plus 1.25 s after it leaves sight.

Interface consumers (labels, reticles, radar contacts, picking, the camera refocus) and the effects below follow *shown*. The sight
test itself (Dock, Focus, target acquisition) is asked directly and does not wait for the re-test.

### Effects of a ship that is not shown

When a ship stops being shown, these of its own effects are switched off, and switched on again when it is shown:

- engine exhausts
- wakes
- damage-section smoke
- fires
- the other effects it carries (attached effects)

Not switched: one-shot effects (muzzle flashes, hits, shells).

A **line between two objects** (grapple and tow ropes) is drawn only while both ends are shown; an end with no drawable part counts as
shown.

**The hull mesh is not hidden by sight.** The ship draw only tests the camera frustum, script visibility and the model's own no-draw
flag. What makes an unseen ship look absent is the combination: no effects, no label, reticle or radar contact (§4), it cannot be
picked, and the camera leaves a focus it cannot see. At the widest battle zoom the view is about 550 units across, so a ship beyond a
typical sight range is off screen anyway.

Checked in the original: a computer Barque approaching an idle Man-O-War (sight 500) was not shown, effects off, at 583 units
centre distance and shown, effects on, at 573 units; a ship 1010-1043 units away stayed unshown for a minute, one 285 units away stayed
shown. In two approaches the fade read 0.57 and 0.67 one sample (of one per second) after the sighting and 1.0 one second later,
which fits 0.8 per second (both at most 0.8) but does not pin the rate. The rate, the 1.25 s fade-out after a loss, the instant full
fade on appearing and the rope rule are read, not checked by a value; so is the hull rule.

Remake: `environment::Sensors` keeps the player's view across snapshots (`advance_sightings`, once per frame after the snapshot,
real time): `player_sees` is *shown* as above and `sees_now` is the sight test of this frame. `mark_unseen_ship_effects` hides a
ship's non-transient effects (exhausts and their plumes and lights, smoke, fires, carried effects) and its wakes while `player_sees`
is false; the hull keeps drawing; grapple and tow lines need both ends shown. A sunk ship keeps its effects (the original's case is
not read). Not modelled: in the original an object dropped from the view while script-hidden keeps its last seen flag while it
stays out of the view.

## Validation and remaining acceptance

The default `tools/check.sh` gate passes (210 Rust tests; 75 Python tests, seven install/optional-fixture tests skipped). Re-running the same gate with the retail root enabled exposes an unrelated existing `test_combat.LocalCombatExportTests.test_cutter` failure: it assumes raw critical chances are in [0,1]. Neither that exporter nor its test was changed in this batch; the retail-enabled run has one failure and two optional-fixture skips. Regressions cover strict range equality, surface versus centre distance, altitude, unrelated group exclusion, group spotting, hidden/dead observers/targets, cloak, missing primary, scenery policy, sphere-centre transforms, despawn cleanup, enabled/disabled and short-distance cloud occlusion, same-cloud and concave segments, Focus availability and camera tracking. Existing tower-fire/tower-destruction and Mission 2 docking tests still pass.

Shared ECS/HUD smoke:

```sh
WAYLAND_DISPLAY= tools/shot.sh local/sight-classic.png --script sight \
  --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0
```

The script freezes three ships, disables their gun states and asserts detection and actual radar widget visibility through out-of-range → group-spotted → unrelated ally → cloaked → revealed states. It is a wiring check, not a combat or full campaign replay. Screenshots/logs: `local/sight-classic.*`. A separate Mission 2 launch reached initialization/NIS without an ECS error but timed out before capture; it is **not** counted as a completed mission runtime smoke.

**Remaining:** the once-a-second list cadence and keep value (§5), two-ended effects, lost-contact memory/timing, precise NIS/cloak/crew-modifier semantics, scenery UI distance policy, live moving-storm/overlap geometry, and normal Mission 1/2 playthroughs after the matrix/detection corrections. Validate against original footage/screenshots before claiming full sensor parity.
