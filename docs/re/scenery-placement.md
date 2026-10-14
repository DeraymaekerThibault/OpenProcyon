# Island/base placement: height and orientation

## Status

**Implemented:** native matrix convention and full scenery orientation. **Still provisional:** native mesh-scene assembly/height. The first-dock height workaround is retained; this change does not claim to resolve every island/base height mismatch.

Evidence comes from the current exporters/runtime, the owner's retail WOT/MSB files read-only, all exported `assets/maps/*/map.json`, and observed behaviour of the original game, described in our own words. No new side-by-side retail comparison was made.

## 1. Orientation: two confirmed faults, now corrected

The map exporter preserves all nine `Orientation Matrix33` values and all position coordinates. Previously the runtime:

- Used only `atan2(rot[3], rot[0])` for ordinary scenery, discarding pitch/roll; only wrecks used a full matrix.
- Interpreted the serialized matrix as row-major. This reversed upright scenery yaw and also affected campaign ship/animal headings.

### Native convention is now established

The original matrix-vector helper **0x0040bfb0** computes output X from stored elements 0, 3, 6; Y from 1, 4, 7; Z from 2, 5, 8. Native island occlusion debug drawing **0x005d8070** uses that orientation and adds object position.

The follow-up serialization trace resolves the previous uncertainty:

- The Matrix33 text reader takes nine sequential floats and stores them **without transposition**.
- Writer **0x004ab910** emits stored elements 0 through 8 in that same order.

Thus TWT contains **native matrix columns**, not rows. `scenario::game_rotation` now constructs those columns and converts axes with `C M Cᵀ`, where `C(x,y,z) = (x,z,-y)`. All scenery uses the full rotation, shared by mesh roots, guns/docks and collision construction. Campaign ship, whale and dragon yaw is derived from the correctly transformed forward axis; living units retain their existing planar motion model. The campaign stand-in ship's synthesized matrix was updated too.

### Affected authored tilts

Catalogue scan: **637 Base_/Island placements, 76 distinct types, seven non-yaw placements** (threshold 0.0001). The tutorial pair duplicates content:

| Map | Object type / ID | Previously lost orientation |
|---|---|---|
| `basic_tutorial`, `tutorial` | `Island04` / 51 | Small tilt; map Z=5 |
| `mission03` | `Island14` / 22 | Tilt; map Z approximately -5.002565 |
| `mission13` | `Island_Rocky_04` / 17 | Large tilt (`rot[8]=0.559193`) |
| `mission13` | `Island_Rocky_05` / 157 | Near-inverted (`rot[8]=-0.99749`) |
| `storm_front` | `Island_Rocky_07` / 21, 24 | Near-sideways (`rot[8]=-0.017452`) |

The correction also changes yaw on otherwise upright scenery and campaign units. Mission approaches and NIS compositions therefore need renewed owner playtesting; old screenshots are not a parity reference for the corrected convention.

## 2. Height: retained workaround, not native assembly

`scenario::scenery_transform` still does:

```
placed translation = converted TWT position - placed rotation * scene origin
scene origin = explicit mod scene_origin, otherwise (0, first dock local Z, 0) in Bevy axes
```

For upright scenery this forces the first dock to the map object's authored altitude. It fixed reachable docking in owner playtests, but **no native first-dock normalization rule has been established**. Existing catalogue tests verify the workaround, not original-game height parity. See [campaign.md — Mission 2 owner-playtest fixes](campaign.md#mission-2-owner-playtest-fixes-islands-and-gun-towers).

Among scenery types referenced by exported maps, 21 Base_/Island types have converted docks; 20 have a nonzero first-dock height:

| Type | First dock local Z | Upright translation correction |
|---|---:|---:|
| `Base_AbandonedCamp` | -17.2 | +17.2 |
| `Base_SmugglersCamp` | 27 | -27 |
| `Base_Safetytown` (Norville in Mission 2) | 9 | -9 |
| `Base_ShipyardFreemason` | 59.3 | -59.3 |
| `Base_PortIvy` | 56.917 | -56.917 |
| `Island_Large01` | 29.25 | -29.25 |

These are exported coordinates/corrections, **not measured original height errors**. Scenery without docks is not normalized. Nonzero map altitude and explicit mod `scene_origin` remain supported.

### Why not simply apply all MSB translations?

- `tools/mdb_to_glb.py::convert` exports MDB vertex positions directly, without MSB mesh-node hierarchy transforms.
- `hardpoints` accumulates parent translations, subtracts the first DamageSection's accumulated offset, and ignores parent rotations. Meshes and attachment points do not come from a fully decoded native scene transform.
- `tools/export_collision.py::triangles` explicitly ignores GLB node transforms.
- Retail Layer1 pivots are Z=-17.2 for AbandonedCamp and Z=27 for SmugglersCamp, matching their docks. Safetytown's Layer1 Z=39 does **not** match its docks' Z=9. Its WindMill child also has yaw approximately -0.568977 and a nonzero position.
- Follow-up native node assembly **0x004850d0** links parents and applies pose data. Node pose application **0x00485790** subtracts a **rotated pivot** from the pose translation; hierarchy composition **0x00479d80** applies parent orientation and translation. This is not simply “add every MSB position”. The parser's node fields still need to be mapped to native pivot versus pose data.

Applying raw MSB translations to already-baked MDB positions risks double-counting pivots. Replacing the current height rule without that trace would exchange a known workaround for another guess.

## Validation and remaining acceptance

- Default `tools/check.sh` passes: matrix/scenery regressions, all shipped dock-datum tests, Mission 2 abandoned-island approach and Coventry/Norville dock/repair tests pass. The retail-enabled Python gate exposes an unrelated unchanged critical-chance assertion; see [visibility.md](visibility.md) for the exact validation limits.
- The matrix regression checks all three transformed native axes for every object in Storm Front, Mission 13, Mission 3 and the basic tutorial, including sideways/inverted orientations. This checks handedness and full rotation, not just self-consistency of yaw.
- Focused Storm Front runtime screenshot: `local/scenery-rotation.png`. Ignored scratch mod `local/scenery-smoke` moves the starting ship near island 21 and removes nebula drawing to inspect scenery. The isolated run completed; an earlier unmodified-nebula attempt captured a frame but timed out before normal screenshot-driver exit. This is a rendering smoke, **not a retail visual comparison**.
- Shared sight ECS/HUD smoke completes in both presentations; see [visibility.md](visibility.md).

**Next:** decode native default poses/pivots for the three Mission 2 bases and an asymmetric island; replace the height workaround only after that evidence. Keep meshes, guns, dock/boarding/flag/effect points and collision in one frame. Any introduced GLB node transforms must also reach collision extraction. Recheck normal Mission 1/2 approaches, firing origins and NIS against original evidence before claiming full placement parity.
