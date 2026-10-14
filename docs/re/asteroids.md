# Asteroid motion and breakup

Source: the retail WOT/EFT files (read-only) and observed behaviour of the original game, described in our own words.

## Tumble is mission-controlled

Mission 1's `Setup Asteroid Belt` actions specify:

| Group | Translation speed | Tumble speed |
|---|---:|---:|
| Asteroid Belt | 0 m/s | 0.5–0.9 rad/s |
| Montressor Asteroids | 0 m/s | 0.5–0.75 rad/s |

Both use `NO PATH`. Stationary centres are therefore intentional, but stationary orientations are not. The ordinary WOT render `ROTATIONSPEED` is zero; reading that field alone misses the physics-driven tumble.

The asteroid move action samples a speed within each action's lower/upper interval for each asteroid in the named group. It uses random directions for translation and tumble. Directions come from three signed random components, normalised; this is not a fixed yaw-only spin. The tumble is a normalised axis plus an angular speed. The `SpaceObjectPhysics` update integrates translation and applies an axis-angle rotation increment. The multiplication order of the rotation update is not verified (PROVISIONAL).

The random stream is the engine's shared LCG (multiplier 1664525, increment 1013904223), not an independent random seed per rock.

## Path follow modes (all missions)

`Setup Asteroid Belt` carries a rule `Follow Mode`; the exe enum is 0 `to end`, 1 `end to end`, 2 `loop`, 3 `teleport loop`.
The path is handed to each rock's AI controller; its update at the path end either stops (mode 0), wraps (2) or,
for mode 3, moves the object to the first path point and restarts. Retail data: missions 3, 5, 7, 8, 9 use `teleport loop` on open
conveyor-style paths (speeds 2-20 m/s), mission 4 uses `loop`, all others `to end`/no path. `Find Closest Point` picks each rock's
starting index (which also filters by a heading test not modelled). Implemented as `combat::PathMode`; before this only
`loop` wrapped, so rock belts in missions 3, 5, 7, 8, 9 stopped at the end of their path. `end to end` is implemented but unused by retail data.
`Meteor Storm` nebulae in missions 7/8 use the same `teleport loop` rule on their group (`Group to follow path` plus `Set Group Space Objects velocity` 25 m/s).
The remake moves each meteor emitter (not a meteor object) with the shared `combat::follow_path`, see `nebula.rs`; PROVISIONAL. Mission 3's Storm Shower is not moved yet.
Mission 3's `MontgomeryIslands` setup names a group with no members in the map, so it does nothing (also true in the data).
Skirmish maps have no setup rule, so their rocks stay static.

## Authored strength and breakup

These are ordinary asteroid custom-info definitions, not armed asteroid ship-section definitions:

| Type | Hit points | Mass | Child objects on death | Effect |
|---|---:|---:|---|---|
| Huge | 100 | 60000 | 2 Large + 2 Med | `Explosion_Huge` |
| Large | 50 | 40000 | 2 Med | `ShipExplosion_Medium` |
| Med | 30 | 20000 | 3 Small | `ShipExplosion_Medium` |
| Small | 10 | 10000 | none | `ShipExplosion_Medium` |
| Darkmatter 1–4 | 300000 | 20000 | 3 Small | `ShipExplosion_Medium` |

Each WOT has ten ordered `WorldObject To Create Upon Death` slots. Empty slots are skipped. Large → medium → small is a real recursive breakup chain, not just decorative debris. Armed asteroids are a separate case: their custom info uses ship damage sections and must not be treated as the ordinary asteroid family.

Damage is subtracted and destruction happens only on the positive-HP to non-positive-HP transition. The death routine creates the configured child world objects in the same world, initializes their motion and tumble, triggers an effect and sound handling, then removes the parent.

### Fragment motion recovered so far

Known so far:

- Spawn slots use a fixed direction table: +X, −X, +Y, −Y, +Z, −Z in retail axes. The built-in asteroid definitions only populate the first four slots.
- Spawn offsets use a child collision-size field multiplied by **1.1** and the slot direction. The exact collision-field interpretation still needs checking before calling it the radius in runtime code.
- Fragments receive linear velocity and a speed cap initialized to its magnitude. Stationary-parent speed interval: **0.8 to 0.8 + (child mass / 60000) × 2.2**. The mass interpolates between constants 0.8 and 3.0; the resulting lower/upper interval is sampled randomly. For moving parents, a signed contribution depends on speed and direction angle; the complete coordinate transformation is not recovered reliably. The runtime's additive parent velocity is explicitly provisional.
- Children take the parent's owner. This confirms owner inheritance, not insertion into the original named rule group.
- Each child gets a new random tumble axis and a speed sampled from **0 to π/2 rad/s** .
- A physics flag is enabled and three limit values are set to the maximum finite float. Their precise movement/acceleration-limit semantics need further tracing.

## Destruction effects

`ShipExplosion_Medium.eft` is a distinct effect from the currently exported `Explosion_Medium`. Its three particle primitives are:

| Primitive | Initial particles | Delay | Duration | Texture |
|---|---:|---:|---:|---|
| Smoke edge | 7 | 0.2 s | 1.5 s | `ThickSmoke` |
| Flash | 1 | 0 s | 3 s | `TestparticleTextureBW` |
| Sparks | 15 | 0 s | 1 s | `SparkFlare` |

All three use additive ONE/ONE blending. Sparks use the velocity-oriented pseudo-billboard chunk. The smoke expands and fades; the flash rapidly expands then fades. These are authored effect timings, not asteroid lifetimes. `Explosion_Huge.eft` also has smoke/flash/sparks, with initial counts 5/1/20. Despite its name, its base sprite-size curves are smaller than `ShipExplosion_Medium`; caller scaling must be understood rather than guessed from the name.

Both effects are now exported into `assets/fx` and spawned through the existing `fx::spawn` path. Native effect scaling and the exact destruction sound-pool selection remain unresolved; the runtime currently uses unit scale and the existing medium-explosion sound.

## Runtime implementation and checks

`tools/export_asteroids.py` exports ordinary WOT definitions to `assets/data/asteroids.json` and converts missing huge/darkmatter models without changing existing model paths. It validates positive finite strength/mass/radius, child references and termination of breakup chains. The campaign exporter preserves named asteroid membership as `group` on map objects; Mission 1's belt has 24 members and Montressor has 20, independent of polygon guesses.

`data.rs` loads/validates the definitions. `lib.rs` tags ordinary asteroids on every map; armed asteroids are excluded. `rules.rs` dispatches `Setup Asteroid Belt` by actual group, supplying translation/tumble intervals and path settings. `combat.rs` owns motion, shooting, death and fragment creation. Death is emitted only on the positive-to-zero HP transition; already dead rocks are excluded from subsequent shell hits, so deferred despawning cannot repeat breakup/kill credit. Destroying a target clears it, and child rocks retain the named group provisionally so the belt attack order can select them.

`collision.rs` rebuilds changed asteroid obstacles before ship movement. Tumbling box obstacles use transformed 3D faces, and broad-phase bounds include all eight transformed corners. Child rocks gain collision at spawn and lose it on despawn. Ship contacts now use finite rock mass and relative velocity, with a separate required `collision::Drift` component for collision motion; substeps translate the collider along with the rock. Explicit rule setup clears contact drift, but path ticks preserve it, and breakup includes it in the provisional parent-velocity inheritance. The planar inelastic contact response is shared with ships, not a recovered native asteroid formula; collision damage, torque and rock/rock or rock/static response remain unimplemented. See [collision.md](collision.md).

Regression tests cover retail HP and asset dependencies, invalid/cyclic definitions, group membership/setup, tumble intervals without centre drift, path velocity/stopping, one-shot death, recursive 2-medium → 6-small breakup/termination, target cleanup, effects, child colliders and rotated/translated collision bounds. `--script asteroid` skips the opening, frames a large belt rock at frame 90 and destroys it at frame 180. Runtime screenshot: `local/asteroid-breakup.png` (frame 195) shows the two children and flash/sparks; no ECS query conflict occurred.

Explicitly provisional: moving-parent fragment velocity, the collision-size field interpreted as spawn-offset radius, initial child orientation, inherited named group membership, effect scale/sound pool, native rotation multiplication order, waypoint path controller and exact engine PRNG sequencing (the runtime reuses combat's random stream). Full Mission 1 progression with the native lower HP and new collisions remains unverified; missions 2–13 remain first-pass, not validated gameplay.
