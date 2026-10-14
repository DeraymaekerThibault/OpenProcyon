# Sight range, visibility and line of sight

## Status

**Implemented, with explicit fidelity limits:** shared range/group/contact checks now drive radar, star map, world labels/reticles, world picking, context menus, Focus, docking and ship/base target acquisition. Ship meshes are **not newly hidden by sight range**: native mesh disappearance versus UI contact loss still needs a render-consumer trace. Existing script visibility and cloak rendering remain separate.

Camera visibility, gun range, script visibility, sight range and nebula occlusion are different things. Detection is not a camera-distance or radar-disk test.

Source: observed behaviour of the original game and its retail data files, described in our own words. Related: [docking-and-orders.md](docking-and-orders.md), [nebula-landmark.md](nebula-landmark.md), [starmap.md](starmap.md), [cloaking.md](cloaking.md).

## 1. Confirmed native sight helper

**0x0055ecf0** supplies the Dock/Focus sight test and is also called by the native UI-visible-list path **0x0055e870**.

For the current ship, then each ship in its group:

1. Read the observer's AI `Default SightRange` (→ AI).
2. Compute distance with **0x00453df0**. Its surface-distance mode uses the two collision bounding-sphere centres and subtracts both radii; it also has a centre-distance mode. The docking investigation identifies surface mode for this use.
3. Require distance **strictly less than** that observer's range.
4. Require occlusion helper **0x0055ec00** to report no blocker.
5. Succeed if any observer qualifies. With no current ship, the native helper returns true.

Thus a large object's centre can lie beyond the sight range while its surface remains in range. This is 3D distance, so altitude matters. Sharing is established for the current group, **not an all-allied-fleets intelligence network**. Last-known-contact positions are not established.

Retail ranges vary: the inspected ship catalogue includes 300, 400, 450, 500, 600, 800, 1000, 2000, 5000 and 10000 (including special/variant objects). AbandonedCamp and SmugglersCamp each specify 3000. Do not substitute one global range.

### Separate scenery policy

**0x0055e870** checks render-entity state and UI/debug/NIS-style flags and distinguishes object flag **0x8000**. Island construction **0x005d7310** sets that flag. The island branch has a separate distance/occlusion path rather than simply using the ship sight helper; its exact constants/flags remain undecoded.

The remake therefore retains scenery as known UI objects with nebula gating, rather than applying an unverified ship-range disappearance rule. Dock/Focus actions still require the ship group's sight. Native context selection **0x0056fdc0** and cursor logic **0x005621a0** also use the sight helper.

## 2. Occlusion: authored polygons, not island triangle raycasts

**0x0055ec00** iterates two world occluder lists (and), calling virtual clear-line tests. The second list is skipped when either endpoint has flag 0x8000. RTTI/debug traversal **0x00539b70** identifies these as `TP::ILineOfSightOccluder`. Island and nebula custom-info entities implement it.

### Shipped island polygons are disabled

Island clear-line method **0x005d8000** tests horizontal game X/Y endpoints against its configured polygon through **0x005dc8a0**; no polygon means clear. Debug drawing **0x005d8070** transforms local polygon points by object orientation and position. This is authored polygon geometry, **not the rendered mesh**.

- All **77 map-referenced scenery WOTs carrying `Occlusion Polygon Point Count`** have count **0**.
- The wider installed WOT catalogue scan also found no positive count among files carrying that field.
- Nonzero candidate point coordinates on AbandonedCamp/SmugglersCamp are unused because their count is zero.

No island mesh-based sensor blocking has been added. Physical collision/projectile blocking does not establish detection occlusion. Positive-count mod polygons are not yet exported/supported as a sensing feature.

### Nebula gate

Nebula clear-line method **0x005cd5e0** checks enable state, compares endpoint containing-nebula identity and tests a polygon segment. Different inside/outside membership can immediately reject visibility. The exact accessor/boundary/overlapping-cloud semantics still need instruction-level confirmation.

The shared nebula gate now:

- Obeys `Nebula Occlusion On/Off`.
- Rejects inside/outside pairs and segments crossing the boundary.
- Allows a segment wholly inside the same cloud; concave-cloud segments that exit/re-enter still block.
- Has **no unverified 40-unit nebula bypass**. The existing 40-unit *cloak* exception remains a separate provisional rule.

Moving storms' sensor polygons still use the authored mission volumes rather than the live storm displacement. That and nested/overlapping clouds remain provisional; cloud energy/rule volumes and rendering are separate consumers.

## 3. Human-player visibility spheres

`TP::IUserInterface::HumanPlayerVisibilitySphere` constructor **0x0055f440** stores centre/radius and prepares a spatial query. UI update **0x0055cee0** builds candidate bounds, then filters candidates through **0x0055e870**; it handles the second occluder list separately.

A group candidate bound reads each member's AI sight, multiplies it by **0x005d5e30(2,0)** and adds member distance from a group centre. The helper aggregates custom-info values; its semantic label is not decoded. It may be a crew/equipment modifier, but that is **not yet a specification**. Candidate bounds and final detection are distinct; the remake does not invent a final range multiplier from this trace.

## 4. Shared runtime implementation

`environment.rs::Sensors` is a **PreUpdate frame-boundary snapshot**, refreshed after scripted fleet-smoke positioning and before HUD input. It collects ship and gameplay-object positions, collision spheres, class sight, group membership, script/life state and cloak state. It avoids reading mutable ship queries from AI/input systems and keeps view-specific code out of the sensing rules. Scenario reset clears it; each refresh discards despawned contacts.

- `in_sight(observer, target, mission)` uses native strict surface distance and live/script-visible observers in the same shared fleet group (mission owner fallback for units without fleet membership), plus the target's script/life state, cloak gate and nebula clear-line test.
- Ships use `Class::sphere`, including its local centre transformed into world space. Some class spheres are exporter-derived fallbacks rather than authored spheres. Unarmed scenery without an exported sphere uses its world collision bounds (provisional), **not offshore dock-point distance**. Collider-free mod docks retain the previous dock-point-bound fallback so they remain usable; all shipped dockable scenery has a collider.
- `player_sees` uses the primary ship's group. Its own group remains known, script-hidden/dead targets do not qualify, and scenery retains its separate policy. Missing-primary and cinematic reveal exceptions are explicit. Blanket cinematic reveal of scripted-visible units remains provisional pending native NIS/debug flag decoding.
- Radar and star map contacts, names, reticles, left/right picking and context cursors use the same result. Stale ship menus close after contact loss; Focus/Dock are checked again when clicked. Focus is greyed without group sight, and the camera falls back from an unseen ship/animal focus rather than tracking its hidden coordinates. Projectile debug focus still works.
- Automatic skirmish/mission ship acquisition and ship/base gun target selection use sensing separately from gun range/arc. Explicit fleet/script attack orders may retain a destination outside sight so units can approach it; firing still needs detection. AI stance/path/contact-loss fidelity remains provisional, as before.
- Crew enemy-spotted speech now consumes shared detection, rather than an independent centre-distance heuristic. Its destruction callout requires a previously detected enemy. Dragon brains and homing/point-defence projectiles retain their specialized existing acquisition rules.
- Rules' actual unit/script visibility is **not overwritten by player detection**. Mission conditions must still know about distant units.

Snapshot changes become visible next frame; exact native update cadence is not recovered. No last-known-position/contact-memory system was invented. No new mesh range-culling or cloak timing/AI-override rules were added. Camera fallback on contact loss and the application of the recovered UI gate to automatic ship/base acquisition are coherent remake policies, not proof of every native consumer's exact behaviour.

## Validation and remaining acceptance

The default `tools/check.sh` gate passes (210 Rust tests; 75 Python tests, seven install/optional-fixture tests skipped). Re-running the same gate with the retail root enabled exposes an unrelated existing `test_combat.LocalCombatExportTests.test_cutter` failure: it assumes raw critical chances are in [0,1]. Neither that exporter nor its test was changed in this batch; the retail-enabled run has one failure and two optional-fixture skips. Regressions cover strict range equality, surface versus centre distance, altitude, unrelated group exclusion, group spotting, hidden/dead observers/targets, cloak, missing primary, scenery policy, sphere-centre transforms, despawn cleanup, enabled/disabled and short-distance cloud occlusion, same-cloud and concave segments, Focus availability and camera tracking. Existing tower-fire/tower-destruction and Mission 2 docking tests still pass.

Shared ECS/HUD smoke:

```sh
WAYLAND_DISPLAY= tools/shot.sh local/sight-classic.png --script sight \
  --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0
```

The script freezes three ships, disables their gun states and asserts detection and actual radar widget visibility through out-of-range → group-spotted → unrelated ally → cloaked → revealed states. It is a wiring check, not a combat or full campaign replay. Screenshots/logs: `local/sight-classic.*`. A separate Mission 2 launch reached initialization/NIS without an ECS error but timed out before capture; it is **not** counted as a completed mission runtime smoke.

**Remaining:** native mesh/render consumers, lost-contact memory/timing, precise NIS/cloak/crew-modifier semantics, scenery UI distance policy, live moving-storm/overlap geometry, and normal Mission 1/2 playthroughs after the matrix/detection corrections. Validate against original footage/screenshots before claiming full sensor parity.
