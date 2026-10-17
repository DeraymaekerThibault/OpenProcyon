# Sight range, visibility and line of sight

## Status

**Implemented, with explicit fidelity limits:** shared range/group/contact checks now drive radar, star map, world labels/reticles, world picking, context menus, Focus, docking and ship/base target acquisition. Ship meshes are **not newly hidden by sight range**: retail mesh disappearance versus UI contact loss still needs comparison with the original. Existing script visibility and cloak rendering remain separate.

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

The UI first gathers candidate objects inside per-group visibility spheres, then applies the sight test (§1) to them. A group's sphere grows from each member's AI sight times a modifier plus the member's distance from the group centre. Retail saves store each computer ship's **scan range = WOT sight × its crew factor** (a Barque's 500 as 575 / 687.5 / 737.5 with Green / Average / Elite generic crews; the factors equal the ships' engineer/navigator factors), so the modifier is the crew's. Remake: `environment::ScanFactor` multiplies the sight used by the sight test (mission ships by crew level 1.15 / 1.30 / 1.40, computer skirmish ships 1.35; PROVISIONAL band values; player ships unchanged). Candidate bounds and final detection are distinct; the remake does not invent a final range multiplier.

## 4. Shared runtime implementation

`environment.rs::Sensors` is a **PreUpdate frame-boundary snapshot**, refreshed after scripted fleet-smoke positioning and before HUD input. It collects ship and gameplay-object positions, collision spheres, class sight, group membership, script/life state and cloak state. It avoids reading mutable ship queries from AI/input systems and keeps view-specific code out of the sensing rules. Scenario reset clears it; each refresh discards despawned contacts.

- `in_sight(observer, target, mission)` uses the original's strict surface distance and live/script-visible observers in the same shared fleet group (mission owner fallback for units without fleet membership), plus the target's script/life state, cloak gate and nebula clear-line test.
- Ships use `Class::sphere`, including its local centre transformed into world space. Some class spheres are exporter-derived fallbacks rather than authored spheres. Unarmed scenery without an exported sphere uses its world collision bounds (provisional), **not offshore dock-point distance**. Collider-free mod docks retain the previous dock-point-bound fallback so they remain usable; all shipped dockable scenery has a collider.
- `player_sees` uses the primary ship's group. Its own group remains known, script-hidden/dead targets do not qualify, and scenery retains its separate policy. Missing-primary and cinematic reveal exceptions are explicit. Blanket cinematic reveal of scripted-visible units remains provisional pending comparison with the original.
- Radar and star map contacts, names, reticles, left/right picking and context cursors use the same result. Stale ship menus close after contact loss; Focus/Dock are checked again when clicked. Focus is greyed without group sight, and the camera falls back from an unseen ship/animal focus rather than tracking its hidden coordinates. Projectile debug focus still works.
- Automatic skirmish/mission ship acquisition and ship/base gun target selection use sensing separately from gun range/arc. Explicit fleet/script attack orders may retain a destination outside sight so units can approach it; firing still needs detection. AI stance/path/contact-loss fidelity remains provisional, as before.
- Crew enemy-spotted speech now consumes shared detection, rather than an independent centre-distance heuristic. Its destruction callout requires a previously detected enemy. Dragon brains and homing/point-defence projectiles retain their specialized existing acquisition rules.
- Rules' actual unit/script visibility is **not overwritten by player detection**. Mission conditions must still know about distant units.

Snapshot changes become visible next frame; exact retail update cadence is not recovered. No last-known-position/contact-memory system was invented. No new mesh range-culling or cloak timing/AI-override rules were added. Camera fallback on contact loss and the application of the recovered UI gate to automatic ship/base acquisition are coherent remake policies, not proof of every retail consumer's exact behaviour.

## Validation and remaining acceptance

The default `tools/check.sh` gate passes (210 Rust tests; 75 Python tests, seven install/optional-fixture tests skipped). Re-running the same gate with the retail root enabled exposes an unrelated existing `test_combat.LocalCombatExportTests.test_cutter` failure: it assumes raw critical chances are in [0,1]. Neither that exporter nor its test was changed in this batch; the retail-enabled run has one failure and two optional-fixture skips. Regressions cover strict range equality, surface versus centre distance, altitude, unrelated group exclusion, group spotting, hidden/dead observers/targets, cloak, missing primary, scenery policy, sphere-centre transforms, despawn cleanup, enabled/disabled and short-distance cloud occlusion, same-cloud and concave segments, Focus availability and camera tracking. Existing tower-fire/tower-destruction and Mission 2 docking tests still pass.

Shared ECS/HUD smoke:

```sh
WAYLAND_DISPLAY= tools/shot.sh local/sight-classic.png --script sight \
  --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0
```

The script freezes three ships, disables their gun states and asserts detection and actual radar widget visibility through out-of-range → group-spotted → unrelated ally → cloaked → revealed states. It is a wiring check, not a combat or full campaign replay. Screenshots/logs: `local/sight-classic.*`. A separate Mission 2 launch reached initialization/NIS without an ECS error but timed out before capture; it is **not** counted as a completed mission runtime smoke.

**Remaining:** retail mesh/render consumers, lost-contact memory/timing, precise NIS/cloak/crew-modifier semantics, scenery UI distance policy, live moving-storm/overlap geometry, and normal Mission 1/2 playthroughs after the matrix/detection corrections. Validate against original footage/screenshots before claiming full sensor parity.
