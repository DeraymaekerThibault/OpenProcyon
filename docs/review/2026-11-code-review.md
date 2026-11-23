# OpenProcyon code review: Bevy 0.20 practice, Rust standards, simplicity, moddability

Review date: 2026-11-23. Baseline: `main` at `9e16926` (Release 0.2.0).
Scope: all of `game/src` (~67k lines of Rust), `tools/` + `tests/` + scripts (~12.8k lines of Python and shell), `content/`, `mods/`, modder docs.

This document has three parts:
- **Part 1** covers the method, what the codebase does well, and the cross-cutting themes, with the findings I verified myself.
- **Part 2** is a prioritised roadmap and the list of items that need maintainer approval.
- **Part 3** is the appendix: the nine detailed bucket reviews (B1 to B9) with every finding, `file:line`, a suggestion, and whether the fix keeps behaviour the same.

---

## Part 1: Method, strengths, themes

### Method
1. **Checklist.** I researched Bevy 0.17 to 0.20 release notes, migration guides and the v0.20.0 examples, plus the Rust API Guidelines, and turned them into one shared checklist (Appendix A0). Each reviewer got it in its prompt, so none of them searched the web themselves.
2. **Nine reviews.** I split the code into nine buckets along the module ownership table in `docs/development.md`. A separate reviewer read each bucket in full against the checklist and against the project's own rules (`AGENTS.md`, `docs/development.md`). Each looked for four things: Bevy practice, Rust standards, behaviour-preserving simplification, and moddability.
3. **My own checks.** I re-checked every high-severity claim and most theme claims myself with grep and reads. The results are marked **✔ verified**, **≈ verified with a correction**, or **? needs a runtime check**. The bucket appendices are the reviewers' own output and were **not** all re-checked line by line. Treat their medium and low items as well-founded leads, not proven facts.
4. **Nothing ran.** No code was changed and no build or game was run. Several findings would change simulation output or need a smoke run to confirm; they say so.

### What is already good (keep doing this)
- **The battle clock.** A 33 ms `FixedUpdate` tick with one strongly chained `BattleSet`, input captured per frame and consumed per tick, and a catch-up cap. `interpolate.rs` swaps the simulated and drawn poses around the fixed loop, and `fixed_tests.rs` pins all of it. This is ahead of most Bevy projects and matches the Bevy 0.20 `physics_in_fixed_timestep` pattern.
- **Layering.** There is a shared library with interchangeable views, and `tests/test_layers.py` enforces "shared code never builds widgets" mechanically. Clicks go through shared `Action`s.
- **Rules interpreter.** It is pure (`Script::step` produces `Fx` effects, and `systems/apply.rs` is the single place they reach the ECS). Mission authors have a schema, `--check-mission` with "did you mean", rule traces, and a dev console that reuses `Script::act`.
- **Typed ship loader.** `data/ships.rs` uses serde structs, `#[serde(default)]`, kept insertion order, and errors that name the file, section and field. It is the template for the rest of the data (theme 2).
- **Graphics layer.** The render profile is properly optional. With no `look.json`, classic is untouched. A frame-data buffer at `@binding(99)` avoids material churn, and the shaders ship with the mod.
- **Documented rules and guesses.** Rules cite `docs/re/*.md`, and 285 `PROVISIONAL` markers sit in the Rust code. The importer stages into a temporary folder, swaps it in atomically and retries on Windows, and `gamefs.ci_file` gives case-insensitive lookup in one place.
- **Modding.** The overlay + `extends`/`set` design, five small and readable example mods, and an honest Troubleshooting table in `docs/modding.md`.

### Theme 1: Determinism leaks in the fixed tick (fidelity risk; highest priority)
The project's goal (faithful, comparable against the original with `PROCYON_TRACE`) depends on the same input giving the same battle. The tick is designed for that, but several leaks remain:

| Leak | Evidence | Status |
|---|---|---|
| Shared combat `Rng` is seeded once per process (`combat/mod.rs:156-161`, `init_resource`) and never reseeded per battle or logged. A restart or second battle continues the stream. | B1-03, B2-03 | ✔ verified (no reset in `scenario*`/`menu*`/`campaign.rs`) |
| A sinking ship's timer `Sinking.elapsed` advances in `Update` (`combat/mod.rs:375`, system registered in `Update` at `:233`), but `drive_health` reads it in the tick (`collision.rs:642`). When a wreck starts drifting depends on frame rate. | B2-02 | ✔ verified |
| Mission rules see the **drawn** pose. `FrameRules` runs in `Update` and `rules/systems.rs::units` reads `Transform`, which there holds the interpolated pose. `Interp::sim_pose` exists but is not used. Volume and distance conditions can flip one frame earlier or later depending on the display rate. | B3-01, B3-02 | ✔ verified |
| The difficulty setting reaches the simulation through a process-global atomic (`menu::difficulty()`), and `fx/sim.rs` reads `detail_scale()` the same way. Traces depend on the player's `settings.json`, and the sim depends on the menu module. | B7-01, B2-11 | reviewer-verified |
| Draws from the RNG happen in unsorted query order in `shells`, `blasts`, `beam::burn`, `crew_repair` and `collision::sail`. `fire`/`base_fire` already sort by `Entity`, so the convention exists but is applied only partly. | B2-01, B2-04 | reviewer-verified |
| `Entity::to_bits()` is used as a "random" draw or ordering key in sim code: `ship.rs:174`, `ship/attack.rs:165`, `crew_arms/posts.rs:99`, `boarding/fight.rs:305`, `crew.rs:302`, `environment.rs:314`. Entity ids follow spawn and allocation history, not game data. | B1-17, B8-14 | ✔ verified |
| The AI think-clock "spawn number" is a `Local<usize>` assigned in query order and never reset on restart (`ship.rs:269-270`). | B1-01, B1-02 | ✔ verified (Locals present) |
| Private, never-reset RNGs: `nebula::storms` (`Local<u32>` seeded at 0) and `cloak::spotted`. | B5-06, B5-34 | reviewer-verified |
| `Local<HashMap<Entity,_>>` state is never cleaned up or reset per battle. | B2-22 | reviewer-verified |

**Recommendation (one coherent change, not nine patches):**
1. **One seeded RNG.** A single `SimRng` resource, reseeded in the battle reset with a seed that is logged and can be overridden (`--seed` / `PROCYON_SEED`). Draw named sub-streams from it for nebula, cloak and boarding.
2. **Stable spawn order.** A `SpawnIndex(u32)` component assigned at spawn, plus one helper that iterates in spawn order. It replaces `Entity` sorting and `to_bits` draws.
3. **Sinking timer.** Advance `Sinking.elapsed` in the tick.
4. **Rules on the simulated pose.** Feed `FrameRules` the simulated pose via `Interp::sim_pose`, or move the rule step into the tick after `Outcome`. That is a fidelity decision: the docs say the original runs rules per frame (see `docs/re/tick.md`).
5. **Difficulty for the battle.** Take a `Difficulty` resource snapshot at battle start.
6. **Guard test.** Bevy 0.20's `ScheduleBuildSettings { shuffle_seed }` (debug feature) can run the existing battle tests under several system orders and assert identical traces.

This **changes simulation output**: RNG streams move, so pinned test values may shift. Do it as one PR with a before/after `PROCYON_TRACE` comparison, and do it before more parity work, so parity results are reproducible.

### Theme 2: Bad mod data crashes or fails silently (main moddability issue)
The overlay design is good, but the error path is not. Across buckets, data is parsed from untyped `serde_json::Value` with `unwrap`, `expect` or indexing, so a modder's typo gives a panic with no file or field name, or nothing at all:

- **Merged mod files.** Invalid JSON in a merged mod file (`guns.json`, `bullets.json`, `strings.json` …) becomes an empty value and is merged in. `modding.rs:182-187` then writes the result with `unwrap()`, and later lookups panic without context. ✔ verified (B8-01).
- **Untyped tables.** Guns, bullets, asteroids, crew and boarding use `Value` plus `expect("number in ship json")` (data.rs:425-529) (B8-02). Indexing `lib.bullets[..]`, `bd.damage[..]` panics when a mod omits an entry (B2-08).
- **HUD bar layout.** `BarLayout::load` (`classic/panels.rs:38-47`) panics on a malformed `hud/bar/<skin>.json` and re-reads the file on every rebuild. ✔ verified (B6-06). The same pattern appears in `sprites.rs` fonts and `hud.json` (B6-19).
- **look.json.** It is untyped. Typos are ignored, a syntax error silently disables the whole profile, and the file is replace-only, so two graphics mods cannot be combined (B4-07, B4-08).
- **Effect files.** One stray or malformed `fx/*.json` crashes startup (`FxLib::from_world` → `read_json`) (B5-14). The sim reads raw JSON every tick in `can_see`, `nebula::storms`, `dragons::think` and `crew::callouts` (B5-09).
- **Mission rules.** They stay as `Value` and are matched by `Type` string every frame. Bad data is found only if the author runs `--check-mission`, and `Script::new` calls `expect("rules")` (B3-03, B3-17). Mission 1 specifics are hard-coded in generic code, so a mod that replaces `mission01` can panic (B3-07).
- **Silent skips.** `try_json` silently skips present-but-invalid files at about 49 call sites, and there is no "mods loaded" report (B8-16).

**Recommendation:**
1. **One typed load path.** Add a `data::load::<T: DeserializeOwned>(logical_path) -> Result<T, DataError>` that reports the logical file, the resolved source mod and the serde path to the bad field, reusing what `data/ships.rs` already does. Port to it, in order: guns/bullets → asteroids/crew/boarding/docks → `fx/*.json` → `look.json` → `hud/bar/*.json` → `theme.json`.
2. **Merged files fail clearly.** A merged file that fails to parse becomes a **load error naming the file and mod**, not an empty value.
3. **Check missions at load.** Run `rules::check_with` when a non-retail mission loads and log the findings, so mission authors get them without knowing the CLI. Then resolve `Type` strings to an enum once at load, driven by `schema.rs`.
4. **Start-of-game mod report.** Print which mods loaded, which files each overrides or merges, and what was skipped and why.

All of this keeps behaviour the same for valid data. It does not change the data format as long as unknown fields stay allowed. A typed error enum would ideally use `thiserror`, **a new dependency that needs approval**; a hand-written enum works too.

### Theme 3: Hand-synchronised copies of one truth
Many tables exist twice or more and are kept equal by hand, sometimes by a test:

- **Rule vocabulary.** It appears in five places: `cond.rs`/`act.rs`, `schema.rs`, `check.rs`, `victory.rs` and the console `TABLE`. "Equivalence" comparison is written four times with different epsilons (B3-04, B3-05).
- **Merged-file list.** `MERGED` in Rust, the list in `docs/modding.md`, and a second merge of skies and nebulae in `render/mod.rs` (B8-05).
- **Theme defaults.** `Theme::default()` repeats `theme.json` (about 100 lines), held equal by a test (B6-10).
- **Look blocks.** `Look::with_shaders` has 14 near-identical blocks, and each gate is repeated three times (B4-11).
- **Skirmish ships.** The seven are listed in both `import_assets.FIRST` and `pack_data.SKIRMISH` (B9).
- **Menu rebuild key.** `MenuKey` (`menu.rs:481`) is a hand-listed tuple plus a `Debug` string that should cover "everything the view draws". It **omits `sk.map_info` and `sk.arms_open`**, which `classic/menu.rs:126-129` draws. The rebuild guard at `classic/menu.rs:52-54` compares only the key, so opening the Map Info dialog probably does not redraw the screen. **? needs a runtime check** (B7-02). This is the most likely user-visible bug found.

**Recommendation:** for each table, pick one source and derive the others from it. Use `schema.rs` for the rule vocabulary, a `merged: true` key or a data file for the merge list, and `theme.json` as the only source with `Default` loading it. For the menu, replace `MenuKey` with change detection on the model resources (`model.changed()` already exists), or derive `Hash` on a `MenuView` struct that the view builds from, so a missing field cannot happen. Add a smoke script for Map Info and the Fleet dialog.

### Theme 4: Oversized wiring and systems
- **`lib.rs::run()`.** About 190 lines with about 60 `add_systems` calls. `ship`, `scenario`, `view` and `course` have no plugin of their own (B1-05, B1-24).
- **Repeated run conditions.** `in_state(Battle)` is repeated **65×** and `overlay_closed` **23×**, ✔ counted, and they are applied inconsistently (B1-06, B8-09). Put them on the `BattleSet` / `FrameRules` sets via `configure_sets(..).run_if(..)`. First check which systems deliberately run while the overlay is open; those stay outside the sets.
- **Monolithic systems:** `gunnery::fire` (about 430 lines, 30+ params in tuples, B2-05), `collision::sail` (about 375 lines, 17 params, B2-06), `ship::ai` (about 145 lines, 20+ params, B1-08), `view::input` (about 175 lines inside an immediately-invoked closure, B1-14), `docking::routine` (about 185 lines, B8-08), `options_menu::menu` (about 300 lines, 28 params, B7-17), `crew_arms::skirmish_screen` (about 200 lines, B8-13), `world_hud::place_labels` (15 params, two jobs, B6-14).
  - **Splitting them.** Bevy's tools for this are `#[derive(SystemParam)]` structs for the parameter groups and splitting along the systems' own seams (decide → act → present).
  - **Lint allows.** The crate-wide `too_many_arguments = "allow"` is what Bevy itself does, but 40 more local `#[allow(clippy::…)]` lack a reason ✔ counted. Switch to `#[expect(lint, reason = "…")]` (B1-29, B4-18).
- **Large files with clear seams.** `ship.rs` (1118 lines: state, helm, captain AI, fleets; B1-26), `rules/console.rs` (991; input, commands, parse, run; B3-15), `menu/skirmish.rs` (1070; lobby model vs computer-fleet AI; B7-06), `campaign.rs` (three modules in one; B3-21).
- **Copy-paste families:**
  - Render: nine copies of the additive `Glow` material and five of the batch-draw skeleton (B4-03, B4-04); `kit::additive` duplicates `fx::RetailAdd` (B4-05).
  - Effects: four copies of the "accumulate dt, run 30 Hz ticks" loop with an unnamed `0.25` clamp (B5-04); `wake.rs` and `blackhole.rs` share the trail code (B5-05).
  - Combat and rules: `Shell` is built field by field in four places (B2-07); unit-list building and voice spawning are each copied three times (B3-09).
  - UI: the despawn-and-rebuild-with-`Local` pattern is copied five times (B6-07).

### Theme 5: Layer and production/test leaks
- **Views decide, not just draw.** `classic/world_hud.rs::place_labels` chooses the reticle target and which ships get banners. `classic/menu.rs::element` maps about 50 retail button names to `Action`s, including their enabled rules. Wheel scrolling and row counts live in the view (B6-01, B6-02, B6-12). A second presentation crate would have to copy all of it. Move these into `panels.rs` and `menu.rs` as shared state or a shared table. `test_layers.py` misses this because it looks for widget constructors, not decisions (B6-26).
- **Test scripts inside production code.** Test-script hooks sit inside production systems (`scenario::shot_target`, `debug_kill`, branches in `ship::ai`, `dev_board`, `mods_bake_hook`). The 789-line dev autopilot lives in `rules/systems/` rather than `smoke/` (B1-11, B3-08, B7-11, B8-20). Script names are compared as strings in 14 places even though `smoke::Script` is an enum (≈ verified: 14 `cfg.script.as_deref() == Some(..)` sites, not ~50 as one reviewer estimated; B1-10).
- **Process control inside systems.** There are 14 `std::process::exit` calls outside tests (✔ counted), some inside systems (B1-23, B7-20). Prefer sending `AppExit` with a status code, so logs flush and the exit is testable.

### Theme 6: Stringly and magic typing
- **Engine order and ship orders.** Engine order is a bare `usize` compared with `0/2/3` (B1-09). `ship.order = 2` appears in docking and boarding (B8-12).
- **Race and enums.** Race is a `u8` (`race == 4`), and lobby choices are tuples (B7-08).
- **Gun and bullet names.** Behaviour is keyed on name substrings, and `== TORPEDO_BULLET` disagrees with `starts_with` (B2-17).
- **Rule booleans.** They are inconsistent: the string `"TRUE"` is used, a JSON `true` silently reads FALSE, and `Run Once` takes JSON booleans (B3-06). This is a **mission-author trap**: accept both, or warn at load.
- **Recommendation.** Parse strings once at load into enums or newtypes (`EngineOrder`, `Race`, `GunFamily`, `ShipId`). The JSON stays the same; only the in-memory types change.

### Theme 7: Bevy API currency and idioms
- **`Interaction` is deprecated in 0.20 but used throughout.** It is allowed with `#![allow(deprecated)]` in **15 files**, shared modules included (`panels.rs`, `menu.rs`, `setup.rs`, `starmap.rs`, `crew_arms.rs`, `alliance.rs`) ✔ verified. It is polled with roughly three different mappings to state (B6-03, B6-21, B7-19). One shared `pressed(&Interaction)` / hover helper now would make the eventual move to `ui_widgets::Button` + `Activate` / `Hovered` / `Pressed` a single-file change. Narrow the allows to items, not whole files.
- **Camera selection.** `Single<…, With<Camera3d>>` is used for "the main camera" in seven or more systems (`audio.rs` ×4, `nebula.rs`, `view.rs`, `briefing.rs`, `classic/world_hud.rs`, `classic/ropes.rs`, `classic/console.rs`). The Fleet-screen ship preview spawns a second `Camera3d` (`classic/ship_preview.rs:58`), so all of those systems **silently skip** while it exists. That includes `audio::play`, so menu sounds are probably silent on that screen. ✔ code verified, **? runtime check** (B5-01). Fix: add a `MainCamera` marker.
- **Hand-rolled states.** The in-game menu overlay, the `Loading` gate and the setup phase are hand-rolled state machines (resource + early returns + manual despawn and unpause in three places). They fit `SubStates` / `ComputedStates` + `OnEnter`/`OnExit` + `DespawnOnExit` (B7-04, B7-05, B7-16).
- **Process-wide data caches.** About 25 `OnceLock` / `static Mutex` hold data that belongs in resources (boarding `DATA`/`MODDED`, `crew_data()`, `Theme::current()`, the `rules::specialize` mutex) ✔ counted (B8-07, B6-11, B4-21). Globals cannot be reset per battle and cannot pick up a mod switch at runtime.
- **Material churn.** Nebula and rule materials are rewritten every frame with the clock, although the frame buffer already carries the clock (`render/nebulae.rs:364`, `render/rules.rs:286`) ✔ verified (B4-01, B4-02).
- **Exclusive `&mut World` systems every tick.** `reconcile_fleets` and `fleet_follow` (B1-18). Asteroid obstacles are rebuilt from all triangles every tick (B2-09).
- **Entity back-references.** `Docker`, `Grapple` and `Dockable.taken` are plain fields with no cleanup on despawn. Bevy relationships (`#[relationship]`) give that cleanup for free (B8-10).
- **Not recommended:** moving settings to Bevy 0.19's `SettingsPlugin`. The `settings.json` format is pinned for players, so keep the hand-rolled file. Do make its save atomic, as `profiles::write` already does, and fix the lossy `x as u8` load (B7-10).

### Theme 8: Hygiene against the project's own rules
- **Wrong guess marker.** `ponytail:` is used **9×** instead of the required `PROVISIONAL` ✔ verified (B1-16). There are also unmarked guesses in combat, HUD and rules (B2-15, B6-24, B3-18) and many unnamed sound and visual tuning literals (B5-13, B4-13).
- **Misplaced doc comments.** Several `///` blocks sit above the wrong function, for example the PROVISIONAL difficulty note above `damage_pool` instead of `side_damage_scale` (B2-13, B7, B8-17, B1-25). They actively mislead and are cheap to fix.
- **Build path in the binary.** `env!("CARGO_MANIFEST_DIR")` is used at six sites (`lib.rs:98,117,122`, `setup/import.rs:111`, `theme.rs:260`, `sprites.rs:518`) ✔ verified. It is a dev convenience that bakes the build machine's checkout path into release binaries. The CI runner's path is not personal, but a local release build's is (rule 8 spirit; B7-13). Gate it behind `cfg(debug_assertions)` or a `dev` feature. `sprites.rs:518` reads a font from it with `unwrap()`.
- **Crash log.** It records the full command line, which may contain personal paths (B7-14). That is low severity, because the log stays local and the header includes the arguments on purpose.
- **Licence statement.** The provenance and licence of `content/voice/events.json` should be written next to the file (rule 7). About 290 of its entries are never read (B9-18, B5-22). **Maintainer decision.**

### Theme 9: Python importer robustness
- **Silent fallbacks.** `tools/msb.py::parse` falls back to a heuristic byte scanner with **no warning** ✔ verified (`msb.py:40-43`); `mdb.py`'s tag table does the same. Exporters `continue` on `(KeyError, ValueError, FileNotFoundError)`. A modded install can therefore import "successfully" with objects missing (B9-01, B9-03). Fix: log each skip with file and reason, and print an end-of-import summary.
- **Encodings and case.** `export_combat.load` reads WOT files as strict utf-8 with exact case. A `UnicodeDecodeError` (a `ValueError`) is swallowed as "skip" (B9-02). There are about 84 `read_text`/`write_text` calls without `encoding=`, including `.env` on Windows (B9-09), and case-sensitive `glob` on Linux (B9-10).
- **Inconsistent JSON output.** Exported JSON is written in five indent styles and has no version marker (B9-12, B9-13). **Format change: approval needed.** A consistent style also changes re-import diffs, so check it with `compare_assets`.
- **Duplicated tooling.** There are three binary-reader styles (`msb._Reader.string` has no bounds check), three text parsers, and about 30 hand-rolled `sys.argv` CLIs (B9-04 to B9-08).
- **CI and gate gaps.** `check.sh` is POSIX `sh` with only `set -e` and uses `eval`. CI runs Ubuntu only, so the Windows paths in `setup/` and the importer are not covered (B9-11, B9-23).

---

## Part 2: Roadmap

### Phase 0: quick wins (small, keep behaviour the same, one or two PRs)
1. Rename `ponytail:` to `PROVISIONAL:` (9 sites). Move the misplaced doc comments to the right functions (B2-13, B8-17, B7 and B1 items).
2. Make merged-file parse failures a named error instead of an empty value (B8-01). Make `BarLayout::load` and `FxLib` report the file instead of panicking (B6-06, B5-14).
3. Add a `MainCamera` marker and use it in every `Single<…Camera3d>` (B5-01). Add the missing `MenuKey` fields plus a Map Info smoke (B7-02). Confirm both bugs at runtime first.
4. Replace the 14 string compares with `smoke::Script` enum matches (B1-10).
5. Switch `#[allow(clippy::…)]` without a reason to `#[expect(…, reason)]`. Narrow the file-wide `#![allow(deprecated)]` to items behind one shared `Interaction` helper.
6. Python: add a warning on the MSB/MDB fallback, log exporter skips plus an end-of-import summary, and pass `encoding=` everywhere (B9-01, B9-03, B9-09).
7. Save settings and bindings atomically (temp + rename, as `profiles::write` already does) and clamp `as u8` (B7-10).

### Phase 1: determinism (one PR, changes simulation output; compare traces before and after)
Theme 1 items 1 to 6. Do it before further parity work.

### Phase 2: typed data and modder error reporting (several PRs, keeps behaviour the same for valid data)
Theme 2. Start with guns and bullets, because they have the most modding traffic. Then run `check_with` at mission load, add the mod-load report, and resolve rule `Type` to an enum. Add a "your first mod in 5 minutes" section at the top of `docs/modding.md` (B8-23).

### Phase 3: structure (incremental, one module at a time, each with its existing tests + smoke)
1. **Plugins:** per-feature plugins for `ship`, `scenario`, `view` and `course`, and trim `lib.rs::run()`.
2. **Run conditions:** move them onto the sets.
3. **Big systems:** split each with `SystemParam` structs. Start with `gunnery::fire`, `collision::sail` and `ship::ai`.
4. **Test code:** move it out of production systems, and the autopilot into `smoke/`.
5. **View decisions:** move them out of `classic/` into shared modules (B6-01, B6-02).
6. **Copy-paste:** remove the render, effects and combat duplicates (B4-03 to B4-06, B5-04 and B5-05, B2-07).
7. **Single sources:** derive each hand-synchronised table from one source (theme 3).

### Phase 4: Bevy API migration
1. **UI input:** move from `Interaction` to `ui_widgets` / picking (`Activate`, `Hovered`, `Pressed`) behind the shared helper from phase 0.
2. **States:** turn the overlay, `Loading` and setup phase into Bevy states.
3. **Resources:** turn the `OnceLock` data caches into resources.
4. **Relationships:** use them for dock and grapple links.

### Needs maintainer approval (per AGENTS.md)
- A new dependency `thiserror` (optional; a hand-written error enum works).
- Data or format changes:
  - a version marker in exported JSON and one JSON style (B9-12, B9-13);
  - a `role` key in `hud/bar.json` (B6-05);
  - merged-file opt-in by key or a data file (B8-05);
  - mergeable `look.json` blocks (B4-08);
  - moving hard-coded tuning into data (repair time, harpoon range, camera, sound volumes, races; B8-11, B1-15, B1-20, B5-13, B7-09).
- A licence or provenance note for `content/voice/events.json` (B9-18).
- The fidelity decision on whether mission rules evaluate on the simulated pose or per tick (B3-01, B3-02).

### Verification notes and corrections to the reviewers
- **≈ corrected.** The count of string-compared script names is 14, not about 50 (B1-10). B6's "eight files with `allow(deprecated)`" is 15 files repo-wide.
- **Downgraded.** I lowered B1-05 (`run()` size) from high to medium: it is readability, not correctness. B7-14 (crash log) is low.
- **Not adopted.** `SettingsPlugin` (the format is pinned) and BSN scenes (still changing in 0.20).
- **Fidelity caution.** B8-14 and the theme 1 items change RNG streams. That is intended, but it moves traces and any pinned random-dependent test values.

---

## Part 3: Appendix
- A0. The review checklist given to every reviewer
- B1. Core and app wiring
- B2. Combat and collision
- B3. Mission rules and campaign
- B4. Render profile
- B5. Effects, world presentation, environment, audio
- B6. Classic view and HUD
- B7. Front end, setup, app states
- B8. Gameplay subsystems, data and modding
- B9. Python tools, tests, scripts


---

## A0. Review checklist
### Review checklist: Bevy 0.20 + Rust (researched 2026-11 from Bevy 0.17-0.20 release notes, migration guides (MG), v0.20.0 examples, Rust API Guidelines)

#### 1. ECS architecture
1.1 Plugins by feature: each feature module exposes one Plugin registering its resources/messages/observers/systems/sets. Smell: lib.rs with hundreds of add_systems lines; one plugin registering systems from many modules. (0.18: App::remove_systems_in_set lets mods drop systems.)
1.2 Name SystemSets per phase, order sets (configure_sets(...).chain()), put systems .in_set. Bevy naming convention now `*Systems` suffix (MG 0.17). 0.20 adds chain_weak/before_weak/after_weak: they only order systems with conflicting data access, ignore hidden deps; NEVER use for sim order. Smell: long cross-module `.after(other::fn)` chains.
1.3 Order-dependent systems must be explicitly ordered; otherwise order is "deterministic but arbitrary" and can change after unrelated edits (0.20 Schedule Randomization; ScheduleBuildSettings{shuffle_seed} for tests). Since 0.17 conflicts are judged by signatures only: two &mut Transform queries conflict unless Without<> filters. Smell: sim systems touching same component unordered.
1.4 Run conditions over early returns (.run_if(in_state), resource_exists, on_message, on sets; observers take run_if since 0.19). Smell: `if !state.x { return; }` at top of systems.
1.5 States/SubStates/ComputedStates; setup in OnEnter/OnExit; DespawnOnExit/DespawnOnEnter replace StateScoped (always on). Since 0.18 NextState::set re-runs OnExit/OnEnter even for same state (0.19 also fires DespawnOnExit); 0.20 renames set_if_neq -> set_if_different. Smell: manual despawn loops, despawn_all::<Marker> helpers, set_if_neq.
1.6 Sim in FixedUpdate; input in RunFixedMainLoopSystems::BeforeFixedMainLoop; interpolation/camera in AfterFixedMainLoop. Smell: gameplay math scaled by delta in Update.
1.7 Resources vs components. 0.19: resources ARE components on singleton entities; a type can't derive both. Smell: Vec<Entity> registry resources duplicating a query/relationship.
1.8 Messages vs observers. 0.17: buffered = Message (MessageWriter/Reader, write_message); Event = observer-only; On<E> replaces Trigger<E>; EntityEvent with entity field; world.trigger. 0.19 Replace->Discard. 0.20 On<Add<A>>, flat pointer events (On<PointerPress>). Use messages for high-volume ordered sim flows; observers for one-off reactive entity/UI wiring; don't put order-dependent sim logic in observers (order guarantee unverified).
1.9 Required Components (#[require]) instead of bundles; children![]/related![] macros. BSN (bsn!) evolving, adopt carefully. Smell: custom Bundle structs grouping defaults; marker components whose spawn sites must remember companions.
1.10 Relationships: ChildOf/Children or custom #[relationship] pairs instead of hand-maintained Vec<Entity>/Entity back-refs. Smell: stale entity IDs, Entity fields without cleanup on despawn.
1.11 Component hooks / lifecycle observers for invariants; 0.20 `Ready` event for whole spawned scenes. Smell: "remember to call init_x() after spawning".
1.12 Queries: With/Without/Changed/Added/Spawned; Single<>, Option<Single<>>, Populated<>; 0.20 iter_many yields Result; summary_tick. Smell: query.single().unwrap(), `for .. { break }` to grab one, ParamSet where Without would do.
1.13 Commands for structural changes; 0.19 commands.delayed() (not for sim timing).
1.14 Systems/observers can return Result (BevyError, `?`); app.set_error_handler; 0.20 panics in systems routed to fallback handler, .context(). Smell: unwrap/expect on Query::get or asset lookups; silent `let Ok(..) else { return }` chains swallowing real failures.
1.15 One job per system; #[derive(SystemParam)] / #[derive(QueryData)] to group params. Bevy itself allows too_many_arguments/type_complexity, but review size. Smell: systems > ~80 lines or > 8 params; systems mixing input, rules and drawing.

#### 2. Rendering / UI / assets / text / audio
2.1 UI: Node{..} single layout component, px()/percent()/vw()/auto() helpers, border_radius is a Node field since 0.18 (2D fields in 0.20), UiTransform not Transform on UI, 0.20 Val::Em/Rem, FixedNode. Smell: BorderRadius component, Style, Transform on UI nodes.
2.2 Widgets: 0.20 deprecates ui::Interaction and ui::widgets::Button -> ui_widgets::Button (Activate event) + Hovered/Pressed components. Smell: polling Query<&Interaction, Changed<Interaction>>.
2.3 Text: 0.19 Parley; TextFont.font is FontSource (handle.into()); font_size is FontSize::Px(..); LineHeight separate component (0.18); 0.20 default size is Rem(1) -> pin Px for fixed-pixel HUDs. Justify (not JustifyText).
2.4 Assets: AssetServer::load / load_builder (load_with_settings*/load_untyped deprecated 0.19); Handle::Uuid replaced Weak (0.17); Assets::insert returns Result; AssetLoader needs TypePath (0.18); hot reload via file_watcher. Smell: std::fs::read + serde_json in Startup systems for data that could be an asset; polling LoadState every frame instead of AssetEvent / loading state.
2.5 0.19: audio and ui cargo features no longer implied by 2d/3d. 0.20: sprites are Mesh2d+SpriteMaterial; shaders moved to WESL (review WGSL #import paths).
2.6 0.19 SettingsPlugin / SettingsGroup for persisted options (a hand-rolled file is fine if the format is pinned for compatibility).

#### 3. Data-driven / moddability
3.1 Game data as typed assets through AssetLoader (#[derive(Asset, TypePath, Deserialize)]) -> hot reload, dependency tracking, error reporting. Smell: tables parsed ad hoc in systems; include_str! of game data.
3.2 Overlay/override directories: one resolver function with fixed, documented precedence (mod -> content -> imported); custom asset source/reader. Smell: path-joining logic duplicated across modules; mod lookup differing between loaders.
3.3 Stable schema: #[serde(default)] for new optional fields; deny_unknown_fields only where typos must fail; never rename a key without alias/migration; round-trip/schema test per data type. Smell: renamed fields without #[serde(alias)], Option fields that panic when absent.
3.4 Reflection: #[derive(Reflect)] auto-registered since 0.17 (except generics); #[reflect(Component, Resource, Default)] for inspector/BRP tooling.
3.5 No hard-coded tuning numbers: data or named consts with a source comment; mark guesses. Smell: magic floats in systems; same constant repeated in two modules.

#### 4. Deterministic simulation
4.1 All sim in FixedUpdate; constant DT; accumulate input per frame, consume per tick. Smell: Time<Real>/wall clock in sim; frame-rate dependent behaviour.
4.2 One strongly chained set sequence in FixedUpdate; no chain_weak; sim systems not in any set is a smell.
4.3 Query iteration order is NOT a guarantee (archetype/table layout). Where results depend on order (target selection, damage, ties) sort by a stable key (ship index / spawn sequence), not Entity bits. Smell: "first match wins" loops over queries; HashMap iteration in the sim (use BTreeMap / sorted Vec).
4.4 Seeded sim RNG resource, separate cosmetic stream, log the seed. Smell: rand::random()/thread_rng() in sim; RNG call order depending on parallel system order.
4.5 Apply sim messages within the tick; avoid sim state mutated from observers or Update.
4.6 Headless replay/trace tests: MinimalPlugins + manual time, step N ticks, assert hash/trace.

#### 5. General Rust
5.1 Module cohesion: one concept per module; split files > ~800-1000 lines along real seams; keep mod.rs small. Smell: utils grab-bags; one file holding loading + rules + drawing.
5.2 Errors: thiserror-style enums for library/loader errors (C-GOOD-ERR); BevyError at the system edge; no unwrap() in non-test code, expect("invariant: ...") only for true invariants. Smell: Result<_, String>, errors logged then ignored.
5.3 Types over strings/bools: newtypes for ids/units (C-NEWTYPE), enums over bool args (C-CUSTOM-TYPE) and over string keys; parse strings once at load. Smell: `match name.as_str()` in systems; fn f(a: bool, b: bool).
5.4 Naming: RFC 430, as_/to_/into_, no get_ prefix on getters, consistent word order; *Systems for sets.
5.5 Visibility: private -> pub(crate) -> pub only for real API used by other presentation crates. Smell: everything pub.
5.6 Lints worth enabling (Bevy does): doc_markdown, manual_let_else, match_same_arms, redundant_else, semicolon_if_nothing_returned, unwrap_or_default, allow_attributes_without_reason (#[expect(.., reason)]), print_stdout/print_stderr. Consider: cast_possible_truncation, cast_sign_loss, float_cmp, unwrap_used, indexing_slicing (sim), wildcard_imports. Smell: #[allow] without reason; println!; `as` casts on data-file values.
5.7 Magic numbers: named consts with units and source doc comment.
5.8 Tests: unit tests beside code; ECS tests via minimal App/World + run_system_once; asset-needing tests ignored/skipped; every bug fix gets a regression test.
5.9 Docs: //! module docs saying what the module owns; /// on non-obvious pub items with errors/panics sections; Debug on public types.
5.10 Common traits eagerly (Debug, Clone, Copy, PartialEq, Eq, Hash, Default); From/TryFrom over ad-hoc to_x; Deref only for smart pointers/ECS newtypes.

#### Outdated-API grep list (flag any hit)
EventReader, EventWriter, Events<, send_event, Trigger<, OnAdd, OnInsert, OnReplace, on_replace, `On<Add, `, trigger_targets, Pointer<, StateScoped, enable_state_scoped_entities, set_if_neq, DefaultErrorHandler, Interaction, ui::widgets::Button, BorderRadius as component, `font_size: <f32>`, JustifyText, load_untyped, load_with_settings, Handle::Weak, register_type (non-generic), get_single, *Bundle structs, chain_weak in sim code.

Unverified for 0.20 specifically: clear_messages_on_exit spelling, observer order guarantee, Time<Virtual> max-delta default, TimeUpdateStrategy name, audio API details, query iteration order stability, whether delayed commands follow fixed time. (Note: the project compiles against 0.20, so anything that *compiles* is valid API; deprecation warnings would fail clippy -D warnings. Treat "outdated" hits as "older-style but still supported" unless clearly deprecated.)

---

## Appendix: Core & app wiring (lib.rs, main.rs, ship.rs + ship/, scenario.rs + scenario/, view.rs, interpolate.rs, fixed_tests.rs, smoke/mod.rs, line_of_fire.rs, course.rs, alliance.rs, stats.rs, logging.rs)

Overall assessment. The core is in good shape architecturally: the 33 ms fixed tick, the single chained `BattleSet` in `FixedUpdate`, the `Removed`/`Joining` deferred-removal protocol, the pose interpolation (`interpolate.rs`, a careful and well-documented before/after-fixed-loop swap) and `fixed_tests.rs` (pause, speed, catch-up cap, message retention, no-replay, interpolation, GlobalTransform on tick) are exactly what the checklist asks for, and the constants carry source citations to `docs/re/*`. Most plugins hook into the right `BattleSet`s, `Fleets` uses a `BTreeMap`, `reconcile_fleets` sorts by a stable key, and the sim RNG is a fixed-seed xorshift separate from the cosmetic ones. The weak spots are (1) a few determinism leaks that depend on ECS query iteration order or on state that survives a restart (`ai` spawn numbers, tie breaks, global `combat::Rng`), (2) `lib.rs` and `ship.rs` have become grab-bags (`run()` ~190 lines, `ai` ~145 lines/20+ params, `input` ~175 lines with an immediately-invoked closure, `ship.rs` 1118 lines mixing state, helm physics, AI, fleets), (3) stringly/magic-typed control (`cfg.script.as_deref() == Some("fleet")` although a `Script` enum exists; engine orders as `usize` with `== 3`), and (4) test-script hooks living inside production systems in `scenario.rs`/`ship.rs`. Nothing found needs a behaviour change; most items are refactors that can be verified by the existing tests plus `tools/check.sh`.

#### Findings

##### B1-01 [severity: high] [category: fidelity-risk] AI captain "spawn number" is query-iteration order and survives battle restarts
**Where**: `game/src/ship.rs:269-297` (`Local<usize>` named `spawned`, `clocks.entry(e).or_insert_with(|| { *spawned += 1; ThinkClock::new(now, *spawned - 1) })`).
**What**: The start delay of each computer captain's think clock is `0.05 s x (spawn number mod 10)` (`ThinkClock::new`, ship.rs:215). The "spawn number" is however the order in which `ai` first *meets* the ship while iterating `ships: Query<...>`, not the order ships were spawned, and the counter is a system `Local` that is never reset by `restart`/`enter_battle`.
**Why**: Query iteration order is not a guarantee (it follows archetype/table layout and changes when components are inserted, e.g. `reconcile_fleets` inserting `FleetPilot`, docking/cloak markers). A second battle in the same process (R restart, F10 + new skirmish, next mission) starts counting at the previous total, so with 3 ships the offsets are 3,4,5 instead of 0,1,2: the same scenario is not reproducible between first launch and restart, and parity traces differ. The sim also uses `time.elapsed_secs()` (f32, `Time<Fixed>` elapsed that never resets) as `now`: precision degrades in very long runs (autoplay at 16x), which quantises the 0.033 s steps.
**Suggestion**: Give every ship a monotonically increasing `SpawnIndex(u32)` (component, set in `spawn_ship`/campaign spawn from a `Scenario`-scoped counter resource reset on battle start) and use it for the think delay, for sorting `snap` in `ai`, and for the `Explore` quadrant hash instead of `Entity::index/bits`. Reset or key `ThinkClock` on battle start. Consider an integer tick counter (`Res<TickCount>` incremented in `BattleSet::Tick`) instead of f32 elapsed seconds for think clocks.
**Behaviour-preserving?**: needs-check (first battle identical if the counter is assigned in spawn order and spawn order equals the current first-think order; restart behaviour intentionally changes to match the first launch).
**Effort**: M

##### B1-02 [severity: high] [category: fidelity-risk] Order-dependent "first match / ties" over query results in `ai`
**Where**: `game/src/ship.rs:286-289` (`snap` built from `ships.iter()`), `:322-326` (candidates), `:253` (`choose_target` -> `min_by` on distance: first minimum wins), `:344` (`adopt`: `snap.iter()...find_map(...)` takes the first fleet mate/ally in query order), `:329-338`.
**What**: Target choice, ally-target adoption and the distance tie-break all depend on the order of `snap`, which is the query iteration order.
**Why**: Checklist 4.3. Equal distances are rare for floats but "which fleet mate's target is adopted" is decided by iteration order whenever two mates have different targets; table order can change mid-battle (archetype moves). This is exactly the kind of difference a parity run flags as an unexplained mismatch.
**Suggestion**: After building `snap`, `sort_by_key` on the new `SpawnIndex` (B1-01) (or `(fleet index, spawn index)`); make the tie-break in `choose_target` explicit (`min_by` on `(distance, spawn_index)` with `total_cmp`). Cheap: one sort of <= ~30 ships per think.
**Behaviour-preserving?**: needs-check (removes nondeterminism; if the original resolves in creation order this is also the faithful order).
**Effort**: S

##### B1-03 [severity: medium] [category: fidelity-risk] `combat::Rng` is created once and never reseeded per battle
**Where**: `game/src/combat/mod.rs:155-162` (`Default` = fixed constant), `init_resource::<Rng>()` at mod.rs:221; nothing resets it in `scenario::enter_battle`/`restart`/`reset_fleets` (scenario.rs:77-90). 16 modules draw from it.
**What**: The seed is fixed (good) but is a process-lifetime stream, so battle N+1 starts mid-stream. Also order of draws between unordered systems in one set decides who gets which number.
**Why**: Two launches of the same scenario via menu vs `--map` differ after the first battle; restart is not a replay. Checklist 4.4 ("seeded sim RNG resource... log the seed").
**Suggestion**: Re-insert `Rng::default()` in `reset_fleets` (it already resets Fleets/Sensors/Target), add a `--seed`/`PROCYON_SEED` override, and `info!` the seed in `spawn_scenario`'s "battle start" line. Keep the cosmetic `FxRng`, `StarRng`, `Namer` streams separate as they are.
**Behaviour-preserving?**: needs-check (first battle unchanged; later battles now match the first).
**Effort**: S

##### B1-04 [severity: medium] [category: fidelity-risk] Wall-clock seeds in battle setup are not logged or overridable
**Where**: `game/src/scenario/spawn.rs:14` (`Namer::new` seeds from `SystemTime` nanos), `:182` (starfield seed from `SystemTime::subsec_nanos`).
**What**: Ship names and the star pattern are cosmetic and rightly separate from the sim, but the seeds are neither logged nor injectable, so a screenshot (`tools/shot.sh`, "pictures compare" in AGENTS.md) differs run to run in the sky and banners.
**Why**: Hidden non-determinism in the screenshot/regression pipeline; also `Namer` re-implements the xorshift that `combat::xorshift_unit` already provides.
**Suggestion**: One `CosmeticSeed` value taken from `--seed`/`PROCYON_SEED` or the clock once at startup, logged with `info!`, used by `Namer`, `starfield` and `FxRng`. Make `Namer::pick` call `combat::xorshift_unit`.
**Behaviour-preserving?**: yes (default still clock-seeded).
**Effort**: S

##### B1-05 [severity: high] [category: bevy] `lib.rs::run()` is a 190-line imperative builder with ~60 `add_systems` lines, arg parsing and state selection inline
**Where**: `game/src/lib.rs:150-336`.
**What**: `run` parses CLI args (own `arg` closure plus second readers: `lobby_scenario` re-reads `std::env::args` at :137, `view::camera` reads `--look` at view.rs:522-527, `--cam`/`--yaw` at lib.rs:271-286), builds `Config`, chooses the start `Screen` with a 15-arm match (:246-262), inserts resources, and registers FixedUpdate/Update systems for ship, scenario, briefing, wake, blackhole, course, view in one place. `ship::reconcile_fleets/ai/fleet_follow`, `out_of_map`, `outcome`, `enter_battle`, `leave_battle` etc. are registered here rather than by their owning module (`ship.rs`, `scenario.rs`, `view.rs`, `course.rs` have no `plugin`).
**Why**: Checklist 1.1: "lib.rs with hundreds of add_systems lines" is the smell; the docs say "feature logic goes in its owning module". Because `ship`/`scenario`/`view` have no plugin, their system ordering lives far from the code (and other modules reference them by function pointer: docking.rs:251-253 `.after(crate::collision::sail).before(crate::out_of_map)`, docking.rs `.before(crate::input)`).
**Suggestion**: Split into: `cli.rs` (`Cli::parse(&[String]) -> Result<Cli, String>` replacing the closure + other `env::args()` readers; unit-testable), `ship::plugin`, `scenario::plugin` (enter/leave/restart/outcome/lighting/reset), `view::plugin` (input, camera, sky, hud, fleet_keys), `course::plugin`, and let `run` be `App::new().add_plugins((DefaultPlugins..., core plugins, view)).run()`. Expose public system sets instead of function-pointer ordering (see B1-07). Update the `docs/development.md` table row for `lib.rs`.
**Behaviour-preserving?**: yes (pure move, keep set membership identical).
**Effort**: L

##### B1-06 [severity: medium] [category: bevy] The giant `Update` chain and repeated state/overlay run conditions
**Where**: `game/src/lib.rs:309-334` (a 12-system `.chain()` with two `run_if`s, mixed with unrelated `screenshot`, `hd_sky`, ...); 65 occurrences of `run_if(in_state(menu::Screen::Battle))` and 23 of `overlay_closed` across the crate (e.g. docking.rs:247-262, nebula.rs:648, combat/mod.rs:232, environment.rs:475-476).
**What**: Whether a FixedUpdate system respects the Battle state / open in-game menu is decided per system, and is inconsistent: docking `Forces/Simulate` systems add `overlay_closed`, nebula/environment/asteroid `Forces` systems do not.
**Why**: Checklist 1.4/4.2. A new plugin author must remember two conditions; omissions are silent.
**Suggestion**: Put the common conditions once on the sets: in `battle_schedule` `configure_sets(FixedUpdate, (...).chain().run_if(loading::done).run_if(in_state(Screen::Battle)).run_if(menu::overlay_closed))` (decide deliberately whether `Forces` should skip while the overlay is open; the virtual clock is probably paused then anyway, which would make both conditions redundant), then drop them from member systems. For the Update chain introduce `FrameSet::{Scripts, Input, Camera, Hud}` so modules order `.in_set(...)` instead of `.before(crate::input)`.
**Behaviour-preserving?**: needs-check (verify what pausing does for the overlay; the inconsistent cases are effectively a latent bug or dead conditions).
**Effort**: M

##### B1-07 [severity: medium] [category: bevy] `BattleSet::Simulate` relies on cross-module function-pointer ordering
**Where**: `game/src/lib.rs:301` `(briefing::halt_arrival, blackhole::pull, sail, out_of_map).chain().in_set(BattleSet::Simulate)`; `docking.rs:249-253` `dock_glide.in_set(Simulate).after(crate::collision::sail).before(crate::out_of_map)`; `dragons.rs:53` `fly.in_set(Simulate)` with no order against `sail`.
**What**: The movement stage has an implicit internal order (halt -> pull -> sail -> glide -> out_of_map) encoded as one chain in lib.rs and a fn-pointer constraint in docking.rs; `dragons::fly` is in the same set with no order relative to `sail`.
**Why**: Checklist 1.2/1.3. An unrelated edit can reorder unconstrained systems (Bevy 0.20 randomises schedule order); `fly` and `sail` both move `Transform`s (dragon is a `Ship`-driven mover per its own comment dragons.rs:50) - check whether they touch disjoint entities, otherwise this is a real ambiguity.
**Suggestion**: Split the stage into ordered sub-sets (`SimulateSet::{Prepare, Move, Glide, Cull}` or extend `BattleSet` with `Movement`, `PostMovement`) and put `fly` in a defined slot. Add `ScheduleBuildSettings` ambiguity detection (`LogLevel::Warn`) in a test app of the full plugin set to catch other unordered conflicting pairs.
**Behaviour-preserving?**: yes if each system keeps its current relative position; needs-check for `fly`.
**Effort**: M

##### B1-08 [severity: high] [category: simplify] `ship::ai` is a 145-line, 20-parameter system with tuple-packed params and a 7-element anonymous tuple
**Where**: `game/src/ship.rs:260-404`; `snap` tuple at :286 (`f.2`, `f.3`, `f.5`, `f.6.1`, `f.6.2`, ...).
**What**: Parameters are grouped in anonymous tuples only to stay under Bevy's 16-param limit (`(mut explore, mut breakoffs): (Local<..>, Local<..>)`, `storms: (Option<Res>, Option<Res>)`); the snapshot is `Vec<(Entity, Vec3, Side, bool, f32, Option<String>, (Transform, usize, Vec<f32>))>` and clones every ship's `h.sections` Vec and fleet `String` each think tick. The body mixes script-skip rules, target selection, fleet adoption, aim, flee, attack planning and exploring. `if mission.is_some() { return; }` (:277) is an early return where a run condition belongs.
**Why**: Checklist 1.15 (system > ~80 lines / > 8 params, one job per system), 1.4 (run conditions over early returns), naming. Allocation per think tick (clone of `Vec<f32>` and `String` for every ship, every tick, even when no captain is due) is wasted; the `snap` is built before knowing whether any clock is due.
**Suggestion**: (a) `struct ShipSnap { entity, pos, side, afloat, radius, fleet: Option<&str>/Arc<str> or index, ... }`; (b) `#[derive(SystemParam)] struct CaptainWorld<'w,'s> { sensors, holes, storms, bases, scripted, ... }`; (c) split into `choose_target(...)` (returns `Option<(Entity,bool)>`), `plan_course(...)`; (d) `.run_if(not(resource_exists::<campaign::Mission>))`; (e) skip building `snap` unless some clock is due (move the `clock.due` pass first). Keep `choose_target`/`attack::*` pure functions and their tests.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-09 [severity: medium] [category: rust] Engine order is a bare `usize` indexing a `(&str, f32)` table, with literal `0/2/3` comparisons
**Where**: `game/src/ship.rs:5` (`ORDERS`), `:47` (`order: usize`), `:477` (`if ship.order == 3`), `:372-402` (`order = 3`, `= 0`), `:737`, `:824` (`s.order = 2`), view.rs:149-153, view.rs:271,273,341,343, `view.rs:565 ORDERS[ship.order].0`; ~46 literal `order = N` / `order == N` sites crate-wide.
**What**: "3 = Emergency Speed, 2 = Ahead Full, 0 = All Stop" is implicit knowledge; the names live only in a debug-text table; out-of-range `order` panics on `ORDERS[self.order]` (ship.rs:114).
**Why**: Checklist 5.3 (types over bool/ints, parse once). A mod or contributor changing the table silently breaks the helm roll rule and AI order literals.
**Suggestion**: `#[repr(u8)] enum EngineOrder { AllStop, Slow, Full, Emergency }` with `fn fraction(self) -> f32` and `fn label(self)`, `next()/prev()` for the +/- keys (replaces `(order+1).min(ORDERS.len()-1)`); keep `usize` mapping only where saves/HUD serialise it (check `campaign` save and `panels`). `Ship.order` becomes `EngineOrder`; `ORDERS` stays as a deprecated const if mods read it. Data format unchanged (serialise as the same integer) - if the save stores `order` this needs a check with the maintainers.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-10 [severity: medium] [category: simplify] Script/test names compared as strings although `smoke::Script` exists
**Where**: `game/src/scenario.rs:117-169,216` (9 sites), `ship.rs:315-316`, `lib.rs:611,630`, `scenario/spawn.rs:255,522`, plus boarding.rs, briefing.rs, cloak.rs, combat/asteroid.rs, environment.rs, menu.rs, menu/saves.rs, options_menu.rs, panels.rs, rules/systems.rs (50 `script.as_deref()` sites in 18 files). `smoke/mod.rs` itself already uses `cfg.script == Some(Script::Impacts)`.
**What**: `Script` implements `Deref<Target=str>` precisely so old code compiles with `.as_deref() == Some("fleet")`, but a typo (`"fxtets"`) compiles and silently never matches, defeating the validated-names macro.
**Why**: Checklist 5.3 (enums over string keys). Also these hooks are in production code paths (see B1-11).
**Suggestion**: Replace every `cfg.script.as_deref() == Some("x")` by `cfg.script == Some(Script::X)` / `matches!(cfg.script, Some(Script::A | Script::B))`, then remove `Deref<Target=str>` (the compiler then finds all remaining string uses). Add `Config::is_script(&self, Script)` helper.
**Behaviour-preserving?**: yes.
**Effort**: S (mechanical)

##### B1-11 [severity: medium] [category: simplify] Test-driver logic embedded in production systems (`scenario.rs`, `ship::ai`, `spawn_scenario`)
**Where**: `game/src/scenario.rs:103-163` (`shot_target`: pause/pause-restart asserts, speedtest orders, course waypoints, target picking), `:165-190` (`debug_kill`: `damage`, `victory`, `defeat`, `restart` scripts at frame 300), ship.rs:314-319 (AI disabled for four scripts), `scenario/spawn.rs:255-270` (the `fleet` script adds an allied fleet), `:522`.
**What**: `shot_target` is named after one script and contains seven; `ai` has script-name conditions inside the captain loop; `spawn_scenario` pushes a test group.
**Why**: The AGENTS/docs rule says `smoke/` owns "Validated --script names, ... keep production simulation in its owning module". These hooks make production code carry test coupling and make `ai`'s skip rule hard to read (`:314-319`).
**Suggestion**: Move each script body into `smoke/` as its own small system (`smoke::pause_check`, `smoke::speedtest`, ...), registered by `smoke::plugin`. For `ai`, add a `ScriptedShip`/`NoAi` marker (or a single `smoke::suppresses_ai(&Config) -> bool`) inserted by the smoke plugin rather than reading `cfg.script` in four places. For spawn, give `spawn_scenario` a `fn extra_groups(cfg) -> Vec<Group>` hook that `smoke` supplies.
**Behaviour-preserving?**: yes (verify with the `--script` smoke runs).
**Effort**: M

##### B1-12 [severity: medium] [category: bevy] `Config` mixes immutable CLI settings with mutable run state, and "private" fields are crate-visible
**Where**: `game/src/lib.rs:70-89`; mutated in scenario.rs:52-54,236 (`cfg.map`, `cfg.groups`, `cfg.ended`), outcome.rs:124 (`cfg.ended`), menu.rs:930-945, menu/saves.rs:99; `cfg.frame` incremented in `screenshot` (lib.rs:606).
**What**: Fields are private but `Config` lives in the crate root, so every module in the crate can read and write them (privacy is only to the outside world). `shot_frame` duplicates `shot_frames.last()`; `parse_shot_frames` never returns empty so `unwrap_or(500)` at :204 is dead.
**Why**: Checklist 1.7 / 5.5. `ResMut<Config>` in `outcome` (a FixedUpdate sim system) and `restart` makes run bookkeeping part of the sim's data dependencies (and blocks parallelism with every other `Res<Config>` reader).
**Suggestion**: Split into `Cli` (immutable `Resource`) and `Run { map, groups, ended, frame }` (or `Battle`-scoped resource); give `ShotRun { frames, late, path, saved }` its own resource and replace the global `static SHOTS_SAVED: AtomicUsize` (lib.rs:598) with a field updated by the observer via `ResMut`. Remove `shot_frame` field in favour of a method.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-13 [severity: medium] [category: rust] lib.rs private `use` list is a hidden prelude for `use crate::*` in child modules, and the re-export list is partly private plumbing
**Where**: `game/src/lib.rs:56-68` (`pub use ...` lists plus `use std::f32::consts::{PI, TAU}`, `AccumulatedMouseMotion/Scroll`, `PrimaryWindow`, `collision::sail` that lib.rs itself does not use except `sail`), `ship.rs:2`, `scenario.rs:2`, `view.rs:4` (`use crate::*;`), `scenario/*.rs` (`use super::*`), `main.rs:7` (`pub use openprocyon::*;` in the binary).
**What**: `PI`, `TAU`, `AccumulatedMouse*`, `PrimaryWindow` are imported in lib.rs only so that `use crate::*` in children finds them. The documented rule ("do not add glob re-exports to lib.rs; write `crate::module::Name`") is bypassed from the other side: the children glob-import the root. Many `pub use` names (`enter_battle`, `restart`, `shot_target`, `debug_kill`, `leave_battle`, `ai`, `step`, `sky`, `hud`, `input`, `camera`, `toggle_debug`, `resume_scenario`) have zero `crate::`/`openprocyon::` users outside lib.rs: they are re-exported only so `run()` can name them bare. The grouping comment says "grouped by owning module" but `pub use combat::{GunStates, Health, Library, Target, Transient}` (line 66) sits outside the grouped block. `main.rs`'s `pub use openprocyon::*` makes the `classic` view write `crate::menu` etc.
**Why**: Checklist 5.5/5.1: hard to see where a name comes from; adding one import in lib.rs can silently change name resolution (shadowing) in three big modules; public API surface of the library is larger than needed for "other presentation crates".
**Suggestion**: Replace `use crate::*` in ship.rs/scenario.rs/view.rs with explicit imports (bevy prelude + the handful of crate items); delete the unused root imports; make `run()` reference `scenario::enter_battle` etc. directly and drop those names from the `pub use` list (keep only what views/other crates really use: check `classic/` and `mods`); fold the `combat` line into the grouped block. In `main.rs` use `use openprocyon as crate_name`... or explicit `pub use openprocyon::{menu, panels, ...}`.
**Behaviour-preserving?**: yes. (Removing a `pub use` is an API change for external presentation crates: flag to maintainers.)
**Effort**: M

##### B1-14 [severity: medium] [category: simplify] `view.rs::input` is a 175-line, 22-parameter system built around an immediately-invoked closure and clone-and-reinsert of `Ship`
**Where**: `game/src/view.rs:109-285` (closure `(|| { ... })();` at :137-276; write-back at :277-284).
**What**: The closure exists only to get early `return`s; `ship`/`pilot` are cloned, mutated, compared with `!=` and re-inserted via `commands.entity(actor).insert(ship)`. Camera input, engine orders, plot accept/cancel, Backspace cancel, target clear, pause toggle, and a 100-line click handler (own-ship select, bank shot, dock, attack, plot, move order) are one system. `Pointer` (:24) is a `SystemParam` that bundles unrelated resources (`Bindings`, `Fleets`, `Plot`, `FreeCamera`, mouse accumulators) and the name is misleading.
**Why**: Checklist 1.15, 1.8. Inserting a full cloned `Ship` back overwrites anything another frame system changed in between (lost update) and always bumps change detection when any field differs. Hard-coded keys (`KeyCode::Enter`, `Backspace`, `KeyC`, `Pause`, `NumpadAdd/Subtract`, `ShiftLeft`, view.rs:147-181, 267) bypass the `Bindings` (`Act::*`) that other actions use, so a modder/player cannot rebind them.
**Suggestion**: Split into `camera_input`, `order_keys` (speed/plot/cancel/clear), `world_click` (one function per click outcome returning an enum `ClickOutcome` such as `SelectOwn | BankShot | Dock | Attack | Plot | Move`), each as its own system sharing a `Pointer`-free `SystemParam`. Mutate `Ship` through `Query<&mut Ship>` (Bevy only flags change when actually written; avoid compare/clone). Route the remaining raw keys through `Act` entries (new `Act` variants default to the current keys, so existing `bindings.json` stay valid; adding variants touches a data format -> ask a maintainer).
**Behaviour-preserving?**: yes (needs the existing `camera_tests` plus a manual smoke).
**Effort**: L

##### B1-15 [severity: medium] [category: modding] Camera and zoom tuning is hard-coded; the "free camera" mod can only change the pitch range
**Where**: `game/src/view.rs:36-46` (`FreeCamera { pitch }` from `data/camera.json`), `:48-59` (retail constants), `:62-103` (0.005 rotate sensitivity, 0.04/tick, 3.0 zoom step, `* 10.0` scroll step, `10.0..300.0` free dist limits, `1.5` rad/s, `60.0` zoom/s, `3.0` vs `2.0` lift in `camera` :529, `6.5` easing at :536, `3000.0` ray length at :620).
**What**: A mod author who wants different zoom range, speed or camera height cannot, except for pitch; `ponytail:` comments mark guesses (view.rs:63) instead of the mandated `PROVISIONAL` keyword.
**Why**: Checklist 3.5, AGENTS rule 5. Camera feel is a prime mod target and these are exactly "named constants with a source or data".
**Suggestion**: Extend `data/camera.json` (`FreeCamera::load` already uses `#[serde]`-like defaults) with optional `dist: [min,max]`, `lift`, `rotate_speed`, `zoom_speed`; name remaining retail numbers as consts citing `docs/re/camera.md`; replace `ponytail:` with `PROVISIONAL:`. Adding JSON keys is backward compatible but is a data-format change: ask a maintainer and document in `docs/modding.md`.
**Behaviour-preserving?**: yes with defaults equal to today's values.
**Effort**: S-M

##### B1-16 [severity: low] [category: simplify] `ponytail:` is used instead of the project's `PROVISIONAL` marker
**Where**: ship.rs:445, ship/attack.rs:164, view.rs:63, victory.rs:199, collision/avoid.rs:178, environment.rs:301, render/sails.rs:145, collision.rs:739, crew_arms/posts.rs:98.
**What**: Nine guesses/approximations use a private word. `rg PROVISIONAL` (the audit command in AGENTS.md rule 5) does not find them.
**Why**: Rule 5 says "Anything not verified... is marked PROVISIONAL". Several are fidelity-relevant (per-ship draw "from the entity, not the original's random generator").
**Suggestion**: Search/replace `ponytail:` -> `PROVISIONAL:` (and keep the explanation). Add a `tests/test_repo_hygiene.py` check that fails on `ponytail`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-17 [severity: medium] [category: simplify] Entity bits used as "random" draws and as ordering keys
**Where**: `game/src/ship.rs:172-175` (`Explore::goal`: `e.index_u32() % 4` initial quadrant; redraw = hash of `e.to_bits()` XOR position bits), `ship/attack.rs:165` (throttle draw from `e.to_bits()`), `ship.rs:712` (`live.sort_by_key(|s| (s.2, s.0.to_bits()))`), `smoke/trace.rs` shell ids by `to_bits`.
**What**: Entity index/generation depend on despawn/free-list history (including all the scenery and effect entities spawned before), so these "draws" and the roster order differ between a fresh start and a restart even in a deterministic sim.
**Why**: Checklist 4.3/4.4. The comments honestly mark them PROVISIONAL (`ponytail`), but the cause is a missing stable per-ship id.
**Suggestion**: Same `SpawnIndex` as B1-01; derive draws from `splitmix(spawn_index, think_count)`; sort the roster by `(fleet index, spawn index)`. Consider a tiny shared `sim_hash(u64) -> f32` helper next to `combat::xorshift_unit` replacing the three open-coded constants `0x9E37_79B9_7F4A_7C15`.
**Behaviour-preserving?**: needs-check (values of the draws change; they are already PROVISIONAL).
**Effort**: S

##### B1-18 [severity: medium] [category: bevy] `reconcile_fleets` and `fleet_follow` are exclusive `&mut World` systems running every tick
**Where**: `game/src/ship.rs:705-781` (76 lines, 6 world queries, 3 `unwrap()`s at :762, :871, :875), `:949-1024` (exclusive; per follower ~15 `world.get`/`get_mut` calls, `groups.clone()` each tick, `vec![aim]` allocation per ship per tick); registered `BattleSet::Commands` and `BattleSet::Fleet` (lib.rs:298,300).
**What**: Exclusive systems cannot run in parallel and serialise the whole FixedUpdate chain at those points; nearly every access they do (Ship, FleetPilot, Transform, Health, MissionShip, Fleets, Target, Orbit) is expressible as ordinary queries. `promote`/`set_formation`/`fleet_help` (called from `commands.queue`) legitimately need `&mut World` as commands.
**Why**: Checklist 1.12/1.14/1.15: world-poking with `unwrap()` and repeated `world.resource::<Library>()` inside loops is the "query.single().unwrap()" smell in disguise; `roster order` bugs are harder to audit when state is read through `world.get`.
**Suggestion**: Make `fleet_follow` a normal system (`ResMut<Fleets>` read-only clone avoided via `Res<Fleets>`, `Query<(&mut Ship, &mut FleetPilot, &Transform, &Health)>` + `Query<&Transform/&Ship>` for the leader using `get_many` or a pre-pass collecting the leader pose), and split `reconcile_fleets` into `rebuild_rosters` (queries) + `repair_command_ship` (`world` only when promoting). Replace the `unwrap()`s with `let Some(..) else { return }` or `expect("invariant: ...")` stating why (the `groups.get_mut(&owner)` after `reconcile_fleets` + roster check is an invariant).
**Behaviour-preserving?**: yes (sorting keys unchanged).
**Effort**: L

##### B1-19 [severity: low] [category: simplify] `helm_step`: needless block, boolean flag, scattered magic numbers
**Where**: `game/src/ship.rs:421-482` (the whole body is wrapped in a bare `{ ... }` at :422/:481; `ai: bool` argument; `step()` wrapper at :416 passes `false`; literals `3.0`, `0.67`, `0.33`, `5.0/3.0`, `/ 3.0`, `3.0` (waypoint reach seconds), `4.0`, `-60.0, 60.0`, `0.1 * v`).
**What**: The extra block adds an indent level; `ai: bool` is a bool-argument (C-CUSTOM-TYPE); several movement constants (speed-band torque scaling 1/3 and 5/3, the +-60 clamp, 3 s look-ahead, x4 boost cap) are inline while neighbours (`TURN_SCALE`, `HELM_FULL`) are named and cited.
**Why**: Checklist 5.3/5.7/3.5. `docs/re/movement.md` evidently documents some of these; the reader cannot tell which are measured and which are the "Assumed mapping (not verified)" the comment at :459 admits.
**Suggestion**: Remove the inner block; `enum Helm Control { Human, Computer }` (or pass `AI_BOOST: bool` as `Option<BoostRange>`); extract named consts (`FAST_TORQUE_DIV`, `SLOW_TORQUE_MUL`, `ANG_ACC_LIMIT`, `CUT_CORNER_SECS`, `AI_BOOST_MAX`) with source/PROVISIONAL note; split `helm_step` into `steer_command` (waypoint pop + think-gated rudder), `turn` and `translate` pure functions for unit testing. Also `ship.waypoints.remove(0)` is O(n): `VecDeque` if waypoint lists grow.
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B1-20 [severity: medium] [category: modding] Gameplay tuning constants cannot be changed by a mod
**Where**: `ship.rs:5-24,143-148,204,232,609,915-920` (`ORDERS` fractions, `FULL_SPEED`, `TURN_SCALE`, `HELM_FULL`, `COMPUTER_CREW`, `HOLE_MARGIN`, `THINK_SECS`, `RESELECT_SECS`, `REPLAN`, `SLOT_ARRIVE`, `ALIGNED`, `TURNING_THROTTLE`, formation spacing `r + 30.0`, angles 35/45 deg in `formation_slots` :627-648, `swap_slots` fit weights 50/50/500), `lib.rs:375` (`MAP_FLOOR`), `lib.rs:479` (`MAX_GAME_SPEED` is the only one with a data override: `data/hud/speed.json`), `scenario/spawn.rs:43-52` (`faction()` keyword table: "grant", "montgomery", "shipyard", "port", "regina").
**What**: These are retail rules with citations (good, faithful default), but a mod that adds a ship, base or faction named differently (or wants a different formation spread) has no data hook; `faction()` guesses a base's fleet type from substrings of its name (marked PROVISIONAL), so a third-party base named "Fort Orion" is silently `None`.
**Why**: Checklist 3.5; the project goal is "easy to mod" and docs/development.md says tuning goes in a mod folder where possible.
**Suggestion**: Add an optional `data/tuning/ship.json` (serde defaults = today's constants) loaded once into a `ShipTuning` resource; for `faction()` read an optional `faction` key from the base/map JSON first and fall back to the name heuristic. This adds data formats/asset paths: ask a maintainer before implementing. Meanwhile gather the constants in one `ship/tuning.rs` module with doc citations so the future data override is a one-file change.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-21 [severity: medium] [category: simplify] `Group` struct literals repeated five times; no `Clone`/constructor
**Where**: `game/src/scenario/spawn.rs:213-240` (two literals, then `cfg.groups.iter().map(|g| Group { player: g.player, side: g.side, ... clone ... })` which is exactly `.clone()`), `:258`, `:291`, `menu/skirmish.rs:957`.
**What**: 10 fields spelled out each time with the same defaults (`row: None, ids: vec![], weapons: vec![], crew: vec![]`, `race: 4`, `team: -1`), and the manual copy at :241-252 re-implements `Clone`.
**Why**: Adding a field means editing five places; magic defaults `4` ("unknown race") and `-1` ("no team") are repeated unnamed.
**Suggestion**: `#[derive(Clone)] pub struct Group`, `impl Group { pub fn new(player, side, human) -> Self }` with `race: Group::UNKNOWN_RACE`, `team: Group::NO_TEAM` consts, and struct-update syntax `Group { race, team, ships, ..Group::new(0, Side::Player, true) }`; `cfg.groups.clone()` replaces the manual map. Also `ships: Vec<(String, Option<[f32;3]>, Option<String>)>` and `crew: Vec<[Vec<String>; 8]>` deserve small named structs.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-22 [severity: low] [category: simplify] Duplicated default light values and Orbit defaults
**Where**: `lib.rs:342-343` (`DirectionalLight` illuminance 9000, `Transform::from_xyz(30,60,20)`, `GlobalAmbientLight` brightness 450) vs `scenario/lighting.rs:35-38` (the same `from_xyz(30.0, 60.0, 20.0).looking_at(...)` and `Color::WHITE`); `lib.rs:273-286` (three `Orbit { yaw: .6/.9, pitch, dist }` literals); `lib.rs:240` and `:281` both call `FreeCamera::load(assets_dir())` (file read twice).
**What**: Same constants in two files; the free-camera data is parsed twice at start.
**Why**: Checklist 3.5 (same constant repeated in two modules); lighting.rs's "default white light" must equal `setup`'s.
**Suggestion**: `const DEFAULT_SUN_POS: Vec3`, `DEFAULT_SUN_ILLUMINANCE`, `DEFAULT_AMBIENT`; `scenario::lighting::default_sun()`; load `FreeCamera` once into a local variable and derive `Orbit::initial(free.is_some(), shot, cam args)` as a function (also testable). Also rename the odd trailing-underscore alias `MapLighting_` (lighting.rs:42) to `SunQuery`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-23 [severity: medium] [category: bevy] Process exit and global state inside systems
**Where**: `lib.rs:631` (`std::process::exit(0)` in `screenshot`), `lib.rs:143-145` (`exit(2)` inside a plugin function), `smoke/mod.rs` `screenshot_hooks` (`process::exit(0)` x2), `lib.rs:598` (`static SHOTS_SAVED`), `view.rs:522` (`static LOOK: OnceLock` reading `std::env::args()` inside the `camera` system), `smoke/mod.rs` (`static ENV: OnceLock` for PROCYON_*_SHOT), `lib.rs:464-467` (`PROCYON_AUTOPLAY` read directly although `smoke::autoplay()` exists), `lib.rs:128-130` (`ASSETS: OnceLock` set with `let _ = ASSETS.set(...)` in `run` after `assets_dir()` may already have initialised it).
**What**: Exit by killing the process skips Bevy's shutdown (pending file writes of screenshots and the log, `Drop`s) and cannot be tested; environment variables are read in systems and scattered in at least six places.
**Why**: Checklist 1.x (messages/app lifecycle) and 4.1 (no wall clock/env in sim). `let _ = ASSETS.set(...)` ignores the "already set" case, which would silently make `--mod` folders not apply if any earlier code called `assets_dir()` (e.g. via `modding::installed`).
**Suggestion**: Write `AppExit::Success` (`MessageWriter<AppExit>`) instead of `process::exit` in systems; collect all dev env vars in one `DevEnv` resource (`autoplay`, `profile`, `trace`, `end_shot`, `nis_shot`, `look`) built once in `run`; make `assets_dir()` panic/log if `ASSETS.set` returns `Err` (or replace OnceLock by passing the path via a resource built in `run`). Keep screenshot counting in a resource (see B1-12).
**Behaviour-preserving?**: yes (exit code semantics for tests/`tools/shot.sh` must be preserved: `AppExit::from_code`).
**Effort**: M

##### B1-24 [severity: medium] [category: rust] `lib.rs` is a grab-bag: tick lifecycle, asset-path resolution, profiling, screenshot driver, `Nearest` helpers
**Where**: `lib.rs:91-130` (asset/content/mod dirs), `:354-452` (`BATTLE_TICK`, `Removed`, `Joining`, `queue_removal`, `MAP_FLOOR`, `out_of_map`, `tick_start`, `FrameRules`, `BattleSet`, `battle_schedule`), `:454-502` (time scale, pause, vsync), `:504-565` (profiling, 17-parameter `profile_counts`), `:567-633` (screenshot driver), `:635-646` (`Nearest`).
**What**: 704 lines of which ~270 are tests; the docs table lists these as lib.rs responsibilities, but the rule "feature logic goes in its owning module" suggests `tick.rs` (BattleSet/Removed/Joining/clock), `paths.rs` (`base_assets/content_dir/assets_dir/bundled_mods_dir`; `campaign::user_dir` is the other half), `devtools.rs` or `smoke::driver` (profile + screenshot + run_is_over). `Nearest` is a trait with a default method + blanket impl *and* a free `nearest`: two spellings for a one-line `min_by`; its doc comment is duplicated/orphaned (:635-636 two doc lines above the trait). `TICK_SECS = 0.033` (:357) duplicates `BATTLE_TICK` (33 ms) - `BATTLE_TICK.as_secs_f32()` is const. `max_frame_step` (:464-470) reads an env var on every speed change.
**Why**: Checklist 5.1; also `run()`'s early `return setup::run(...)` path duplicates asset resolution logic (`player_assets`, `user_assets`).
**Suggestion**: Extract `tick.rs`, `paths.rs`, `devtools.rs`; make `pub const TICK_SECS: f32 = BATTLE_TICK.as_secs_f32();` (const since Rust 1.83; MSRV is 1.97); drop the free `nearest` or the trait (32 call sites use one of them; keep `.nearest_to`). Keep re-exports (`pub use tick::{BattleSet, ...}`) so paths stay stable ("mods depend on layouts" is about data, not Rust paths, but external presentation crates use `openprocyon::BattleSet`).
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-25 [severity: low] [category: simplify] Misplaced doc comment on `run`
**Where**: `game/src/lib.rs:132-135`.
**What**: The doc block for `run` ("Runs the game with one presentation ...") is followed by "--lobby <spec>: play a skirmish setup ..." which actually documents `lobby_scenario` (:136), and `run` (:150) has no doc at all; rustdoc attaches both paragraphs to `lobby_scenario`.
**Why**: Misleading docs on the library's one public entry point.
**Suggestion**: Move the first paragraph to `run`, keep the `--lobby` note on `lobby_scenario`. Same for `ship.rs:99-110` where `stand_still` (:108) is a one-line alias of `halt()` with a doc claiming different semantics: delete `stand_still` and call `halt()` (or document why two names exist).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-26 [severity: medium] [category: rust] `ship.rs` is 1118 lines mixing six concepts
**Where**: `game/src/ship.rs`: `Ship`/`Side`/`Helm` state (:26-116), explore/flee/target selection/think clock (:144-258), `ai` (:260-404), helm physics (:406-488), fleet types (`Formation`, `Fleet`, `Fleets`, `FleetPilot`, `SlotPlan`: :490-622), formation layout and reconcile/promote/follow (:624-1024), five `#[cfg(test)] mod`s (:1031-1118), and `pub mod attack; pub mod path_end;` declared at the bottom (:1026-1029).
**What**: Natural seams already exist: `ship/helm.rs` (`step`, `helm_step`, `approach_speed`, consts), `ship/captain.rs` (`ai`, `Explore`, `ThinkClock`, `CaptainTargets`, `choose_target`), `ship/fleet.rs` (everything from `FleetMember` down plus `formation_slots`/`swap_slots`). `Formation` has parallel hand-written lists (`parse`, `from_index`, `BUTTONS`, variant list repeated).
**Why**: Checklist 5.1 ("split files > ~800-1000 lines along real seams; keep mod.rs small"). docs/development.md says new files should "own a real feature": each of these does.
**Suggestion**: Do the three-way split with `pub use` re-exports from `ship.rs` so `crate::ship::X` paths stay; replace the `Formation` string/index functions by one `const ALL: [Formation; 8]` + `strum`-free `fn name(self)`/`FromStr`. Put `pub mod attack;` at the top.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-27 [severity: low] [category: modding] `Explore` reads the map JSON via `serde_json::Value` inside the AI, and `Config.map` string compare
**Where**: `game/src/ship.rs:164-170` (`crate::data::map_json(id)` at runtime inside `ai`, `map["world_half"].as_f64().unwrap_or(1000.0)`, re-loads whenever `cfg.map` differs).
**What**: Per-captain exploring pulls the whole map JSON on first use, with a silent magic default of 1000.0 when the key is missing (a mod's map with another key name would wander to the wrong quadrants unnoticed); `Explore.quadrant` map is never pruned of dead ships (the sibling `breakoffs`/`clocks` are pruned at :290-291).
**Why**: Checklist 3.1/3.3: data accessed ad hoc inside a system; silent default; leak per battle (small). `Starmap` already reads `world_half` from the map JSON too (docs table), so there are two readers.
**Suggestion**: Put `world_half` on a `MapExtent(f32)` resource inserted by `spawn_scenario` (it already has `map`), used by both `Explore` and the starmap; log a warning when the key is absent; prune `quadrant` with the other `retain` calls.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-28 [severity: low] [category: bevy] `alliance.rs` mutates sim state from PreUpdate, and polls deprecated `Interaction` with a file-wide `allow(deprecated)`
**Where**: `game/src/alliance.rs:4` (`#![allow(deprecated)]`), `:86-100` (comment says "read from the ships each tick" but the system is in `Update`), `:102-160` (`click` in `PreUpdate` writes `Ship.side`, `MapObject.start`, `SkirmishScript.sides`), also `smoke/fleet.rs` has 6 `#[allow(deprecated)]`.
**What**: The click handler changes team membership immediately in a frame schedule, while the sim reads `Ship.side` in FixedUpdate: a change lands between ticks (fine) but is not routed through a message, so it cannot be ordered or replayed. `press()` is evaluated twice per click (:122 and :127). The module-wide allow has no reason text, and checklist 2.2 says `Interaction` polling is deprecated in 0.20 (`ui_widgets::Button` + `Activate`).
**Why**: Checklist 4.5, 5.6 (`allow_attributes_without_reason`), 2.2.
**Suggestion**: Make the click handler write an `AllianceChange { owner, side }` message consumed in `BattleSet::Commands`; compute `press(...)` once; replace the file-wide allow with `#[expect(deprecated, reason = "migrate to ui_widgets::Button with panels.rs")]` on the two query fields only, and fix the stale comment ("each frame"). Migration of `Interaction` is shared with `panels.rs` (other bucket) and may need `classic/` changes.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-29 [severity: low] [category: rust] `#[allow(...)]` without reasons and too-many-arguments helpers
**Where**: `ship.rs:31` (`#[allow(non_upper_case_globals)]` for `Side::Player/Enemy` consts), `view.rs:507` (`#[allow(clippy::too_many_arguments)]` on `camera`, which has 8 params so the allow is stale), `scenario/outcome.rs:53` (`outcome`, 16 params in tuples), `ship/attack.rs:118,124` (`course`, `course_to`, 8 args each), `smoke/report.rs:100`.
**What**: No `reason = "..."`, and for `camera` the allow is unnecessary.
**Why**: Checklist 5.6 (`allow_attributes_without_reason`, prefer `#[expect(.., reason)]` so stale allows warn).
**Suggestion**: Convert to `#[expect(..., reason = "...")]`; delete the stale one; for `Side`, make it a proper enum-like newtype with associated `const PLAYER: Side`/`ENEMY` (or keep the allow with a reason: the names mirror the old enum variants). For `course/course_to` collapse (pos, heading, now, breakoff) into `struct Stance { pos, heading, now }` and drop the `course` wrapper if unused outside tests.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-30 [severity: low] [category: simplify] `stats.rs` stores per-hull state as an anonymous 4-tuple and hard-codes English owner labels in the sim layer
**Where**: `game/src/stats.rs:32` (`hulls: HashMap<Entity, (String, f32, bool, Option<String>)>`, accessed as `entry.0..3`), `:61-63` (`"Your fleet"`/`"Enemy fleet"`), `:42-48` (English column headings, `table()`), no pruning of `hulls` for despawned entities.
**What**: Tuple fields are positional; labels are not from the string table; `hulls` grows to the number of hulls ever seen in the battle (reset on `OnEnter(Battle)`, so bounded).
**Why**: Checklist 5.3, modding (the view already shows retail strings via `cells()`; the `table()`/owner labels are the only English text).
**Suggestion**: `struct HullRecord { owner, hp, destroyed, last_shooter }`; use the string-table ids for "Your fleet/Enemy fleet" (same source the Game Summary uses) or keep them as `Option` keys resolved by the view; document that `collect` relies on running after `Combat` (it is in `Cleanup`, which is correct).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-31 [severity: low] [category: bevy] Frame-time/profiling and debug text systems do work when nobody looks
**Where**: `lib.rs:513-565` (`profile_counts` has 16 params, queries entities every frame; guarded by `profiling()` at registration - fine), `view.rs:550-598` (`hud`: builds the whole F1 text with `format!` every frame and only compares afterwards, while the `Slot` texts are `Visibility::Hidden` by default).
**What**: `hud` allocates ~1 KB of strings per frame in every battle even though the F1 debug text is hidden; `Slot` is a vague name for "debug text slot"; `sky`/`hd_sky`/star generation also live in `view.rs` although the module doc says "Camera, world mouse/keyboard input and the F1 debug text".
**Why**: Checklist 1.4/1.15, 5.1. The name `view.rs` also collides conceptually with the "views" of the Layers rule (it is shared code, not a view), which confuses new contributors.
**Suggestion**: Run `hud` only `.run_if(debug_text_visible)` (a resource toggled by `toggle_debug` instead of reading `Visibility`), rename `Slot` -> `DebugSlot`, move `sky`/`starfield`/`star_mesh`/`hd_sky` into `sky.rs` (docs already say `docs/re/sky.md`), and consider renaming `view.rs` -> `camera.rs`/`world_input.rs` or at least stating in its module doc that it is shared, not a "view".
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B1-32 [severity: low] [category: simplify] Test setup duplication in `view.rs` tests
**Where**: `game/src/view.rs:657-681` (`input_world()`) vs `:741-761` (the same 20 `init_resource`/`spawn` lines pasted into `keyboard_camera_rotation_and_zoom_stay_at_one_x_at_every_simulation_speed`).
**What**: The long test re-implements `input_world` (without `Time<Real>`/`Time` which it adds later).
**Why**: Checklist 5.8, simplification; any new resource required by `input` must be added in two places.
**Suggestion**: Call `input_world()` and add the Free camera resource; also add an `App`-level test that runs `input` + `fleet_keys` + `camera` together in the `Update` order of `run()` once B1-05 gives them a plugin (guards the chain order that currently only exists inside `run`).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-33 [severity: low] [category: bevy] `Target` is a positional tuple resource and is mutated field-by-field in many places
**Where**: `combat/mod.rs:146` (`Target(pub Option<Entity>, pub Option<Vec3>, pub GroundFire)`), uses in this bucket: `ship.rs:765-768,832-834` (three separate `resource_mut::<Target>()` calls to set `.0`, `.1`, `.2`), `view.rs:165-179,234-238` (`target.2.armed`, `target.2.panic`, `target.1 = ...`), `scenario.rs:157-162`.
**What**: `.0` = entity, `.1` = hull-local aim point, `.2` = ground fire state; setting a target requires remembering to clear `.1`/`.2.panic` (as at view.rs:234-238 and 176-180).
**Why**: Checklist 1.11 ("remember to call init_x()" smell), 5.3.
**Suggestion**: Named fields (`entity`, `aim`, `ground`) plus `Target::set(entity, aim)` and `Target::clear()` that apply the invariants once; `install_primary` then does `*world.resource_mut::<Target>() = Target::with(target)`. (Struct is in `combat/`, so coordinate with that bucket.)
**Behaviour-preserving?**: yes.
**Effort**: M

##### B1-34 [severity: low] [category: modding] Defaults in `run()` baked in: default player/enemy fleet and fallback sky/music
**Where**: `lib.rs:194,197` (`ship::DEFAULT_SHIP`, `"Ship_Pirate_Schooner,Ship_Pirate_Sloop"`), `scenario/spawn.rs` (`"skybox_07"` fallback at :~173, `"BTL_NavyBig_Full02"` music fallback at :~190).
**What**: Retail asset names as literals scattered in setup code; a total-conversion mod that removes those assets crashes `--map` with a missing-asset error rather than a clear message.
**Why**: Checklist 3.5; PROVISIONAL not marked on the fallbacks.
**Suggestion**: Named consts in one place (`defaults.rs` or `scenario::DEFAULT_*`) with a doc that they are retail ids used only by CLI shortcuts and map fallbacks; log a warning when a fallback is taken.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B1-35 [severity: low] [category: fidelity-risk] Sensors have two writers on different clocks (cross-bucket note)
**Where**: `environment.rs:465-474` registers `refresh_sensors` both in `PreUpdate` (per frame, on the interpolated pose, with `advance_sightings` driven by `Time<Real>`, environment.rs:134) and in `FixedUpdate` `BattleSet::Sense`; `ship::ai` reads `sensors.in_sight(...)` (ship.rs:324).
**What**: If `in_sight` is influenced by state advanced on real time (once-a-second re-test), AI target selection could depend on frame rate. The fixed-tick refresh recomputes from sim poses, which probably makes the AI side tick-deterministic, but this needs verification by the environment bucket.
**Why**: Checklist 4.1/4.5.
**Suggestion**: Confirm that `in_sight` (AI) reads only state produced inside the tick (positions + tick-clocked sighting timers), and keep the real-time `advance_sightings` for the player's fade/HUD view only (document in `Sensors`). A headless test running the same scenario at 30 and 144 fps and comparing `CaptainTargets` would pin it.
**Behaviour-preserving?**: needs-check.
**Effort**: M (investigation)

##### B1-36 [severity: low] [category: bevy] `sail` and other systems take `Option<Res/ResMut<...>>` for resources that always exist
**Where**: `collision.rs:462-466` (`sail` takes `Option<ResMut<Messages<Sfx>>>`, `Option<ResMut<Rng>>`, `Option<ResMut<Messages<Ram>>>`, `Option<ResMut<Messages<Destroyed>>>`), `ship.rs:263-275` (`Option<Res<campaign::Mission>>` is legitimate; `Option<Res<Storms>>/StormShift` come from `nebula::plugin`), `lib.rs:289` (`add_message::<collision::Ram>()` registered in `run`, not in `collision`).
**What**: The Optionals exist so unit tests can run `sail` without registering plugins, but production wiring has them all, so a missing registration is silently skipped instead of failing loudly (and `Ram` is registered far from `collision`).
**Why**: Checklist 1.14 (silent `let Ok/None` swallowing real failures). Cross-bucket (collision.rs) but the registration of `Ram`, `Shapes`, `Fleets`, `CaptainTargets`, `Plot` in `run` is this bucket's wiring.
**Suggestion**: Use `MessageWriter<...>`/`ResMut<Rng>` (non-optional) in production signatures and register them in `collision::plugin`/`ship::plugin`; tests call `app.init_resource::<..>().add_message::<..>()` via a small `test_app()` helper shared in `tests/`.
**Behaviour-preserving?**: yes.
**Effort**: M

#### Quick wins
- Replace `cfg.script.as_deref() == Some("...")` with the `Script` enum everywhere (B1-10): mechanical, removes the typo hazard, then drop `Deref<Target=str>`.
- `ponytail:` -> `PROVISIONAL:` in 9 places and add a hygiene test (B1-16).
- `Rng::default()` reset in `scenario::reset_fleets` plus log the seed on "battle start" (B1-03); log the cosmetic clock seeds (B1-04).
- Delete `Ship::stand_still` (alias of `halt`), the bare `{ }` block in `helm_step`, the dead `unwrap_or(500)` at lib.rs:204, and fix the misplaced doc on `run`/`lobby_scenario` (B1-19, B1-25).
- `TICK_SECS = BATTLE_TICK.as_secs_f32()` (const), de-duplicate the default sun/ambient values and call `FreeCamera::load` once (B1-22, B1-24).
- `#[derive(Clone)]` + `Group::new` to remove five 10-field literals (B1-21).
- Sort `snap` in `ai` by a stable key and make the `choose_target` tie-break explicit (B1-02): smallest determinism fix, big payoff for parity runs.
- Convert `#[allow(...)]` to `#[expect(..., reason = "...")]`; drop the stale `too_many_arguments` on `view::camera` (B1-29).
- Run `hud` only when the F1 text is visible and build its strings lazily (B1-31).
- Prune `Explore.quadrant` alongside `clocks`/`breakoffs` in `ai` (B1-27).
- Replace `process::exit(0)` in `screenshot` with `AppExit` and move the shot counter into a resource (B1-23).

---

## Appendix: Combat & collision (game/src/combat/, game/src/collision.rs, collision/)

**Overall assessment.** The combat code is unusually well documented for a reverse-engineering project. Most rules cite `docs/re/*.md` or say "read from the original's code". Guesses are usually marked PROVISIONAL. The pure rule cores (`FireRequest`, `ground_point`, `advance`/`track` for beams, `damage_control`, `tree::overlap`, `avoid::think`, `asteroid::control`/`physics`) are small, testable and free of ECS types. Messages (`Hit`, `Fired`, `Destroyed`, `Detonate`, `Shoot`, `Strike`, `Ram`) are used for the cross-feature flows. Shooters and bases are iterated with `.sort::<Entity>()` and the reason is documented, which shows the author already knows the determinism problem. The main weaknesses are these:
- The determinism discipline stops partway. Several RNG-drawing systems iterate unsorted queries, and the shared `Rng` is never reseeded per battle.
- `gunnery::fire` (about 430 lines) and `collision::sail` (about 375 lines, a 16-parameter system) are monoliths.
- `Shell` is constructed field by field in four places.
- Retail names, sound pools and numbers are hard-coded in systems, and `lib.bullets[..]` / `bd.damage[..]` indexing will panic on a mod that omits an entry.
- Several `///` blocks sit above the wrong function, which misleads readers.

#### Findings

##### B2-01 [severity: high] [category: fidelity-risk] Shell, blast, beam and crew-repair systems draw from the shared RNG in unsorted query order
**Where:** `combat/shells.rs:397` (`for (e, mut s, mut tf) in &mut q`, draws at 520, 533, 586, 592, 642 via `hit_ship`); `combat/lob.rs:176` (`blasts`: `for (e, mut b, tf) in &mut q`, then inner `&mut ships`/`&mut bases`/`&mut rocks`/`&mut shells` loops, each calling `apply_ship_hit` with `rng`); `combat/beam.rs:118` (`burn`: `&mut shooters`); `combat/damage.rs:525` (`crew_repair`: `&mut ships`, `rng.f()` per slot); `combat/lob.rs:313` (`ram_damage`, the order of `msgs` comes from `sail`); `collision.rs` (`bump` draws `rng.f()` inside the ship/rock/pair loops, and `iter_combinations_mut` order is query order).
**What:** `gunnery::fire` and `base_fire` explicitly sort by `Entity` because "every shooter draws from the shared `Rng`, and the query's own order can differ from launch to launch" (gunnery.rs:409). The same hazard exists in every system above, and none of them sorts. All of them consume the same `Res<Rng>` in the same tick.
**Why:** AGENTS.md and docs/development.md rely on `PROCYON_TRACE` and whole-battle comparison. Query order is archetype/table layout order, not a guarantee. Any unrelated component change, or a despawn/respawn, can reorder it and change every later roll in the battle. Two shells hitting the same section in one tick also resolve in query order, so which one kills the section changes.
**Suggestion:**
- Iterate the RNG-consuming queries with `.sort::<Entity>()` (as `fire` does), or collect `(Entity, ..)` and sort by a stable key. `Ship` index or a spawn-sequence component is better than `Entity` bits, because entity indices are reused after a restart. See B2-03.
- For shells, sorting by `Entity` is fine within a battle.
- For `collision::sail`, sort the ship list once per tick and iterate pairs with indices instead of `iter_combinations_mut`.
**Behaviour-preserving?** needs-check. It changes RNG consumption order, so traces will shift once, but behaviour becomes reproducible. **Effort:** M

##### B2-02 [severity: high] [category: fidelity-risk] `Sinking.elapsed` is advanced in `Update`, but the simulation reads it on the tick
**Where:** `combat/mod.rs:328-336` (`Sinking::drive_health`, used by `collision.rs:642` inside `sail`) versus `combat/mod.rs:365` (`sinking.elapsed += time.delta_secs()` inside `breakup`, which is registered in `Update` at mod.rs:226).
**What:** The 0.14 s switch from "sections as they died" to "wrecked drive" (`CLIP_DELAY`) is a simulation input: it changes thrust, top speed and rudder torque of the sinking hull. But the clock is `Sinking.elapsed`, accumulated per rendered frame (and `breakup` also skips frames while `Loading` or paused differently). At 30 fps the threshold is crossed up to one frame (33 ms to 1 tick) later than at 144 fps, and a headless/screenshot run differs from an interactive one.
**Why:** This breaks "sim in FixedUpdate at 33 ms" (docs/development.md "Battle clock and scheduling") and makes sinking-ship trajectories frame-rate dependent.
**Suggestion:**
- Split the clock. Keep a tick counter or `age_ticks` in `Sinking`, advanced in a `FixedUpdate` system in `BattleSet::Combat` right after `deaths`, and have `drive_health` read it.
- The presentation (`breakup`) can keep its own frame-time clock for the clip.
- Both clocks start at the same event, so the data stays aligned.
**Behaviour-preserving?** no (intentional fix; tick-quantised). **Effort:** S

##### B2-03 [severity: medium] [category: fidelity-risk] The combat `Rng` is never reseeded per battle, and its seed is not logged
**Where:** `combat/mod.rs:155-170` (`Rng::default()` uses the constant `0x9E3779B97F4A7C15`); `init_resource::<Rng>()` at mod.rs:211; `scenario.rs` restart/leave only despawns `Scenario`/`Transient` entities (no `Rng` reset found).
**What:** The first battle in a process is reproducible, but the second battle (restart, next mission, `--lobby` replay) continues the stream where the first stopped. A saved `PROCYON_TRACE` therefore cannot be replayed from the menu, and "same input, same battle" only holds for the first battle after launch.
**Why:** Whole-battle parity (AGENTS.md "parity rows") needs a known seed at battle start. Mods that want to share a replay or bug report need the seed.
**Suggestion:**
- Reset `*rng = Rng::default()` (or `Rng::new(seed)` with a `--seed`/`PROCYON_SEED` override) in the scenario spawn path.
- Log the seed with `info!`.
- Consider a newtype `CombatRng` with the sim/cosmetic split explicit. Today cosmetic streams are `Local<Rng>` in `breakup`, `damage_fx` and `crew.rs`, all starting from the same constant seed, so they are correlated.
**Behaviour-preserving?** yes for battle one, needs-check otherwise. **Effort:** S

##### B2-04 [severity: medium] [category: fidelity-risk] Unsorted ship iteration makes avoider phase and pair resolution order dependent
**Where:** `collision.rs:~561` (`let a = avoiders.entry(e).or_insert_with(|| Avoid::new(0.05 * (seen % 10) as f32))`); `collision.rs:~766` (`ships.iter_combinations_mut()` pair loop, each pair pushes both hulls and kicks `ang_vel`); `collision.rs:~545` (`Key::Rock(i)` with `i` from `rocks.iter().enumerate()`).
**What:** The think-stagger phase of each ship depends on how many ships were seen before it in query order. Ship-ship separation is sequential and non-commutative: A is pushed by B, then B's overlap is tested with the new A. Rock identity for the "same obstacle" counter is its query index, which changes whenever a rock is removed or the table reorders.
**Why:** Same class as B2-01, but in the part of the simulation that decides where ships are. It is also the reason `PROCYON_TRACE` runs can diverge without a rules difference.
**Suggestion:** Build `let order: Vec<Entity> = ships.iter().map(..).sorted()` once, use the sorted index for the stagger, and use `Key::Entity(rock_entity)` for rocks (the `Key::Rock(usize)` variant then disappears). For pairs, iterate sorted entities and use `get_many_mut`.
**Behaviour-preserving?** needs-check. **Effort:** M

##### B2-05 [severity: medium] [category: simplify] `gunnery::fire` is one 430-line system with 30+ parameters hidden in tuples
**Where:** `combat/gunnery.rs:314-740`.
**What:** One function does these jobs:
- contact snapshot (370-381);
- rock/base target lists (382-407);
- target choice per ship (424-450);
- line-of-fire test (453-475);
- rock-watch clock (477-508);
- mine-laying state (510-515);
- ground-aim clock (518-541);
- per-mount panic/point-defence/lob target (556-582);
- turret aiming (593);
- beam cycle (596-631);
- burst start (673-689);
- shell launch (690-729);
- panic and click cleanup (732-738).

Params are packed into tuples (346-356) and the `shooters` query has 12 columns.
**Why:** It cannot be unit-tested below the whole ECS, and every fidelity fix touches it. docs/development.md says new files should "own a real feature, not just hide a long function", and these seams are real features.
**Suggestion:** Split along the existing seams without changing order:
1. `gunnery/targeting.rs`: `choose_ship_target(...) -> Option<AimTarget>`, a pure function over `&[Contact]`, rocks, bases and the `CaptainTargets`/`FleetPilot`/`MissionShip` facts.
2. `RockWatch::look(...)` as a method.
3. `fn mount_target(mount, state, ctx) -> Option<AimTarget>` for the `match (primary && is_indirect, pending)` block, which is the densest part.
4. `fn step_beam(...)` for 596-631.
5. A `fn start_burst(state, gun, ...)` / `fn launch_burst(...)` pair shared with `base_fire` (lines 923-953 duplicate 673-729).

Pass a small `struct ShipCtx<'a>` instead of 12 loose locals. Use `#[derive(SystemParam)]` for the tuple of Locals/queries.
**Behaviour-preserving?** yes (mechanical). **Effort:** L

##### B2-06 [severity: medium] [category: simplify] `collision::sail` is a 375-line, 17-parameter system doing eight jobs
**Where:** `collision.rs:452-826`.
**What:** In one body:
- ram clock bookkeeping;
- NIS stop;
- collision-sound closure;
- tow-cable registration;
- avoider build and think;
- rock drift;
- ship integration (unstick / sinking / helm / currents);
- cable pull;
- ship-vs-land/rock;
- ship-vs-ship push, kick and ram messages.

It uses `Option<ResMut<Messages<Sfx>>>`, `Option<ResMut<Messages<Ram>>>` and `Option<ResMut<Messages<Destroyed>>>` so that unit tests can omit the plugins.
**Why:** Harder to review than any other function in the repo, it is order sensitive (so the order must stay visible), and the Option-resource pattern bypasses `MessageWriter`.
**Suggestion:**
- Keep one system but extract named phases into free functions taking a small `struct SailCtx`: `avoid_phase`, `integrate_phase`, `cable_phase`, `contact_phase`, `pair_phase`.
- Alternatively make them chained systems inside a `CollisionSet` (docs say `BattleSet::Simulate`/`Forces` order matters).
- Replace the `Option<ResMut<Messages<..>>>` with `MessageWriter` and register the messages in the test app (`add_message`). `MessageWriter<Ram>` would also make the `Ram` flow match `Hit`/`Destroyed`.
- Create `bump` as a method of a small `ContactSounds` struct so the 0.07/700/pool-name constants live in one place (B2-12).
**Behaviour-preserving?** yes. **Effort:** L

##### B2-07 [severity: medium] [category: simplify] `Shell` is built field by field in four places
**Where:** `combat/gunnery.rs:803-823`, `combat/minefield.rs:27-46`, `combat/lob.rs:39-58`, `combat/beam.rs:140-159`.
**What:** Each site lists all 17 fields, most of them zero/None/false (`angular_velocity: 0.0, pd: false, harmless: false, base: false, mine_scan: Default::default(), lost_target: false`). `lifetime` differs silently: `b.lifetime` (gunnery, mines), `b.lifetime.min(30.0)` (lob, magic 30), `0.0` (beam).
**Why:** Adding a field (as `lost_target`, `mine_scan` and `ricochet` were) means four edits; a missed one compiles only because every site lists the field. Intent is hidden in the noise.
**Suggestion:** Add `Shell::new(bullet: &str, def: &BulletDef, vel: Vec3, side: Side, shooter: Entity) -> Self` plus builder-style `.gravity()`, `.target()`, `.aim()`, `.ricochet()`. Derive `Default` for the numeric/flag fields. Name the 30 s cap (`MAX_SECONDARY_LIFETIME`) with its source.
**Behaviour-preserving?** yes. **Effort:** S

##### B2-08 [severity: medium] [category: modding] Indexing `lib.bullets[..]`, `lib.guns[..]`, `bd.damage[..]`, `defs.0[..]` panics on a mod that omits an entry
**Where:** `combat/shells.rs:402` (`lib.bullets[&s.bullet]`), 520/530/575 (`bd.damage["Dragon"]`, `["Stone"]`, `[key]`), `combat/gunnery.rs:129,510,548,793,912` (`lib.guns[gun]`, `lib.bullets[&lib.guns[gun].bullet]`), `combat/lob.rs:200,219,230,240` (`bd.damage[..]`), `combat/asteroid.rs:299,304,309` (`defs.0[..]`, `Asteroid::new(..).unwrap()`), `combat/asteroid.rs:111` (`assert!` on rule-supplied intervals), `collision.rs:18-52` (`.expect(..)` on every collision.json field).
**What:** A mod that adds a bullet without a "Dragon" damage row, a gun whose bullet is missing, an asteroid whose child kind does not exist, or a collision.json with a layer without nodes crashes a system mid-battle (or at startup) with a bare panic. These are the exact edits a modder makes (`docs/modding.md`).
**Why:** AGENTS.md says mods depend on the data layout. Panic messages here carry no mod/file context, and Bevy 0.20 routes system panics to the fallback handler.
**Suggestion:**
- Validate once at load (`Library::load`, `AsteroidDefs::from_world`, `Shapes::from_world`): every gun's bullet exists, every `BulletDef.damage` has all `MATERIALS` plus `"Dragon"`, every asteroid child kind resolves, every shape has finite bounds. On failure return a `Result` with a message naming the file and entry.
- In systems use `.get()` with `continue`, or give `BulletDef::damage_for(key) -> [f32; 5]` a documented default (zeros).
- Replace `assert!` in `asteroid_motion` with a skipped message plus `warn!`.
- Parse collision.json with a `#[derive(Deserialize)]` struct instead of the `Value`/`from_value(v.clone())` chain (also removes ~10 `expect`s).
**Behaviour-preserving?** yes for valid data. **Effort:** M

##### B2-09 [severity: medium] [category: bevy] `collision::update_asteroids` rebuilds every moving rock's obstacle (all triangles) every tick
**Where:** `collision.rs:225-231` (`Changed<Transform>` then `shapes.obstacle(..)`), and `Shapes::obstacle` at 185-222 (clones `s.vertices`/`s.indices`, allocates a `Vec<Triangle>` and a `BoxBody`).
**What:** Rocks move every tick, so `Changed<Transform>` is always true for them. Each tick every live rock gets a fresh `Obstacle` with new allocations. For a belt of dozens of rocks that is the largest per-tick allocation in the bucket. For non-tumbling rocks the work is wasted, because `sail` already calls `obstacle.translate(delta)` when it displaces a rock. The axis-aligned `Asteroid` box case (188-192) also rebuilds an 8-vertex/12-triangle vec each call.
**Why:** Per-tick allocation in the fixed loop (checklist 4.x, and "per-tick allocation" focus). It also scales with belt size, which matters for missions with large belts.
**Suggestion:**
- Keep the rest-pose triangle list in `Shapes` as an `Arc<[ [Vec3;3] ]>` (or store local-space vertices in `Obstacle`) and transform into a reused buffer (`obstacle.triangles.clear(); extend(..)`).
- Precompute the box mesh for `Asteroid*` shapes at load time (once) instead of per call.
- Alternatively keep a rigid-pose `Transform` in `Obstacle` and transform query points into local space on test, which avoids touching the triangles at all for rocks.
**Behaviour-preserving?** yes. **Effort:** M

##### B2-10 [severity: medium] [category: simplify] The "human's ship" test and the "generic posts" fallback are copy-pasted across modules
**Where:**
- `humans = ship.side == Side::Player && member.is_none_or(|m| fleets.as_ref().is_none_or(|f| m.0 == f.local))` appears at `combat/damage.rs:286`, `combat/lob.rs:311` and (as a closure with `human.contains(e)`) at `collision.rs:536`.
- `let generic = posts.is_none().then(|| crate::crew_arms::fallback_posts(ship, boarding, &class.name)).flatten(); let posts = posts.or(generic.as_ref());` appears at `combat/gunnery.rs:419`, `combat/damage.rs:303-304` and `combat/damage.rs:530-531`.

**What:** The rule "which ships does the difficulty option's incoming factor apply to" and "which crew does a ship have if it has no named posts" live in three places each. They differ subtly (the collision version also accepts `PlayerShip`).
**Why:** The difficulty rule is the single most important fidelity number in combat (`side_damage_scale`). Three copies can drift.
**Suggestion:** Put `fn is_human_ship(ship: &Ship, member: Option<&FleetMember>, fleets: Option<&Fleets>) -> bool` next to `Fleets` in `ship.rs`, and `fn effective_posts<'a>(posts: Option<&'a CrewPosts>, generic: &'a Option<CrewPosts>)` or a `CrewView` SystemParam in `crew_arms.rs`. Replace all call sites.
**Behaviour-preserving?** yes (if the `PlayerShip` nuance is kept as a parameter). **Effort:** S

##### B2-11 [severity: medium] [category: bevy] The difficulty option is a global `AtomicU8` read from deep inside the simulation
**Where:** `menu/settings.rs:83-87` (`DIFFICULTY`, `difficulty()`), read at `combat/shells.rs:522,710`, `combat/lob.rs:195,285,320`, `combat/damage.rs:245,314`.
**What:** A hidden global feeds simulation damage. It is invisible in system signatures, cannot be set per test app without leaking into other tests, and cannot be recorded in a trace or saved with a replay.
**Why:** Determinism (checklist 4.1/4.4): inputs to the sim should be resources. Tests that set difficulty can race when `cargo test` runs in parallel threads.
**Suggestion:** Introduce `#[derive(Resource)] struct Difficulty(u8)` (or an enum `Easy/Normal/Hard`) owned by `menu/settings.rs`, mirrored from the setting, and pass `Res<Difficulty>` into the combat systems. Also turn the `[3.0, 2.0, 1.0][d]` / `[0.25, 0.5, 1.0][d]` tables (shells.rs:149, damage.rs:259, which duplicates `side_damage_scale`'s incoming column) into one `const DIFFICULTY_SCALE` (see B2-14).
**Behaviour-preserving?** yes. **Effort:** M

##### B2-12 [severity: medium] [category: modding] Hard-coded retail names, sound pools and ranges inside systems
**Where:**
- `combat/shells.rs:144,257,409,478-479,536,606-613`: `"Damage_{pool}0{n}"`, `max: 600.0`, `"Weapon_Novamortar_Detonate"` (900.0), `"Lancer_Muzzle_Flash"`, `"Explosion_Light"`, `"Explosion_Med01"` (600.0, vol 0.8), `"Damage_Stone0{}"`, `"FireOnShip"`, `"ShipExplosion_Medium"`, `"Rock_Chunks"`, `"Splintering_Wood"`.
- `combat/mod.rs:290`: `"Explosion_Heavy0{}"` with 8000.0.
- `combat/damage.rs:32,180`: `"Explosion_Med01"`, 6000.0.
- `combat/damage.rs:171`: `"Damaged Section_Light"` ... names.
- `combat/asteroid.rs:302`: `"Explosion_Med01"`, 6000.0.
- `collision.rs:505-516`: 0.07 chance, 0.5 coin, `"Ship_Collision_{pool}0{n}"`, 700.0.

**What:** The sound/effect each event plays, its volume and its audible range are literals in the system. Some carry a PROVISIONAL note; many do not. They also repeat: `"Explosion_Med01"` / 6000.0 appears three times (death wipe, section loss, asteroid death), and the "pick variant 1..n" idiom (`format!("..0{}", rng.variant(5))`) is repeated.
**Why:** Moddability goal. A modder who wants a different explosion has to edit Rust. The original keeps these in data (bullet tables carry impact pools; the others look like hard-coded tables in the original, but a sound-event table is the natural home). `content/` already hosts "the crew voice event table", so there is precedent.
**Suggestion:**
- Short term: collect them as named consts in one `combat/sounds.rs` (`SECTION_LOST_SFX: SfxSpec`, etc.), each with its source (docs/re/sound.md) and PROVISIONAL marking where it applies. Add a `fn variant_name(prefix, n_variants, rng)`.
- Medium term (needs maintainer approval, since it adds a `content/` data file): a `data/combat/events.json` event-to-effect/sound table overlaid like other data, with the current literals as defaults.
**Behaviour-preserving?** yes. **Effort:** M (consts) / L (data table)

##### B2-13 [severity: low] [category: simplify] Doc comments are attached to the wrong function in several places
**Where:**
- `combat/shells.rs:60-62`: "Native launch coast (0.5 s) ..." is a leftover of `guide_torpedo` but is attached to `wear_down`; `guide_torpedo` (74) has no doc.
- `combat/shells.rs:128-132`: the "Measured on retail ... difficulty" paragraph sits above `damage_pool`, while `side_damage_scale` (147) has none.
- `combat/shells.rs:206-207`: "A shell's `roll` on base part `k` ... The parts this hit brought down" runs two sentences together.
- `combat/gunnery.rs:1063-1064`: "Damage rules of one shell hit on section `k`..." (criticals) heads `gunner_reload_cut`; the real doc follows it.
- `combat/mod.rs:23-24`: `Health` has no doc and the `IslandGunnery` doc is stuck to it.
- `collision.rs:312`: the PROVISIONAL line is on `contact_point`, but the doc above `closest_contact` is missing.

**What:** Comments describe a different item than the one below them.
**Why:** For a new contributor `cargo doc` and hover docs lie. The PROVISIONAL marker on `side_damage_scale` (the most numerically important function in combat) is currently under `damage_pool`.
**Suggestion:** Move each block to its function; add a one-line doc for `guide_torpedo` and `side_damage_scale`.
**Behaviour-preserving?** yes. **Effort:** S

##### B2-14 [severity: low] [category: modding] Difficulty tables and many tuning numbers are inline literals without a named const
**Where:**
- `combat/shells.rs:147-161` (`[3.0, 2.0, 1.0]`, `[0.25, 0.5, 1.0]`), `combat/damage.rs:259` (repeats `[0.25, 0.5, 1.0]`).
- `combat/shells.rs:71` (0.05, 0.025, 3.0), 97 (20 degree saturation), 105-109 (0.67/0.33/3.0/5/3), 111 (`clamp(-60.0, 60.0)`), 84 (`dt * 60.0` substeps).
- `combat/shells.rs:271` (`0.6` ricochet speed), 435-436 (`3.0 * dt`, `-1.5 * dt`, `3.0`, `2.0`), 471 (2.5 point-defence radius), 490 (`0.5` land overshoot), 592 (`0.5` splinter chance), 249-252 (ricochet material offsets 25/50/-100).
- `combat/shells.rs:200,212,291` (0.4 spill factor, three copies).
- `combat/gunnery.rs:1037` (0.8), 560 (`/ 2.0`), 298 (5..10 lead seconds), 302 (50..70 gate), 754-755 (20 s / 10 s / 50 units / 1 unit/s mine laying).
- `combat/damage.rs:509` (3.0 hit points, factor 10.0, 0.67/0.33 repair factors).

**What:** Most are decoded from the original, but only some cite where. Several are used twice (0.4 three times, difficulty table twice).
**Why:** Checklist 3.5 / 5.7: magic numbers should be named consts with units and a source, and guesses should be marked. The same decoded constant in three places can drift.
**Suggestion:** Introduce `const SPILL_FRACTION: f32 = 0.4;` `const RICOCHET_SPEED_FACTOR`, `const DIFFICULTY_OUT/INC: [f32; 3]`, and so on, each with a one-line source ("docs/re/gunnery.md#...") or `PROVISIONAL`. Put the torpedo guidance numbers beside `physics` in a `TorpedoSteering` const struct. Gun/torpedo numbers that the WOT file can supply (`BulletDef.physics`) already come from data; the ones that cannot should be consts, not inline.
**Behaviour-preserving?** yes. **Effort:** S-M

##### B2-15 [severity: low] [category: fidelity-risk] Unmarked guesses in code
**Where:** `combat/shells.rs:471` (`< 2.5` point-defence hit radius, comment says PROVISIONAL only for the HP exchange, not the radius); `combat/shells.rs:490` (`+ 0.5 / ...` overshoot, "a hair past the surface" is not marked); `combat/gunnery.rs:277` (marked); `combat/mod.rs:288-289` (death sound pool, marked); `combat/damage.rs:92-98` (marked); `combat/asteroid.rs:305` (child ejection direction table, slot to direction mapping has no source or PROVISIONAL mark); `combat/lob.rs:42` (`min(30.0)`); `collision.rs:~795-800` (`0.1 * ... / TICK`, `clamp(-60, 60)`, no source mark); `collision.rs:~625` (`/ 3.0` unstick, source stated but not PROVISIONAL-checked); `collision.rs:~690` (`-2.0 * substep` lateral damping, marked).
**What:** AGENTS.md rule 5: anything not verified against the original is marked PROVISIONAL. The listed numbers have no source and no mark.
**Why:** A contributor cannot tell which constants are measured and which were eyeballed.
**Suggestion:** Add `// PROVISIONAL` or a doc-link next to each, or move the number into a named const that carries the note.
**Behaviour-preserving?** yes (comments only). **Effort:** S

##### B2-16 [severity: medium] [category: bevy] Per-tick allocation in `fire`, `shells` and `blasts`
**Where:**
- `combat/gunnery.rs:362-366` (`incoming` Vec), 370-381 (`contacts` Vec with `Contact` copies), 382-398 (`rocks` Vec), 400-407 (`base_targets` Vec), 518-525 (`lob_banks` Vec plus `sort_unstable` plus `dedup` per ship per tick), 542 (`ground.get(&me).map(|g| g.banks.clone())`), 187 (`state.muzzles` indexing is fine) and `Fired { bullet: gun.bullet.clone() }` at 606/680/925, `Sfx { name: n.clone() }`.
- `combat/shells.rs:393-395` (threats Vec plus a `std::collections::HashMap<Entity,f32>` built every tick even with no threats), 397 (`downed.contains` is O(n) per shell, O(n²) overall), 411 (`NOVA_SECONDARY.into()`), 494 (`s.bullet.clone()` into each `Detonate`).
- `combat/lob.rs:269` (`Strike.damage: HashMap<String,[f32;5]>` carried inside a message).
- `combat/status.rs:160` (`bank_state` collects a Vec per call, called per HUD frame per bank), `status.rs:178` (`gun_icon` returns a fresh `String` every call).
- `combat/shells.rs:187` (`hp.sections.clone()` per hit, `Vec<usize>` for `seen` with `vec![k]`), shells.rs:209.

**What:** All are small, but they run at 30 Hz for every shooter/shell and mostly allocate for data that rarely changes. `bullet: String` is cloned for every fired shell and message.
**Why:** The user asked specifically about per-tick allocation; most of it can go away with a few `Local<Vec<..>>` buffers reused (the code already does this once: `seen` at gunnery.rs:408).
**Suggestion:**
- Reuse `Local<Vec<_>>` for `incoming`, `contacts`, `rocks`, `base_targets`, `threats`, `downed` (use a `bevy::platform::collections::HashSet` or `SmallVec`; `downed` as a `Vec` is fine for tiny sizes).
- Skip building `threat_hp` when `threats.is_empty()`.
- Intern bullet and gun names as a small `BulletId(u16)` newtype (see B2-17) so `Shell`, `Fired`, `Detonate`, `Shoot` carry a `Copy` id rather than a `String`.
- Compute `lob_banks` once per ship class (it depends only on the class and the fitted guns) and cache on `GunStates`.
**Behaviour-preserving?** yes. **Effort:** M

##### B2-17 [severity: medium] [category: rust] Stringly-typed guns and bullets drive the rules
**Where:**
- `combat/status.rs:102-109` (`is_indirect(gun: &str)` is `contains("Mortar") || contains("Mine") || contains("GravCharge")`), 106-109 (`lob_kind` with `usize` kinds 0/1/2 and a parallel `C: [[&str;2];3]`), 177-203 (`gun_icon` matches substrings, including the retail typo `Mediuam`).
- `combat/gunnery.rs:364,584,799,993` (`bullet.starts_with(TORPEDO_BULLET)` in one place, `== TORPEDO_BULLET` in others: they disagree on `Bullet_Torpedo_*` variants), 592 and 917 (`gun.contains("Harpoon")`), 884 (`class.name.starts_with("Island_Volcano")`), shells.rs:732 (`class.name != "Ship_Navy_Lifeboat_Mission9"`), gunnery.rs:510 (`is_mine` does two `HashMap<String,_>` lookups, called about six times per mount per tick), damage.rs:456 (`"Damage_"` node-name parsing), mod.rs:101 (`MATERIALS` ordered `[&str;6]` plus `contains` for `material_key`), shells.rs:133,246 (`match key { "Wood" | ... }`).
- `data.rs:468` uses another `starts_with(TORPEDO_BULLET) || == "Bullet_Gattling"`.

**What:** Behavioural categories (indirect, torpedo, harpoon, mine, point-defence) are re-derived from names at every use, including per-tick inner loops. `==` vs `starts_with` on `TORPEDO_BULLET` already differs (shell flight uses `starts_with`; muzzle velocity/aim uses `==`), so a `Bullet_Torpedo_Heavy` mod bullet would fly as a torpedo but be launched and led as a shell.
**Why:** Checklist 5.3 (types over strings). It also blocks modding: a mod gun named `Gun_MyFlak` that should behave as a mortar cannot, because behaviour is keyed on the name.
**Suggestion:**
- Derive flags once in `GunDef`/`BulletDef` at load (`is_torpedo`, `is_mine`, `is_indirect`, `is_harpoon`, `lob_kind: Option<LobKind>`), taken from the payload/custom-info where they exist and falling back to the name test only inside the loader. Make `Material` an enum (`Reinforced, Wood, Iron, Cloth, Stone, Dragon`) with `from_section(&str)`.
- Data format changes (new optional JSON keys) need maintainer approval; derived fields at load time need none.
**Behaviour-preserving?** needs-check (the `==`/`starts_with` split). **Effort:** M

##### B2-18 [severity: low] [category: simplify] `u8` and `usize` state codes where enums fit
**Where:** `combat/damage.rs:6,44-48,142-156` (`DamageFx(Vec<(u8, Option<Entity>)>)` levels 0..4; `Fire.state: u8` 0..3 with `match f.state { 2 => .., 1 => .., _ => ..}`), `combat/status.rs:158-170` (`bank_state -> usize` 0..3 documented only in a comment), `status.rs:106-129` (`LobAimed.kind: usize`).
**What:** Magic integer codes encode lifecycle states (`Burning, Out, Detached, Gone`; `None, Light, Medium, Heavy, Destroyed`; `Ready, Recharging, Destroyed, NothingInRange`).
**Why:** `if level == 4 && cur < 4` and `NAMES[level as usize]` are easy to get wrong when a state is added; HUD views index sprite frames by `bank_state` number.
**Suggestion:** `enum DamageLevel { Intact, Light, Medium, Heavy, Destroyed }` with `effect_name()`; `enum FireState`; `enum BankState` with `fn frame(self) -> usize` for the view (keeps the sprite-frame contract in one place).
**Behaviour-preserving?** yes. **Effort:** S-M

##### B2-19 [severity: medium] [category: bevy] The scene-node `Name` scans in `breakup` and `swap_meshes` depend on string conventions of the exporter
**Where:** `combat/mod.rs:296-304` (`name.as_str().replacen("Damage_", "_", 1)` then `eq_ignore_ascii_case(&s.layer)`), `combat/mod.rs:343` (`!name.as_str().contains("Damage_")`), `combat/damage.rs:447-466` (`resolve_swap_nodes`: skips names containing `'.'`, `split_once("Damage_")`, rebuilds `"{a}_{b}"` with `format!`), `damage.rs:385-444` (`rescan` is triggered by any `Changed<Name>`/`Changed<ChildOf>` anywhere in the world).
**What:** Section/mesh mapping goes through glTF node names. `breakup` re-does the descendant walk and the `replacen` allocation for every node of every sinking ship on its first frame, in a different way than `resolve_swap_nodes` does it for the same node set (two implementations of "which node is section i"). `renamed` triggers a rescan of every ship and base whenever any scene loads anywhere, since the query has no entity filter.
**Why:** Two parsers of the same naming convention can drift. A mod GLB with a different naming convention breaks both silently (no node found, nothing hidden).
**Suggestion:**
- Resolve once, at scene-ready time, into the existing `SwapCache`, and let `breakup` reuse `SwapCache.nodes` (it already holds `(Entity, section, is_damaged)`; add the rest pose lazily). That removes the second parser.
- Better: use Bevy 0.20's scene `Ready` event (checklist 1.11) to resolve once per ship instead of polling `Changed<Name>`.
- Put the `"Damage_"` convention into one `const DAMAGE_PREFIX` and document it in `docs/data-format.md` for modders.
**Behaviour-preserving?** yes. **Effort:** M

##### B2-20 [severity: low] [category: simplify] Three independent ray/segment-triangle implementations
**Where:** `combat/hit_test.rs:97-123` (`seg_mesh_hit`, with normal), `collision.rs:110-130` (`Obstacle::ray_hit`) and `collision.rs:140-165` (`Obstacle::segment_hit`). All three are Moller-Trumbore with different epsilons (`1e-9`, `1e-8`, `1e-9`) and different edge acceptance (`(0.0..=1.0).contains(&u)` vs `u >= 0.0 && v >= 0.0 && u + v <= 1.0`), and the sphere helper `seg_sphere` lives in hit_test while the segment-vs-box one is `seg_aabb`.
**What:** One intersection routine is reimplemented three times. `ray_hit` and `segment_hit` differ only in the `t` range (open-ended vs `[0,1]`), so `ray_hit` could be `segment_hit(o, o + d * BIG)`.
**Why:** Fixes (epsilon, degenerate triangles, back-face policy) must be made three times. Fidelity depends on exactly which edge cases hit.
**Suggestion:** One `fn seg_tri(a, b, tri) -> Option<(f32, Vec3)>` in a small shared `geometry.rs` (or `collision/geom.rs`) used by all; keep the double-sided note. Keep an explicit test that fixes today's edge behaviour first.
**Behaviour-preserving?** needs-check (epsilon differences). **Effort:** S-M

##### B2-21 [severity: low] [category: simplify] `hit_test.rs` uses `ship_segment_hit` etc. with positional tuples where small structs would read better
**Where:** `combat/shells.rs:346` (`nearest_ship_hit` returns `Option<(Entity, f32, usize, Vec3)>`, destructured positionally at 501, 502, 605, 635 and in beam.rs:133-138), `combat/shells.rs:328` (`ShipTargets` 7-tuple query destructured as `(_, _, _, hp, _, mission, _)` everywhere), `combat/gunnery.rs:326,344,870,873` (5-7 element optional tuples).
**What:** Anonymous tuples with five to seven elements, with `.3`, `.5`, `x.5` style access (`ships.get(ship).ok().and_then(|x| x.5.map(..))` at shells.rs:614, `o.3.is_some()` at gunnery.rs:535).
**Why:** Readability; tuple order errors compile when types coincide.
**Suggestion:** `struct ShipHitAt { entity, t, section, at }`; `#[derive(QueryData)] struct ShipTargetData { entity, tf, ship, hp: &mut Health, fires, mission, drift }` for `ShipTargets`; `#[derive(QueryData)] struct ScenicTarget` for the `objects` query (`o.3.is_some()` becomes `o.base.is_some()`).
**Behaviour-preserving?** yes. **Effort:** M

##### B2-22 [severity: low] [category: bevy] `Local<HashMap<Entity, _>>` state is never cleaned up for despawned entities and is not reset on restart
**Where:** `combat/gunnery.rs:352,353,355` (`mine_laying`, `ground`, `rock_watch`) (only `ground` is `remove`d, and only for a destroyed ship; `mine_laying`/`rock_watch` never are), `collision.rs:~455` (`cables` is `retain`ed, `avoiders` is `retain`ed, `rammed` is time-pruned: these are fine), `combat/damage.rs:518` (`crew_repair`'s `acc: Local<f32>`), `combat/beam.rs:213` (`drawn` is `retain`ed, fine), `combat/mod.rs:226` (`Local<Rng>`).
**What:** `RockWatch` / `MineLaying` entries for entities that despawn (or when a battle restarts and entity indices are reused) stay in the map; a reused index inherits the old ship's `since` clock. `crew_repair`'s 12 s accumulator carries across restarts, so the first damage-control tick in the next battle comes early.
**Why:** Entity-keyed `Local` maps are the "stale entity IDs" smell (checklist 1.10), and they make "battle two" differ from "battle one".
**Suggestion:** Make these per-ship components (`RockWatch`, `MineLaying`, `GroundAim` on the ship entity, added by `spawn_guns`) so they die with the ship and reset with the scenario. Same for the 12 s crew-repair clock (`CrewRepairClock` resource reset at battle start, or a component).
**Behaviour-preserving?** yes (and fixes a restart leak). **Effort:** M

##### B2-23 [severity: low] [category: bevy] Messages: `Destroyed` is rock-only while ships and bases use other paths; `Fired.target` doc is stale
**Where:** `combat/mod.rs:104-108` (`Destroyed`: "An asteroid crossed the death threshold"), `combat/mod.rs:60-62` shows ships use `lifeboats::ShipLost`, `combat/mod.rs:122-123` (`Fired.target` doc says "None never happens for ships; kept for asteroids/scenery later", but `gunnery.rs:605,679` sets `None` for ground points and PD, and 925 for bases).
**What:** Three "something died" channels (`Destroyed`, `ShipLost`, `Health.destroyed` polled by each system) with different meanings; base part destruction is not signalled at all (`apply_base_hit` returns a list, `base_parts_burst` plays it directly). The `Hit` message is written even for invulnerable victims (documented), but `Splashed` is blast-only.
**Why:** The rules interpreter and crew speech each have to poll different things. A modder subscribing to "a ship was destroyed" has no single message (checklist 1.8).
**Suggestion:** Document the three channels in the module header or unify: a `Destroyed { by: Option<Entity>, victim: Entity, kind: Victim }` message emitted from `deaths` and `asteroid_deaths`/blast/ram, with `ShipLost` becoming a reader. Fix the stale `Fired.target` doc. `Fired.bullet: String` (B2-16) could be `BulletId`.
**Behaviour-preserving?** yes if added alongside. **Effort:** M

##### B2-24 [severity: low] [category: bevy] Plugin and ordering: the combat chain and `Update` presentation are registered as two long tuples
**Where:** `combat/mod.rs:215-229`.
**What:** `plugin` is a function that does `.add_systems(FixedUpdate, ( (minefield::arm, fire, beam::burn, base_fire, lob::shoot, shells, lob::detonate, lob::blasts, lob::grav_wells, lob::ram_damage, lob::strikes).chain(), (asteroid_deaths, fires, status::tick_nets, crew_repair, deaths).chain() ).chain().in_set(BattleSet::Combat))` and a long unnamed `Update` chain `(breakup, draw, engine_rate, damage_fx, debris, swap_meshes, beam::draw).chain()`. Dependencies are implicit in the position: for example `lob::shoot` must run before `shells` of the next tick (`Joining` marker), `ram_damage` must run after `collision::sail` (which is in `Forces`), and `asteroid_deaths` must run after every system that writes `Destroyed` (`shells`, `blasts`, `sail`).
**What:** Ordering rules are not stated and cannot be extended by a mod (`remove_systems_in_set` needs sets). `breakup`/`damage_fx`/`swap_meshes` are presentation systems on a chain only to order them, though they share no data dependency except `Sinking`/`DamageFx`.
**Why:** Checklist 1.1/1.2: named `*Systems` sets per phase make order explicit and let mods reorder or remove a stage.
**Suggestion:** Introduce `pub enum CombatSystems { Fire, Flight, Blasts, Deaths }` (and `CombatPresentationSystems`) configured with `.chain()`, put each system `.in_set`, and add a doc comment per set stating the invariant it protects. Do not use `chain_weak`.
**Behaviour-preserving?** yes. **Effort:** S-M

##### B2-25 [severity: low] [category: modding] Shell appearance and cosmetic constants are fixed in Rust; `combat/mod.rs` also owns presentation
**Where:** `combat/mod.rs:239-247` (`Look`: sphere radius `0.35`, gold `(1.0, 0.8, 0.3)` / red `(1.0, 0.25, 0.25)` placeholder materials), `combat/mod.rs:373-381` (`ring` with 32 segments), `combat/mod.rs:215-229` registers `breakup`, `draw`, `engine_rate` (`size_scale = 2.6` PROVISIONAL), `damage_fx`, `debris`, `swap_meshes`, `beam::draw` (`beam.rs:203-286`, several magic lights 300_000/400_000/500_000 lumens).
**What:** The module doc says combat is "guns, shells, damage sections", but the same plugin also owns gizmos, a mesh-based beam renderer, particle emitters and debris. AGENTS.md: "Views only draw; shared code never builds widgets". Those are not UI widgets, so `tests/test_layers.py` passes, but the retail view and render profile must now work around shared presentation (e.g. `beam::Replaced`, `ExhaustParticles`).
**Why:** A new presentation crate must disable or duplicate these behaviours. The `Replaced` and `ExhaustParticles` resources are evidence of that friction.
**Suggestion:** Longer term, move `breakup`, `draw`, `engine_rate`, `damage_fx`, `debris`, `swap_meshes`, `beam::draw` into `classic/` (or a `combat::presentation` plugin that `run(view)` registers), leaving `combat::plugin` with simulation systems only. Shell colours/sizes should come from the view too. Needs a maintainer decision (layer change, not a data format change).
**Behaviour-preserving?** yes if moved intact. **Effort:** L

##### B2-26 [severity: low] [category: rust] Visibility and re-exports in `combat/mod.rs` are very wide
**Where:** `combat/mod.rs:6-9,22-35` (`pub use crate::data::*;`, `pub use asteroid::*; pub use damage::*; pub use gunnery::*; pub use hit_test::*; pub use shells::*; pub use status::*;`), `Engine` private fields with `thrust()`/`effect()` accessors (good), but `Health`, `Target`, `GroundFire`, `Rng` and most `Shell` fields are `pub`.
**What:** Glob re-exports flatten six modules plus all of `crate::data`, so `crate::combat::X` does not tell the reader which file owns `X` (docs/development.md says for `lib.rs` "do not add glob re-exports"; the same reasoning applies). `use super::*` in each child then imports everything, hiding dependencies (e.g. beam.rs gets `fx`, `Sfx`, `Library` implicitly). Several items are `pub` only to be used by tests or within the crate (`pub fn xorshift_unit`, `pub fn seg_aabb`'s siblings).
**Why:** Checklist 5.5 and the project's own rule.
**Suggestion:** Replace the globs with explicit `pub use` lists naming the real public API, make helper functions `pub(crate)`, and give child modules explicit imports.
**Behaviour-preserving?** yes. **Effort:** M

##### B2-27 [severity: low] [category: simplify] `Rng` helpers and the xorshift generator
**Where:** `combat/mod.rs:155-197`.
**What:**
- `xorshift_unit` returns `(*state >> 40) as f32 / (1u64 << 24) as f32`, which is in `[0, 1)` exactly: good.
- `Rng::variant(n)` computes `1 + (self.f() * n as f32) as u32 % n`. The `% n` is dead because `f() < 1`, so the result is already `< n`; it is harmless but misleading.
- `Rng::pick` has the same `% pool.len()`.
- `Rng` holds a bare `u64` with a zero state hazard: seed `0` yields all zeros forever (`Rng::new(0)` in tests, or a future `--seed 0`).
- `range` / `f` are public without docs, and `direction()` samples a cube (non-uniform on the sphere) and retries nothing; that is documented nowhere and may be a fidelity point.

**Why:** Small, but the RNG is the determinism root (B2-03).
**Suggestion:** Guard `Rng::new` against a zero seed (`seed.max(1)` or splitmix), remove the redundant modulo, add docs saying `direction()` is the original's cube-normalised draw if that is what it is (or mark PROVISIONAL).
**Behaviour-preserving?** yes (except the zero seed). **Effort:** S

##### B2-28 [severity: low] [category: simplify] Duplication between `fire` and `base_fire`, and `unwrap` on a query result
**Where:** `combat/gunnery.rs:673-729` vs `923-953` (burst start, per-shot deviation, per-muzzle turn, `Launch` construction and `launch_shell` call are near-identical; differences are `gunner` accuracy factor, volcano cycle, `harmless`, `base: true`); `combat/shells.rs:639` (`ships.get_mut(si).unwrap()` after a `get(si)` succeeded earlier in the same iteration: safe but fragile); `combat/gunnery.rs:775-791` (`launch_shell` takes 15 parameters, `#[allow(clippy::too_many_arguments)]` is absent but the function is `pub(super)` with `mgt`/`mz`/`mgt` abbreviations).
**What:** The two systems differ only in target source and accuracy; the rest is shared logic maintained twice. `launch_shell` bundles side effects (sound, mesh, travel sound, fx, muzzle flash) behind a long positional signature.
**Why:** A fix to burst logic (e.g. per-shot deviation, ricochet band) has to be applied in two places.
**Suggestion:** Extract `fn run_burst(state, gun, mount, ctx: BurstCtx, rng, ...)` returning the shells to launch, called by both; make `LaunchCtx` (commands, assets, lib, fxl, look, sfx) a `#[derive(SystemParam)]` or a struct so `launch_shell` takes `(&mut LaunchCtx, &GunDef, MuzzleInfo, Launch)`. Replace `.unwrap()` at shells.rs:639 with `let Ok(..) = .. else { continue }` or restructure to take the data from the first `get`.
**Behaviour-preserving?** yes. **Effort:** M

##### B2-29 [severity: low] [category: fidelity-risk] Timers use accumulated `dt` floats; a few are frame-rate sensitive when the Fixed step changes
**Where:** `combat/gunnery.rs:31-32,71,277,963-964` (`REQUEST_LAPSE`, `ROCK_PERIOD`, `GROUND_THINK`, `LOB_*_DELAY`), `combat/damage.rs:52,474` (`FIRE_TICK = 6.0`, `CREW_REPAIR_TICK = 12.0`), `combat/shells.rs:337` (`REJECTION_RESET`: the comment says "31 ticks of 0.033 s, so 1.023 s" but `firer_exempt(age) = age < 1.0` compares the float sum, so after 30 ticks of 0.033 (0.99) it is still exempt and exempt for 31 ticks, which is consistent only while the tick is exactly `0.033`), `collision.rs:~440` (`RAM_COOLDOWN` against `time.elapsed_secs()`).
**What:** Time is accumulated as `f32` seconds with `>`/`>=`/`<=` comparisons whose tick-boundary outcome depends on float rounding; the checks use different inequality directions (`>` for requests, `>=` elsewhere), and `REQUEST_LAPSE` uses "more than". The project states "sim at 33 ms", which makes this deterministic, but the original counts ticks (the shell comment even says "31 ticks"). The `speed` option (virtual time scale) feeds into `Time<Fixed>`'s `delta`, not into float accumulation, so this is safe today, but any change of `BATTLE_TICK` would shift every cooldown by one tick.
**Why:** Fidelity rows are expressed in original ticks; expressing timers in ticks (`u32`) makes the correspondence explicit and removes float drift.
**Suggestion:** Where the original is tick-based (rejection list, fire tick, avoider think, mine rescan), store `u32` tick counters and `const FIRE_TICKS: u32 = 182`, with the seconds value in the comment. At minimum add a unit test that pins the boundary tick count (`firer_exempt` for 30, 31, 32 ticks).
**Behaviour-preserving?** needs-check. **Effort:** M

##### B2-30 [severity: low] [category: rust] Error handling: `let ... else { continue }` swallows real failures; panics in load paths
**Where:** `combat/shells.rs:571` (`if let Some(..) && let Ok(..) = bases.get_mut(be)` silently drops a hit if the base vanished), `combat/lob.rs:35,119,185` (`lib.bullets.get(..) else { continue }`: unknown bullet in `Shoot`/`Detonate`/`Blast` is silently skipped; a mod typo is invisible), `combat/minefield.rs:25` (`lib.bullets.get(&mine.bullet) else { continue }`: the map mine never arms and nothing is logged), `combat/asteroid.rs:116` (`rocks.get_mut(e) else continue` is fine), `collision.rs:35-64` (`FromWorld` with `panic!`/`expect`).
**What:** Mod-data mistakes produce no message at all (unknown bullet) or a bare panic (collision.json).
**Why:** Checklist 1.14/5.2.
**Suggestion:** `warn_once!` (or a `Local<HashSet<String>>` to log each bad name once) on the `continue` branches that mean "data is wrong"; make `Shapes` loading return `Result` with the file path and shape name, reported via the same error type the importer check uses.
**Behaviour-preserving?** yes. **Effort:** S

##### B2-31 [severity: low] [category: simplify] `status.rs` mixes HUD vocabulary with rules; small clean-ups
**Where:** `combat/status.rs:1-229`.
**What:**
- `status.rs` (documented as "Read-only ship status derived from guns and sections") also owns `tick_nets` (a mutating FixedUpdate system) and `Netted`, which are simulation state, not read-only status.
- `guns_ready` (205-215) hand-rolls a filter-count that is `filter(..).count()`.
- `section_lines` formats a HUD string in shared code.
- `speed_factor`/`turn_factor`/`loss` take `fn(&Section) -> bool` flags where the `Section` fields `speed`/`turn` could be passed as an enum.
- `bank_state` allocates a `Vec` just to test emptiness and `any`.

**Why:** Module cohesion (checklist 5.1).
**Suggestion:** Move `Netted`/`tick_nets` to a `combat/nets.rs` (or `shells.rs`' status-effects neighbour). Rewrite `guns_ready` and `bank_state` as iterator folds without the intermediate Vec. Keep `gun_icon`/`section_lines` where the HUD contract (`panels.rs`) can reach them, or move them to `panels.rs`.
**Behaviour-preserving?** yes. **Effort:** S

##### B2-32 [severity: low] [category: modding] Dev-only scripts live in the production module and `combat` reaches into `crate::menu`, `crate::environment`, `crate::crew_arms`, `crate::campaign`
**Where:** `combat/asteroid.rs:329-357` (`asteroid_demo`, a `--script asteroid` smoke driver, registered in `combat::plugin`, mod.rs:230), the 11-way cross-module query in `gunnery::fire` (campaign, crew_arms, boarding, environment, cloak, ship, collision, line_of_fire).
**What:** AGENTS.md says smoke drivers belong in `smoke/` ("keep production simulation in its owning module"). `asteroid_demo` checks `cfg.frame == 90` and `cfg.script` every frame in production builds.
**Why:** Ownership clarity (docs/development.md table lists `smoke/` as the home for script names and checked scripts).
**Suggestion:** Move `asteroid_demo` to `smoke/` and register it from there. For `fire`, define a small `GunneryInputs` SystemParam in `combat/gunnery/inputs.rs` so the dependency on other features is in one named place.
**Behaviour-preserving?** yes. **Effort:** S (demo) / M (inputs)

#### Quick wins
- **B2-13:** move the mismatched doc comments (`wear_down`/`guide_torpedo`, `damage_pool`/`side_damage_scale`, `gunner_reload_cut`). Pure text; fixes misleading hover docs on the most important damage function.
- **B2-07:** add `Shell::new(..)` and delete four 17-field literals.
- **B2-10:** one `is_human_ship()` helper and one `effective_posts()` helper replace three copies each.
- **B2-02:** give `Sinking` a tick counter advanced in `FixedUpdate`; the 0.14 s drive switch becomes frame-rate independent.
- **B2-03:** reset `Rng` at battle start and log the seed (one line in the scenario spawn path).
- **B2-14/B2-15:** name `SPILL_FRACTION = 0.4`, the difficulty tables and the unmarked radii (2.5, 0.5), each with a PROVISIONAL or docs/re link.
- **B2-17 (partial):** use `starts_with(TORPEDO_BULLET)` or `==` consistently (helper `is_torpedo(bullet)`), and compute `is_mine` once per mount (cache on `GunState` at spawn).
- **B2-16 (partial):** skip the `threat_hp` HashMap build when `threats.is_empty()`, and use `Local<Vec<_>>` for `incoming`/`contacts`/`rocks`/`base_targets`, as `seen` already does.
- **B2-30:** `warn!` once per unknown bullet name in `lob.rs`/`minefield.rs`.
- **B2-27:** drop the dead `% n` in `Rng::variant`/`pick` and guard the zero seed.
- **B2-32:** move `asteroid_demo` into `smoke/`.

---

## Appendix: Mission rules & campaign (game/src/rules/, campaign.rs, briefing.rs, victory.rs, lifeboats.rs, docs/missions.md, docs/rule-types.md)

**Assessment.** The pure/ECS split is real and well kept: `Script::step` takes a unit snapshot and returns `Fx` values, and `systems/apply.rs` is the single place that turns effects into ECS changes. The campaign and skirmish scripts share it, which is good design. The mission-author tooling is better than most projects at this stage: a machine-readable schema (`eval/schema.rs`), `--check-mission` with "did you mean" hints, `--rule-trace` and `--why`, a developer console that reuses `Script::act`, and a test that fails if the schema and the interpreter disagree. `victory.rs` parses its JSON once into a typed `Cond` enum and is the model the rest should follow. The weak spots are structural. Rules stay as raw `serde_json::Value` and are matched by retail string on every frame, so a bad rule gives no load-time error. The mission data model is also several hand-synchronised copies: `cond.rs`/`act.rs`, `schema.rs`, `check.rs`, `victory.rs` and the console `TABLE`. Mission-1 specifics leak into generic modules (`briefing.rs`, `systems.rs::update`, `state.rs::skip_to_tender`). A dev autopilot of about 800 lines ships inside the production rules module. The rule evaluator runs in `Update` and reads the interpolated `Transform`, so volume and distance conditions depend on frame rate.

#### Findings

##### B3-01 [severity: medium] [category: fidelity-risk] Rules are evaluated on the interpolated (drawn) pose, not the simulation pose
**Where:** `game/src/rules/systems.rs:~120-150` (`units()` reads `transform.translation`), `systems.rs:update`, `rules/skirmish.rs:run`; `game/src/interpolate.rs` (`Interp::sim_pose` is `#[cfg(test)]`); `lib.rs:402` (`FrameRules`).
**What:** `FrameRules` systems run in `Update`. `docs/development.md` says everything in `Update` sees `previous.lerp(current, overstep)`. `units()` builds each `Unit.pos` from that blended `Transform`. `Enter Volume`, `Group to Group Distance` and `Unit is Within any Nebula` therefore see a position that lags the simulation by up to one tick. The lag depends on frame time and speed, so edge detection can fire on a different frame at 30 Hz than at 144 Hz.
**Why:** The docs promise that sim results do not depend on frame rate, and mission-flow timing is the thing a trace comparison would catch. Level-triggered conditions are only delayed. Edge-triggered ones (`prev_inside`, `Enter Volume` for repeating rules) can differ.
**Suggestion:** In `units()`, read the sim pose: `Interp.sim.unwrap_or(*transform)`. This means making `Interp::sim_pose` non-test and adding `Option<&Interp>` to the `RuleShip` query. Do the same for the animals, rocks and mines unit builders. Alternatively document in `FrameRules` that rules see the drawn pose.
**Behaviour-preserving?** needs-check (it changes rule timing by up to 33 ms; it is the more faithful behaviour).
**Effort:** S-M

##### B3-02 [severity: medium] [category: fidelity-risk] Rule effects are applied in `Update` while mission AI and the sim run in `FixedUpdate`
**Where:** `systems.rs:apply_fx`, `apply.rs` (orders, `waypoints`, `order`, `cap`, `Transform` writes), `mission_ai` in `BattleSet::Ai`.
**What:** `Script::step` advances by the frame delta (`time.delta_secs()`) in `Update`, and the effects mutate `Ship`/`Health`/`Transform` between ticks. Which tick sees an order depends on where in the frame loop it was applied. A frame with zero ticks can run rules twice per tick; a frame with several ticks runs them once. The docs state this as a deliberate choice (`re/tick.md`), but timer thresholds and `OP Wait` resolution are quantised to frame time, not the 33 ms tick.
**Why:** For a faithful, replayable simulation this is a hidden source of run-to-run differences. `--script mission` runs seem to have needed a fixed-tick workaround for the same reason (`briefing.rs:autoplay_skip` comment: "how many ticks 60 frames hold depends on the machine's frame times").
**Suggestion:** Record in `docs/re/tick.md` how the original orders rule evaluation relative to the tick. If it is per tick, move `FrameRules` into `BattleSet::Rules` (it already exists in `BattleSet`). If not, keep it and accumulate `dt` per tick.
**Behaviour-preserving?** no (it changes timing).
**Effort:** M

##### B3-03 [severity: high] [category: simplify] Rules stay as `serde_json::Value` and are string-matched every frame; bad mission data is only found by an optional CLI
**Where:** `rules/eval.rs:23-36` (`Rule { conds: Vec<Value>, acts: Vec<Value> }`), `eval/cond.rs` (~60 arms of `match s(c, "Type")`), `eval/act.rs`, `types.rs:196-203` (`s`/`f`/`b` accessors that return `""`/`0`/`false` on any mismatch).
**What:** Every condition is re-looked-up by string key each frame, and the `match` on the `Type` string is re-done each frame. A misspelled field, a number written as a string or a JSON `true` where `"TRUE"` is expected silently becomes `""`/`0`/FALSE. The checker (`check.rs`) describes the problem well, but it runs only under `--check-mission`. `Script::new` also does `.expect("rules")` (`eval.rs:27`), so a mod mission with no `rules` array panics at load.
**Why:** For a mod-friendly engine, errors should surface when the mission loads, with the rule name and field. Matching on strings repeatedly also hides typos in the engine itself. The schema table already lists every Type and field, so most of a typed form already exists.
**Suggestion:** In two steps. (1) Cheap: when a non-retail mission loads (`campaign::load_id`, `SkirmishScript::load`), run `check::check_with` and `warn!` each Error line, so a mod author sees the problems without knowing the flag. Replace the `expect("rules")` with `unwrap_or_default` plus a warning. (2) Larger: resolve `Type` once into a `CondKind`/`ActKind` enum at load, with `schema.rs` as the single table that maps name to enum. The match in `cond.rs` then becomes exhaustive (the compiler finds a missing arm, which replaces the `schema_matches_the_interpreter` test). Field accessors stay `Value`-based, so JSON layouts do not change.
**Behaviour-preserving?** yes (step 1 only adds logs; step 2 is a refactor).
**Effort:** step 1 S; step 2 L

##### B3-04 [severity: medium] [category: simplify] The rule vocabulary lives in five hand-synchronised places
**Where:** `eval/cond.rs` + `eval/act.rs` (behaviour), `eval/schema.rs` (`RULE_TYPES`, 654 lines), `check.rs` (validation), `victory.rs:condition` (a second interpreter for a subset of the same Types), `console.rs:TABLE` + `parse` (command table vs match).
**What:** Each pair is kept consistent by a test (`schema_matches_the_interpreter`, console table tests). Adding a Type touches `cond.rs`, `schema.rs`, `docs` (generated) and perhaps `victory.rs`. `victory.rs` parses `"Group Destroyed"`, `"Does Group Contain Unit Name"`, `"Enter Volume"`, `"Team has X points"` independently from `cond.rs`, with different semantics for some (for example `Exists?` accepts JSON bool or string there, only the `"TRUE"` string in `b()`).
**Why:** Duplication means drift. A modder who learns that `Exists?` takes `true` in a skirmish end rule will be surprised in a mission rule.
**Suggestion:** After B3-03 step 2, have `victory.rs` consume the same parsed condition type (it is the one with `Handler::Victory` in the schema) and evaluate it against its `Snapshot`. Until then, list in `schema.rs` the Types that `victory.rs` also evaluates, so a new one is not forgotten. Keep the console `parse`/`TABLE` pair, but generate the `parse` dispatch's word list from `TABLE` (it already has `alias`).
**Behaviour-preserving?** yes
**Effort:** L

##### B3-05 [severity: medium] [category: simplify] "Equivalence" comparison is implemented four times with stringly typed values
**Where:** `eval/types.rs:204-211` (`cmp`), `check.rs:~245-255` (checks whether a comparison can hold), `victory.rs:257-263` (`Test` enum), `eval/schema.rs:273` (`COMPARE`).
**What:** The same three strings are parsed in four places. `victory.rs` already has a `Test` enum; `cmp` takes `&str` on every evaluation and returns `false` for unknown strings.
**Why:** Easy to make the four disagree (the `Equal To` epsilon is `1e-4` in `cmp`, `1e-4` in `check.rs`, `0.001` in `victory.rs`).
**Suggestion:** One `Comparison` enum with `FromStr` and `fn holds(self, a: f32, b: f32) -> bool` in `eval/types.rs`; `victory.rs` and `check.rs` use it. Keep the per-site epsilon only if the original differs (note it).
**Behaviour-preserving?** yes (keep the `0.001` in the victory base-damage test explicitly).
**Effort:** S

##### B3-06 [severity: medium] [category: modding] Boolean conventions are inconsistent and a JSON `true` silently reads FALSE
**Where:** `data.rs:532` (`wot_flag`), `types.rs:200` (`b()`), `eval.rs:30` (`r["Run Once"] != false`), `ext.rs:56` (`flag()`, accepts both), `victory.rs:~268` (`Exists?` accepts both), `docs/rule-types.md` Conventions, `docs/missions.md:199-202`.
**What:** Inside conditions and actions, a boolean is the string `"TRUE"`/`"FALSE"`, and a JSON `true` is false. `Run Once` needs a JSON boolean (the string `"FALSE"` still means once). `Is Active` accepts both. `Exists?` in a victory rule accepts both. The docs have to explain all of this in three places.
**Why:** The most common first mistake of a mission author is writing `true`. The engine's answer is silent. The checker reports it, but see B3-03.
**Suggestion:** Make `b()`/`wot_flag` accept JSON `true`/`false` as well as the retail strings, and `Run Once` accept `"FALSE"`. Retail data uses strings only, so no retail rule changes. A mod that relied on `true` meaning FALSE would be a bug. Keep the checker's note as a style warning. This is a data-semantics change, so ask a maintainer.
**Behaviour-preserving?** yes for retail data; no for malformed mod data.
**Effort:** S

##### B3-07 [severity: medium] [category: modding] Mission-1 specifics hard-coded in generic modules
**Where:**
- `briefing.rs:195-214`: world-object ids `222`, `227`, `228`, `229`, `231`; path names `"Opening NIS"`, `"Opneing NIS 2c"`, `"Opening NIS 2d"`; camera speeds `115.0`, `65.0`, `125.0`, `70.0`; landmark `"Montressor Archipelago"`; `_ => unreachable!()` for a fifth speech cue.
- `campaign.rs:366`: `script.briefing_opening = id == "mission01"`.
- `systems.rs:229-243`: `"RLS Bounty"`, `"Dock With Tender"`, the hint string `"Left-click RLS Bounty (or press D) to dock"`, and `KeyCode::KeyD` hard-wired (`systems.rs:89`) instead of `Bindings`.
- `state.rs:244`: `skip_to_tender` names mission-1 rules and flags and does `.expect("mission 1 rule")` inside the generic `Script`; its only non-test caller is `smoke/mod.rs:183`.
- `presentation.rs:~46`: `speaker_portrait` only knows `M01` + AMELIA/JIM/ONYX.
**What:** These are retail-mission behaviours encoded as Rust. A mod that replaces `mission01` (the docs say a retail id replaces the mission) with a different `opening_speech` of five cues, or other ship ids, panics with `unreachable!()`, `expect("briefing ship")` or `unwrap()` (`briefing.rs:141,166,167,183,189,196,205,214`).
**Why:** The opening is the one place where the data-driven design stops. It is also the least mod-safe code in the bucket.
**Suggestion:** Move the opening description into the exported mission data (an `opening` block: cue to path/focus/speed table) and read it generically, with the current values as the retail export. Failing that, replace every `expect`/`unwrap`/`unreachable!` in `briefing.rs` with "skip the opening and log" (so a bad mod opening ends the NIS rather than panicking). Move `skip_to_tender` to `smoke/`. Move the mission-1 hint into the rules (it is a task text plus a proximity check; a rule action `Set Hint Text` or the existing `hint` field fits). Make the dock key a `Bindings` act. A data/format change needs maintainer approval.
**Behaviour-preserving?** yes (for retail mission 1).
**Effort:** M-L

##### B3-08 [severity: medium] [category: simplify] The dev autopilot (`systems/autopilot.rs`, 789 lines) is production code inside the rules module
**Where:** `rules/systems/autopilot.rs`, called from `systems.rs:update` via `cfg.script == "mission"`; `extra.pilot`, `extra.fleet_fleeing`, `extra.player_guns`, `extra.land` in `Extra`.
**What:** A path planner, stuck detector and English-text heuristics (`defend_task`, `flee_task`, `is_dock_task`) that exist only for unattended test runs. They add four `Extra` fields, 12 `PROVISIONAL` consts and `std::env::var_os("PROCYON_AUTOINVULN")` read every frame (`autopilot.rs:20`). `Script::wants`/`dock_targets`/`never_fired` (`state.rs:~395-520`) and the `Want` enum also exist only for it.
**Why:** The project already has `smoke/` for "scripted drivers". Keeping the bot beside `update` makes `Extra` and the `RuleShip` query bigger than the game needs and is the second-largest file in the bucket.
**Suggestion:** Move `autopilot.rs`, `Want`, `Script::wants`, `dock_targets`, `skip_to_tender` and the `dock_smoke_skipped` handling to `smoke/` (or a `smoke/autopilot/` folder) behind a `SmokeAutopilot` system that reads `Mission` and the `RuleShip` queries itself. `update` then loses the 5 extra `SystemParam` fields. Read `PROCYON_AUTOINVULN` once into a resource.
**Behaviour-preserving?** yes
**Effort:** M

##### B3-09 [severity: medium] [category: simplify] Unit-list building and voice spawning are copied three times
**Where:** `systems.rs:update` (units + `keep_sunk` + animals + bases + rocks + `tag`), `systems/dev_console.rs:run` (the same, minus `keep_sunk`), `rules/skirmish.rs:run` (a hand-written `Unit {..}` literal instead of `units()`, plus mines). Voice entity spawn (`Scenario, Voice, AudioPlayer, PlaybackSettings::DESPAWN.with_volume(...)`) appears in `systems.rs:~205`, `skirmish.rs:~106` and `briefing.rs:~188`.
**What:** The console's unit list is deliberately "as `update` builds it each frame" (comment), but nothing enforces it; `skirmish.rs` already differs (no `damage`, no bases).
**Why:** A new unit source (a new scenery type) must be added in three places, and the console and skirmish silently miss it.
**Suggestion:** A `UnitSource` `SystemParam` (queries for animals, bases, rocks, mines) with `fn snapshot(&mut self, ships, lib) -> Vec<Unit>`, and a `fn start_voice(commands, assets, file)` helper in `briefing.rs`. `skirmish.rs` should call `units()` (and then differences become visible in one diff).
**Behaviour-preserving?** needs-check for skirmish (`units()` also fills `damage` and `primary`, which skirmish rules might start to see; that is more correct).
**Effort:** M

##### B3-10 [severity: medium] [category: simplify] `Script` is a ~100-field struct and `Script::new` is a 100-line literal
**Where:** `eval/state.rs:8-150`, `eval.rs:20-175`.
**What:** Rule flow state (`fired`, `ran`, `init_done`), interpreter data tables (`media`, `paths`, `points`, `objects`, `volumes`, `point_sets`), world bookkeeping (docks, tows, hits, kills, lifeboats, teams), presentation state (`subtitle`, `hint`, `zoom`, `radar`, `crew_speech`, `abyss`, ...) and debug options (`start_rules`, `start_at`) are all fields of one struct, constructed positionally with 30 fields initialised to `HashMap::new()`/`false`/`None` by hand.
**Why:** Adding a feature means a field in `Script`, an init line in `new` and often a `pub(in crate::rules)` accessor. `Script::new` has to be touched even when the new state defaults.
**Suggestion:** Group by owner into sub-structs with `#[derive(Default)]`: `Data` (the read-only tables from the JSON), `World` (docked/tows/hits/kills/captured/destroyed/lifeboats/teams), `Hud` (the presentation toggles views read), `Debug`. `Script::new` then builds `Data` and `..Default::default()`. External callers keep `script.point`/`script.hint` by re-export or accessor. Alternatively derive `Default` on `Script` with `radar: true`, `crew_speech: true`, `all_visible: true`, `point_on_map: true` via a small manual `Default`, and keep `new` to the data-derived fields.
**Behaviour-preserving?** yes
**Effort:** M

##### B3-11 [severity: low] [category: rust] Magic `usize` discriminators and string-concatenated keys in edge-state maps
**Where:** `state.rs:74,562-580` (`prev_inside: HashMap<(String, usize), bool>` with `0`, `1`, `2` meaning enter/exit/armed), `cond.rs:~102,~119,~126,~132` (`s(c,"Group Name").to_string() + "|" + ...`, `format!("captured|{team}|{group}")`, `format!("once|...")`, `format!("killed|{player}|{kind}")` for `events`).
**What:** The key's meaning is only in the comment in `group_in_volume`. A format-string key typo would silently create a new counter.
**Suggestion:** `enum Edge { Entered, Exited, Armed }` and `struct EventKey { kind: EventKind, a: String, b: String }` (or `(EventKind, Box<str>, Box<str>)`). Also in `console.rs:880` the whole of `prev_inside/entered/events/mortar_seen` is cloned to run one condition; with a typed `EdgeState` struct that becomes `let saved = self.edges.clone()`.
**Behaviour-preserving?** yes
**Effort:** S-M

##### B3-12 [severity: low] [category: simplify] Identical `match` arms in `cond.rs`
**Where:** `cond.rs:37-41` (`"Group/Unit Vital Section has >,<,= X damage"`) and `cond.rs:65-69` (`"Group/Unit has >,<,= X damage"`) have the same body. `"Group/Unit Hit at least x Times by Player ( with eqivalence )"` and `"Player has hit Group/Unit at least x Times"` differ only in `cmp(...)` vs `>=`.
**Suggestion:** Merge the first pair with `|` (keep the `PROVISIONAL: vital sections are not modelled` comment on it). For the second, compute `n` once in a helper `hits_by(w, spec, player)`. `Group/Unit Docked` and `Group/Unit Is Docked` already use `|`, so there is precedent.
**Behaviour-preserving?** yes
**Effort:** S

##### B3-13 [severity: low] [category: simplify] Rule firing is implemented in two places
**Where:** `eval.rs:fire_start_rules` (`fired[i] = true; ran[i] = true; info!; run_from`) and `console.rs:~790` (`Command::Fire`, which also builds a throw-away `Rule { conds: vec![], acts: r.acts.clone() }` to satisfy `run_from`'s `&Rule` parameter), plus the main loop in `step` (`eval.rs:~96`).
**Suggestion:** A `Script::fire(&mut self, i, rules: &[Rule], w, out, why: &str)` doing the three assignments and the `run_from`. `run_from` takes `(i, from)` and reads the rule from `rules`, which also avoids the `std::mem::take(&mut self.rules)` / restore dance (`step`, `eval.rs:~158`, which is easy to break with an early return).
**Behaviour-preserving?** yes
**Effort:** S

##### B3-14 [severity: low] [category: simplify] `step` clones the whole unit list each frame
**Where:** `eval.rs:165` (`let mut w = w.to_vec();`), `systems.rs:units()` (every `Unit` allocates `name`, `owner`, `flag`, `attack: Vec<String>`, `groups`), `types.rs:members()` (a `Vec<&Unit>` per condition per frame).
**What:** `to_vec` exists so rules can see their own visibility changes (`run_from` writes `u.visible`). That is one clone of each unit's strings per frame, on top of the `Unit` creation.
**Why:** At 14-30 ships it is harmless; with larger skirmish fleets and many rules it is avoidable allocation in a per-frame system.
**Suggestion:** Take `w: &mut [Unit]` in `step` (the caller already builds a fresh `Vec` per frame and throws it away; check `systems.rs:update`, which uses `world` afterwards for the hint and autopilot, where seeing the post-rule visibility is arguably more correct). Intern owner/group names (`Arc<str>` or an interned id) when B3-03 step 2 happens.
**Behaviour-preserving?** needs-check (the autopilot and the hint would read post-step visibility).
**Effort:** S

##### B3-15 [severity: medium] [category: simplify] `console.rs` (991 lines) mixes four jobs; module-size seams are clear
**Where:** `rules/console.rs`: `Console` input state and line editing (`1-200`), `Cheats`/`Command`/`Entry`/`TABLE` (`205-460`), `parse` (`481-660`, 180 lines, one `match`), JSON builders (`660-705`), `Script::console_run_with` (`720-880`, 160 lines) and `validate`/`schema_lines`.
**Why:** The doc comment says "This file is the pure part", but it is the largest file in the bucket and the one a contributor must read to add a command.
**Suggestion:** `console/input.rs` (`Console`, `Edit`, `key_with`, history, Tab), `console/commands.rs` (`Command`, `Entry`, `TABLE`, `help_lines`), `console/parse.rs` (`parse` + builders), `console/run.rs` (`console_run_with`, `Outcome`). `console.rs` keeps the `pub use`. `Console::complete` already lives half in `complete.rs`. Move `Cheats` next to `systems/dev_console.rs`, which owns its lifecycle.
**Behaviour-preserving?** yes
**Effort:** M

##### B3-16 [severity: low] [category: simplify] Large functions with real seams
**Where:**
- `campaign.rs:spawn` (~170 lines, `campaign.rs:361-513`): scenery bases, island setup parse, ship spawn, per-ship `setups("*State Init* Setup Ship").find(...)` which re-scans every rule action for every ship (`campaign.rs:486`).
- `mission_ai.rs:mission_ai` (~150 lines, 11 parameters including a tuple-of-three to dodge the 16-parameter limit).
- `systems.rs:update` (17 parameters + `Extra` with 22 fields).
- `apply.rs:apply` (one 270-line `match`).
**Suggestion:**
- `campaign::spawn` into `spawn_bases(...)`, `spawn_ships(...)`, and a one-time `HashMap<i64, &Value>` of `Setup Ship` actions by `World Object ID`.
- `mission_ai`: extract `choose_target` (the `near`/`ordered`/`hostile` block) and `steer(chase)` (the three-way `if` that builds `(goal, order, throttle)`), both testable without the ECS.
- `update`: after B3-08 and B3-09, the `Extra` param set shrinks by about a third.
- `apply`: group arms into `apply_orders`, `apply_flags`, `apply_scenery` helpers by `Fx` family. Keep one `match` per helper.
**Behaviour-preserving?** yes
**Effort:** M

##### B3-17 [severity: low] [category: rust] Panics and `unwrap` on mod-controlled data in non-test code
**Where:**
- `campaign.rs:360-363` (`panic!("missing maps/{id}/{name}.json: run tools.export_campaign")`, `expect("invalid mission JSON")`), `campaign.rs:436` (`expect("mission ships")`), `campaign.rs:444,455` (`s["id"].as_i64().unwrap()`), `campaign.rs:462` (`s["type"].as_str().unwrap()`), `campaign.rs:255,256` (`parent().unwrap()`, `to_string_pretty(..).unwrap()` in `write_save`).
- `briefing.rs`: see B3-07.
- `eval.rs:27` (`.expect("rules")`).
- `victory.rs`: fine (uses `?`).
**What:** A mod mission with a ship that has no `id` or `type`, no `media.json` (the docs say "the game stops at the start if it is missing"), or no `ships` array aborts the process. The "run tools.export_campaign" hint is wrong for a mod author.
**Why:** The checklist (5.2) wants no `unwrap` outside tests, and `load_id` is on the mod path. `docs/missions.md:48` documents the missing `media.json` crash as expected behaviour.
**Suggestion:** `load_id` returns `Result<Mission, MissionError>` (a small `thiserror`-style enum: Missing(file), Invalid(file, serde error), BadShip(index, field)). `menu.rs` / `scenario` show the message and return to the menu. Treat a missing `media.json` as `Value::Null` (as `SkirmishScript::load` already does for skirmish). Skip, with a warning, a ship entry that lacks `id` or `type`.
**Behaviour-preserving?** yes for valid data.
**Effort:** M

##### B3-18 [severity: low] [category: modding] Unmarked guesses and magic numbers in the rule interpreter
**Where:**
- `cond.rs:163`: `self.time - t < 6.0` ("attacking" window; no source, no PROVISIONAL).
- `state.rs:547`: speech with no audio shows its text for `(text.len() / 14.0).max(2.0)` s and audio lines get `duration + 0.3` (no source; `text.len()` is bytes, not characters).
- `act.rs`: `NIS Set Camera Path` default camera speed `60.0`.
- `types.rs:near_inside`: `15.0` units.
- `act.rs:Fx::Cloak` in `apply.rs`: "never within 5 s of the ship's creation" is only in a comment.
- `mission_ai.rs:~85`: `engage * 1.3`.
- `campaign.rs:379-390`: gunnery scatter `0.7/0.4/0.1` (documented in a comment but not marked as verified or provisional).
- `lifeboats.rs`: `PICKUP_POINTS = 2` (doc says "as the original does"; fine) and race list `Navy|Civilian|Pirate|Procyon` (a mod cannot add a race).
- `presentation.rs:~92`: end-screen text, key hints "R restart   F10 menu / N next mission" and "Mission failed" (`ext.rs`) are English literals and ignore `Bindings`.
**Suggestion:** Each constant becomes a named `const` with a one-line source, or the word PROVISIONAL (AGENTS.md rule 5). Where a mod would plausibly tune it (lifeboat race, speech reading time), read it from `data/` with the current value as default. Build the end-screen key hints from `Bindings`.
**Behaviour-preserving?** yes
**Effort:** S

##### B3-19 [severity: low] [category: bevy] Plugin and set wiring: rules plugin registers other modules' systems; set names
**Where:** `rules/systems.rs:plugin` registers `crate::briefing::autoplay_skip`, `crate::smoke::autoplay_end`, `crate::smoke::screenshot_hooks`, `crate::briefing::update` ordering (`.before(crate::briefing::update)`), `crate::camera` ordering. `FrameRules` is a unit struct set; `BattleSet` has no `*Systems` suffix.
**What:** A plugin that registers systems of three other modules is the checklist's 1.1 smell (1.2 for the long `.after(other::fn)` chains). `capture_input` is ordered `.before(crate::briefing::update)` and `presentation.after(nis_camera)`, `nis_camera.after(crate::camera)`: ordering against functions rather than sets.
**Suggestion:** Let `briefing.rs` and `smoke/` register their own systems (`briefing::plugin`, already used by others?) and expose sets (`BriefingSet`, `CameraSet`) for the ordering. Keep `FrameRules` but add a doc line with the ordering inside it (`capture_input` -> `update` -> `console::run`). Renaming sets to the `*Systems` convention is cosmetic; do it only if Bevy migration notes are applied elsewhere.
**Behaviour-preserving?** yes
**Effort:** S-M

##### B3-20 [severity: low] [category: bevy] Per-frame work and `Option<Res>` handling in systems that could use run conditions
**Where:** `systems.rs:update` (`Option<ResMut<Mission>>` plus `let Some(..) else { return }` although the plugin already adds `.run_if(resource_exists::<Mission>)`), `mission_ai` (`Option<Res<Mission>>` in a system that is `.run_if(resource_exists::<Mission>)`), `dev_console.rs:run` (early return when nothing is pending), `briefing::update` (`Option<ResMut<Mission>>`; registered elsewhere), `lifeboats.rs` (`mission: Option<ResMut<Mission>>` is genuinely optional there: skirmish).
**What:** Redundant `Option` + `let Some else return` hides a registration mistake and double-checks. Use `ResMut<Mission>` and trust the run condition (as `ui`, `music`, `nis_camera` already do).
**Also:** `dev_console::enabled()` re-scans `std::env::args()` and the environment on each call (`Console::new(enabled())` is called once, but `systems.rs` re-exports it as `console_enabled`). Cache it in a `OnceLock<bool>`.
**Behaviour-preserving?** yes
**Effort:** S

##### B3-21 [severity: medium] [category: modding] `campaign.rs` mixes unrelated jobs and its header is stale
**Where:** `campaign.rs:1-3` ("Mission 1 setup: retail ship placements and the HUD text nodes ... rules.rs"), `MissionShip` (mission-ship state), `CampaignPoints` + persistence, `user_dir` (a general data-dir helper, 677 lines total), `CampaignSave` + `list_saves`/`write_save`, `custom_missions*`, `load_id`, `spawn`.
**What:** `user_dir()` is used by settings and saves everywhere, but lives in `campaign.rs`. The header describes a file that no longer exists. `AGENTS.md`'s module table says `campaign.rs` owns "Mission loading ... campaign points and save files", so the structure is documented, but the file is three modules.
**Suggestion:** Split into `campaign/mod.rs` (`Mission`, `load_id`, `is_mission`, `next`), `campaign/spawn.rs`, `campaign/points.rs` (`CampaignPoints`, `CampaignSave`, saves), `campaign/custom.rs` (`CustomMission`, `custom_missions_in`), and move `user_dir`/`clean_name` to a `paths.rs`/`modding.rs`. Fix the header. `mod.rs` re-exports, so `crate::campaign::X` stays stable.
**Behaviour-preserving?** yes
**Effort:** M

##### B3-22 [severity: low] [category: rust] Stringly typed owners/groups where a newtype would prevent mix-ups
**Where:** `Unit.owner: String`, `Unit.groups: Vec<String>`, `MissionShip.owner`, `Script.hostile: HashSet<(String, String)>`, `Fleets.local` (outside bucket), spec strings `"Group,Unit"` parsed with `split_once(',')` in `types.rs:matches`, `state.rs:answers`, `cond.rs:Does Group Contain Unit Name`, `check.rs:407`, `autopilot.rs`; the magic names `"HUMAN CONTROLLED COMMAND SHIP"` (`types.rs`, `state.rs`, `act.rs`, `console.rs:flagship`), `"Player0 Group"` (`PLAYER`), `"NO PLAYER"` (in `Kind::placeholders`).
**Suggestion:** `struct Spec { group: String, unit: Option<String> }` parsed once (with `Spec::parse`), and consts `HUMAN_COMMAND_SHIP` and `NO_PLAYER`. The `split_once(',')` logic then exists once. A `PlayerId` newtype is optional.
**Behaviour-preserving?** yes
**Effort:** S-M

##### B3-23 [severity: low] [category: rust] Magic ship order indices
**Where:** `mission_ai.rs:52,58,69,83,168,197`, `apply.rs:116,267,293`, `lifeboats.rs:90`, `autopilot.rs:771,783` (`ship.order = 0 / 1 / 2 / 3`).
**What:** `Ship.order` is an index into `ship::ORDERS` (`All Stop`, `Ahead Slow`, `Ahead Full`, `Emergency Speed`). `ship.order = 3` means Emergency in the attack branch, and `2` means "follow waypoints at full".
**Suggestion:** `impl Ship { pub const STOP: usize = 0; ... }` or `enum Order` with `usize::from` where the array is indexed. At minimum, named consts in `ship.rs` used by this bucket.
**Behaviour-preserving?** yes
**Effort:** S

##### B3-24 [severity: low] [category: fidelity-risk] Order/hash-iteration dependence in rule state
**Where:** `state.rs:team_of` (`self.teams.iter().find(...)` over a `HashMap`: a player on two teams resolves arbitrarily), `state.rs:points()` / `skirmish.rs:SkirmishScript::points` (HashMap order; only consumed with `.any`, so safe), `ext.rs:counter_list` sorts (good), `lifeboats.rs` (`min_by` over a query: a tie on distance picks by query order), `mission_ai.rs` (`.nearest_to(...)` ties).
**Suggestion:** Make `teams` a `BTreeMap` (the data is tiny) or a `Vec<(String, Vec<String>)>` in map order (the skirmish lobby has an order). For ties in `lifeboats`/`near`, break by `Unit.id`/ship slot (`FleetMember`'s slot) instead of query order, as checklist 4.3 asks.
**Behaviour-preserving?** yes in practice (no tie changes in retail data); makes ties deterministic.
**Effort:** S

##### B3-25 [severity: low] [category: simplify] Smaller clean-ups
**Where and what:**
- `victory.rs`: the "sides, sorted, dedup'd" block is duplicated in `from_map_in` and `scripted`; `ours` (the human's team name) is computed in `ours()` and again inline in `decide_with_message` (`~line 195`). The `alive_teams` computation exists twice (in `decide_end` and `decide_with_message`).
- `state.rs:~534`: the doc comment "Speech queue: one line at a time... Returns the sound file to start" is attached to `skip_speech`, not `speech_tick`.
- `skirmish.rs`: `SkirmishScript.paths` duplicates `script.paths` (a private `Value` clone of the same data).
- `eval.rs:~117-124`: the `first` closure and `members(...).into_iter().find(|u| u.alive)` appear three times; `Script::first_alive(w, spec)`.
- `check.rs:511`: the `Checker { ... }` literal lists 13 fields by hand; derive `Default` and write `Checker { defs: &defs, skirmish, wants_true: true, at: "rule".into(), ..Default::default() }`.
- `types.rs:Stance::parse` and `ship::Formation::parse`/`PathMode::parse`: `contains()` on lowercase strings accepts any text containing the word (`"not aggressive"` is `Aggressive`) and falls back to `Neutral` with no warning. Warn once for an unknown value (the `Script::warn` machinery exists).
- `campaign.rs:string_list` and the local `list` closure in `CampaignSave::from_json` do the same thing.
- `campaign.rs:load_from` / `from_json`: `u32::try_from(v["x"].as_u64()...)` is repeated five times; a `u32_field(v, "x")` helper.
**Behaviour-preserving?** yes
**Effort:** S each

##### B3-26 [severity: low] [category: modding] Docs: what a mission author trips on
**Where:** `docs/missions.md`, `docs/rule-types.md`.
**What (verified in code):**
- The docs are strong: the generated reference, the worked example kept valid by a test, a "claims checked against code" note. Gaps:
- There is no mention that `check_with` is the only validator and that a mission loaded with errors still starts (B3-03).
- `docs/missions.md:48` documents that a missing `media.json` stops the game: an author-hostile failure that the code could soften (B3-17).
- The `Opneing NIS 2c` typo story (a retail path name that must be preserved) is in code comments only (`briefing.rs`); say in the docs that field and path names keep retail typos so authors copy them.
- `docs/rule-types.md` says "Equal To on a measured number is rarely useful"; the epsilon (`1e-4`) is not stated.
- Nothing documents the evaluation order rule: conditions are all evaluated (so edge state advances) even after the first false one, rules are evaluated in file order, and an effect that hides a unit is visible to the next action but not to the next rule's `Is Group In Volume` in the same step. These affect how an author orders rules.
**Suggestion:** Add a short "How a step works" section to `docs/missions.md`, derived from `step` and `run_from`.
**Behaviour-preserving?** yes
**Effort:** S

#### Quick wins
- Log `check::check_with` errors at load for any mission that is not retail (B3-03 step 1); replace `expect("rules")` with a warning.
- Make `briefing.rs` fail soft (skip the opening and log) instead of `unreachable!()`/`expect`/`unwrap` (B3-07, B3-17).
- Merge the identical `cond.rs` arms and add a `Comparison` enum (B3-05, B3-12).
- Mark `6.0` (attack window), the speech reading-time formula, `60.0` camera speed and the Cloak 5 s rule with a source or PROVISIONAL (B3-18).
- Move `skip_to_tender` to `smoke/`, and the whole autopilot after it (B3-07, B3-08).
- `ship::Order` consts instead of `order = 2/3` (B3-23).
- Fix the stale `campaign.rs` header and the misplaced doc comment on `skip_speech` (B3-21, B3-25).
- `OnceLock` for `dev_console::enabled()`; drop redundant `Option<Res<Mission>>` where a `resource_exists` run condition already guards (B3-20).
- Read the sim pose (not the drawn one) in `units()` (B3-01), after confirming with the maintainers which pose the original uses.

---

## Appendix: Render profile (game/src/render/, mods/remaster, docs/modding.md "Graphics mods")

Overall: this is a well-documented, carefully gated optional layer. Every block is `Option<T>` in `Look`, registered only when its key and shader are present (`with_shaders`), and an absent `look.json` returns early in `render::plugin`, so classic is genuinely untouched. Good practices already in place: a shared frame-data storage buffer at `@binding(99)` so most materials are never rewritten, `QuadBatch` reusing mesh vectors so steady frames do not allocate, bindless-friendly "write only when changed" guards in most blocks, shaders shipped by the mod rather than the engine, `Changed`/`Added` filters, `try_insert`/`try_remove` for short-lived entities, and good tests per block. The main weaknesses are (1) heavy copy-paste across the glow/batch effect files (materials, `specialize`, the per-frame "find camera / find mesh / spawn / upload" skeleton, smoothing laws), (2) a few blocks that still write their materials every frame despite the frame-data design (nebulae, rules), (3) kit helpers that duplicate the classic `fx` ones, and (4) an untyped `serde_json::Value` parse layer that silently drops typos and a bad file. Nothing found that breaks classic fidelity, but a few systems touch every `Camera3d` rather than only styled ones.

#### Findings

##### B4-01 [severity: medium] [category: bevy] Nebula and volume materials are rewritten every frame, though the clock is frame data
**Where**: nebulae.rs:466-476 (`update`), nebulae.rs:352-367 (volume proxy update); `NebulaUniform.params.x` / `VolumeUniform.params.x` carry `time.elapsed_secs()` (nebulae.rs:296, 426); mods/remaster/shaders/nebula.wesl:96 and volume.wesl:148,182,228 read `nebula.params.x` / `vol.params.x`.
**What**: `m.extension.u = rule.uniform(.., time.elapsed_secs(), ..)` is executed for every cloud, every frame, via `materials.get_mut` (which marks the asset changed, so Bevy re-prepares the bind group and re-uploads the uniform). The same file's neighbours (beams, tracers, holes, sails) already moved the clock to `frame.clock` and document "a material whose uniform holds none of them is never written again". docs/modding.md even lists `params (clock, ...)` for `nebula.wesl` as an engine-provided value.
**Why**: it is the one block that still pays the per-frame material-prepare cost the architecture exists to avoid, and the contract tells modders to put the clock in the uniform, the opposite of every other shader.
**Suggestion**: read `frame.clock.y` in `nebula.wesl`/`volume.wesl`, drop the clock from `params`, and write the uniform only when it differs (`!=` compare, as `trails::set_length` does) or when `Changed<GlobalTransform>`/cloud fade/breath changes. Update the docs table row. Changing the uniform meaning of `params.x` changes the shader contract: mods that ship their own `nebula.wesl` need a note (maintainer should OK).
**Behaviour-preserving?**: yes visually (same clock source) if the real/battle clock distinction is kept (`elapsed_secs` of virtual `Time` is `frame.clock.y`).
**Effort**: M

##### B4-02 [severity: medium] [category: bevy] `rules::motion` writes clock, sun and ship rows into every rule material every frame
**Where**: rules.rs:258-293 (`motion`), `RuleUniform.clock`/`sun` (rules.rs:113-125).
**What**: for every asset in `made.assets` it does `m.extension.u.clock = clock; .sun = sun; .ship = ship;` through `get_mut`, even though `RuleShader` also binds the frame buffer (`frame: FrameBinding`) whose clock and sun it duplicates. One asset per (retail material, rule, ship), so a fleet is dozens of re-prepared materials per frame.
**Why**: same cost as B4-01, and `RuleUniform.clock/sun` duplicate `frame.clock`/`frame.sun`. Only `ship` (speed share, heading, seed) is genuinely per-material.
**Suggestion**: keep only `ship` in the uniform and write it when the value changed (speed share is smoothed, so compare with a small epsilon like `THRUST_STEP` in trails.rs); point modders at `frame.clock`/`frame.sun`. Better still, put per-ship rows in a storage buffer indexed like `sails::STATE`, so the material is never written. Keeping `RuleUniform.clock/sun` as deprecated, no-longer-written fields would break mod shaders that read them, so removal needs a documented migration (mod contract; ask a maintainer).
**Behaviour-preserving?**: needs-check (shaders in the wild reading `u.clock`).
**Effort**: M

##### B4-03 [severity: medium] [category: simplify] The additive `Glow` material (uniform + `AsBindGroup` + `specialize`) is copy-pasted nine times
**Where**: beams.rs:55-92, bolts.rs:78-112, tracers.rs:114-148, currents.rs:52-95, bursts.rs:273-300, holes.rs:151-195 and 247-285, nebulae.rs:148-190, trails.rs:97-160. `BeamUniform` and `BoltUniform` are field-for-field identical (`core: Vec4, params: Vec4`), and `TracerUniform` has the same shape.
**What**: each file declares `#[derive(Asset, AsBindGroup, TypePath, Clone, Default)] struct XGlow { #[uniform(100)] u, #[storage(99, read_only)] frame }` plus a `MaterialExtension` impl whose `specialize` ignores everything and calls `kit::additive`.
**Why**: ~25 lines of boilerplate per block, and `kit.rs` says "New blocks use these instead of their own copies" yet the part that is repeated most is not in kit. A change (e.g. adding a prepass opt-out) must be made nine times.
**Suggestion**: add a small `macro_rules! additive_glow { ($glow:ident, $uniform:ty, $shader:expr) => {..} }` in kit.rs that expands to the struct and the `MaterialExtension` impl (fragment shader, `kit::additive`), and a shared `CoreParams { core: Vec4, params: Vec4 }` uniform for beam/bolt/tracer (each keeps a `pub type`). Keep binding indices 99/100 unchanged so shaders and mods are unaffected. Do not use a generic `AdditiveGlow<U, const SHADER>`: `AsBindGroup` derive needs concrete types.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B4-04 [severity: medium] [category: simplify] The per-frame batch-drawing skeleton is repeated in beams, bolts, tracers, smoke trails, bursts
**Where**: beams.rs:296-343, bolts.rs:237-282, tracers.rs:192-243 and 246-290, bursts.rs:518-600.
**What**: each system does: `look.x` early return; pick the active camera with `cams.iter().filter(|(c, _)| c.is_active).min_by_key(|(c, _)| c.order)` (verbatim at beams.rs:311, bolts.rs:255, bursts.rs:537, tracers.rs:225,263); `let mut target = target;` (bolts.rs:256, tracers.rs:226); `target.iter_mut().next()` else make material, `spawn_batch_mesh`, `return` (a wasted frame); `batch.get_or_insert_with(QuadBatch::with_normals)`; `clear()`; fill; `vis.set_if_neq(batch.upload(..))`. `frame.rs:121` already has `battle_camera` but returns only a `Vec3`, so nobody uses it.
**Why**: 5 copies of a 15-line skeleton that all need to stay in step; the `Local<Option<QuadBatch>>` + `get_or_insert_with` dance exists only to dodge `Default` (QuadBatch derives `Default`, the `with_normals` flag is the only difference).
**Suggestion**: (a) make `frame::battle_camera` return `&GlobalTransform`/`Transform` and use it everywhere (or a `#[derive(SystemParam)] BattleCamera`); (b) add to kit.rs a `fn ensure_batch_mesh<M: Material>(commands, meshes, target_query_empty, make_material, marker)` or a `BatchTarget<M>` SystemParam that returns `(mesh_handle, vis)` or spawns; (c) `Local<QuadBatch>` initialised by `QuadBatch::with_normals()` via `FromWorld`/a `NormalBatch` newtype with `Default`. Drop `let mut target = target;` by declaring the parameter `mut`.
**Behaviour-preserving?**: yes (the "spawn then return" first-frame behaviour can be kept).
**Effort**: M

##### B4-05 [severity: medium] [category: simplify] `kit::additive` and `kit::additive_base` duplicate `fx::RetailAdd::specialize` and `fx::additive_material`
**Where**: kit.rs:12-45 vs game/src/fx/mod.rs:26-70 (identical ONE/ONE `BlendComponent`, identical `depth.bias.constant = 0`, identical unlit/double-sided/`AlphaMode::Blend`/`ADDITIVE_SORT_BIAS` base).
**What**: two implementations of "retail ONE/ONE additive" exist, one for classic, one for the profile; a fix to one (e.g. sort bias, which fx/mod.rs marks PROVISIONAL) would silently miss the other.
**Why**: single source of truth for the retail blend; also reduces the "profile must match classic additive look" risk.
**Suggestion**: have `fx::RetailAdd::specialize` call a shared `fn retail_add(descriptor)` and have `kit::additive` re-export/call it (or the reverse, since `fx` is shared code and `render` is optional, put the function in `fx`). Build `kit::additive_base(texture)` from `fx::additive_material(texture).base` or share a `fn additive_standard(texture)`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-06 [severity: medium] [category: simplify] Smoothing/phase laws are re-implemented in several modules and can drift
**Where**: smoothed share of full speed `(sh.speed.abs() / full_speed(..)).clamp(0,1)` with `+= (share - wind) * (dt * 1.2).min(1.0)` at sails.rs:822-825 and rules.rs:268-272; thrust smoothing `(target - t) * (dt * 2.5).min(1.0)` at trails.rs:289 and lights.rs:~170 (the light's comment says "the plume's rule (render/trails.rs)"); per-entity phase `(e.index().index() as f32 * 0.618).fract()` at sails.rs:294, rules.rs:285, trails.rs:248; `e.index().index() % 64` seeds at tracers.rs:191, bursts.rs:~582.
**What**: the same state machines exist twice, each with its own copy of the tuning constant, held in different places (`Sail.wind`, `Made.wind`, `Plume.thrust`, `EffectGlow.thrust`).
**Why**: the plume and the engine's light must agree (lights.rs says so) but only by hand; `rules`' "ship wind" and the sail's `wind` should be the same number for one ship, a mod author reading `RuleUniform.ship.x` expects it to match the sails.
**Suggestion**: introduce one `ShipFlow` component (or a `Resource<HashMap<Entity, f32>>` keyed by ship) in render/ updated once per frame with named consts `WIND_RESPONSE = 1.2`, `THRUST_RESPONSE = 2.5` and a `fn phase(e: Entity) -> f32`; sails, rules, plumes, lights read it. The constants are visual tuning: mark them PROVISIONAL (not retail).
**Behaviour-preserving?**: yes if constants are kept.
**Effort**: M

##### B4-07 [severity: medium] [category: modding] `look.json` parsing is untyped, silent on typos, and a syntax error silently disables the whole profile
**Where**: mod.rs:154-255 (`Look::parse` over `serde_json::Value` with `num(&v[..], default)`), mod.rs:302 (`crate::data::try_json(&root.join(PROFILE)) else { return }`), docs/modding.md Troubleshooting ("a JSON file seems ignored, with no message").
**What**: every block parses with `v["key"]` indexing plus inline defaults; an unknown or misspelled key (e.g. `"bloom_treshold"`), a wrong type (`"bloom": "0.3"` falls back to the default) or a syntax error in the file produce no message. Also seen as clamps with no warning (`beams::parse` `.clamp(1.5, 30.0)`).
**Why**: this is the main surface a modder uses; most graphics-mod debugging time will go to "why did nothing change". The project's own troubleshooting table admits the gap.
**Suggestion**: (1) cheapest and self-contained: in `plugin`, if the file exists but `try_json` returns `None`, `warn!("render profile: {PROFILE} is not valid JSON; classic look kept")`. (2) For typos: after parsing, walk the top-level and each block's keys against a per-block `const KEYS: &[&str]` and `warn!` on unknown ones (no format change). (3) Longer term, `#[derive(Deserialize)] #[serde(default)]` structs per block (serde is already a dependency; no new crate), with `deny_unknown_fields` replaced by a warning. Defaults live in `impl Default`, which also lets docs/modding.md and tests share them (see B4-14).
**Behaviour-preserving?**: (1)(2) yes; (3) needs-check for lenient behaviour (a wrong-typed value currently falls back silently).
**Effort**: S for (1), M for (2), L for (3)

##### B4-08 [severity: medium] [category: modding] `look.json` is one replace-whole file; the only per-mod composition is two special merged side files
**Where**: mod.rs:60-95 (`add_mod_skies`, `add_mod_nebulae`), docs/modding.md lines 124-126, 140-141.
**What**: two mods cannot both tune the render profile, so a "bloom only" mod and a "sky" mod are mutually exclusive, and the engine grew `skies.json`/`nebulae.json` special merge hooks with hand-written merge logic (`add_mod_nebulae` has bespoke rule-ordering code, mod.rs:83-93) to work around it.
**Why**: moddability; each new block that content mods want to extend needs another hook.
**Suggestion**: consider making `data/render/look.json` a deep-merged table like `guns.json` (objects merge key by key, arrays replace) so a second mod can override `camera.bloom` or add `lights`. This changes a documented data format/merge rule, so it needs maintainer approval (`AGENTS.md`: "changing data formats"). Independently, `add_mod_nebulae` (rules prepended) could be expressed as "arrays named `rules` concatenate mod-first" in one generic place.
**Behaviour-preserving?**: needs-check (a profile that relied on "last file wins" would now merge).
**Effort**: M

##### B4-09 [severity: medium] [category: bevy] Per-frame `Vec` allocation and full scan in `fx_gain`, and one-shot logic polled every frame
**Where**: materials.rs:15-22 (`fx_gain`), materials.rs:27-38 (`shell_glow`), holes.rs:355-370 (`ribbon_gain` is fine, uses `Added`).
**What**: `fx_gain` iterates every `EffectMaterial` each frame, collects stale ids into a `Vec`, then `get_mut(id).unwrap()` each. It does this even at the default `fx_gain` 1.0 and forever after all materials were set. `shell_glow` runs every frame only to check a `Local<bool>`.
**Why**: avoidable work and an `unwrap` on an id that is almost certainly present but nothing proves it; `AGENTS.md`-style cleanliness (no `unwrap` in non-test code).
**Suggestion**: apply the gain when the material is created: read `Look` in `fx::additive_material`'s call site, or react to `AssetEvent::<EffectMaterial>::Added` (`MessageReader`), or run `fx_gain` only `.run_if(|m: Res<Assets<EffectMaterial>>| m.is_changed())` with an early `if look.fx_gain == 1.0 { return }`. Replace `shell_glow` polling with `.run_if(resource_added::<crate::combat::Look>)`. Use `if let Some(mut m) = materials.get_mut(id)`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-10 [severity: low] [category: rust] Two types named `Look` in one system signature
**Where**: render/mod.rs:106 (`pub struct Look`, the render profile) vs `crate::combat::Look` (shell meshes), both referenced in materials.rs:27 (`look: Res<Look>, shells: Option<Res<crate::combat::Look>>`); smoke/fleet.rs:43 reads `crate::render::Look`.
**Why**: confusing for new contributors; the render one is a 40-field config god-struct called "look" while `data/render/look.json` is its file.
**Suggestion**: rename the render resource `RenderProfile` (file stays `look.json`, so no mod impact) and/or the combat one `ShellLook`. Pure rename.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-11 [severity: medium] [category: simplify] `Look::with_shaders` is 14 hand-written near-identical `if` blocks, and each block repeats its gating three times
**Where**: mod.rs:242-298; the same `Option` is then re-checked in `plugin` (`if look.beams.is_some()`, e.g. beams.rs:91, bolts.rs:114, tracers, currents...) and again inside every system (`let Some(b) = &look.beams else { return };`).
**What**: gate 1 shader exists, gate 2 `plugin` registers conditionally, gate 3 every system re-unwraps `Res<Look>` field. The `look` resource is a 40-field `Clone` struct passed around by reference.
**Why**: adding a block means touching `Look`, `parse`, `with_shaders`, `plugin`, docs; the unwrap inside every system is noise and each `Res<Look>` change detection trips on any field.
**Suggestion**: (a) a helper `fn gated<T>(block: &mut Option<T>, root, name, shader) ` collapses `with_shaders` to a list of calls (bursts' two shaders: pass a slice). (b) Insert each block's config as its own resource (`app.insert_resource(beams_cfg)`) in its `plugin`, and use `.run_if(resource_exists::<Beams>)`; systems take `Res<Beams>` directly (no `Option` dance), and `look.fx_gain` style cross-block reads stay on `Look`. (b) is the larger refactor but also removes `Look`'s 20 `Option<..>` fields and makes the "block plugin" a real `impl Plugin`.
**Behaviour-preserving?**: yes.
**Effort**: M (a) S

##### B4-12 [severity: low] [category: fidelity-risk] Camera-wide systems ignore the `Styled` opt-out used by the Ships-screen preview
**Where**: bursts.rs:612-630 (`shake`/`unshake` move every `Camera3d` `Transform`), bursts.rs:633-650 (`aberration` inserts `ChromaticAberration` on every `Camera3d`), versus mod.rs `camera`/`anti_alias`, which honour `Styled` and skip image render targets.
**What**: with a profile that sets `aberration` or `bursts`, the off-screen preview camera (classic/ship_preview.rs marks it `Styled` precisely to stay plain) still gets colour fringing, and jolts from battle explosions would apply to any other camera (map, preview) if one exists while particles are alive.
**Why**: the profile must not alter non-battle presentation; today only the explicit opt-out is respected by two of the four camera systems.
**Suggestion**: reuse one query filter for "the battle camera" (the same helper as B4-04): active, `RenderTarget` not `Image`, and not a camera whose `Styled` was inserted by the preview. A `BattleCamera` marker component inserted in `camera()` is the simplest.
**Behaviour-preserving?**: yes for battles; only fixes the preview case.
**Effort**: S

##### B4-13 [severity: low] [category: rust] Magic numbers without name, unit or source (visual tuning)
**Where** (examples): mod.rs `threshold_softness: 0.4`, `num_cascades: 4`, `first_cascade_far_bound: shadow_distance / 20.0`; bursts.rs `ambient.color * 0.35` (smoke shade), shake `37.0/23.0/31.0/29.0`, `* 0.35`, `* 4.0` decay, motes `half * 0.08`, `% 10007`; sails.rs `0.35 + 0.65 * wind`, `dt / 1.5`, `BELLY_MAX = 0.65 + 0.25 + 0.12` (named but unexplained); trails `dt * 2.5`, `p.thrust < 0.02`; lights.rs flicker `13.0, 7.3, 1.1, 23.0, 2.0, 0.7/0.3`; beams.rs `7.0 + 7.0 * heat`, `1.8 + 1.6`, `0.4`, hit-sparks `3.0 + 30.0 * x`; holes `FLASH_SECONDS`; deckhands `time * 10.0 & 0xffff`.
**Why**: AGENTS.md "no magic floats; mark guesses PROVISIONAL". Only one `PROVISIONAL` exists in render/ (sails.rs:827). These are intentionally not retail, but a contributor cannot tell "taste" from "matches footage".
**Suggestion**: for tuning that is plausibly modder-facing (smoke shade 0.35, shake amplitude/decay, flicker, beam flare/spark scale, fold time 1.5 s, wind and thrust response) either name them as `const`s with one-line "taste, not retail" doc comments, or add them to the relevant block's JSON with these as defaults (defaults keep behaviour). Add one line to the module header: "constants in this module are presentation taste, not original rules" instead of per-number PROVISIONAL.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B4-14 [severity: low] [category: modding] Defaults live in code and in docs separately; several uniform fields are "unused"
**Where**: defaults at parse sites (beams.rs:46-56, trails.rs:68-95, camera `bloom` 0.15, `shadow_distance` 2000, ...); docs/modding.md block table; `BurstUniform.params` "unused" (docs line for `burst.wesl`), `TracerUniform.params` "unused", `PlumeUniform.params.z` unused, `Bolt/Beam params` only `.x`.
**What**: the numeric defaults a modder gets by writing `{}` are discoverable only by reading Rust; reserved/unused uniform vec4s cost bytes and give modders nothing.
**Suggestion**: generate (or at least test) the docs defaults from a `Default` impl per block; drop unused fields at the next contract bump. Removing uniform fields changes the shader contract (ask a maintainer first).
**Behaviour-preserving?**: yes for defaults, needs-check for field removal.
**Effort**: S-M

##### B4-15 [severity: low] [category: modding] `camera` block is "on" with non-neutral defaults the moment `look.json` exists
**Where**: mod.rs:182-198 (`bloom: num(.., 0.15)`, `Tonemapping::TonyMcMapface`, HDR + deband always inserted in `camera`, mod.rs:~345); docs/modding.md says "start from an empty `data/render/look.json`".
**What**: every other block is off unless its key is present, but an empty profile still switches the camera to HDR + Tony McMapface tonemapping + 0.15 bloom + dither, so "empty file = classic" does not hold; the start-from-empty instruction yields a different image.
**Suggestion**: either document it ("a profile always enables the HDR camera pipeline") or make the camera block Option-like (`camera` key absent = leave the camera alone). Since classic fidelity is the baseline, the second is safer, but changes what an existing mod file does when it has no `camera` key: ask a maintainer.
**Behaviour-preserving?**: needs-check.
**Effort**: S

##### B4-16 [severity: low] [category: rust] `deckhands::load_kit` unwraps and indexes mod-supplied JSON after a validation done elsewhere
**Where**: deckhands.rs:217 `head["part"].as_array().unwrap()`, :233 `head["clips"].as_array().unwrap()`, `u.clips[i]` (:236), `bin[i * 8 + k * 2]` inside `texel` (:211), after `read_kit` (:185-197) validated them.
**What**: correctness depends on `read_kit` being called first; if `CLIPS`/array size (the `u.clips` array length) and the JSON diverge, this is a panic from a mod file. `head["verts"].as_u64().unwrap_or(0)` then arithmetic `verts * frames * 16` could overflow on a hostile header (debug panic).
**Suggestion**: deserialize the header into a typed `struct DeckhandHeader { verts, frames, fps, part: Vec<f32>, clips: Vec<ClipHeader>, bounds, speeds }` in `read_kit` (returning `Option<Kit data>` with `checked_mul`), so `load_kit` receives validated data and has no `unwrap`. Also `f16_to_f32` re-implements `half`-style conversion; Bevy depends on `half`, so check whether it is already a direct dependency before using it (new dependency = maintainer approval).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-17 [severity: low] [category: simplify] Long files and systems that mix simulation with drawing
**Where**: sails.rs (836 lines: parsing, mesh subdivision, fold data, state buffer, tests-module split), sky.rs (808), deckhands.rs (808), bursts.rs `draw` (bursts.rs:518-600: retains particle list, builds meshes, uploads, writes smoke shade, decays `shake`), bursts `start` + `emitters` + `scorch`.
**What**: `bursts::draw` also does particle expiry (`parts.list.retain`, line ~535) and the camera-shake decay (`parts.shake *= ...`, ~598), i.e. simulation inside a drawing system, with ordering implied only by `.chain()`.
**Suggestion**: move expiry and shake decay into a `step` system placed first in the chain; keep `draw` pure. In sails.rs split `subdivide`/`fold_data`/`uv_world_scale` (pure mesh math, already testable) into `sails/cut.rs`; in sky.rs the cube-image handling (`as_cube`, stars/clouds file lookup, lines ~430-600) into `sky/cube.rs`. Behaviour identical; makes each file cohesive and the pure parts reusable by a mod-tool.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B4-18 [severity: low] [category: rust] `#[allow(clippy::too_many_arguments)]` without reason, repeated
**Where**: beams.rs:296, bolts.rs:237, bursts.rs:351,461,518, deckhands.rs:448,565, frame.rs:125, rules.rs:224, sky.rs:528, tracers.rs:192,246.
**What**: bare `allow` (checklist 5.6 prefers `#[expect(.., reason = "..")]`). Several can disappear with a `SystemParam`: the `(cams, target, meshes, materials, batch)` quintet (B4-04), `(ships, lib)` pairs.
**Suggestion**: introduce `#[derive(SystemParam)] struct BatchTarget<'w, 's, M: Material> { .. }` and `BattleCamera`, then delete most of the `allow`s; convert the rest to `#[expect(clippy::too_many_arguments, reason = "...")]`.
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B4-19 [severity: low] [category: bevy] Material-per-engine for thrust, written whenever it moves
**Where**: trails.rs:244-262 (one `PlumeMaterial` per engine per layer), trails.rs:280-300 (`plume_motion` writes `m.extension.u.params.y = p.thrust` when it moves by `THRUST_STEP`).
**What**: plumes break batching (one bind group per plume layer) and re-prepare on every thrust step, the exact pattern sails.rs already solved with the shared `STATE` buffer (docs: "Bevy prepares a changed material again, which is slow for many sails every frame").
**Suggestion**: reuse the sail-state approach: one storage buffer of per-plume rows (thrust, seed), one material per layer per race colour; the shader indexes by row. Keep `THRUST_STEP` unused. This changes the plume uniform/binding contract documented in modding.md (maintainer approval).
**Behaviour-preserving?**: yes visually.
**Effort**: M

##### B4-20 [severity: low] [category: fidelity-risk] Classic code depends on the profile module
**Where**: fx/draw.rs:12 `use crate::render::batch::QuadBatch` (classic effects), flag.rs:154 `crate::render::sails::sail_of`, smoke/fleet.rs:43 `crate::render::Look`, view/classic `crate::render::Styled`; batch.rs doc says "Not profile-only ... classic uses it too".
**What**: shared, always-on code lives under the module whose header says "Not retail: ... Absent, everything renders as classic". A contributor editing `render/batch.rs` for a profile need changes classic drawing.
**Suggestion**: move `batch.rs` (and the `put!` macro) to `fx/` or a top-level `quadbatch.rs`; keep the sails cutter helper that flag.rs needs outside `render::sails` too. Pure move; update `docs/development.md` ownership table.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-21 [severity: low] [category: bevy] `rules::specialize` keys on Bevy's internal pipeline label and a global `static Mutex`
**Where**: rules.rs:~145-185 (`PREPASS: &str = "prepass_pipeline"`, `descriptor.label.as_deref() != Some(PREPASS)`; `static SHADERS: Mutex<Vec<(String, Handle<Shader>)>>`).
**What**: correctness relies on a label string that is an implementation detail of Bevy 0.20's prepass pipeline (a rename silently makes the rule shader replace the prepass fragment stage, which would produce wrong/black shadows or a pipeline error), and a process-global holds shader handles.
**Suggestion**: add a unit/smoke test that fails if Bevy's prepass label changes (assert the constant against `bevy::pbr` if it is exported, else a `--script` smoke); prefer the `MaterialExtension::fragment_shader`/`MaterialExtensionKey` path that distinguishes passes if 0.20 offers one. Keep the static but document that it is write-once per path. A mod cannot reach this; it is a maintenance trap.
**Behaviour-preserving?**: yes.
**Effort**: S (test)

##### B4-22 [severity: low] [category: simplify] `parse_*` / `names` / `num` helpers are partial re-implementations of `serde`
**Where**: mod.rs:143-170 (`num`, `rgb`, `names`) used 150+ times across blocks, e.g. `rgb()` silently takes `c.len() >= 3` after `filter_map` (a `[1, "x", 2, 3]` becomes `[1, 2, 3]`), `num(&v["motes"], 18.0).clamp(0.0, 64.0) as usize` (beams.rs:51), `as_f64 as f32` on file values.
**Why**: `as` casts on mod-file numbers (checklist 5.6): negative or NaN values saturate to 0 silently; `rgb` accepts malformed arrays.
**Suggestion**: part of B4-07 (3). Short term: make `rgb` require exactly 3 numbers and `warn!` on rejection; add a `num_clamped(v, default, lo, hi)` helper so the clamp range is next to the default and a "clamped" warning can be emitted once.
**Behaviour-preserving?**: needs-check (malformed arrays currently half-accepted).
**Effort**: S

##### B4-23 [severity: low] [category: bevy] `bolts::draw` ages with `Time<Real>` in `Update` and mixes simulation into draw
**Where**: bolts.rs:241-253 (`time: Res<Time<Real>>`; `bolt.age += dt; live.bolts.retain(..)` inside `draw`).
**What**: everything else in the profile uses `Time` (virtual), `frame.rs` carries both clocks. Bolt life uses real time, so on pause the bolt keeps fading while bursts/beams freeze; the shader's glow uses `bolt.glow(b.seconds)` from CPU age. Not wrong (cosmetic) but inconsistent with `frame.clock.x/y` semantics (real vs battle) the shaders document.
**Suggestion**: state in the doc comment which clock a bolt follows and why; split `age/retain` into the `strike`-chained system. `sky.rs:510` also takes `Time<Real>`; same note.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-24 [severity: low] [category: modding] Hard-wired structure a modder plausibly wants to change
**Where**: rules.rs `roughness.clamp(0.02, 1.0)`; mod.rs `Tonemapping` list limited to 5 names (others, e.g. `tony_mc_mapface`, `none`, `somewhat_boring_display_transform`, unreachable except as fallback); `bloom.prefilter.threshold_softness: 0.4`; `CascadeShadowConfigBuilder { num_cascades: 4, .. }`; `Smaa` preset fixed High; `SKY_LIGHT_FRAMES = 30`; `lit_smoke` sets roughness 1 / reflectance 0 (materials.rs:70-74); `PROFILE`/`SKIES`/`NEBULAE` paths pub consts.
**Suggestion**: expose `camera.bloom_softness`, `sun.cascades`, a full tonemapping name list (use a `match` over all `Tonemapping` variants or a `FromStr`), and make unknown tonemapping names `warn!`. All additive keys with the current values as defaults.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B4-25 [severity: low] [category: rust] Small clean-ups
**Where**: 
- mod.rs:84-90: `extra.get_mut("rules").map(Value::take).and_then(|r| r.as_array().cloned())` clones, then `extra.as_object_mut().map(|o| o.remove("rules"))` (result discarded); use `extra.remove("rules")` on the map you already hold as `Map`.
- mod.rs:158-162 `names(..)` `default.iter().map(|s| s.to_string())` fine, but `hide_effects: names(.., &[]).into_iter().filter(|n| !n.is_empty())`: move the empty filter into `names` (rules.rs/`skip` also use it and would drop empty matches that match everything: `find` uses `n.contains("")`, an empty `skip` fragment disables every rule).
- mod.rs `anti_alias` takes `u8` 0/1/2 (`crate::menu::anti_alias()`), use an enum `AntiAlias { Off, Smaa, Msaa4 }` in menu/settings.rs (checklist 5.3).
- lights.rs test module imports `RunSystemOnce` twice (lights.rs:~137 and the module head); kit.rs test imports `RunSystemOnce` after use at the bottom of `mod tests`.
- replace.rs:21 `["BlackHoleRibbons_Small", "Lightning"].iter().any(|n| replaced(look, n))`: a hard-coded probe list that must be kept in step with `replaced`; return a `fn replaces_anything(&Look) -> bool` next to `replaced`.
- bursts.rs:574,576 `.single_mut().expect("one glow mesh")` panics if both meshes exist more than once (two battles overlapping on a reload frame); use `let Ok(..) = .. else { return }`.
- `pub` visibility: most items (`pub struct XUniform`, `pub fn parse`) live in private modules (`mod beams;`), so they are effectively crate-private; use `pub(super)`/`pub(crate)` to make the real API (`Look`, `Styled`, `sails::sail_of`, `batch`) visible.
**Behaviour-preserving?**: yes, except the empty-fragment filter (needs-check).
**Effort**: S

#### Quick wins
- B4-07(1): add a `warn!` in `render::plugin` when `data/render/look.json` exists but does not parse (log only, zero behaviour change).
- B4-05: make `kit::additive`/`additive_base` call the `fx` versions (or the reverse) so there is one ONE/ONE implementation.
- B4-01: move the nebula and volume clock to `frame.clock` and stop rewriting those materials each frame.
- B4-09: replace the per-frame `Vec` + `unwrap` in `fx_gain` and the polling `shell_glow` with `Added`/`run_if`.
- B4-10: rename `render::Look` to `RenderProfile` (pure rename, file name unchanged).
- B4-12: apply the `Styled`/non-image-target filter to `shake` and `aberration`.
- B4-04: use `frame::battle_camera` (made to return the transform) in the five copies of the camera pick; drop `let mut target = target;`.
- B4-18: replace bare `#[allow(clippy::too_many_arguments)]` with `#[expect(..., reason = ...)]` or a SystemParam.
- B4-24: warn on unknown `camera.tonemapping` names and accept the full variant list.

---

## Appendix: Effects, world presentation, environment sim, audio (B5)

Files read in full: `game/src/fx/{mod,lib,sim,draw}.rs`, `environment.rs`, `nebula.rs`, `blackhole.rs`, `wake.rs`, `decals.rs`, `clips.rs`, `cloak.rs`, `flag.rs`, `dragons.rs`, `audio.rs`, `crew.rs`, `retail_tex.rs`, most of `shipart.rs`, plus `content/voice/events.json` structure and the matching `docs/modding.md` / `docs/re/sound.md` sections. Test modules were only skimmed.

**Overall.** The code is in good shape for a faithful-recreation project. Sim state is mostly separated from drawing (dragons, `Sensors`, `Caught`/`Pull`, `Cloak` are pure data steps with unit tests). Constants are usually named, sourced and marked PROVISIONAL, and the retail quirks are documented where they are applied (the cloak dive, the 15-tick segment cadence, the `lcg_unit` generator). The mesh-write guards are thoughtful: `set_ribbons_if_changed`, the flag `REDRAW`/`ViewVisibility` skip, the render-fx key check. The modding story for sounds and effects is real and documented: sounds are found by name anywhere under `sound/`, effects are picked up by file name, and `wake_tint.json` is a good example of a data hook.

The main weaknesses are these:
- Four copies of the "accumulate dt, run 30 Hz ticks" loop.
- Wake and black-hole trail code that is copy-pasted.
- Several sim systems that read raw `serde_json::Value` every tick, with `unwrap`/index panics on mod data.
- Sim-affecting RNG that is not reset per battle and is consumed in query order (nebula storms, cloak spotting).
- A `Single<…With<Camera3d>>` pattern that is fragile once the Fleet-screen preview camera exists.
- Sound and effect tuning numbers (distances, volumes) that are scattered literals, not all marked PROVISIONAL, and that a modder cannot change.

#### Findings

##### B5-01 [severity: medium] [category: bevy] `Single<&GlobalTransform, With<Camera3d>>` breaks silently when a second camera exists
**Where:** `audio.rs:59` (travel), `:98` (ambience), `:221` (play), `:240` (engines); `fx/draw.rs:69` (render_fx); `nebula.rs:658`; `view.rs:514`. The second camera is spawned in `classic/ship_preview.rs:59` (`Camera3d::default()` in `spawn_rig`, `OnEnter(Screen::Fleet)`). The main camera is spawned unmarked at `lib.rs:341`.
**What:** While the Fleet screen is open there are two `Camera3d` entities. Every `Single<…, With<Camera3d>>` system then fails validation and is skipped silently. For `audio::play` that means no UI/Sfx messages are turned into sounds on that screen. For `render_fx` and nebula `update` it means no particle or cloud mesh updates, and `view::camera` is skipped too. Whether the Fleet screen currently has a live battle behind it is unclear, so I could not confirm an audible symptom.
**Why:** A silent skip is the worst failure mode: no warning, no panic. It also means a render-profile or mod camera, or a future second window or minimap camera, silences audio and effects. The fix is trivial and should not depend on the preview camera being despawned in time.
**Suggestion:** Add a `#[derive(Component)] pub struct MainCamera;` on the camera at `lib.rs:341` and query `With<MainCamera>` everywhere. Add `Without<RenderLayers>` or the marker to the preview rig if a marker is not wanted.
**Behaviour-preserving?** needs-check (it fixes a skip, so it can only add behaviour on the Fleet screen). **Effort:** S

##### B5-02 [severity: medium] [category: bevy] `wake`, `blackhole` have no Plugin: systems are wired from `lib.rs`
**Where:** `lib.rs:301` (`blackhole::pull` inside a sim chain) and `lib.rs:320-321` (`wake::update`, `blackhole::update` inside the big `Update` chain with `camera`, `hud`, `leave_battle`). Compare `environment::plugin`, `nebula::plugin`, `cloak::plugin`, `dragons::plugin`.
**What:** Two feature modules are registered by the app root, and their cosmetic updates sit in a long `.chain()` with unrelated input and HUD systems, so they inherit orderings they do not need. `docs/development.md` says "do not put their tuning in `lib.rs`"; this is the same smell for wiring.
**Why:** It is the one place a mod or alternate-presentation crate cannot drop or reorder them, and a reader of `blackhole.rs` cannot see when `update`/`pull` run. `blackhole::pull` (sim) is also unordered against `dragons::fly` (both `BattleSet::Simulate`, both write `Transform`, disjoint entities only by convention).
**Suggestion:** Give `wake.rs` and `blackhole.rs` a `pub fn plugin(app)`. Put `pull` in `BattleSet::Forces`, which it conceptually is ("environmental forces"), or order it explicitly. Put `update` in `Update` or `PostUpdate` with `run_if(in_state(Screen::Battle))`. Keep the `overlay_closed` condition in one shared helper.
**Behaviour-preserving?** needs-check (moving `pull` between sets changes tick order; keep `Simulate` and add `.after/.before` to freeze today's order). **Effort:** S

##### B5-03 [severity: low] [category: fidelity-risk] `run_if(overlay_closed)` is applied to some sim systems and not others
**Where:** `lib.rs:303-304`, `docking.rs:246-263` use `.run_if(menu::overlay_closed)`. `environment.rs:474-475`, `nebula.rs:648`, `dragons.rs:52-53`, `cloak.rs:135` use only `in_state(Screen::Battle)`.
**What:** If the in-game menu always pauses `Time<Virtual>`, the extra condition is redundant everywhere and noise. If it does not (menu open while a tick is still catching up, or Fixed time not paused), whales, storms, dragons and cloaks would advance under an open menu while ships do not.
**Why:** Inconsistency in a gate that exists to keep the sim frozen is a latent desync. I did not verify the pause semantics.
**Suggestion:** Decide one rule: either every `BattleSet` system gets the gate once via `configure_sets(FixedUpdate, BattleSet::*.run_if(...))`, or none do and virtual pause is the single mechanism. Document it in "Battle clock and scheduling".
**Behaviour-preserving?** needs-check. **Effort:** S

##### B5-04 [severity: medium] [category: simplify] Four copies of the "accumulate frame dt, run 30 Hz ticks" loop
**Where:** `fx/sim.rs:46-60` (frame clamp, `tick_acc`, `while tick_acc >= TICK`), `nebula.rs:669-674` (`c.acc += dt.min(0.25)`), `blackhole.rs:~335` (`h.acc += time.delta_secs().min(0.25)`), `wake.rs:173,209-215` (`dt.min(0.25)`, `w.acc`).
**What:** Each owns `acc: f32`, the literal `0.25` frame clamp, and the `while acc >= TICK { acc -= TICK; … }` loop. `fx` also tracks the remainder for interpolation. Wake additionally has a second gate (`since_refresh > REFRESH`) driven by the clamped frame dt.
**Why:** A bug or a retuning of the clamp (for example the 5x/16x cap discussed in "Battle clock") has to be fixed in four places, and the magic `0.25` is unnamed. Removing the duplication also makes the tick-vs-frame-rate relationship readable (`docs/re/tick.md`).
**Suggestion:** Add `struct TickClock { acc: f32 }` with `fn drain(&mut self, dt: f32) -> u32` (clamps to a named `MAX_PRESENTATION_DT` const and returns the whole ticks owed) and `fn remainder(&self)`. Use it in all four. Per-tick RNG consumption stays identical because the loop count is the same.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-05 [severity: medium] [category: simplify] Wake and black-hole trails are copy-pasted
**Where:** `wake.rs:11-19,49-56,80-123` versus `blackhole.rs:15-16,107-114,141-143,162-200,187-194,330-375`.
**What:**
- Identical `Seg {a,b,out,back,life,alpha}`, `SEG_LIFE = 300`, `SEG_EVERY = 15`, and a `jitter()` with the same ±0.07/±0.02 numbers (`blackhole.rs:141-143`, `wake.rs:82`).
- The same alpha ramp: `wake.rs:116` uses the named `FADE_IN`/`FADE_OUT`; `blackhole.rs:193` repeats it as `1.0 / 60.0`, `1.0 / 240.0` and `< 60`.
- The same ribbon-mesh builder: `blackhole.rs:~350-370` versus `wake.rs:221-234`, both ending in `set_ribbons_if_changed`.
**Why:** The wake's constants are sourced and named, the black hole's are not, so they can drift; the retail rule ("Trail segment init and update") is the same one. One module would document it once.
**Suggestion:** Extract `trail.rs` (`Seg`, `step_segments(segs, rng)`, `ribbon(segs, uv_step, scroll) -> (pos, uv, col, idx)`) and have both feature modules own only their emitter. Use the named consts in both.
**Behaviour-preserving?** yes (keep the RNG call order: `jitter` draws two per segment per tick in both). **Effort:** M

##### B5-06 [severity: high] [category: fidelity-risk] Sim RNG that affects damage/visibility is unlogged, not reset per battle, and consumed in query order
**Where:**
- `nebula.rs:344-356` (`storms`): `mut seed: Local<u32>` starts at 0 and is never reset, because only `timers.clear()` happens on `map.is_changed()`. `rnd()` is called inside `for … in &mut ships` at `:389-390` (meteor roll and section pick, per ship, in `Query` iteration order) and again for lightning at `:365,404-405,420,436-438`.
- `cloak.rs:178,184` (`spotted`): `Local<Option<combat::Rng>>`, default seed, never reseeded.
- `crew.rs:223` has the same pattern but is cosmetic.
**What:** Storm damage (meteor hits, lightning strikes) and who spots a cloaked sub are simulation outcomes. They draw from private generators, not the seeded combat `Rng` that the trace/parity workflow logs. The stream continues across restarts in one process, so "restart" does not reproduce a battle. Per-ship rolls depend on archetype iteration order (checklist 4.3).
**Why:** AGENTS.md says whole-battle comparison is how unknown rules are found. A storm battle that is not replayable from its seed produces false differences. It is also the one place the sim consumes RNG in an order Bevy does not guarantee.
**Suggestion:** Move the storm and spotting draws onto a dedicated, seeded `Resource` (re-initialised in `scenario::spawn` like `StormShift`), logged under `PROCYON_TRACE`. Iterate ships in a stable order (sort by `ship.id` or spawn index) when rolling per ship. Do not change the draw sequence otherwise.
**Behaviour-preserving?** no for ordering (it makes it deterministic); the first draws of a fresh process stay the same if the seed is kept. **Effort:** M

##### B5-07 [severity: medium] [category: bevy] Per-tick and per-frame allocations in the sight snapshot
**Where:** `environment.rs:143-222` (`refresh_sensors`), scheduled twice: `PreUpdate` (`:468`) and `FixedUpdate` (`:474`).
**What:** Every refresh clears the map and re-inserts every object, allocating `group: String` (`fleet.map(..).into()` at `:183`, `.into()` at `:214`) and cloning the cloak `knows: Vec<Side>` (`:185`) for each ship and base, twice per frame at 30+ fps. `advance_sightings` also builds a `Vec` and a `HashSet` each frame (`:107-108`), and `in_sight` scans all objects for each queried pair (`:60-68`).
**Why:** It is the hottest shared path (HUD, radar, labels, picking, crew callouts all query it) and it allocates per object per refresh.
**Suggestion:** Intern groups (`Arc<str>` or a `GroupId(u32)` held by `FleetMember`/`MissionBase`) and store `knows` as a bitset. Keep the `HashMap` and reuse its capacity (it is already cleared, not recreated). Optionally drop the `PreUpdate` refresh and keep only the tick one plus a dirty flag if frame input is the only reason for it.
**Behaviour-preserving?** yes. **Effort:** M

##### B5-08 [severity: medium] [category: bevy] `Sensors::advance_sightings` and `player_sees` use wall-clock time for something gameplay-adjacent
**Where:** `environment.rs:21-24,134-136` (`Time<Real>`, `SIGHT_RETEST = 1.0`, fade 0.8/s), consumed by `docking.rs:876` (click-to-dock pick), `view.rs:221,528,609` (picking, focus), `panels.rs:479`, `crew.rs:251,302`.
**What:** What counts as "seen" changes with real elapsed time, not game time. It runs on while paused or at 16x, so a 16x battle re-tests every 16 game seconds. At 1x the re-test is wall-time-1 s ≈ 30 ticks. The `docs/re/visibility.md` §5 rule is written in seconds; this function has no fixed-tick form. The dock-click and target-pick code reads it, so the player's inputs depend on frame timing.
**Why:** Comment says "real seconds" on purpose (originals' wall clock?), but any consumer that feeds the sim (dock orders, crew callouts that mute/unmute per frame) is not tick-deterministic, and automated parity runs at different speeds differ.
**Suggestion:** Confirm in `visibility.md` whether the original used wall or game time. If game time, drive `advance_sightings` from `Time<Virtual>` on the tick; if wall time is faithful, keep it but mark the consumers that feed the sim, and note it in the module doc.
**Behaviour-preserving?** needs-check. **Effort:** S

##### B5-09 [severity: medium] [category: simplify] Sim systems walk raw `serde_json::Value` every tick/frame (and panic on bad mod data)
**Where:**
- `environment.rs:651-657` (`can_see`, called from `in_sight` for every group member × target) and `:623-646` (whale `update` reads `def["sounds"]`, `sound_interval`, `sound_distance` per call).
- `nebula.rs:357-441` (`storms`: `n["storm"]["lightning"] == true`, `storm["points"][0]["magnitude"]`, `n["polygon"]` per nebula per tick), `:622-640` (`slowdown`: per ship × nebula × tick), `:243` (`storm_circles`).
- `crew.rs:230-238` (every frame: volume lookup by name for each nebula).
- `dragons.rs:146-151,173-182` (`def["physics"]["MaxSpeed"]`, `def["gun"]`…).
- Panicking spots on mod data: `environment.rs:636` `sounds[..].as_str().unwrap()`, `:489,501-502` `expect("animal group"/"whale scene")`; `nebula.rs:385,393,419` `damage.0["wind"].clone()` (index panic if a mod's `data/nebula.json` lacks a key, and a clone of the damage table per strike), `:573` `expect("nebula weather damage tables")`.
**What:** String-keyed JSON navigation in hot loops is slow, silently defaults on typos (`unwrap_or(300.0)` etc.), and panics on the wrong type. `Storms(pub Value)` is the raw map JSON kept as a resource.
**Why:** It hides the schema (nothing says which keys the code needs, or their units/defaults, except by reading the code), and a modder's map with one wrong key crashes or silently changes behaviour.
**Suggestion:** Parse once at spawn into typed structs (`NebulaSetup { name, polygon: Region, cloud, storm, meteors }`, `DragonDef`, `WhaleDef`, `WeatherDamage { wind, meteor, lightning: DamageProfile }`) with `#[serde(default)]` for optional keys, and keep the JSON layout unchanged. Return/log errors instead of `expect`.
**Behaviour-preserving?** yes (same data layout, same defaults if copied). **Effort:** M

##### B5-10 [severity: medium] [category: bevy] `nebula::update` rebuilds and clones whole mesh buffers every frame per cloud
**Where:** `nebula.rs:680-693`. Same shape in `blackhole.rs:~350-375` (new `pos/uv/col/idx` vecs every frame), `fx/mod.rs:88-102` (`set_ribbons_if_changed` takes owned `Vec`s).
**What:** Each frame, per cloud, it allocates three vertex Vecs (the vertex count is the cloud mesh's), clones the constant index buffer (`c.idx.clone()`), then `set_ribbons_if_changed` compares them against the existing mesh and usually (when the camera is still) discards them. The vertex colours depend on the camera (`facing`) so positions rarely repeat, but UVs and indices never change between ticks.
**Why:** Per-frame allocation and a full upload for something whose topology never changes. `NoFrustumCulling` plus a camera-dependent colour keeps the upload alive.
**Suggestion:**
- Write indices once at creation.
- Keep scratch `Vec`s in `Cloud`.
- Change `set_ribbons_if_changed` to take slices and compare before copying.
- Optionally move the camera-dependent `facing` into the render profile's shader (it already has a custom nebula shader) so classic only uploads on the 30 Hz tick.
**Behaviour-preserving?** yes (for the buffer reuse). **Effort:** M

##### B5-11 [severity: low] [category: simplify] `render_mesh_fx` lives in `sim.rs` and re-implements the mesh write
**Where:** `fx/sim.rs:176-232` (header at `:1-2` says "Nothing here draws a particle"); `fx/mod.rs:73-79` (`set_ribbons`).
**What:** `render_mesh_fx` is a drawing system (creates a `Mesh3d`, materials, writes attributes) in the "simulation" file. It repeats `insert_attribute` × 4 instead of `set_ribbons`, and clones `def.uv` and `def.indices` on every update. Its `Key`-skip of unchanged meshes is good but only partly avoids the clones.
**Why:** The sim/draw split the module docs advertise is not what the file layout shows; a contributor looking for "what draws" misses it.
**Suggestion:** Move it to `draw.rs`; use `set_ribbons` (it takes `impl Into`, so `def.uv.clone()` is the only needed copy; consider storing `Arc<[..]>` or building the mesh once with static UV/index attributes and updating only positions/colours).
**Behaviour-preserving?** yes. **Effort:** S

##### B5-12 [severity: low] [category: simplify] Per-effect string lookup and a hard-coded effect name in the particle hot path
**Where:** `fx/sim.rs:52` `let meteor = lib.effects.get("MeteorStorm") == Some(&fx.eff);` runs per `Fx` per frame; `nebula.rs:606` spawns `"MeteorStorm"` by name; `audio.rs:130` picks sound names by `"MeteorStorm_Inside/Outside"`.
**What:** A `HashMap<String, usize>` lookup for every live effect every frame, and "meteor" special casing keyed to one retail effect name. A mod effect cannot opt in to `align_meteor_particle`.
**Why:** Needless work and a hidden coupling between data names and engine behaviour (this one is PROVISIONAL-marked, `sim.rs:154`).
**Suggestion:** Cache `FxLib::meteor: Option<usize>` once, or better compute per `Effect` a `surface_aligned: bool` at load from the name. Keep the name in one named const.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-13 [severity: medium] [category: modding] Presentation tuning numbers are literals, and several PROVISIONAL guesses are unmarked or unconfigurable
**Where (unmarked or unlabelled):**
- `audio.rs:110-111,129` black hole hearing distance 1500, meteor 400, "inside" at 150, bed volumes 0.6 (`:116`) and 0.5 (`:147`). The doc comment at `:83` says the distances are PROVISIONAL, but the abyss volume and the 0.6 are not marked.
- `nebula.rs:395,420,438` (`max` 600/800/1500, vol 0.8), `:696` loop volume 0.25, `:677` fade ramp (marked PROVISIONAL), `:688` colour modulation.
- `dragons.rs:166,175-179,271` (2000/700, vol 0.8, `fly` slerp rate `3.0` at `:123`, aim cone `0.5` at `:255`, mouth offset `6.0`, `Vec3::Y * 2.0`).
- `cloak.rs:301` max 600, `:323` loop volume 0.3.
- `environment.rs:638-645` whale `vol 0.6`, `% 101` jitter table; `:417` `1.0 - exp(-2 dt)` turn rate.
- `wake.rs:82` jitter ±0.07/±0.02 (the original constant? not stated), `flag.rs:256-261` ~15 cloth magic numbers.
- `fx/sim.rs:25,29` `LOWEST_DETAIL_DISTANCE = 350`, `SCENE_LIVE_BUDGET = 15_000` (marked), `MOVE_STEP = 0.1`.
**What:** Many are named at the top of their own file, but others are inline literals, and few say whether they are retail-derived or eyeballed.
**Why:** Checklist 3.5 / AGENTS rule 5. For a modder, the audible/visible character of storms, dragons and cloaks is not changeable without recompiling.
**Suggestion:**
1. Short term: lift each literal into a named `const` with units and a `// retail:` or `// PROVISIONAL:` tag.
2. Where a number is plausibly a mod knob (ambient bed distances and volumes, sound levels of hazards, the effect-detail budget), read it from the existing per-map or per-effect JSON where the key already exists (`sound_distance` is already read for storms/whales/dragons), or add one small `data/audio.json`/`data/fx/settings.json`. The data-file addition needs maintainer approval (new data format).
**Behaviour-preserving?** yes for the const lift; no for new data knobs unless defaults match. **Effort:** S (consts), M (data)

##### B5-14 [severity: medium] [category: modding] A broken or odd effect file in a mod crashes the game at launch
**Where:** `fx/lib.rs:321-352` (`effect_names` includes every `*.json` in `assets/fx/` that is not `index`; `FromWorld` calls `crate::data::read_json`, which panics on invalid JSON, `data.rs:536-538`); `:259-267` (`expect("Morph source mesh")`, `panic!("Missing/invalid effect mesh …")`, `assert!` topology mismatch); `:206` `.file_stem().unwrap()`.
**What:** The mod overlay feature "a new effect file is loaded without being listed anywhere" means a stray `notes.json`, a half-written effect, or an effect referencing a mesh a mod forgot, aborts startup with a panic. `docs/modding.md:144` advertises this as a feature.
**Why:** It makes the friendly path (drop in a file) the fragile one, and the error text points at `tools/build_assets.sh`, not at the mod.
**Suggestion:** In `FromWorld`, use `try_json` and `warn!("fx/{name}.json: …; skipped")` for unreadable files. Make `parse_prim` return `Result<Prim, String>` (or drop the primitive with a warning) for missing meshes/textures. Keep panics only for the retail index being incomplete.
**Behaviour-preserving?** yes for valid data. **Effort:** M

##### B5-15 [severity: low] [category: rust] `fx` naming and small leftovers
**Where:**
- `fx/mod.rs:15` `mod lib;` (module named `lib`, referenced as `super::lib::…`; confusing next to the crate's `lib.rs`).
- `fx/lib.rs:15` `struct Curve(Vec<(f32,f32)>, Option<(f32,f32,u32)>, bool)` accessed positionally (`prim.rate.2` at `sim.rs:68`, `rate.0.iter()` at `lib.rs:243,248`, `invalid.1` in tests).
- `fx/lib.rs:214,221` `let _ = src;` (the blend source is parsed and then unused).
- `fx/lib.rs:446-447` duplicated `#[cfg(test)] #[cfg(test)]`.
- `fx/lib.rs:2-12` `curve_keys` is `pub` and used by `nebula.rs` with `Value` plumbing.
**What:** Readability and mild naming debt.
**Suggestion:**
- Rename the module to `library.rs` or `parse.rs`.
- Give `Curve` named fields (`keys`, `looping`, `keys_only`).
- Delete the unused `src` read.
- Drop the duplicate attribute.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-16 [severity: low] [category: rust] `#[allow(clippy::too_many_arguments)]` is dead, and `spawn(..., transient: bool)` is an opaque bool
**Where:** `fx/mod.rs:~238` (`spawn_with`), `fx/draw.rs:60` (`render_fx`); `Cargo.toml:18-20` already allows `too_many_arguments` crate-wide. `fx::spawn(&mut commands, fxl, name, parent, local, world, transient)` has 7 positional params, two `Transform`s that are easy to swap, and a bool.
**What:** The local `#[allow]`s are redundant noise. Call sites like `fx::spawn(&mut commands, fxl, "Explosion_Light", None, t, t, true)` (`nebula.rs:398`) hide what `true` means.
**Suggestion:** Remove the two `#[allow]`s. Replace the bool with `enum Lifespan { UntilDone, Everlasting }`, or fold `transient` into `Origin` (rename it `SpawnOpts`), and offer `spawn_at(commands, lib, name, Transform)` for the very common "local == world, no parent" case.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-17 [severity: medium] [category: modding] Sound names, volumes and distances are scattered literals at ~60 call sites
**Where:** 32 `Sfx::ui("…")` and ~30 `Sfx { name, at, vol, max }` constructions, e.g. `cloak.rs:301`, `dragons.rs:166-179,271`, `nebula.rs:395,420,438`, `environment.rs:635-641`, `combat/asteroid.rs:302`, `combat/damage.rs:32,180`, `combat/shells.rs:409,479,536`, `boarding/*.rs`, `collision.rs:515`. `audio.rs:35-37,124,130-131,146,164-167,282-295` hold the audio module's own.
**What:** Event → sound mapping and loudness live inline in gameplay code. A modder can replace a `.ogg` of a known name (documented), but cannot re-map, re-level, or silence one event. The docs list "which event uses which pool" as PROVISIONAL, so it is likely to change, and every change touches Rust.
**Why:** Checklist 3.1/3.5. It is also an obvious extension point: the existing `engine_snd`/`impact_snd` data already prove the format.
**Suggestion:** Introduce `audio::cue::{…}` consts or a `SoundEvent` enum (`ShipExplosion`, `SectionLost`, `Lightning`, `CloakStart`, …) with a default table in code, and optionally an overlayable `data/sound_events.json` (`{"cloak_start": {"sound": "IC_Sub_Cloak", "vol": 1.0, "max": 600}}`). The JSON is a new data format, so it needs maintainer approval; the enum alone is behaviour-neutral and gives one place to mark PROVISIONAL.
**Behaviour-preserving?** yes (defaults identical). **Effort:** M-L

##### B5-18 [severity: medium] [category: modding] `audio::sound()` duplicates silently collide, unknown names fall back silently, voices bypass it
**Where:** `audio.rs:172-192` (`INDEX` `OnceLock`, `.collect()` into a `HashMap`), `:328` and `crew.rs:341` (`format!("voice/{f}.ogg")`), `crew.rs:180-182` (`voice_exists` hits the filesystem per candidate at call time).
**What:**
- `.collect::<HashMap>()` keeps the last duplicate stem; the walk is `read_dir` order, so which of two same-named files in different folders wins is arbitrary. `docs/modding.md:500-501` documents "which one wins is not defined".
- An unknown name returns `sound/<name>.ogg` with no log until the asset loader later fails.
- `to_lowercase` and `cloned()` allocate on every call.
- The voice path uses a different lookup scheme than sounds, plus `is_file()` syscalls during a battle.
**Why:** A modder who ships a replacement in a new folder (a natural thing to do) gets nondeterministic results, and a typo in a gun's `SoundName` plays nothing with no mod-time warning.
**Suggestion:**
- When building the index, sort and, on duplicate stems, `warn!` naming both paths and pick deterministically (later overlay folder wins).
- `warn_once!` on an unknown name at resolution.
- Return `Cow<'static, str>`.
- Build a `voice` index the same way and have `voice_exists` consult it.
- Consider making the index a `Resource` built in `Startup` instead of a global `OnceLock`, so tests can supply their own.
**Behaviour-preserving?** yes for unique names; deterministic for duplicates. **Effort:** S-M

##### B5-19 [severity: low] [category: simplify] The "spawn a sound with channel volume" pattern is duplicated 18 times
**Where:** `audio.rs:74,124,131,136,144,215,230,262,327`, `nebula.rs:547-548`, `cloak.rs:320-324`, `crew.rs:339-343`, `rules/*.rs`, `briefing.rs:189` (18 `AudioPlayer::new` sites, 18 `menu::volume(` calls). The audibility cut `v > 0.01` is repeated at `audio.rs:66,229`.
**What:** Each site hand-assembles `AudioPlayer::new(assets.load(sound(name)))` + `PlaybackSettings::{LOOP,DESPAWN}.with_volume(Volume::Linear(base * menu::volume(Setting::X)))`. Looped sounds whose volume follows a setting need a per-frame `sink.set_volume(...)` (also duplicated: `audio.rs:68,116,250`, `nebula.rs:696`).
**Suggestion:** Add helpers in `audio.rs`: `one_shot(commands, assets, name, volume, channel)`, `looped(…) -> impl Bundle`, `set_gain(sink, base, channel)`, and `const AUDIBLE: f32 = 0.01`. Also make `Sfx.name` a `Cow<'static, str>` or `Arc<str>` so `n.clone()` in `combat/gunnery.rs:608,796` stops allocating a `String` per shot.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-20 [severity: low] [category: simplify] `ambience` mixes entity lifecycle, source lookup and volume in one system
**Where:** `audio.rs:94-158`.
**What:** One system queries three source types, then uses a chain of `.or_else` lookups (`:108-112`) to find the source per bed, keeps a `Vec<Entity>` of covered sources (`has.contains` is O(n²)), and has a special-case `Local<Option<Entity>>` abyss bed. The source distance (1500/400/`island.max`) is a per-type literal inside the loop.
**Why:** Adding a new ambient source (a mod's nebula, an asteroid belt) means touching this function. The `IslandLoop { sound, max }` component already shows the right shape.
**Suggestion:** Generalise to one component `AmbientSource { sound: String, max: f32, vol: f32 }` inserted where the black hole, meteor emitter or island spawns (`blackhole::spawn`, `nebula::spawn`, island spawn). The audio system then handles one query. The abyss becomes a spawned source too. The `Added<AmbientSource>` filter replaces the `has` vector.
**Behaviour-preserving?** yes. **Effort:** M

##### B5-21 [severity: medium] [category: simplify] `crew::callouts` is a 140-line system that builds the snapshot, schedules and plays
**Where:** `crew.rs:204-346` (17 params via a `SystemParam`, 8 more direct).
**What:** One function does these jobs:
- Hazard sensing (`:228-260`, including a JSON polygon lookup per nebula per frame).
- Reading `Fired`/`Ram` messages.
- Critical-section tag mapping.
- Assembling `Snapshot` (about 12 `Vec`s and `String`s allocated per frame, e.g. `lib.classes[s.class].race.to_lowercase()` per enemy per frame at `:303`).
- Cooldown and queue management.
- Voice selection and playback.
Also: an awkward `match s.side { _ if s.side != Side::Player => …, _ => … }` (`:300-307`), and two faction classifiers (`faction_crew` `:154`, `races` `:304`).
**Suggestion:** Split into `gather_snapshot(...) -> Snapshot` (the only part that touches the ECS), `fn schedule(tracker, snapshot, now, &mut state)` (already testable), and `play_next`. Replace the odd `match` with `if/else`. Precompute race → faction key once per class in `Library` or a `OnceLock`.
**Behaviour-preserving?** yes. **Effort:** M

##### B5-22 [severity: medium] [category: modding] Crew-callout rules are hard-coded; ~290 entries of `events.json` are unused
**Where:**
- Thresholds: `crew.rs:17` `COOLDOWN = 15.0`, `:21-26` `play_chance`, hazard radii 600/250/300 (`:244,247,258`), hull levels `[0.75, 0.5, 0.25]` (`:64`), sail 0.75 (`:69`), `queue.len() < 3` (`:328`).
- Tag strings: `"Power Transformer Engines"`, `"Munnitions"`, … (`:275-278`).
- Tables: `SPECIES_VOICE`, `POSTS`; `voice_line` assumes a `"hm"` file prefix (`:168`).
- `audio.rs:323` hard-codes the helm event names.
- `content/voice/events.json` holds four voice sets (`HumanMale` 97, `HumanFemale` 96, `Procyon` 95, `Robot` 52 events), but `crew.rs:166` reads only `["HumanMale"]`; the other three are never read (all are listed in `docs/re/sound.md:44`).
**What:** The header marks the thresholds and cooldown as guesses ("PROVISIONAL … thresholds and 15 s cooldown are guesses"), but they are not individually tagged, and the radii live nowhere a mod can reach. The unused voice sets are either planned or dead data in a shipped file.
**Suggestion:**
- Name and tag each constant (`HAZARD_RADIUS_BLACK_HOLE`, …).
- Say in `events.json`'s doc (or `sound.md`) which sets are consumed.
- If mods are meant to supply a crew voice, add a `"rules"` section to the same file (cooldown, chances, radii), which is a data-format change and needs maintainer approval.
**Behaviour-preserving?** yes for naming; no for new knobs. **Effort:** S (naming), M (data)

##### B5-23 [severity: medium] [category: simplify] `cloak.rs` mixes sim state and presentation, and smoke drivers live in production modules
**Where:** `cloak.rs:56-59` (`shown`, `sound`, `seen_state` in the sim `Cloak` component, labelled "Presentation"), `:279-337` (`present` mutates `c.seen_state`/`c.shown` from `Update` while `advance` mutates the same component from `FixedUpdate`), `:253-269` (`cloak_smoke`), `environment.rs:224-277` (`sensor_smoke`), registered at `cloak.rs:137`, `environment.rs:468`.
**What:**
- Presentation fields on the sim component force `present` to take `&mut Cloak`, which blocks parallelism and invites accidental sim reads of view fields.
- The `--script` drivers are 50+ lines each of test choreography with magic frame numbers (`[30, 45, 65, 85, 105, 125]`) inside gameplay modules. `smoke/` exists for exactly this (development.md table).
**Suggestion:**
- Move `shown`/`sound`/`seen_state` to a `CloakView` component added by `attach`.
- Move `cloak_smoke` and `sensor_smoke` to `smoke/cloak.rs` and `smoke/sight.rs`, keeping the same schedule position.
**Behaviour-preserving?** yes. **Effort:** M

##### B5-24 [severity: medium] [category: simplify] `dragons.rs`: struct literal repeated three times; `think` is 150 lines; entity-index randomness in sim
**Where:**
- `dragons.rs:73-90` (spawn), `:290-307` (test), `:331-348` (test): identical 16-field `Dragon { … }` literals.
- `think` `:128-276` does death, roars, target scan, steering goal, movement command and breath in one function with raw JSON reads (`:146-151`).
- `:211-214`: the flee loiter point uses `now * 7.3 + e.index().index() as f32 * 1.7` (the `Entity` index is not stable across spawn order or runs).
- `:218-225` uses `now` (elapsed) for the 20 s pass restart.
**What:** A `Dragon::new(hp)` is missing. The entity-index term makes the flee orbit depend on spawn order, which can differ with mod content. Nearest-target ties (`crate::nearest`) depend on query order.
**Suggestion:**
- Add `Dragon::new(hp)` and use it in all three places.
- Split `think` into `scan_target`, `steer_goal`, `breathe`, `roar`, each a plain fn over `&Dragon`/`&DragonDef`.
- Use the dragon's own `id` (already available at spawn) as the phase seed instead of `e.index()`.
**Behaviour-preserving?** yes for the split and constructor; the seed change shifts the flee circle's phase only (needs-check, mark PROVISIONAL). **Effort:** M

##### B5-25 [severity: low] [category: modding] `Pull::for_terrain` hard-codes numbers its own comment says come from the WOTs
**Where:** `blackhole.rs:55-65` (`"Small" => (9000.0, 145.0)`, `"Med" => (15000.0, 260.0)`, `"Big" => (25000.0, 625.0)`) and `:228-233` (`kind` suffix → `"Medium"`/`"Large"`/`"Small"` fallback), `:283` (the gravity-well force repeats the `CLASS_GAIN * length / mass.max(MIN_PULL_MASS) * … / 9000.0` expression of `Pull::strength`).
**What:** The doc says "retail values 9000/145, 15000/260, 25000/625" from `Terrain_BlackHole_*.wot`, but they are not read from data, so a mod hole or an unusual retail one gets `None` (visual only, no pull; `spawn` still draws it as "Small"). The `/ 9000.0` normalisation is a magic number shared by two formulas.
**Suggestion:** Read `Magnitude`/`Radius` from the exported terrain JSON (the exporter already has them), or accept `pull: {magnitude, radius}` in the map's JSON, keeping the table as the fallback. Add a small `fn class_gain(length, mass) -> f32` used by both the hole and the well. Both `CLASS_GAIN` and `MIN_PULL_MASS` are already marked PROVISIONAL, which is good.
**Behaviour-preserving?** yes if defaults stay. **Effort:** S-M (data read may need exporter/format approval)

##### B5-26 [severity: low] [category: simplify] API wart: `Sensors::player_sees(&self, target, _mission)` ignores its second parameter
**Where:** `environment.rs:72` (`_mission: Option<&Mission>`) and `player_sees_both` (`:76-78`); ~20 call sites (`panels.rs:479`, `crew.rs:251,302`, `docking.rs:876`, `starmap.rs:156,161`, `view.rs:221,528,609`, `classic/world_hud.rs:372,417`).
**What:** Every caller threads `mission.as_deref()` (and often takes a `Mission` resource solely for this).
**Suggestion:** Drop the parameter from `player_sees`/`player_sees_both`; callers that only needed `mission` for it can drop the `Res<Mission>` param. (Do it in one mechanical change.)
**Behaviour-preserving?** yes. **Effort:** S

##### B5-27 [severity: low] [category: simplify] Name collision `environment::Clip` vs `clips::Clip`; `clips.rs` keyframes are an untyped `[f32; 8]`
**Where:** `environment.rs:282` (`Clip` = glTF animation handles) vs `clips.rs:12` (`Clip` = retail keyframe motions); `clips.rs:9` `type Key = [f32; 8]`, `:53,58` `a[2] == 0.0`, `a[2] == 1.0` (float equality as an enum), `:64` index arithmetic.
**What:** Two unrelated `Clip` types with a shared word and a shared "scene clip" meaning confuse readers. The keyframe `kind` is stored as `f32` and compared with `==`, with positional indices for tension/continuity/bias.
**Suggestion:** Rename `environment::Clip` to `SkeletonClip` (or `ModelAnimations`). Parse each key into `struct Key { t, v, kind: Interp, tcb: [f32;3], tan_in, tan_out }` at load; `sample` then matches on the enum. The per-frame `clips::Rest` pattern (`pose_scene` inserting `Rest` on first pass) is fine.
**Behaviour-preserving?** yes. **Effort:** S-M

##### B5-28 [severity: low] [category: rust] `decals.rs`: size classes as raw `u8` indices
**Where:** `decals.rs:15,18,22-39,50,97,122` (`SIZE[class as usize]`, `KEEP_BELOW[new as usize][class as usize]`, `s.size.min(2)`, `class..=2`).
**What:** Size class is a `u8` whose valid range (0..=2) is enforced by scattered `min(2)` and manual `(l + 1).min(2)`; a mod bullet with `decal` size 5 would index out of bounds only if some `min(2)` is forgotten (`Scar::size` is public).
**Suggestion:** `enum Size { Small, Medium, Large }` with `fn index()`, `fn next()`, `TryFrom<u8>`; `SIZE`/`KEEP_BELOW` indexed by it. `mend` (every frame, every scarred ship) could use `Changed<Health>`.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-29 [severity: low] [category: simplify] `shipart.rs` and `retail_tex.rs` are overlay-build tooling living among presentation modules
**Where:** `shipart.rs` (519 lines: rasteriser, dilate/erode, hand-written PNG/crc32/adler at `:324-369`) and `retail_tex.rs` (GLB rewriting), both called "when the overlay is built" (`shipart.rs:386-390`, `retail_tex.rs:1-5`).
**What:** They are not part of world presentation; they sit beside `wake.rs`/`flag.rs` and share nothing with them. `retail_tex.rs:77` uses `eprintln!` and `shipart.rs` silently swallows `fs::write` errors with `let _ =` (`:376-379`), so a failed icon write leaves a ship without art and no message.
**Suggestion:**
- Move both under `modding/` (or a `derive/` folder next to `data::derive`) so the overlay pipeline is one place.
- Log write failures with `warn!`.
- The hand-written PNG is justified by "no dependency" (adding the `image`/`png` crate would need maintainer approval; Bevy already pulls one transitively, but that is a decision for the maintainers).
**Behaviour-preserving?** yes. **Effort:** S

##### B5-30 [severity: low] [category: modding] `flag.rs` flag texture is chosen by `class.name.contains(..)`, and the cloth shape is ~15 unnamed numbers
**Where:** `flag.rs:66-73` (marked `shortcut`, good) and `:248-273` (`1.6`, `0.25`, `4.0`, `7.0`, `0.08`, `0.7`, `0.3`, `14.0`, `27.0`, `9.0`, `15.0`, `2.3`), plus `:224-229` wind constants (`1.5`, `0.35`, `0.65`).
**What:** A mod ship named `Ship_Rebel_X` always gets the Navy flag unless a mission rule sets `ms.flag`. The cloth numbers are art direction, but the comment at the top ("Not retail") does not mark them PROVISIONAL, and a render profile's `Cloth::flutter` only scales one of them.
**Suggestion:**
- Read a `"flag": "<texture>"` key from `ship.json` (new optional key; `serde(default)` safe, but it is a data-format extension, so ask the maintainer) with the current name test as fallback.
- Group the cloth numbers into a `const CLOTH: ClothShape` with a one-line comment each.
**Behaviour-preserving?** yes. **Effort:** S

##### B5-31 [severity: low] [category: bevy] `Clip::scenery` reads a whole GLB synchronously with `std::fs::read` at spawn
**Where:** `environment.rs:292-297` (called per scenery model at battle setup).
**What:** The only reason is to count glTF `animations`. For big scenery models that is a blocking multi-MB read on the main thread during scene setup, while `retail_tex::header` (`retail_tex.rs:19-29`) and `data::glb_json` already know how to read just the JSON chunk. The same bytes are then read again by the asset server.
**Suggestion:** Read only the GLB header chunk (reuse `retail_tex::header`'s approach), or cache the animation count in `data/animations.json`/the scene index at import time (exporter change, so approval).
**Behaviour-preserving?** yes. **Effort:** S

##### B5-32 [severity: low] [category: bevy] `flow_currents` and a few frame systems mutate materials every frame
**Where:** `environment.rs:607-613` (two current ribbons: `material.base.uv_transform.translation.y = …` every frame); `cloak.rs:~345-377` (`fade` re-adds a modified clone material each frame while `alpha < 1`).
**What:** `materials.get_mut` marks the asset changed every frame, which re-extracts and re-prepares the bind group. The ribbon count is small, so the cost is low, but the same pattern repeated for ship-count × material-count in `cloak::fade` is not free while a sub is fading (5 s per cloak).
**Suggestion:** For the current ribbons, guard with the same "only write when it changed by more than epsilon" pattern used in `flag::wave`, or scroll the UV in the shader via `Time`. For `cloak::fade`, skip the `get_mut` when the alpha equals the last written value (store it on `Faded`).
**Behaviour-preserving?** yes. **Effort:** S

##### B5-33 [severity: low] [category: fidelity-risk] Frame-rate-dependent gates inside "30 Hz" presentation sims
**Where:** `wake.rs:203-208` (`since_refresh += dt; if > REFRESH`, where `dt` is the clamped frame delta, so the speed-multiplier refresh happens on the first frame after 1 s, not on the 30th tick); `nebula.rs:677` (fade uses the camera distance of the current frame); `fx/sim.rs:46,60` (`frame` clamp 0.25; at speed 16x the clamp makes particles run slower than game time while the sim catches up).
**What:** All are cosmetic, but they are exactly the "looks frame-rate dependent" class. Wake `mult` in particular is documented as a per-second gate of the original ("the keeper's update gate").
**Suggestion:** Count refresh ticks (30) in the drained tick loop (see B5-04) so the gate is in engine ticks. Note the 16x particle slowdown in `docs/re/effects.md` or accept it deliberately.
**Behaviour-preserving?** no (tiny phase shift; needs-check). **Effort:** S

##### B5-34 [severity: low] [category: simplify] LCG helpers are implemented four ways with different float mappings
**Where:** `fx/mod.rs:68-72` (`lcg_unit`, mantissa trick `from_bits(… | 0x3f80_0000) - 1.0`), `nebula.rs:99-100` (inline step), `nebula.rs:353-356` (`rnd`, uses `(*seed >> 8) as f32 / 2^24`), `nebula.rs:582,588` (seed stepping), `view.rs:453-457` (the cosmetic generator in `view.rs`, a fifth).
**What:** The same constants `0x19660d` / `0x3c6ef35f` appear in five places. The two unit mappings give different distributions (the mantissa trick maps the low 23 bits, the shift uses the top 24), which may be unintended in `storms` (and storms is sim: see B5-06).
**Suggestion:** One `Lcg(u32)` type (`next_u32`, `unit()`) used by all; keep the mapping each call site has today via two named methods (`unit_mantissa`, `unit_top24`) so behaviour is unchanged, and document which one matches the original.
**Behaviour-preserving?** yes (with both methods kept). **Effort:** S

##### B5-35 [severity: low] [category: modding] `fx/mod.rs` global state: `FxLib` is built synchronously in `FromWorld`, textures in `Startup`, none hot-reloadable
**Where:** `fx/lib.rs:337-352`, `fx/mod.rs:127-133`.
**What:** Hundreds of effect JSONs are parsed during world init (a startup hitch) from `assets_dir()`; textures are loaded from `fx/tex/{n}.png` by name, so a mod adding a texture only needs the file. That part is good. Not an asset type, so there is no hot reload or typed error reporting (checklist 2.4/3.1).
**Suggestion:** Not urgent. If hot reload for modders ever matters, wrap `FxLib` parsing in an `AssetLoader`; until then, document in `docs/modding.md` that effect JSON is read once at launch. (Do not change the format.)
**Behaviour-preserving?** yes. **Effort:** L (only if wanted)

##### B5-36 [severity: low] [category: doc-check] Falloff description in `docs/re/sound.md` versus code
**Where:** `docs/re/sound.md` ("full volume inside a tenth of it") versus `audio.rs:194-205` (`FULL_VOLUME_DISTANCE = 25`, inverse-distance, no tenth-of-max rule).
**What:** The "Built into the original" paragraph says each pool plays at full volume inside a tenth of its max distance; the "PROVISIONAL in the remake" paragraph and the code use 25 units. Both can be true (original vs remake) but a reader will not know which the engine does.
**Suggestion:** One sentence in `sound.md` ("the remake does not use the tenth-of-max rule; see Falloff") and, if the tenth-of-max rule is the original's, add it as a tracked row on the fidelity board.
**Behaviour-preserving?** yes (docs only). **Effort:** S

#### Quick wins
- Add a `MainCamera` marker and query `With<MainCamera>` in the 7 `Single<…With<Camera3d>>` systems (B5-01).
- Delete the redundant `#[allow(clippy::too_many_arguments)]` at `fx/mod.rs` and `fx/draw.rs:60`, the `let _ = src;` and the duplicate `#[cfg(test)]` at `fx/lib.rs:446-447` (B5-15, B5-16).
- Name the repeated literals: `MAX_PRESENTATION_DT = 0.25`, `AUDIBLE = 0.01`, `SEG_LIFE`/`SEG_EVERY`/`FADE_IN`/`FADE_OUT` shared by `blackhole.rs` and `wake.rs` (B5-04, B5-05, B5-19).
- Cache `FxLib::meteor: Option<usize>` instead of a string-keyed lookup per effect per frame (B5-12).
- Drop the unused `_mission` parameter of `player_sees` and `player_sees_both` (B5-26).
- `Dragon::new(hp)` to replace three 16-field literals (B5-24).
- In `audio::sound`, warn on duplicate stems and unknown names (B5-18); in `FxLib::from_world`, use `try_json` plus `warn!` so one bad mod effect does not crash launch (B5-14).
- Mark the inline PROVISIONAL guesses (hazard radii in `crew.rs:244-258`, ambient distances in `audio.rs:110-129`, storm/dragon/cloak sound distances) with `// PROVISIONAL:` tags and units (B5-13, B5-22).
- Move `cloak_smoke` and `sensor_smoke` into `smoke/` (B5-23).
- Give `wake.rs` and `blackhole.rs` their own `plugin()` instead of being wired from `lib.rs` (B5-02).

---

## Appendix: Classic view & HUD (game/src/classic/, panels.rs, sprites.rs, theme.rs, content/data/hud/*)

Overall: the layering is mostly honoured. `tests/test_layers.py` holds, the HUD contract in `panels.rs` (markers, gauge values, Radar, HeadState) is clean, and `theme.rs` plus `theme.json` is a good model of a moddable look (per-key overlay, a test that pins defaults to the file, hex colours, 800x600 units). PROVISIONAL marking is frequent and honest (40 hits in classic/), and numbers are often tied to named hud.hdt keys. The weak points are these:
- Several views contain real decision logic (what to show, which target, which button does what), so a second presentation crate would have to copy it.
- Every UI file opts out of the Interaction deprecation with a file-wide `#![allow(deprecated)]`.
- The widget kit is large but has two unrelated `Ctx` types and three duplicated helper sets.
- Pixel layout and colours are mostly literals in code; only the front-end chrome is data-driven.
- The HUD bar roles are inferred from sprite-name strings.

Reviewed without building. Code references are to the checked-out tree.

#### Findings

##### B6-01 [severity: high] [category: fidelity-risk] "Views only draw" is broken for the target reticle and name-banner visibility
**Where**: `game/src/classic/world_hud.rs` `place_labels` (about lines 340-440): the `shown` closure and the visibility `match`; also `navigation` (141-210).
**What**: `place_labels` decides, inside the view:
- which entity the reticle sits on (`selection.0.direct()` ? `target.0` : fleet-selected ship's `FleetPilot.target` or `MissionShip.target`);
- whether it is visible (`!h.destroyed && ms.is_none_or(|m| m.visible) && sensors.player_sees(..) && !cinematic(..) && !hud_hidden()`);
- the same rule for banners.

`navigation` chooses the objective goal point (`campaign::objective_point` for missions, `script.team_objective(team_of(local))` for skirmish) and which ship the ring follows.
**Why**: `docs/development.md` "Layers" says the view reads shared state and draws. A second presentation crate would have to re-derive "what is the HUD target" and "is this ship visible to the player", and the two could drift. This is also gameplay-visible behaviour (the fleet-selected-ship fallback).
**Suggestion**: add shared resources in `panels.rs` or `environment.rs`, computed once per frame in `HudSet`: `HudTarget(Option<{entity, radius}>)`, `BannerVisible(Entity)` / a per-ship `HudVisible` marker, and `ObjectiveGoal(Option<Vec3>)`. The view then only projects and draws. Move the existing tests (`banner_alpha` stays presentational).
**Behaviour-preserving?**: yes if the logic is moved verbatim.
**Effort**: M

##### B6-02 [severity: high] [category: modding] The retail-element-name to `Action` table lives in the classic view
**Where**: `game/src/classic/menu.rs` `element` (137-390): about 50 match arms such as `(Screen::Lobby, "Open Next Screen Button") => Some(Action::Launch)`, `(Screen::Fleet, "Add Ship To Fleet Button") => (sk.ship_chosen, Action::AddShip)`, plus the enabled/disabled conditions (`sk.launch_error().is_some()`, `sk.historical`, fleet non-empty).
**What**: which button on which screen performs which action, and when it is disabled, is decided in the view. Only the drawing (arrow, text_button, ...) is view-specific.
**Why**: a second view (or a retheme that reuses retail `.tst` layout) must copy the whole table. The shared `menu.rs` already owns `Action` and `Screen`, so this is the "if a view needs something new, add it to the shared model" case. The 250-line function is also the least readable part of the bucket.
**Suggestion**: add to `menu.rs` a pure `fn button_for(screen, ingame_page, element_name, &model) -> ButtonSpec { action: Option<Action>, state: Enabled|Disabled|Hidden, art: ArrowPrev|ArrowNext|Text }`. The view maps `ButtonSpec` to widgets. This is unit-testable without Bevy.
**Behaviour-preserving?**: yes.
**Effort**: L

##### B6-03 [severity: medium] [category: bevy] Eight UI files carry a file-wide `#![allow(deprecated)]` for `Interaction`
**Where**: `classic/{menu,widgets,panels,crew_arms,setup,player_names,ship_preview}.rs` (line 3-10 each), `classic/starmap.rs:48`, shared `panels.rs:6`. About 15 sites test `*i != Interaction::None` in `panels.rs` alone (lines 234, 550-588, 626, 645, 660).
**What**: the checklist (§2.2) says 0.20 deprecates `ui::Interaction` in favour of `Hovered`/`Pressed` and `ui_widgets::Button` with an `Activate` event. The allow is inner-file, so it also hides any other deprecation that appears in those files, including the 1000-line `menu.rs`. The comments say "shortcut ... port when it is removed".
**Why**: Interaction polling is spread over shared and view code; when it is removed the port touches about 10 files. The contract markers (`Solid`, `PanicButton`, `GunButton` ...) are where click detection lives, so the port must start in shared `panels.rs`, not in views.
**Suggestion**: step 1 (S, no behaviour change): add one shared helper, e.g. `pub fn touched(i: &Interaction) -> bool` (and a `UiState { None, Hover, Pressed }` mapping) in `panels.rs`. Replace the three separate mappings (`widgets::state_of`, `widgets::hover`'s match, `panels::state_index`) and the `!= Interaction::None` tests with it. Narrow the allow to that one helper. Step 2 (M): migrate to `Hovered`/`Pressed` components behind the helper, keeping the `Solid` ("UI ate the click") semantics.
**Behaviour-preserving?**: yes (step 1), needs-check (step 2: click timing and `FocusPolicy::Block` semantics).
**Effort**: S then M

##### B6-04 [severity: medium] [category: simplify] Two unrelated `Ctx` types with the same name and three copies of absolute-node/px helpers
**Where**: `classic/widgets.rs:161` (`Ctx{assets,hud,fonts,scale,theme,pad}`, front end, 800x600 canvas) vs `classic/panels.rs:169` (`Ctx{assets,hud,s}`, HUD, REF_H 960). `player_names.rs:109` defines a local `px()` identical to `crate::panels::px`; `boarding_bar.rs:87` `absolute` closure and `mission_ui.rs:42` `absolute` duplicate `panels::Ctx::abs` / `widgets::Ctx::node`; `world_hud.rs:126-139` and `:281-290` re-implement reference-pixel scaling (`RefRect`, `LabelText`) a third and fourth time.
**What**: same idea (reference px to screen px, place a sprite) with different names (`node` vs `abs`, `scale` vs `s`).
**Why**: confusing for a newcomer and a modder reading both; `crew_arms.rs` imports the widgets one, `player_names.rs` the panels one. Each system also redoes `window.height() / REF_H`.
**Suggestion**: rename to `FrontEndCtx` and `HudCtx`, move the shared bits (`px`, `abs`, `put`, `put_box`) to `sprites.rs` or a small `classic/layout.rs`, and delete the local copies (`player_names::px`, the two `absolute`s). Longer term, a `HudScale(f32)` resource updated on window change replaces the per-system recompute.
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B6-05 [severity: medium] [category: modding] HUD bar part roles are inferred from sprite-name string comparisons
**Where**: `classic/panels.rs` `spawn_part` (442-497): `name == "MAINBAR_IS_NOT_PAUSED_BUTTON"`, `"MainBar_PanicButton"`, `"MainBar_ShipIcon"`, `"MainBar_FleetFormationsButton"`, `"MainBar_ChatButton"`, `"MainBar_BeginnerMovementTypeButton"`, and the three gauge arms mapping `MAINBAR_PRIMARY_SHIP_*_BAR_BACKGROUND` to (fill sprite, offset key, `Gauge`). The paused-art name `"MAINBAR_IS_PAUSED_BUTTON"` is also hard-coded. `content/data/hud/bar.json` is the part list, but carries only `sprite` and `backdrop`.
**What**: a mod or second skin that reorders, drops or adds a bar part cannot say what a part does. The roles (marker component, gauge, fill sprite, offset key) are a code table keyed by retail names. A string typo compiles and silently draws nothing (`let Some(art) = c.sprite(name) else { return }`).
**Why**: moddability; also `bar.json` has no PROVISIONAL marker even though the HUD part order is a known PROVISIONAL area (AGENTS.md). JSON arrays cannot carry a `_comment`, so the guess is invisible next to the data.
**Suggestion**: (a) now: replace the `if name == ...` chain with one `match` on a `Role` enum parsed once in `BarPart` (`Role::from_sprite(&str)`), and mark PROVISIONAL where the part order is read. (b) with maintainer approval (data format): add optional `"role"`, `"gauge"`, `"fill"` keys to `bar.json` entries, or wrap in `{"_comment": "PROVISIONAL ...", "parts": [...]}` (needs `tools/compose_bar.py` and `BarLayout::load` changes). Document either in `docs/modding.md`.
**Behaviour-preserving?**: yes.
**Effort**: S (a), M (b, needs approval)

##### B6-06 [severity: medium] [category: rust] `BarLayout::load` does sync JSON reads with panics, per rebuild, from two systems
**Where**: `classic/panels.rs:34-49`, called at `:254` (`build`) and `:941` (`chat_panel`, on every new chat line); also the test at `:1092`.
**What**: `read_json` plus `.expect("HUD bar needs parts")`, `panic!("HUD bar {skin}: ...")` and `unwrap_or_else(|| panic!)` on each number. It runs inside a system on every window resize, ship change and chat line.
**Why**: a mod that overlays `hud/bar/<skin>.json` incorrectly crashes the game instead of falling back (checklist §5.2). Reading and parsing JSON every chat line is wasteful.
**Suggestion**: load once into a `Resource` keyed by skin (or cache via `OnceLock` like `Theme::current`). Use a `#[derive(Deserialize)]` struct (`BarLayout { width, height, parts: Vec<BarPart> }`) with `serde(default)` on optional fields, and on error log and fall back to no bar (or the previous one) instead of panicking. Return `Result<_, BarError>` from `load`.
**Behaviour-preserving?**: yes for valid files.
**Effort**: S

##### B6-07 [severity: medium] [category: simplify] Despawn-and-rebuild-with-a-`Local` key pattern copied five times
**Where**: `panels.rs:222` `build` (`Local<Option<(usize,&'static str,u32,Vec<String>)>>`), `chat_panel` (`built`), `player_names.rs:23` `window` (`last`), `menu.rs:34` `build` (`last` + `model.changed()`), `crew_arms.rs:49` (`shown` + `dialog_changed`).
**What**: each is "compute a key, return if unchanged and the root exists, despawn the old roots, rebuild". `roots.iter().next()` (chat_panel, player_names) is used where `Single`/`Option<Single>` would state the intent (checklist 1.12).
**Why**: five slightly different guards; the `window.height() as u32` key means the HUD rebuilds at every pixel of a window drag.
**Suggestion**: introduce a tiny helper (e.g. `classic/rebuild.rs`: `struct Rebuild<K: PartialEq>(Option<K>)` with `fn stale(&mut self, key: K, exists: bool) -> bool`) or use run conditions plus change detection (`resource_changed::<Roster>`, `.run_if(window_resized)`). Use `Option<Single<Entity, With<HudRoot>>>`.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B6-08 [severity: medium] [category: modding] Layout of the HUD is mostly numeric literals in code, not data
**Where**:
- `panels.rs:408-412` weapon panel height `255.0`; `:243-250` dial `left: px(76.0)` and the `44.0` shoulder offset (`:344-347`); `speed_plate`/`name_plate`;
- chat panel `28.0`, `20.0`, `14.0`, `26.0`, `4.0`, thumb `7.0 x 20.0`, `chat_scroll` `24.0` per wheel notch (`:914-969`, `:975-993`);
- `boarding_bar.rs:11-13` `BAR_VISIBLE=250`, `BAR_SIZE`, `BAR_INNER`, with colours `(0.25,0.55,1.0)` and `(0.9,0.25,0.2)` at `:97`;
- `world_hud.rs`: banner `50.0` px above anchor, `font_scale = 1.05`, reticle `0.3`/`16..40`/`0.8`/`0.2` factors, `BANNER_NEAR=150` (documented as the hud.hdt key, but typed in code);
- `crew_arms.rs:36` `PANEL = (286.5, 116.5)`;
- `starmap.rs:14-16`.

Some are annotated "measured on reference footage", some PROVISIONAL, many have no note.
**What**: the retail art offsets come from `hud.json` (good: `offset(hud, "...")`), but everything that was eyeballed lives in code. Only the front-end chrome (`theme.json`) is overlayable.
**Why**: a modder who swaps in different-size art cannot move the weapon panel, resize the chat window or retint the boarding bar without recompiling. Unmarked literals also hide which numbers are guesses.
**Suggestion**: short term, give each a named `const` with a one-line source ("measured on 1440x1080 footage" / "PROVISIONAL"), grouped at the top of the file. Longer term, extend `data/hud/theme.json` (already overlaid, `serde`-lenient) with a `hud` block (`chat: {size, scroll_step}`, `boarding_bar: {friendly, foe, visible_range}`, `banner: {near, fade, lift}`) read through a `Theme::hud()` accessor. Any new key is a data-format addition and needs maintainer approval.
**Behaviour-preserving?**: yes.
**Effort**: S (consts), M (data)

##### B6-09 [severity: medium] [category: modding] Colours: dozens of literals, five different "golds", several inline error reds
**Where**: 80 `Color::srgb*` literals in classic/. Golds: `widgets.rs:13 GOLD (1,0.796,0)`, `menu.rs:669 SETTINGS_GOLD (1,0.85,0.2)`, `starmap.rs:137 LABEL_GOLD (0.95,0.85,0.3)`, `mission_ui.rs:13 SPEECH_GOLD (1,0.65,0.1)`, `setup.rs:10 GOLD (1,0.8,0.4)`, plus inline `(1,0.9,0.4)` at `menu.rs:438` and `:906`. Error red `(1,0.4,0.3)` at `menu.rs:868`, `:997`, and `(1,0.45,0.35)` at `:916`. List-cell colours `CELL/CELL_ON/CELL_HOVER` (`widgets.rs:24-28`) and `LIST_BACKING`. `crew_arms.rs:179` health ramp.
**What**: palette is split between `theme.json` (panels, buttons) and hard-coded constants; near-duplicates look accidental.
**Why**: retheming needs a source edit; if the differing golds are retail-measured they need a comment, otherwise they are an inconsistency.
**Suggestion**: add a `text` palette to `Theme` (`gold`, `label`, `error`, `speech`, `cell`, `cell_on`, `cell_hover`, `list_backing`) with the current literals as defaults, mark PROVISIONAL, and have the call sites ask the theme. Merge exact duplicates; keep each distinct value but comment its source. New keys need maintainer approval; the Rust-side consts do not.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B6-10 [severity: medium] [category: simplify] `Theme::default()` duplicates `theme.json` (about 100 lines) guarded by a test
**Where**: `theme.rs` (`impl Default for Theme` plus the `look(...)` calls with 100+ hex strings) and test `shipped_theme_file_matches_the_defaults` (~line 140). `Theme::current()` falls back to `unwrap_or_default()` if the file is missing.
**What**: two sources of truth that must be kept equal by hand and by test. Every new key is edited in three places.
**Why**: the content file ships with the code and is first-party, so it can be embedded.
**Suggestion**: `Theme::default() = Theme::from_json(&serde_json::from_str(include_str!("../../content/data/hud/theme.json")).unwrap())` (the unwrap is an embedded-constant invariant; keep a unit test that it parses). Delete the hand-written defaults and the equality test. Check that `game/` packaging can reach `../content` (`include_str!` is relative to the source file and the crate is in the same workspace).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-11 [severity: low] [category: modding] `Theme::current()` is process-wide `OnceLock`; mod enable/disable at runtime cannot restyle
**Where**: `theme.rs` `Theme::current` (about line 249-252); `classic/menu.rs` `mods_page_view` (the Mods screen).
**What**: the theme is read once, from `assets_dir()` joined with `data/hud/theme.json`. Same pattern for `sheet_scale` (`sprites.rs:273`) and `slice_caps`.
**Why**: if the Mods screen applies toggles without a restart, theme overlays will not apply until relaunch; modders will find edits are ignored while testing. If a restart is required, it should be stated.
**Suggestion**: confirm how mod toggling is applied (restart?). If live, make `Theme` a `Resource` reloaded on the mod-change message. Either way, put one sentence in `docs/modding.md` and `theme.json`'s `_comment`.
**Behaviour-preserving?**: needs-check
**Effort**: S (doc) / M (resource)

##### B6-12 [severity: medium] [category: simplify] `menu.rs` (1066 lines) and `widgets.rs` (940 lines) mix several concerns; the front-end input scrolling lives in the view
**Where**:
- `menu.rs`: `element` (250 lines), `options_page`, `sound_display_page`, `mods_page_view`, `missions_page_view`, `players_list` (100 lines, 767-875), `summary`.
- Scrolling: `wheel`, `scroll_controls`, `scroll_mods`, `scroll_missions`, `scroll_saves` (1000-1055).
- `widgets.rs`: Ctx, panel/tube drawing (314-480), scrollbar/slider, fleet bar, list rows, dropdown.

**What**: five near-identical systems read `MouseWheel`, test the screen, compute `max = len.saturating_sub(ROWS)` and call `wheel_step`. The row counts are inconsistent about ownership: `MOD_ROWS`/`MISSION_ROWS` come from shared `menu`, while `MAP_ROWS=8`, `SHIP_ROWS=8`, the controls `17`/`8` (`:1024`) and saves `10` (`:1054`) are literals in the view. Each calls `wheel.read().sum()` separately, which is fine for messages (independent cursors), but it is five systems for one concept.
**Why**: wheel input and list geometry are shared state (`sk.scroll`, `cui.scroll`, `ui.scroll`), so they belong in `menu.rs` ("input lives in shared modules"). A second view would have to copy all five and the magic row counts.
**Suggestion**: in shared `menu.rs` add one `scroll_lists` system and a `fn visible_rows(&MenuModel) -> Option<(usize /*len*/, usize /*rows*/)>` per scroll target, with the row-count consts defined next to the data. The view only draws. Then split `menu.rs` by seam: `menu/element.rs` (retail layout), `menu/pages.rs` (options, settings, mods, missions), `menu/skirmish.rs` (map/players/fleet lists). Keep `widgets.rs` as is, or split `panel`/`tube` (pure drawing, ~170 lines) into `widgets/chrome.rs`.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B6-13 [severity: low] [category: simplify] Chat panel wheel/thumb logic is behaviour inside the view
**Where**: `classic/panels.rs` `chat_scroll` (973-993), `chat_panel`.
**What**: scroll step (`ev.y * 24.0 * s`) and the thumb position are computed in the view, writing `ScrollPosition` directly. `over` is derived from `Interaction` on the panel.
**Why**: it is presentation-only state (no gameplay), so acceptable, but it is the same "wheel over a panel scrolls it" idea as B6-12 and hard-codes `CHAT_SIZE.y - 28.0 - 20.0` (track length) separately from the thumb size and offset constants in `chat_panel` (`14.0`, `20.0`, `7.0`). A change in one breaks the other silently.
**Suggestion**: name `CHAT_PAD=14.0`, `THUMB=20.0`, `TRACK = CHAT_SIZE.y - 2*CHAT_PAD - THUMB` once and use them in both functions. Optionally let Bevy's own scroll handling (`overflow: scroll_y` plus a wheel observer) replace the manual step.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-14 [severity: medium] [category: simplify] `place_labels` is a 15-parameter, 100-line system that does two jobs and works around its own query conflicts
**Where**: `classic/world_hud.rs:340-440`. Parameters include a tuple `(Res<Fleets>, Query<&FleetPilot>)`, `(Query<&Children>, Query<&mut TextColor>, Res<Time<Real>>)`; `Without<Reticle>` filters on `labels` and `imgs` exist only to keep the reticle query disjoint.
**What**: banner fade/placement and target-reticle placement share one system and one `project` closure, `s` and `cam`.
**Why**: checklist 1.15 (one job per system, params grouped via `SystemParam`). The `Without<Reticle>` filters and the tuple params exist purely to satisfy the borrow checker.
**Suggestion**: split `place_banners` and `place_reticle`; with B6-01 done, the reticle one needs only `Res<HudTarget>`, camera and window. Drop the `Without<Reticle>` filters. Add a `#[derive(SystemParam)] struct BannerSources` if needed.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B6-15 [severity: low] [category: simplify] Banner text scaled by two systems every frame
**Where**: `world_hud.rs:126` `place_ref_rects` and `:281` `scale_label_text` run each frame over all banners and write `Node`/`TextFont` unconditionally; `place_labels` then also rewrites alpha on every child each frame.
**What**: assignments to `Mut<Node>` and `Mut<TextFont>` trigger change detection (and text relayout for `TextFont`) every frame even when `s` is unchanged.
**Why**: Parley text re-layout is not free with many ships on screen.
**Suggestion**: compute `s` once (see B6-04, `HudScale` resource) and run the scale systems `.run_if(resource_changed::<HudScale>)`; or use `set_if_neq` for the fields. Alpha: only write when it changed (`label.alpha` already tracks it).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-16 [severity: low] [category: bevy] Reticle and banner entities use `Interaction`-free nodes but `Visibility::Hidden` is re-written for every reticle every frame
**Where**: `world_hud.rs` last loop of `place_labels`: `vis.set_if_neq(Visibility::Hidden)` then immediately `set_if_neq(Inherited)`.
**What**: the toggling pair is a hidden-then-shown dance that can flip change detection (the two `set_if_neq` produce a real change only if the visibility differs at the end, but `Mut` deref still marks `Changed` on the first write when value differs and on the second).
**Suggestion**: compute the final `Visibility` first and write once. Pure cleanup.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-17 [severity: medium] [category: bevy] Mission/hint/subtitle text uses unscaled pixels and the default font, unlike the rest of the HUD
**Where**: `classic/mission_ui.rs:35-48,62-80`: `left: Val::Px(120.0)`, `font_size: FontSize::Px(20.0)/18.0`, `absolute(6.0, 1.0, 104.0)`, `Val::Px(12.0)`, no `Fonts` entry. Compare `world_hud.rs:63` `end_line` which uses `17.0 * window.height() / 1080.0` and the retail `Immi14` font.
**What**: HUD text is sized in window-height-relative reference px (panels, end line) but mission speech/hint text is fixed 20/18 px with Bevy's default font. At 4K the speech line is tiny; at 640x480 it overflows the 14%-high strip.
**Why**: inconsistency with the rest of the view; probably a fidelity gap too (retail uses its own font). It also diverges from "pin Px for fixed-pixel HUD" meaning here "pixels at one resolution".
**Suggestion**: use `Fonts` `font.text(.., s, ..)` with `s = window.height() / REF_H` (or 1080) like `end_line`; make the strip pieces `percent`/scale-based. Mark PROVISIONAL where the retail font is unknown. Visible change, so check with the maintainers/reference footage first.
**Behaviour-preserving?**: no (visual change, intended as a fix).
**Effort**: M

##### B6-18 [severity: low] [category: modding] Three different reference heights (960, 1080, 600) with no single source
**Where**: `panels::REF_H = 960` (`panels.rs:15`); `world_hud.rs:70,105` `17.0 * h / 1080.0`; `panels.rs:731` (test) `1080.0 / REF_H`; front end 800x600 (`widgets::canvas`, `pad`); `crew_arms.rs:32` `window.width()/800`, `/600`.
**What**: the numbers are individually documented in places ("one reference-footage frame: about 16 px of 1080") but not tied together.
**Suggestion**: named consts `HUD_REF_H`, `FOOTAGE_H = 1080.0`, `FRONTEND_REF = (800, 600)` in `sprites.rs`/`panels.rs` and a short comment on why 960 (hud.hdt reference is 640x480 at 2x?) so a modder knows which to use. `crew_arms::follow_cursor` should call `widgets::canvas` instead of re-deriving the scale (`:25-29`).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-19 [severity: medium] [category: rust] `unwrap`/`expect`/panics in asset loading paths reachable from mods
**Where**: `sprites.rs:327` `expect("hud.json")`; `:386` `e.path().file_stem().unwrap()`; `:393-394` `.expect("UI font")` and `.expect("UI font parse")` (a mod font that is not a valid OTF crashes at startup); `:108,:112` `.lock().unwrap()` on the caches (poisoning cascade, acceptable but note); `panels.rs` `BarLayout` (B6-06); `theme.rs:15,21` `hex(s).unwrap_or(Color::BLACK)` in `look()` (defaults, fine).
**What**: mod overlays are meant to be loadable by third parties, but a bad font or hud.json aborts.
**Suggestion**: replace `expect` with `match` plus `warn!` and skip the font (the code already handles `fonts.0.get(..)` being `None` everywhere: `let Some(font) = ... else { return }`). For `hud.json` keep a clear error message. Use `std::sync::Mutex` + `lock().unwrap_or_else(|e| e.into_inner())` or switch to `OnceLock<RwLock>` if desired.
**Behaviour-preserving?**: yes for valid input.
**Effort**: S

##### B6-20 [severity: low] [category: rust] Magic `as` casts and `f32` parsing of JSON for retail data without a typed schema
**Where**: `menu.rs:140-152` (`e["rect"].as_array().map(...as_f64().unwrap_or(0.0) as f32)`, `e["color"]`); `player_names.rs:68-70,95` (`hud.comps["Player Names Window"]["_"][format!(...)]` indexing straight into `serde_json::Value`, defaulting to 0.0); `sprites.rs:318` `offset()` indexes `d["TopLeft X"]`.
**What**: stringly-typed JSON access scattered across views; a missing key silently becomes 0.0 (art at the screen corner) and cannot be told apart from a real 0.
**Why**: debugging a mod that renames or drops a key means a layout that is silently wrong. Checklist §5.3/§3.3.
**Suggestion**: introduce small typed structs for the three shapes (`TstElement { rect: [f32;4], impl_, text, font, color, enabled }`, `Offset`, `PlayerNamesParams`) with `serde(default)`, and `warn_once!` when a named offset used by the HUD is missing (`offset()` is called with literal names in about 30 places, so one `warn` in `offset` covers all).
**Behaviour-preserving?**: yes.
**Effort**: M

##### B6-21 [severity: low] [category: simplify] `Interaction` to state mappings are triplicated
**Where**: `widgets.rs:52-59` `state_of`, `widgets.rs:140-144` inline `match i { Pressed=>"CursorDown", Hovered=>"CursorOver", None=>IDLE }`, `panels.rs:786-792` `state_index`, plus `starmap.rs`/`options_menu.rs` ad-hoc checks.
**Suggestion**: one `fn art_state(Interaction) -> (usize, &'static str)` (or enum with `index()` and `retail_name()`) in `sprites.rs`; part of B6-03 step 1.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-22 [severity: low] [category: simplify] `spawn_part`'s doc comment is stale/duplicated and its `fill` closure builds per call
**Where**: `panels.rs:440-442`: two `///` paragraphs describing different things stacked on one function ("One bar part plus whatever sits on it ..." followed by "The box of one bar part ..."). `fill` closure at `:466` takes `&mut Commands` as an argument because it cannot capture it, which signals it should be a method on `Ctx`.
**Suggestion**: keep one doc comment; move `fill` to `HudCtx::gauge_fill(commands, parent, sprite, offset, gauge)`. Replace the `if name ==` chain plus `match name` with a single `match Role` (B6-05).
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-23 [severity: low] [category: modding] `plain` fallback presentation and the setup screen use unrelated hard-coded palettes and fixed pixel sizes
**Where**: `crew_arms.rs:262-299` (`plain`, `FontSize::Px(14.0/20.0)`, colours `(0.02,0.04,0.1)` etc.) and `setup.rs:8-15` (eight colour consts).
**What**: both are documented as "no retail data" fallbacks, so legitimate. But they are the only UI a modder without retail assets sees, and neither is overlayable or shared.
**Suggestion**: low priority; if kept, move `setup.rs` palette to `Theme` defaults only if `Theme` can be read without imported assets (it can: `Theme::default`). A short module comment saying "intentionally independent of theme.json" is enough otherwise.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-24 [severity: low] [category: fidelity-risk] PROVISIONAL gaps: values that read as guesses but are unmarked
**Where**:
- `world_hud.rs`: reticle size/offset factors `edge*0.3`, `clamp(16,40)`, `*0.8 + size*0.2`, `20.0` min; banner `50.0` "Offset_ShipName_Background"; `font_scale = 1.05` is marked, the `+3.0` baseline fudge is commented but not PROVISIONAL.
- `world_hud.rs:~165` the backing circle `Circle::new(5.2)`, `-4.2` is marked.
- `boarding_bar.rs:11-13`: `BAR_VISIBLE=250`, `BAR_SIZE`, `BAR_INNER`, `1.6` height factor.
- `starmap.rs:14-16`: `OPEN_SECS`, `FRAME_SECS`, `ALIEN_COUNT=10`.
- `ropes.rs:14-16`: `POINTS=24`, `MIN_WIDTH`.
- `mission_ui.rs:36`: `14%` strip (the "margin" is marked PROVISIONAL; the strip is not).
- `bar.json` part order (see B6-05).

**Suggestion**: add `// PROVISIONAL: eyeballed against footage` (or the source) next to each; AGENTS.md rule 5 asks for it. No behaviour change.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-25 [severity: low] [category: simplify] `starmap::rnd` and "Rebuilt every frame" `Dynamic` nodes
**Where**: `starmap.rs:39-41,138-` (`redraw`), `Dynamic` component doc "Rebuilt every frame".
**What**: icons and labels are despawned and re-spawned every frame while the map is open (a few dozen entities), rather than moved. The alien pseudo-random generator is a hand-rolled hash in the view; it is cosmetic only.
**Why**: churn is bounded (map open only), but entity ids and layout dirtiness change every frame.
**Suggestion**: keyed update (`Local<HashMap<Entity, Entity>>` from world entity to icon) or accept and note. If the hash is only for visuals, document "cosmetic, not the sim RNG" (checklist 4.4).
**Behaviour-preserving?**: needs-check (visual identity must stay the same)
**Effort**: M

##### B6-26 [severity: low] [category: bevy] Layer test regex misses common widget constructors
**Where**: `tests/test_layers.py` `WIDGETS = r"\bNode \{|\bImageNode::new|\bText::new|\bBackgroundColor\(|\bTextFont \{|\bText2d\b"` (outside this bucket's files but it guards them).
**What**: shared code that builds UI via `Node::default()`, `ImageNode { .. }`, `Button`, `BackgroundGradient`, `children![ ... ]` with `Text(..)`, or `commands.spawn(Node::DEFAULT)` would pass. Currently clean (verified: only `lib.rs`, `sprites.rs` build UI in shared code).
**Suggestion**: extend the regex to `Node::default|Node::DEFAULT|ImageNode \{|ImageNode::default|BackgroundGradient|BorderColor|\bText\(|UiTransform`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B6-27 [severity: low] [category: simplify] `ship_preview.rs` and `console.rs` are not presentation-only and sit beside retail views
**Where**: `ship_preview.rs` (offscreen render target `SIZE 786x570`, `LAYER 7`, drag rotation `DRAG=0.01` via `Interaction`) and `console.rs` (dev overlay with `ShapeKind` colours).
**What**: both are generic tools (3D viewport, debug console) rather than retail look; the console reads `crate::rules::console` state.
**Why**: a second presentation will need both; neither is retail-specific. Not wrong, just ownership: they are reusable functionality inside `classic/`.
**Suggestion**: leave as is, but record in `docs/development.md` "classic/" row that ship preview and the debug console are reusable by other views, or move them to shared `ship_preview.rs`/`debug_console.rs` (they only need `Node`/camera, and the layer test forbids Node in shared; so they would need to stay in a view crate or `DRAWS_OK`). Keep as is.
**Behaviour-preserving?**: yes.
**Effort**: S (doc)

##### B6-28 [severity: low] [category: bevy] `Time<Real>` used for banner fade and speech-time based UI is fine, but `bar_button_art` reads `Time<Virtual>` paused state in a view
**Where**: `world_hud.rs` `place_labels` (`Res<Time<Real>>`), `panels.rs:622` `bar_button_art(vtime: Res<Time<Virtual>>, ...)`.
**What**: the paused-art swap on the PAUSE button asks Bevy's virtual clock directly; `crate::panels` owns `pause_button` and the pause semantic (also `Menu` overlays pausing).
**Why**: if the pause concept ever gains a second reason (menu overlay, briefing), the art and the real state can diverge.
**Suggestion**: expose `panels::is_paused(&Time<Virtual>) -> bool` or a `PauseState` resource and use it in both places. Needs a quick check that Esc-menu pause sets `Time<Virtual>` (the doc says it pauses).
**Behaviour-preserving?**: needs-check
**Effort**: S

#### Quick wins
- Add `pub fn touched(&Interaction) -> bool` and an `art_state(Interaction)` helper, and replace the ~15 `!= Interaction::None` tests and three `Interaction`-to-state matches (B6-03/B6-21). Prepares the 0.20 Interaction migration.
- Make `Theme::default()` come from `include_str!` of `content/data/hud/theme.json` and drop the duplicate (B6-10).
- Replace the panics in `BarLayout::load` and the font `expect`s with logged fallbacks (B6-06, B6-19).
- Name and comment the loose constants (chat panel metrics, `255.0`, `76.0`, `44.0`, boarding bar colours, banner `50.0`) and add PROVISIONAL tags (B6-08, B6-24); name `CHAT_PAD`/`THUMB` once (B6-13).
- Rename the two `Ctx` types (`FrontEndCtx`, `HudCtx`), delete `player_names::px` and the two duplicate `absolute` helpers (B6-04).
- Collapse the `if name == ...` chain in `spawn_part` into one match on a `Role` enum and fix its stacked doc comment (B6-05, B6-22).
- Make `crew_arms::follow_cursor` use `widgets::canvas` and give `1080.0`/`960.0`/`800x600` named consts (B6-18).
- Tighten `tests/test_layers.py` `WIDGETS` regex (B6-26).
- Write the reticle with a single `Visibility` write (B6-16).

---

## Appendix: Front-end model, setup and app states

Reviewed: `game/src/menu.rs`, `menu/{skirmish,lobby_scenario,missions,mods,saves,settings}.rs`, `options_menu.rs`, `setup.rs`, `setup/{crash,import,install}.rs`, `bindings.rs`, `loading.rs`, `starmap.rs`, `profiles.rs`. I skimmed the tests.

Overall: the front-end model is clean where it matters. `Screen` is a real `States` type. Run conditions (`in_state`, `menu_active`, `resource_exists`) are used rather than early returns. The layering rule holds: views draw from `MenuModel` and tag buttons with `Action`. The file formats are careful: `profiles::write` and `campaign::write_save` write to a temp file and then rename, and `Bindings::load` repairs clashing keys. The setup app is well thought out: staged import with swap-in and rollback, a retry for Windows renames, a crash screen, and parsers that are tested without a registry. PROVISIONAL marking is mostly diligent, and the computer fleet builder cites measured data. The weak spots are as follows.
- The sim reads UI settings through process-global atomics (`menu::difficulty()`).
- `MenuKey`'s hand-listed fields appear to omit some dialog flags.
- `Action` is a 45-variant god enum handled in several places.
- `skirmish.rs` mixes the lobby UI model with the computer-fleet AI.
- Race, difficulty and similar choices are raw `u8` values with magic numbers.
- Dev-script hooks are compared against strings in production modules.
- The process-spawning plumbing is duplicated and lives in the wrong module.

#### Findings

##### B7-01 [severity: high] [category: fidelity-risk] Combat sim reads a UI setting through a process-global atomic
**Where**: `menu/settings.rs:65-100` (`CHANNELS`, `DETAIL`, `AA`, `VSYNC`, `CONSOLE`, `DIFFICULTY` statics, `difficulty()`), written by `apply_settings` (`settings.rs`, `Update`). Read from the FixedUpdate sim at `combat/shells.rs:522,710`, `combat/lob.rs:195,285,320`, `combat/damage.rs:245,314`, and `fx/sim.rs:61` (`detail_scale()`). `Settings::load()` is called in `menu::plugin` (`menu.rs:286`).
**What**: Damage scaling by difficulty is a sim input. It is copied from a `Resource` into a static in `Update` and read from `FixedUpdate` through `crate::menu::difficulty()`. `combat/` therefore depends on the front-end module and on hidden global state.
**Why**:
- It breaks the "sim state is explicit and traceable" rule (checklist 4.1, 4.5, 4.6).
- A headless trace or replay test depends on whatever `settings.json` is in the developer's user dir.
- The static is written in `Update`, one frame after the resource changes. A tick can therefore see a stale value.
- Tests of `side_damage_scale` need the global to be at its default.
**Suggestion**:
- Add a small `Difficulty` resource (enum `Easy/Normal/Hard`) owned by `combat/` or `scenario`.
- Snapshot it from `Settings` at `OnEnter(Screen::Battle)`, so a mid-battle option change cannot change the rules.
- Sim code takes `Res<Difficulty>`.
- Keep the atomics only for the pure presentation values (`volume`, `detail_scale`, `anti_alias`) until they can be resources too.
- `--script`/trace runs should force `Difficulty::Normal` unless a flag says otherwise.
**Behaviour-preserving?**: yes for normal play (the same value, snapshotted at battle start), but it changes the behaviour of changing difficulty mid-battle, so check with the maintainers. Retail applies it at battle start, but I could not verify that here.
**Effort**: M

##### B7-02 [severity: high] [category: bevy] `MenuKey` appears to omit lobby/dialog state, so the menu may not rebuild
**Where**: `menu.rs:482-529` (`MenuKey`, `MenuModel::key`); consumer `classic/menu.rs:52-60`.
**What**:
- The key is a 10-tuple plus a `format!("{:?}|...")` string.
- It lists some `Skirmish` fields (`dropdown`, `name_open`, `flag_picker`, `slot_scroll`, ...).
- It does not list `sk.map_info`, `sk.arms_open`, `sk.arms_tab` or `sk.profile` (the `ProfileDialog`).
- Those are mutated by `Action::MapInfo`, `OpenArms`, `OpenCrew` and `OpenProfile`/`PickProfile`/`CloseProfile` in `apply` (`menu.rs:797-809`).
- `classic/menu.rs:126-129,889` branch on them inside `build`.
- `build` skips the rebuild when `last == key`.
**Why**:
- Either the dialogs are drawn some other way I did not trace, or opening Map Info or the Load/Save profile dialog does not redraw until something else changes.
- Smoke scripts set these flags before the first build, so they would not catch it.
- Either way the design is fragile. Every new `Skirmish` field must be remembered in a positional tuple and in a Debug-formatted string.
**Suggestion**: Confirm with a click-through of Map Info and Save Fleet.
Replace the key with something that cannot go stale:
- Option 1: derive `PartialEq` on a `MenuSnapshot` struct that holds clones of the drawn state (`Skirmish` already derives enough once `map_json` is excluded).
- Option 2: drop the key and rebuild on `model.changed()`, with `ResMut` access in `apply` guarded by `if *new != old` so no spurious change is flagged.
- Option 3: a `MenuRevision(u32)` resource bumped by `apply`/`buttons`/`save_actions` after real changes.
**Behaviour-preserving?**: needs-check (it may fix a latent bug).
**Effort**: M

##### B7-03 [severity: medium] [category: bevy] `Action` is a 45-variant god enum with scattered, catch-all handling
**Where**: `menu.rs:60-130` (enum), `menu.rs:745-846` (`apply`, `_ => {}`), `menu.rs:868-953` (`buttons`, `_ => {}`), `menu/saves.rs:31-170` (`save_actions`, `_ => {}`), plus `crew_arms.rs:247`.
**What**: Every button press is matched in up to four systems, each ending in `_ => {}`. `buttons` takes 15 resources in tuples. `Action::Goto`, `Quit` and `Campaign` are handled in `buttons`, `PickMap` in `apply`, and `OpenLoad` in `save_actions`. The same `Query<(&Interaction,&Action), Changed<Interaction>>` is scanned in three systems.
**Why**:
- Adding a variant gives no compiler help about who handles it.
- Systems that touch `ResMut<Skirmish>`, `ResMut<Bindings>`, `ResMut<ModList>` and so on cannot run in parallel.
- It is hard for a new contributor to find "where does Launch happen".
**Suggestion**:
- Group variants into sub-enums by owner (`SkirmishAction`, `SaveAction`, `ModsAction`, `SettingsAction`, `NavAction`), keeping `Action` as a thin wrapper if the views need one tag.
- Better, make the press an entity event, so each owning module registers an observer or system for its own sub-action.
- The classic view only needs to attach the component. The view contract (`Action` as the marker) stays.
- The `_ => {}` arms become exhaustive per owner.
**Behaviour-preserving?**: yes.
**Effort**: L (mechanical, many call sites in `classic/`)

##### B7-04 [severity: medium] [category: bevy] The in-game menu overlay is a hand-rolled sub-state, with manual despawn and unpause in three places
**Where**: `menu.rs:142-173` (`Page`, `InGame`, `overlay_closed`, `menu_active`), `menu.rs:644-712` (`esc_menu`, `summary_on_end`, `close_overlay`), `menu/saves.rs:100-145` (calls `close_overlay` twice), `menu.rs:339-344` (`clear_menu`).
**What**:
- `InGame { page: Option<Page>, paused_by_us }` is effectively `SubStates` of `Screen::Battle`.
- Opening it pauses `Time<Virtual>` and remembers whether it did.
- Closing despawns every `MenuRoot` and unpauses.
- Each of the three open paths (`esc_menu` scripted, `esc_menu` key, `summary_on_end`) repeats `(ig.paused_by_us, ig.page) = (!vtime.is_paused(), Some(..)); vtime.pause()`.
**Why**: The pause/despawn symmetry is what `OnEnter`/`OnExit` of a sub-state and `DespawnOnExit` are for. `overlay_closed` and `menu_active` would become `in_state(...)` conditions.
**Suggestion**:
- Add `#[derive(SubStates)] #[source(Screen = Screen::Battle)] enum InGameMenu { #[default] Closed, Open }`, or one `Option<Page>` resource as now with a single `fn open_page(&mut ig, &mut vtime, page)` helper.
- Pause and unpause in `OnEnter`/`OnExit(InGameMenu::Open)`.
- Replace `clear_menu` with `DespawnOnEnter(Screen::Battle)` on `MenuRoot`, if the view adds it at spawn.
- Note the 0.18+ semantics: `NextState::set(Battle)` from inside Battle re-runs `OnEnter` (`saves.rs:140` already relies on this).
**Behaviour-preserving?**: needs-check. The Esc handling order in `PostUpdate` and the scripted frame-60 open must stay intact.
**Effort**: M

##### B7-05 [severity: medium] [category: bevy] The battle-start gate is a resource plus polling, not a state
**Where**: `loading.rs:20-134`; the `Loading` resource is read in about 10 other modules via `loading::done` / `resource_exists`.
**What**:
- `Screen::Battle` covers both "loading under the cover" and "playing".
- The gate is a `Loading` resource plus `wait` in `PreUpdate`, which each frame runs `pending()` over every `WorldAssetRoot`/`HdSky` and scans `Assets<StandardMaterial>` (`plain`).
- It then requires `SETTLE = 10` quiet frames.
**Why**:
- A two-value `SubStates` or `ComputedStates` (`BattlePhase::{Loading, Running}`) would give `OnEnter(Running)` for unpause and music, and `in_state(Running)` for systems.
- Per-frame `Assets::iter()` over all materials, plus the settle count, is the polling smell in checklist 2.4. It is acceptable here, because dependencies are dynamic and the code documents why.
**Suggestion**:
- Keep the polling core.
- Expose the result as a `BattlePhase` computed state, so systems write `run_if(in_state(BattlePhase::Running))` instead of `loading::done`.
- This also gives `hush` and `wait` a clear `OnExit(Loading)` hook.
- Make `SETTLE` and `GIVE_UP` PROVISIONAL-tagged tuning consts. They are guesses that merely work today.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B7-06 [severity: medium] [category: simplify] `skirmish.rs` (1070 lines) mixes the lobby UI model, the computer-fleet AI, plan building and stats
**Where**: `menu/skirmish.rs`: UI model `Slot`/`Skirmish`/`choose_inner` (1-300, 690-900), AI fleet build (`Build`, `style_list`, `swap_pool`, `auto_build`, ~300-641), `plan` (913-980), `skirmish_maps`, `ship_stats`, `flag_icon`.
**What**: Lines ~300-641 are a self-contained, measured, mostly pure algorithm with its own consts and tests (`ai_weapon_tests`). They share only `Slot`, `Library` and `offered` with the lobby.
**Why**:
- It is the "one concept per module, split along real seams" case (checklist 5.1).
- The doc comment for `auto_build` sits above `computer_guns` (lines 304-310: "Auto Assign. Measured on the original..." is attached to `fn computer_guns`).
- Two other doc comments are also misplaced: `scroll_slots`/`open_profile` (820-821, "Keep the picked fleet slot inside the slot bar." above `open_profile`) and `launch_error` (887-888, "Why the game cannot launch yet" above `points_left`).
**Suggestion**:
- Split into `menu/skirmish.rs` (the `Skirmish` resource, slots, pull-downs, launch check), `menu/skirmish/fleet_build.rs` (`Build`, styles, swap pools, `auto_build`), and `menu/skirmish/plan.rs` (`Skirmish::plan`).
- Move each doc comment to its function in the same change.
**Behaviour-preserving?**: yes (pure move).
**Effort**: M

##### B7-07 [severity: medium] [category: simplify] `Skirmish::plan` is a 70-line closure-in-iterator doing three jobs
**Where**: `menu/skirmish.rs:913-980`.
**What**:
- One `.map(|s| {...})` builds ships, a side, and a `Group`.
- It carries a `computer_build: Option<(weapons, crew)>` tuple that is unpacked twice with near-identical `match` blocks (`weapons:` and `crew:` fields).
- It uses 3-tuples such as `(String, Option<[f32;3]>, Option<String>)` plus a parallel `Vec<Option<i64>>` of ids.
**Why**: This is hard to read and hard to extend, for example for a new slot kind. The placed-ship tuple and the parallel `ids` vector are unnamed.
**Suggestion**:
- Extract `fn slot_ships(&self, s, lib, h) -> SlotShips { ships, ids, weapons, crew }` with one arm per case (placed, computer-built, human-assembled).
- Extract `fn side_of(&self, s, h, foes) -> Side`.
- This removes the double `match computer_build`.
- Give the tuple a struct (a `PlacedShip` type may already exist in `crate::Group`).
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B7-08 [severity: medium] [category: rust] Race, "no race" and other enums are bare `u8`/tuples with magic numbers
**Where**:
- `Slot.race: u8` (`skirmish.rs:54`, "0 Navy, 2 Pirate, 3 Procyon, 4 none").
- `race == 4`: `skirmish.rs:898,961`, `lobby_scenario.rs:100`, `menu.rs:7xx`.
- `RACES: [(u8,&str,&str);3]` (`:293`), `race_multiplier` (`:415`), `flag_icon` (`:1042`).
- The literal `[0u8, 2, 3]` repeated at `:742` (twice) and `:787`, instead of `RACES`.
- `Skirmish.maps: Vec<(String,String,bool,usize)>` (`:217`, `:985`), indexed as `m.0`/`m.2` in `menu.rs`, `lobby_scenario.rs`.
- `Settings.aa/detail/difficulty: u8` with `% 3` and `min(2)` (`settings.rs`).
- `MenuKey`.
**What**: There are many numeric codes where a type would document and constrain them. Race ids 0/2/3 are retail TWT values and must stay, but they can sit behind an enum with `as_twt()` and `from_twt()`.
**Why**:
- C-CUSTOM-TYPE and C-NEWTYPE.
- A bad value cannot be represented, and `match` becomes exhaustive.
- Anyone reading `h.race == 4` has to find the comment on a struct field.
**Suggestion**:
- `enum Race { Navy, Pirate, Procyon, Unset }` with `fn twt(self) -> u8` and `fn from_twt`, and an `ALL_PLAYABLE: [Race; 3]` that `choices`, `race_multiplier`, `flag_icon` and `RACES` all use.
- A `struct SkirmishMap { id, name, open, players }` replaces the 4-tuple.
- `enum AntiAlias`, `EffectDetail` and `Difficulty` replace the `u8`s. Serialisation stays numeric (see B7-10).
**Behaviour-preserving?**: yes.
**Effort**: M

##### B7-09 [severity: medium] [category: modding] Skirmish races, flag blocks and computer fleet pools are hardcoded; a mod can add maps and ships but not a race
**Where**:
- `skirmish.rs:293` (`RACES`).
- `:742,787` (`[0,2,3]`).
- `:415-422` (`race_multiplier`).
- `:349-368` (`swap_pool`).
- `:373` (`style_list`).
- `:1042` (flag icon block of 10 per race).
- `:343` (`computer_hull` excludes "Tender"/"Tug" by name).
**What**:
- Good: `skirmish_maps()` reads `maps/<id>/map.json` flagged `"skirmish"`, so a mod can add maps through the overlay.
- Good: ships appear if they carry `skirmish: true` and a race string in `RACES`.
- Good: `data/ai/weapons.json` already controls computer guns.
- The following are code-only: the race list, the scaled-price multipliers, the swap pools by mount size (a mod gun is never swapped in), and the flag layout. They are all marked or measured, but not data.
**Why**: A modder who adds a fourth fleet type, or a gun that computers should use, must edit Rust.
**Suggestion** (needs maintainer approval: a new data file under `data/`):
- An optional `data/skirmish/races.json` (id, name, string id, flag icon base, `price_multiplier`) with the current values as the built-in default.
- Extend `data/ai/weapons.json` with an optional `pools` block, or fall back to `swap_pool`.
- Keep the retail values as consts (`RETAIL_RACES`) so classic behaviour is pinned by the tests.
- Replace the `Tender`/`Tug` name test by a class flag if the exporter can provide one (already marked PROVISIONAL).
**Behaviour-preserving?**: yes with a retail default.
**Effort**: L

##### B7-10 [severity: medium] [category: rust] Settings persistence is hand-rolled, lossy and non-atomic
**Where**: `menu/settings.rs:112-143` (`load`, `save`); `bindings.rs:178-205`.
**What**:
- `load` reads each key with `v[k].as_u64().map_or(def, |x| (x as u8).min(max))`. `x as u8` wraps, so `256` loads as `0` (mute) instead of clamping or falling back.
- `Settings::save` and `Bindings::save` use plain `fs::write` (the others, `write_save` and `profiles::write`, use temp + rename), and ignore `create_dir_all` errors.
- A corrupt or unreadable `settings.json`/`bindings.json` silently becomes defaults, with no `warn!`. `try_json` swallows both errors.
- The struct is written with a 12-line `json!` literal and read with a mirrored 12-line block, so a new field must be added in four places (struct, `Default`, `load`, `save`).
**Why**: Bevy 0.19's `SettingsPlugin` is the idiomatic route. The file format can stay compatible if keys and names are pinned, as the checklist notes. A hand-rolled file is acceptable if it is robust and has one definition of the format.
**Suggestion**:
- `#[derive(Serialize, Deserialize)] #[serde(default)] struct Settings` with `#[serde(deserialize_with = ...)]` clamps, or a post-load `sanitize()`. The existing keys `master, music, effects, dialogue, fullscreen, vsync, aa, detail, background, flags, difficulty, console` stay exactly as they are, so the file stays compatible.
- Add a regression test that round-trips the exact current JSON.
- Introduce one `fn write_atomic(path, &str) -> io::Result<()>` in `data.rs` and use it for settings, bindings, `write_save`, `profiles::write`, and `modding::save_enabled_in`.
- `warn!` on parse failure.
- Adopting `SettingsPlugin` itself would be a dependency or format decision, so flag it to a maintainer. Serde derive alone achieves the cleanup.
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B7-11 [severity: medium] [category: simplify] Dev and smoke hooks live in production modules and compare against strings
**Where**:
- `menu.rs:657-665` (`esc_menu`: `Some("ingame-load" | "ingame-do-load")` and so on).
- `menu/saves.rs:48,56-59` (`"ingame-do-load"`, `"quick"`).
- `options_menu.rs:481-486,515-520` (`"dock-click"`, `"menu-selected"`).
- `mods_bake_hook`, `missions_play_hook`, `fleet_hook`, and the screenshot hook in `autolaunch` (`menu.rs:532-640`).
- `dev_click`, `dev_menu`, `Script` itself (`smoke/mod.rs:27`).
**What**: `cfg.script` is a typed `Option<smoke::Script>`, but `Script` derefs to `str`, so these sites compare `cfg.script.as_deref() == Some("quick")`. A typo, or removing a name from `smoke/mod.rs`, compiles silently. Other sites in the same files already use `Some(crate::smoke::Script::ModsBake)`.
**Why**:
- AGENTS and `docs/development.md` say `smoke/` owns script names and drivers.
- About 150 lines of test-only logic sit inside `menu.rs`, `saves.rs` and `options_menu.rs`, next to the real code.
- The `Deref<Target=str>` impl invites this.
**Suggestion**:
- Compare enum variants (`Some(Script::Quick)`, `Some(Script::IngameDoLoad)`, ...).
- Then move the hooks to `smoke/menu.rs`, registered from the `smoke` plugin with the same sets (`.after(BuildMenu).before(RunButtons)` for `missions_play_hook` and so on).
- Consider removing `Deref for Script` in favour of an explicit `.name()`.
**Behaviour-preserving?**: yes.
**Effort**: S (enum comparisons), M (move)

##### B7-12 [severity: medium] [category: simplify] Process-spawning plumbing is duplicated and lives in `setup`, but is used by `menu/mods.rs`
**Where**: `setup/import.rs:137-190` (`spawn_tool`, returns `(String, Arc<Mutex<Child>>)`), `setup.rs:60-70,231-241` (`Setup`: `rx: Mutex<Receiver>`, `child`, `Drop` kills), `menu/mods.rs:30-52` (`Job`: same `rx`, `child`, `_stdin`, `Drop` kills, `last`), `use crate::setup::{ToolMsg, spawn_tool, parse_progress}`.
**What**: Two resources hand-implement "a running tool process with an output channel that is killed on drop". The Mods screen imports from the first-run setup module, an odd dependency direction. The completion thread polls `try_wait` every 50 ms while holding the `Mutex`, so a `kill` from the main thread waits on it.
**Why**: The checklist wants one owner per concept, and `docs/development.md` says `setup.rs` owns "how a player gets from a download to the menu". The bake is not that.
**Suggestion**:
- Extract `tool.rs` (`ToolRun { rx, child, stdin }`, `ToolMsg`, `spawn_tool`, `importers`, `parse_progress`, `no_window`) and use it from both `Setup` and `BakeJob`.
- `ToolRun::poll() -> Vec<ToolMsg>` and `Drop` become the single implementation.
- Replace the returned tuple with a `ToolHandle` struct.
- Keep `setup::*` re-exports so the public paths stay stable.
- Wait for the child with `wait()` on a thread that owns it, and signal kills through a handle, to remove the 50 ms poll.
**Behaviour-preserving?**: yes.
**Effort**: M

##### B7-13 [severity: medium] [category: rust] The importer lookup embeds the build machine's source path in release binaries
**Where**: `setup/import.rs:111` `PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")`.
**What**: `CARGO_MANIFEST_DIR` is an absolute path at compile time (for example `/home/<user>/...` or a CI runner path). It is baked into every shipped binary. The "from the checkout" fallback also only works when the binary runs on the machine that built it, or from the unmoved checkout.
**Why**:
- Hard rule 8 is about tracked files, but a release binary that carries the builder's home path leaks personal information.
- The fallback is wrong for anyone who downloads a binary.
**Suggestion**:
- Locate the checkout at run time: walk up from `current_exe()` (`target/{debug,release}/openprocyon` implies the checkout two levels up) and look for `tools/import_assets.py`. Optionally honour a `PROCYON_REPO` or `PROCYON_IMPORTER` env var.
- Or cfg-gate it behind `debug_assertions`.
**Behaviour-preserving?**: yes for dev checkouts, as long as the walk-up finds the repo.
**Effort**: S

##### B7-14 [severity: medium] [category: rust] The crash report includes the full command line (may contain personal paths)
**Where**: `setup/crash.rs:25-37` (`crash_report`: `arguments: {}` from `std::env::args().skip(1)`), and the backtrace and `info.location()` (build-machine source paths).
**What**: The log says "Please attach this file when you report the problem". Arguments such as `--game C:\Users\Alice\...`, `--mod`, `--assets` or `--out` carry user names and install paths. Release builds' backtraces can include the builder's source paths.
**Why**: The project treats personal paths as a hard rule (8). A report intended to be posted publicly should not contain them.
**Suggestion**:
- Redact the home directory prefix (replace `HOME`, `USERPROFILE`, `APPDATA` values with `~`) in `arguments` and the backtrace, or list only flag names.
- Say in the header that the file contains paths and that the player should review it.
**Behaviour-preserving?**: yes (report text only).
**Effort**: S

##### B7-15 [severity: medium] [category: bevy] Install discovery runs synchronously while the setup window is being built
**Where**: `setup.rs:Setup::new` (`find_installs()` on the main thread), `setup/install.rs:181` and `disc_candidates` (three `reg query ... /s` over whole `Uninstall` hives on Windows, Steam library parsing, directory scans).
**What**: This runs before `App::run`, so the window does not appear until it finishes. `reg query /s` on the 32-bit hive can take seconds.
**Why**: The first-run screen is the first thing a new player sees, and it appears to hang.
**Suggestion**:
- Create `Setup` with `found: vec![]`, spawn the scan on a thread (or `AsyncComputeTaskPool`), and deliver the result as a `Msg::Found(Vec<PathBuf>)` through the existing channel.
- The `Msg` enum and `poll` are already built for this.
- Fill `path` from the first result only when the user has typed nothing.
**Behaviour-preserving?**: yes (it changes timing only).
**Effort**: S-M

##### B7-16 [severity: low] [category: simplify] The setup app's phase is a hand-rolled enum guarded inside systems; `shot` re-collects argv every frame
**Where**: `setup.rs:34-48` (`Phase`), guards in `keys`, `dropped`, `buttons` and `poll` (`if setup.phase != Phase::Choose`), `shot` (`setup.rs:391`: `std::env::args().collect()` on every `Update`), `run` (`setup.rs:410`: the same `arg` closure repeated).
**What**: `Phase` carries data (`Importing { step, name, ... }`), so it cannot be a `States` directly. A data-less `SetupScreen` state (`Choose/Importing/Failed/Starting/Crashed`) beside the `Setup` resource would still allow `run_if(in_state(Choose))` and `OnEnter(Starting)` for `relaunch`, which today needs a `Local<bool>` guard.
`shot` allocates a `Vec<String>` each frame before returning early.
**Suggestion**:
- Parse `--setup-shot`/`--shot-frame` once in `run` and store them in a `ShotRequest` resource (or run `shot` only when it is `Some`, using `.run_if(resource_exists::<ShotRequest>)`).
- Share the `arg("--x")` helper with `lib.rs::Config`, which presumably parses the same flags.
- Optional: add a data-less state for the phase.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-17 [severity: low] [category: simplify] `options_menu.rs::menu` is a ~300-line, 28-parameter system holding its own state machine in `Local<Local2>`
**Where**: `options_menu.rs:180-415` (`menu`), `:45-55` (`Local2`, `Open`), `:25-40` (`OptionsMenu` with `AtomicBool`s).
**What**:
- The one system does:
  - open/close bookkeeping;
  - right-click gesture detection;
  - slot picking;
  - a 100-line `match` that performs every action;
  - three near-identical `ships.iter().filter(...).filter_map(ray_hit).min_by(...)` chains (`ally`, `enemy`, `own`, and a third in `context_cursor`);
  - menu spawning.
- State lives in `Local<Local2>` with `Entity` back-references (`root`, `actor`, `target`) and a `commands.get_entity(o.root).is_err()` stale check (checklist 1.10).
- `OptionsMenu` holds `AtomicBool`s "so the menu system can share the resource with `Dockees`", but `menu` and `context_cursor` only ever read or write it through `Res`. A plain `bool` in a `ResMut`, or a marker component on the menu root, would do.
- Naming: `Local2`, and private `Kind`/`Act` enums that clash with `menu::Kind` and `bindings::Act`.
**Suggestion**:
- Split into `detect_click`, `choose_slot -> Option<Act>`, and `run_act(act, ...) -> sound`.
- Extract `fn nearest_ship_hit(ships, lib, ray, pred)`.
- Make the open menu a component on its root entity, so the entity is the state.
- Replace the atomics with a `ResMut`.
- Rename the types (`MenuVariant`, `MenuItem`).
**Behaviour-preserving?**: yes.
**Effort**: M

##### B7-18 [severity: low] [category: simplify] Stale module doc and PROVISIONAL gaps in `options_menu.rs`
**Where**: `options_menu.rs:1-9` ("Hold position and Cloak are drawn greyed out", but `items` implements both and `AGENTS.md` and `docs/development.md` say they are implemented), `:15-16` (`CLICK_PX = 6.0`, `CLICK_SECS = 0.4`), `menu_centre` (`70.0 * s` clamp).
**What**:
- The header contradicts the code.
- The click thresholds and the 70 px edge margin are guesses with no PROVISIONAL mark or source.
**Suggestion**:
- Fix the doc line.
- Name the consts with a "PROVISIONAL: the original's click tolerance is not measured" comment, and name the 70.0 margin.
- Use the same threshold names as the `DOUBLE_CLICK` comment style in `menu.rs:848`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-19 [severity: medium] [category: bevy] The `Interaction`-polling UI is blanket-allowed as deprecated
**Where**: `#![allow(deprecated)]` at `menu.rs:6`, `setup.rs:7`, `starmap.rs:15` (the same also in `panels.rs`). Polls: `button_sounds`, `buttons`, `save_actions`, `scroll_drag`, `slider_drag`, `dropdown_release`, `setup::buttons`.
**What**: Bevy 0.20 deprecates `ui::Interaction` in favour of `Hovered`/`Pressed` components and the `Activate` event. The comment says "shortcut ... port when it is removed". The module-wide allow also hides any unrelated deprecation in these 900-line files, and the allow has no `reason =` (checklist 5.6).
**Why**: This is the largest pending API port, and it has a clean seam: all UI input goes through `Action` (B7-03). Porting there once serves menu, setup and starmap.
**Suggestion**:
- Port as part of B7-03. The view attaches `Action` plus the new button component, and an observer on `Activate` runs the action.
- Interim: narrow the allow to the handful of items that name `Interaction` (`#[expect(deprecated, reason = "...")]`), so new deprecations still warn.
- Replace "port when it is removed" with a tracked issue.
**Behaviour-preserving?**: yes.
**Effort**: M-L

##### B7-20 [severity: medium] [category: rust] `std::process::exit(2)` and `eprintln!` inside a system
**Where**: `menu.rs:613-619` (`lobby_scenario`).
**What**: On a bad `--lobby` spec it prints and calls `std::process::exit(2)` from an `Update` system. That skips `AppExit` handling (the `Last` schedule, `stop_bake_on_exit`, audio, window teardown) and cannot be tested.
**Suggestion**: `error!("{err}")` and `exit.write(AppExit::Error(NonZeroU8::new(2).unwrap()))`. Better, validate the spec in `lib.rs::lobby_scenario` (`lib.rs:136`, which already parses it) as far as possible without the library. `LobbyScenario::apply` needs the map and library, so exiting through `AppExit::Error` is the minimum.
**Behaviour-preserving?**: yes (the same status code).
**Effort**: S

##### B7-21 [severity: low] [category: rust] Remaining `unwrap`/`expect` and silent failures in non-test code
**Where**:
- `skirmish.rs:871` (`self.profile.as_mut().unwrap().error = Some(e)`; the dialog was cloned a few lines earlier, so it is probably always `Some`, but the pattern is fragile).
- `options_menu.rs:243` (`open_menu.take().unwrap()` right after `is_some()`).
- `options_menu.rs:501` (`expect("dock icon")` in a dev hook).
- `profiles.rs:123` (`serde_json::to_string_pretty(..).unwrap()`).
- `profiles::list`, `Bindings::load`, `Settings::load`, `list_saves` all drop parse errors silently (`filter_map`/`.ok()`).
**Why**: Checklist 5.2 and 1.14. One unreadable save or profile just vanishes from the list. A player will not know why a fleet they saved is missing.
**Suggestion**:
- Use `if let Some(open) = open_menu.take()`.
- `if let Some(d) = self.profile.as_mut()`.
- Log `warn!("skipping {path}: {err}")` once per bad file in the list readers, via a `read_json_logged` that returns the error.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-22 [severity: low] [category: simplify] Duplicated text-entry and file-name logic
**Where**: `menu/saves.rs:118-136` (`save_text`) versus `:140-170` (`profile_keys`); `campaign::clean_name` and the 32-character limit repeated; `Rebind` capture in `menu.rs:715`; `setup::keys`.
**What**: `Key::Character` / `Key::Space` / `Key::Backspace` handling with the same `chars().count() < 32` and control-char filter is written twice.
**Suggestion**: `fn edit_text(buf: &mut String, key: &Key, max: usize) -> bool` in `menu.rs` (or a small `text_input.rs`), used by both. A `const NAME_MAX: usize = 32` replaces the literals.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-23 [severity: low] [category: modding] Hard-coded mod ids and sprite/sound id strings in engine code
**Where**: `menu/mods.rs:12-15` (`HD_SKIES = "hd-skies"`, `BAKE_FROM = [HD_SKIES, "remaster"]`, and `split_stars` checks `id == "remaster"`); `menu.rs` `button_sound` returns literals (`"FE_Button_Select"` ...); `menu.rs:322` (`"FrontEndMusic"`).
**What**:
- The mod browser special-cases two mods by id. The "bake" offer cannot be attached to a third mod, and the `split_stars` rule is tied to a mod's folder name.
- The sound and track names come from retail files, so they are fine as consts, but they are scattered.
**Suggestion**:
- A `bake` key in a mod's manifest (needs maintainer approval: the data format) so `offers_bake` and `split_stars` become manifest-driven.
- Collect front-end sound names in one `const SOUNDS` table.
- Short term, add a comment saying the two ids are the two mods that `mods/` maintains.
**Behaviour-preserving?**: yes.
**Effort**: S-M

##### B7-24 [severity: low] [category: bevy] The menu plugin does file I/O and parses argv while the app is built
**Where**: `menu.rs:243-312` (`plugin`: `skirmish_maps()` reads every `maps/*/map.json`, `list_saves()`, `Settings::load()`, `ModList::load()`, `MissionList::load()`, and `std::env::args()` for `--map`), `bindings::Bindings::load()` in `lib.rs:269`.
**What**: The plugin builds eight resources from disk in `build`, before the asset overlay order is visible to the plugin as an explicit step. It re-parses `--map` from argv even though `crate::Config` exists (`menu.rs:246` versus `Config.map`).
**Why**: The checklist wants loading in `Startup`/`OnEnter` so it can be re-run (for example, after a mod toggle) and unit-tested. `menu::plugin` is hard to test without the assets directory.
**Suggestion**:
- Insert `Default` resources in the plugin, and load them in a `Startup` system ordered after the asset root is set.
- Take the initial map from `Config`.
- A "reload lists" action (after the bake, after a mod toggle) then reuses the same functions.
**Behaviour-preserving?**: yes if ordering relative to `Config` is kept.
**Effort**: M

##### B7-25 [severity: low] [category: simplify] Small cleanups in `menu.rs`, `settings.rs`, `saves.rs`
**Where** and **What**:
- `menu.rs:350-351`: two consecutive doc lines for `strings()`.
- `menu.rs:346` `text_of` could be `strings[id].as_str().unwrap_or("").to_owned()` or `Cow`.
- `menu.rs:468-471` `if v != sk.victory_limit { sk.victory_limit = v }` (the hand-rolled change guard is fine, but `set_if_neq`-style helper exists for resources).
- `menu.rs:899-901` `Action::Quit` writes `AppExit::Success` while `Setup` does the same, so the match arm is identical.
- `settings.rs:80-90`: `volume()` maps `Setting` -> index with a `_ => 0` fallback, so `volume(Setting::Fullscreen)` silently returns master volume. Use a dedicated `enum Channel { Master, Music, Effects, Dialogue }`, which also removes `CHANNELS[i]` indexing.
- `settings.rs` `Settings::rows`: label strings hard-coded in English; if the project localises later, they bypass `strings()`.
- `saves.rs` `acts.reverse(); while let Some(a) = acts.pop()` pops in forward order after reversing. A `VecDeque::pop_front` states the intent.
- `Settings::step` uses `% 3` and `>= 10` magic. A `const VOLUME_MAX: u8 = 10` and enum lengths would remove it.
**Suggestion**: Do these as one tidy commit.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-26 [severity: low] [category: simplify] `starmap.rs::toggle` mixes sound, input and disk I/O with odd `Local` state
**Where**: `starmap.rs:75-122`.
**What**:
- `Local<Option<bool>> auto` plus `std::env::args().any(|a| a == "--starmap")`, evaluated per frame until `auto` is set. A `Local<bool>`, or a `Config` flag, would do.
- On open it reads `maps/<id>/map.json` from disk (`crate::data::map_json`) to get `world_half` and `backdrop`. The map JSON is already loaded in `Skirmish.map_json` or `Mission.data`.
- `1000.0` default for `world_half` is unmarked.
- The system takes 11 parameters and also owns the radar-hover sound and the chat-log close.
**Suggestion**:
- Split `radar_hover_sound`, `toggle_map` and `open_map`.
- Take `world_half` and the backdrop from the loaded scenario data (or a `MapInfo` resource).
- Mark `1000.0` PROVISIONAL.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-27 [severity: low] [category: simplify] `bindings.rs`: label-as-key and a stale PROVISIONAL note
**Where**: `bindings.rs:1-4` (module doc: "only the actions marked `wired` have an effect"), `table()`, `load`/`save`, and `Bindings.rows: Vec<(Act, String, KeyCode)>`.
**What**:
- No `wired` marker exists in the table; every `Act` except a few appears wired (`grep` finds a reader for each). The doc is stale.
- The English label ("Select Secondary Ship ( 3 )") is both the UI text and the persistence key in `bindings.json`. Changing a label to fix a typo orphans every player's saved binding.
- `Bindings::key` and `just_pressed` do a linear scan of 30 rows per call, per frame. This is minor.
- `Act::Secondary(u8)` is generated 10 times from `format!`.
**Suggestion**:
- Fix the doc line.
- Keep the current labels as the on-disk key (format compatibility), but add a `fn id(&self) -> &'static str` separate from the display label, so later text changes do not break saved files.
- Store `rows` as a struct (`BindingRow { act, label, key }`) instead of a tuple.
- Optional: an `EnumMap`-style array indexed by `Act`.
**Behaviour-preserving?**: yes.
**Effort**: S

##### B7-28 [severity: low] [category: modding] `skirmish_maps()` and `SkirmishMap` ordering and visibility are not documented for mod authors
**Where**: `skirmish.rs:983-998`, `docs/modding.md` (not read in detail).
**What**: `skirmish_maps()` lists maps by directory name sorted by id (`maps.sort()` sorts tuples), so a mod's map appears in alphabetical position among retail ones, and mod maps cannot hide retail maps or set order. The `historical` filter uses `assemble == false`, so a mod-made "open" map appears in the open list.
**Suggestion**: If a manifest field `"order"` or `"sort"` is cheap, add it later. For now, document in `docs/modding.md` that skirmish maps are listed by id, and which `map.json` keys the lobby reads (`skirmish`, `name`, `assemble`, `players[].playable`, `teams`, `alliance_change`, `world_half`). Adding map JSON keys is a data-format change, so ask a maintainer.
**Behaviour-preserving?**: yes (docs only).
**Effort**: S

##### B7-29 [severity: low] [category: simplify] `retry()` in the staged import swap retries every error for five seconds
**Where**: `setup/import.rs:58-70`.
**What**: It retries any `Err` 20 times at 250 ms. On a permanent error (the destination is missing, not permitted) a failing swap blocks the screen for 5 s, on the main thread (`Setup::poll -> finish -> swap_in` runs inside the `Update` system).
**Suggestion**:
- Retry only on `PermissionDenied`, `ErrorKind::Other` or Windows sharing-violation codes.
- Run `finish` on the tool-reader thread, or at least signal "Finishing..." in `Phase` before blocking.
**Behaviour-preserving?**: yes for the success path.
**Effort**: S

##### B7-30 [severity: low] [category: simplify] `profiles.rs` and `skirmish.rs` repeat the crew posts array type, and `Profile` has an unused `name` round trip
**Where**: `[Vec<String>; 8]` in `skirmish.rs:63,125,255`, `profiles.rs:35`, `crew_arms`; `Profile.name` is set from the file stem on read and blank on `profile_of`.
**What**: The post count `8` is a bare literal in four modules, where `crew_arms::ROLES.len()` exists.
**Suggestion**: `pub type CrewPosts = [Vec<String>; ROLES.len()]` in `crew_arms`, used everywhere. The `8` becomes a single source of truth if a mod ever adds a post.
**Behaviour-preserving?**: yes.
**Effort**: S

#### Quick wins
- Compare `Script` enum variants instead of `Some("quick")`, `Some("ingame-do-load")`, `Some("dock-click")`, `Some("menu-*")` strings (B7-11).
- Fix `Settings::load`'s `x as u8` wrap with `u8::try_from(x).unwrap_or(def).min(max)` (B7-10).
- Add a shared `write_atomic` and use it in settings, bindings, `write_save` and `profiles::write`; `warn!` when a settings, bindings or profile file fails to parse (B7-10, B7-21).
- Replace `std::process::exit(2)` in `menu.rs::lobby_scenario` with `AppExit::Error` (B7-20).
- Redact home paths in the crash report's `arguments` and backtrace (B7-14).
- Remove `env!("CARGO_MANIFEST_DIR")` from the release importer lookup (B7-13).
- Move the three misplaced doc comments in `skirmish.rs` (`auto_build`/`computer_guns` at 304-310, `open_profile` at 820, `launch_error` at 887), and fix the stale doc in `options_menu.rs:4` and `bindings.rs:2-3` (B7-06, B7-18, B7-27).
- Parse `--setup-shot` once in `setup::run` instead of allocating argv every frame in `shot` (B7-16).
- Use `RACES` instead of the literal `[0u8, 2, 3]` at `skirmish.rs:742,787` (B7-08).
- Replace the unwraps at `skirmish.rs:871` and `options_menu.rs:243` with `if let` (B7-21).
- Narrow `#![allow(deprecated)]` to `#[expect(deprecated, reason = ...)]` on the items that name `Interaction` (B7-19).

---

## Appendix: Gameplay subsystems + data and modding (crew_arms, boarding, docking, data, modding, docs, example mods)

Overall: the modder-facing design is strong in its core idea. One merged overlay tree that Bevy and every loader read gives effectively a single path resolver. `extends` + `set` makes a new ship a few lines, and the five example mods are small and readable. The typed ship loader (`data/ships.rs`) is the model to copy: serde structs, `#[serde(default)]`, order-preserving maps, and errors that name file, section and field, with tests for each. `docs/modding.md` is thorough and its Troubleshooting table is honest about known sharp edges. Gun/bullet/asteroid/crew/boarding/dock data is the weak side. It is parsed from untyped `serde_json::Value` with `unwrap()`/`expect("number in ship json")`, so a typo in a mod gives a panic with no file or field. Invalid JSON in a merged file silently wipes the whole base table (documented, but fixable). The gameplay modules are mostly clean (small modules, messages for commands, FixedUpdate sets, PROVISIONAL marking). Remaining issues are a few oversized systems (`docking::routine` ~185 lines, `crew_arms::skirmish_screen` ~200 lines), per-module repeated `run_if` pairs, Entity back-references without relationships, hidden global statics for data, and tuning numbers that a modder would want but that sit in code.

#### Findings

##### B8-01 [severity: high] [category: modding] Invalid JSON in a merged mod file wipes the base table
**Where**: game/src/modding.rs:181-190 (`MERGED` branch: `merge(&mut b, &read(&from).unwrap_or_default())`), `read` at :130, `merge` at :42-51; documented as a known trap in docs/modding.md Troubleshooting ("merged in as `null` and wipes the whole table", "every menu text is blank").
**What**: `read` returns `None` on any parse error, `unwrap_or_default()` turns that into `Value::Null`, and `merge`'s fallback arm `(b, o) => *b = o.clone()` replaces the base object with `null`. The overlay then writes `null` to `guns.json` / `strings.json` / `collision.json`. A comma typo in a mod's `guns.json` becomes "called Option::unwrap() on a None value" later in `data.rs`, with no mention of the mod file.
**Why**: This is the most likely first-mod mistake and it produces the least helpful failure in the whole pipeline. The same applies to a bad `mod.json` and to any `extends` file whose JSON is invalid (the file is then just symlinked, so the error surfaces later with no mod context).
**Suggestion**: In `overlay`, make a failed parse of a mod's `.json` an explicit error naming the mod, the relative path and serde's line/column, using the same `read_typed`-style context. Skipping that mod file with a loud `error!` is the softer choice; panicking at launch is acceptable and consistent with ship errors. Also make `merge` ignore a `Null` overlay when the base is an object, or reject non-object overlays for `MERGED` files. Add a test with a deliberately broken mod file. Keep the troubleshooting rows but change them to say the error names the file.
**Behaviour-preserving?** yes for valid mods; only the invalid-input path changes.
**Effort**: S

##### B8-02 [severity: high] [category: modding] Guns, bullets, asteroids: untyped `Value` + `unwrap()` give panics with no file/field
**Where**: game/src/data.rs:425-427 (`num`: `expect("number in ship json")`), :436-472 (`gun_def`: `g["BulletsPerBurst"].as_u64().unwrap()`, `g["BulletAffectedByGravity"].as_bool().unwrap()`, `g["BulletTypeName_Primary"].as_str().unwrap()`, `g["visual"]["muzzles"].as_array().unwrap()`), :474-529 (`bullet_def`: `impact_snd`, `damage` `.unwrap()`), :109-153 (`AsteroidDefs::from_world` with `panic!("... (re-run the asset importer)")`), :536-538 (`read_json`), :549-552 (`load_weapons`).
**What**: `GunDef`/`BulletDef` are built field by field from JSON values, and a missing key panics with a Rust location, not the gun name or key. `num()`'s message even says "ship json" when used for guns. A mod gun that forgets `VerticalMinAngle` panics inside an iterator closure with `called Option::unwrap() on a None value`, and the modder cannot tell which of dozens of guns or keys. The asteroid panic tells modders to "re-run the asset importer", which is wrong for a mod.
**Why**: "A new weapon" is the second most common mod type. The ship loader already proves the typed pattern works (B8-12 of the same module family), with `read_typed`, `ordered`, and line/column context. Two loaders with opposite quality in one folder is the main modding-clarity gap.
**Suggestion**: Add `GunDoc`/`BulletDoc`/`AsteroidDoc` `#[derive(Deserialize)]` input structs with the same key spellings (`#[serde(rename = "MuzzleSpeed")]`), `#[serde(default)]` for the existing `unwrap_or` defaults, and a keyed deserializer so errors read `guns.json: Gun_X.MuzzleSpeed: missing field`. Convert to the runtime `GunDef`/`BulletDef` unchanged. Keep unknown fields allowed (like ships). This is not a format change, since the JSON stays identical. Reuse `read_typed`'s context helper. Test: one missing-field case per type, as `invalid_ship_reports_file_section_and_field_or_location` does.
**Behaviour-preserving?** yes (same data accepted; only errors improve).
**Effort**: M

##### B8-03 [severity: medium] [category: modding] Overlay errors name the mod folder but not the file; every failure is an `unwrap`/`panic`
**Where**: game/src/modding.rs:150, 155, 162, 174, 188, 198 (`panic!("mod {}: extends unknown '{parent}'", m.display())` omits `name`), :205-206, :211, plus `link` `expect("canonical asset path")`/`expect("symlink")` at :17-20; 48 `unwrap()` in the file.
**What**: For `extends unknown 'X'` the message prints the mod directory but not which `.json` inside it (`name` is available and is used in the sibling `sets '...'` panic). `b.as_object_mut().unwrap()` at :200 and :202 panics if a mod's `extends` file is an array or the parent is not an object. `create_dir_all(...).unwrap()` and `fs::write(...).unwrap()` give no path.
**Why**: Modders hit these as raw panics. One bad ship folder also stops the whole game before the menu (docs/modding.md "If it will not load").
**Suggestion**: Make `overlay` return `Result<PathBuf, ModError>` (`thiserror`-style enum or a `String` with context, as the ship loader does), with variants `UnknownParent{mod, file, parent}`, `BadPointer{mod, file, ptr}`, `Json{mod, file, source}`, `Io{path, source}`. `lib.rs::assets_dir()` can still `panic!("{e}")` or print and exit 2 (as `--lobby` does), but the message becomes one line naming mod and file. Adding a dependency (`thiserror`) needs maintainer approval; a hand-written enum with `Display` needs none.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-04 [severity: medium] [category: simplify] `modding::overlay` is one 90-line function that interleaves four policies
**Where**: game/src/modding.rs:143-233.
**What**: One loop handles (1) base linking, (2) `mod.json` skip and banner, (3) `MERGED` table merge, (4) `extends`+`set`, (5) GLB retail-texture baking, (6) plain link, and (7) shipart generation, with a four-armed `if/else if` chain. The `extends` base-path rule (`name.split('/').count() == 3 && (starts_with("ships/") || starts_with("maps/"))`) is an unnamed magic expression. `read(&m.join(r))` parses every mod JSON twice (once to sort, once to apply).
**Why**: This is the file a contributor must read to learn the merge semantics, and a new rule (e.g. a third merge strategy) has no obvious place. The docs table has three rows; the code has no matching enum.
**Suggestion**: Introduce `enum Layer { Skip, Merge, Extends(Value), BakeGlb, Link }` and `fn classify(name, parsed) -> Layer`, then one `match` that calls `merge_table`, `extend_file`, `bake_glb`. Parse each JSON once (cache `Option<Value>` per file). Extract `fn parent_path(name, to, parent) -> PathBuf` with a doc comment citing the "ships/<id>/ship.json vs sibling" rule. Test the classification table directly.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-05 [severity: medium] [category: modding] The "merged vs replaced" list is a hard-coded Rust slice; adding a merged data file requires an engine change
**Where**: game/src/modding.rs:117-128 (`MERGED`), docs/modding.md "What a mod can change" table, game/src/render/mod.rs:60-90 (`add_mod_skies`, `add_mod_nebulae` do a second, custom merge of the already-merged `skies.json`/`nebulae.json`).
**What**: Nine paths are merged; everything else is replaced whole, so two mods that both ship `data/render/look.json`, `data/hud/slices.json` or `data/hud/theme.json` silently lose one. `skies.json` and `nebulae.json` are in `MERGED` *and* merged again in `render/mod.rs` with different semantics (rules in front, etc.), which is two merge paths for the same files. The docs must restate the list by hand and can drift.
**Why**: Two mods that want to add a HUD slice group collide. The list is also the only place that defines merge policy, but it is invisible to a modder reading `mods/`.
**Suggestion**: Let a file opt in to merging instead of the engine listing paths: e.g. top-level `"merge": true` (or `"extends": "<self>"` already means this for same-path files, so documenting "extends yourself = merge over the file below" for any JSON may already cover it; verify the sibling-lookup branch handles same-name, it does in the `set_replaces_one_value_inside_an_array` test). If that is acceptable, shrink `MERGED` to the retail table files and document the rule once. That is a data-format change, so it needs maintainer approval. Without approval, at least generate the docs table from `MERGED` (a test that greps the doc for each entry) so they cannot drift.
**Behaviour-preserving?** needs-check (format decision).
**Effort**: S (test) / M (opt-in)

##### B8-06 [severity: medium] [category: modding] `extends` is silently triggered by any top-level `"extends"` key and `extends` chains are one-level
**Where**: game/src/modding.rs:168-171, :191, comment "shortcut: one level; a chain of extends inside one mod needs a topological order".
**What**: The sort puts `extends` files last by a stable sort on a boolean, so A extends B extends C within one mod works only by alphabetical luck (B before A fails; A sorted first sees the base C only). The extension key is detected by `v.get("extends").is_some()` on any JSON in the overlay, including e.g. a gun table whose entry... only top-level, so low risk. Non-string `extends` becomes `""` via `unwrap_or_default()` and reports `extends unknown ''`.
**Why**: Modders making a ship family (`Fast` extends `Base` extends retail) will hit this, with a launch panic that blames the parent as "unknown".
**Suggestion**: Order mod files by dependency (collect `(rel, parent)`, process a file after its parent exists in the overlay, error on cycles with the cycle in the message). Report a non-string `extends` as such. Document in modding.md "Limits" until done.
**Behaviour-preserving?** yes for working mods.
**Effort**: S

##### B8-07 [severity: medium] [category: bevy] Process-wide `OnceLock`/`static Mutex` data caches instead of Resources
**Where**: game/src/boarding/odds.rs:145-175 (`DATA: OnceLock<Value>`, `MODDED: Mutex<Vec<(String,[u32;8],Vec<Vec3>)>>`, `register`/`counts`/`points`); game/src/crew_arms/posts.rs:5-8 (`crew_data()`); modding.rs:29-39 (`ACTIVE`); `assets_dir()` (113 uses); similar `OnceLock<Value>` in crew.rs:124, menu.rs:353, wake.rs:146, sprites.rs:283, data.rs:24 (`model()` index).
**What**: Data tables are global statics read lazily through `assets_dir()`. `Library::load` calls `boarding::register` (a side effect inside a data loader that mutates a global `Mutex`) so that `boarding::counts(name)` can later answer. `counts()` locks and linear-scans the Mutex on every call (called from UI paths and `place_crew`, `auto_post`). `crew_data()` uses `try_json(...).unwrap_or_default()`, so a broken mod `crew.json` becomes an empty table and every crew member silently has no skills.
**Why**: Statics make tests order-dependent, forbid two worlds in one process (replay/headless tests, checklist 4.6), hide the dependency of "who must load first", and block hot reload or `Library`-driven data. `register`-in-`Library::load` is exactly the "remember to call init" smell (checklist 1.11).
**Suggestion**: Put `counts`/`points` into `Class` (it already carries `crew` and `boarding_points`, data.rs:395-396) and consult `data/boarding.json` once in `Library::load`, or build a `BoardingTable` resource at load. Same for `CrewData` as a resource built next to `Library`, with a typed `HashMap<String, CrewMember{race, species, name, points, skills, portrait}>` and one error message when the file is missing/invalid. Keep the `assets_dir()` static for now (rewriting it is a larger project) but stop adding new statics.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-08 [severity: medium] [category: simplify] `docking::routine` is a 185-line state machine in one system
**Where**: game/src/docking.rs:573-756.
**What**: One system with 8 parameters holds all seven `Phase` arms (Wait, Approach, Docked, Clear, Away, Turn, Return) plus a closure `gone` that mutates mission state, `bumped` post-processing, and `commands.entity(e).remove::<Docker>()` repeated six times. The Approach-arrival block alone (:647-690) is ~45 lines of repair setup, sound, FX spawning and mission bookkeeping.
**Why**: Hard to review against `docs/re/docking-and-orders.md`, and each phase cannot be unit-tested without building the full query set (the tests file is 527 lines mostly through whole-app runs).
**Suggestion**: Keep the single system (order matters), but extract per-phase helpers that take `&mut Docker, &mut Ship, &Dockable...` and return a small `enum Step { Stay, Release, ToWait }`: `fn on_arrival(...)`, `fn leave(...)`, and `fn release(...)` for the `gone` closure (it also deduplicates the six `remove::<Docker>()` calls). Pure helpers `away_target`, `approach_point`, `dock_duration` already exist and are the pattern to extend.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-09 [severity: medium] [category: simplify] The same two `run_if` conditions are pasted onto 15+ `add_systems` calls
**Where**: game/src/docking.rs:241-266 (7 uses of `run_if(in_state(Battle))`), game/src/boarding.rs:206-235 (5), 20 uses of `run_if(crate::menu::overlay_closed)` in the crate.
**What**: Every FixedUpdate system registers `.in_set(BattleSet::X).run_if(in_state(Screen::Battle)).run_if(overlay_closed)`. `BattleSet` is already the per-phase set.
**Why**: Checklist 1.2/1.4: run conditions belong on the set. A new system that forgets one gets simulated while a menu is open (a silent bug, since the sim runs in the same schedule).
**Suggestion**: Configure the run condition once in the `lib.rs` `configure_sets(FixedUpdate, (BattleSet::Tick..Outcome).chain().run_if(in_state(Battle)).run_if(overlay_closed))` (keep `Tick` out of it if it must run always) and drop the per-system pairs. Verify with the existing `fixed_tests.rs` and a smoke run. Where a system is intentionally exempt, leave an explicit comment.
**Behaviour-preserving?** needs-check (confirm no BattleSet system needs to run while overlay open).
**Effort**: S

##### B8-10 [severity: medium] [category: bevy] Entity back-references held in plain fields/Vecs with no relationship or cleanup hook
**Where**: game/src/boarding.rs `Grapple{grappler, target}` (:110-122), `Boarding{target, fx: Vec<Entity>}` (:85-92), `GrappleOrder(Entity)`; game/src/docking.rs `Dockable{taken: Vec<Option<Entity>>, markers: Vec<Entity>}`, `Docker{dockee}`, `Tower`, `CranePlay`; BoardMsg uses `Option<Entity>` where `None` means the player (boarding.rs:140-155).
**What**: Both modules store Entity ids and then re-query them with `.get()`/`if let Ok(...) else { remove }` (docking.rs `routine` first branch: `let Ok(..) = dockees.get_mut(docker.dockee) else { remove::<Docker> }`). `Dockable::release(docker)` loops the `taken` vector by entity equality; if a docker despawns, its slot is only freed when `routine` runs on a surviving `Docker`. `Option<Entity>` as "the player" repeats a convention in five variants.
**Why**: Checklist 1.10. A relationship (`#[relationship(relationship_target = DockedShips)] Docker`) would give automatic cleanup and a query from the dockee side. It would remove the manual `taken`/`bumped` bookkeeping. It is also the fix for stale ids after despawn.
**Suggestion**: Short term: add `#[derive(Component)]` hooks or an `On<Remove, Docker>` observer to free the dock slot, and replace `Option<Entity>`+"None is the player" with a `Party::{Player, Ship(Entity)}` enum. Long term: model `Docker{dockee}` and `Grapple` as relationships (needs a check that sim order stays deterministic: relationships despawn via hooks at command-apply time, and the sim must apply them within the tick).
**Behaviour-preserving?** needs-check.
**Effort**: M-L

##### B8-11 [severity: medium] [category: modding] Gameplay numbers a modder would tune are in code, some duplicating data that already exists
**Where**:
- boarding/grapple.rs:5-18 (`HARPOON_RANGE = 100.0`, `HARPOON_SPEED = 45.0` with the comment "Gun_Harpoon_Light MaximumRange and MuzzleSpeed" — the values are already in `guns.json`; a mod's harpoon gun is ignored), `SHIP_SPEED`, `BASE_SPEED`, `REEL_PER_TICK`;
- boarding/odds.rs:4-5 (`TICK_BASE 9.0`, `TICK_SPREAD 1.5`), :27-60 (`Skill::handling/scan/member` constants 1.163/1.326/1.441, 1.15/1.30/1.40, 0.1/0.3/0.4), boarding/ai.rs:5-8 (`AI_*_CHANCE/MIN_ATTACK`);
- docking.rs:24-43 (`ARRIVE`, `WAIT_DIST`, `GLIDE_*`, `RETURN_LEG`, `TOW_RANGE`) and :94-101 (`repair_secs`: per-repair-class seconds table 4/7, 10/15, 20/25, 30/35);
- crew_arms/posts.rs:49-65 (`post_factor`: `1.0 + 0.05 * rating`, `/ 1.125`, `1.075 + 0.5*skill`, `min(1.5)`), `leadership_bonus` 0.1/0.05, `accuracy` `1.0 - 0.8 * g`, `DEFAULT_HAND = 0.1`;
- crew_arms/posts.rs `generic_posts` hard-codes the retail ids `Crew_General_Navy_Green|Average|Elite` while `Skill::member()` hard-codes the matching ratings (0.1/0.3/0.4) — the same fact in two places and in two modules (one of them could be read from `crew.json`).
**What**: The retail-derived numbers are commented with their source, which is good practice (and several are marked PROVISIONAL). But they cannot be changed by a mod (no `data/` file), and `repair_secs` and the harpoon numbers are *data in the original* (hud.hdt Repair Speed Info; the gun table).
**Why**: The project goal is "easy to mod"; a mod like "faster repairs", "longer harpoons", "harder boarding" needs a Rust patch today. Duplicates (`member()` vs `crew.json`, `HARPOON_*` vs `guns.json`) will drift if either side is edited.
**Suggestion**: (a) Read harpoon range/speed from the harpoon gun's `GunDef` (the helper `harpoons()` already finds the mount). (b) Derive `Skill::member()` from `crew.json`'s `Crew_General_*` combat rating instead of a literal. (c) Offer a single optional `data/rules.json` (or per-module `data/docking.json`, `data/boarding.json` additions) with these constants; defaults stay in code, mods override by key. (c) is a new data file and asset path, so it needs maintainer approval (AGENTS.md). Never tune the values to change behaviour; just move them.
**Behaviour-preserving?** yes if defaults are identical.
**Effort**: M

##### B8-12 [severity: medium] [category: bevy] `ship.order: usize` magic numbers (0, 2) in docking and boarding
**Where**: game/src/docking.rs:439, 634, 638, 698, 740, 768; boarding/grapple.rs:144; field at game/src/ship.rs:47 (`pub order: usize`); 57 sites of `order = N / order == N` across the crate.
**What**: Gameplay modules set the ship's helm mode with bare integers (`ship.order = 2` to follow waypoints, `0` to stop), with no named constant or enum. The meaning is not recoverable from the call site.
**Why**: Checklist 5.3 (types over magic numbers). A reader of `docking::routine` cannot tell what `2` means without reading `ship.rs`.
**Suggestion**: Introduce `enum Order { Hold = 0, ..., Waypoints = 2 }` (or `const ORDER_STOP/ORDER_WAYPOINTS`) in `ship.rs` and migrate the call sites mechanically; this touches other buckets' files, so coordinate.
**Behaviour-preserving?** yes.
**Effort**: M (crate-wide), S (just these modules via consts)

##### B8-13 [severity: medium] [category: simplify] `crew_arms::skirmish_screen` and `screen` are UI+model systems with 12-15 parameters and a 200-line body
**Where**: game/src/crew_arms.rs:114-166 (`screen`, 11 params) and :237-440 (`skirmish_screen`: 15 params incl. 5 `Local`s, nested `if let ... && let ...` chains, one `match action` with 12 arms, 4 early `return`s).
**What**: Input interpretation (double-click, shift-click, drag release, right-click unpost, wheel), state transitions, mission/skirmish model mutation and view-model rebuild live in one function. `campaign_action` (:171-228) is the clean counterpart for the campaign screen, but the skirmish screen has no equivalent, so `skirmish_screen` re-implements `Ship/Post/Page/Tab/SelectShip` handling that `campaign_action` also implements (lines ~178-203 vs ~330-355; the two differ only in `post_picked`/message clearing).
**Why**: Duplicated action handling means a rule change (cycle semantics, `choice.page` reset) has to be made twice; the system is hard to test (the tests in `crew_arms/tests.rs` go through the free functions).
**Suggestion**: Extract (1) `fn pointer_action(...) -> Option<Action>` for lines :270-316 (pure on `Choice`, `Carry`, input snapshots), (2) a shared `fn apply_nav(choice: &mut Choice, action: &Action, fleet_len, banks_len)` used by both screens, (3) a `#[derive(SystemParam)] SkirmishInput` for keys/mouse/wheel/clicks. Return the `Action` and keep the screen systems as thin wiring.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-14 [severity: low] [category: bevy] Per-ship random draw derived from `Entity::to_bits()` rather than the sim RNG
**Where**: game/src/crew_arms/posts.rs:118-125 (`leadership_bonus`, comment "ponytail: fixed per-ship draws from the entity, not the original's random generator").
**What**: The leadership roll hashes `e.to_bits()`. Entity index and generation depend on spawn order and on how many entities were recycled before, so the result differs between runs that spawn anything different earlier (a mod that adds a prop changes every ship's leadership bonus), and it is not part of the logged seed.
**Why**: Checklist 4.3/4.4: sim RNG should be seeded, one stream, logged. It makes `PROCYON_TRACE` comparisons noisy for a reason unrelated to rules. Fidelity: the original used a random draw per ship (the code comment says so).
**Suggestion**: Draw the two chances from the seeded sim `Rng` once when a ship's crew is first posted and store them on `CrewPosts` (or a `Leadership{draws: [f32;2]}` component). This changes which ships get the bonus per seed, but not the distribution, so call it out to the maintainer as a behaviour-visible determinism change. Mark `PROVISIONAL` either way.
**Behaviour-preserving?** no (changes per-ship results; distribution unchanged).
**Effort**: S

##### B8-15 [severity: low] [category: rust] `Result<_, String>` and English literals in `crew_arms` while the same screen uses string-table ids
**Where**: game/src/crew_arms/fitting.rs:34-47 (`assign` returns `Err("Recruit this crew member first.".into())` etc.), posts.rs:132-165 (`place_crew` returns messages, `"You do not have enough victory points."`), crew_arms.rs:203-215 (`"No weapon bank."`, `"No surviving own ship."`), versus fitting.rs:100-106 (`fit_message` via `said("IDGS_TPFRONTENDTEXT02_...")`).
**What**: Two error styles in one screen: typed `FitError` + string-table ids for the arms side, raw English `String` for the crew side. An empty `String` also doubles as "success" in `place_crew` (returns `String::new()` on success).
**Why**: Checklist 5.2/5.3. A mod/translation `menu/strings.json` cannot reach the literals, and "empty string means ok" is easy to misuse.
**Suggestion**: Add `enum PostError {NoSuchPost, NotEnoughPoints, ...}` and map it to ids/strings in one `fn post_message`, mirroring `FitError`/`fit_message`; return `Result<(), PostError>` from `place_crew`. New strings in `menu/strings.json` would be new ids, so check with maintainers.
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-16 [severity: low] [category: modding] A bad `mod.json` or any invalid optional JSON is skipped silently; no mod load report
**Where**: game/src/modding.rs:77-100 (`read(...).unwrap_or_default()` for `mod.json`/`mods.json`), :178 (`eprintln!("mod {} ...")` banner), data.rs:540-542 (`try_json` swallows all errors), 49 call sites of `try_json`.
**What**: `try_json` conflates "file absent" (fine) with "file present but invalid" (a modder error). docs/modding.md's Troubleshooting lists five JSON files that "seem ignored, with no message".
**Why**: The most frequent mod bug (a trailing comma) is silent for all optional files.
**Suggestion**: Change `try_json` to return `None` for absent but `warn!("{path}: {err}")` (once, with file and serde line/column) for present-but-invalid. One function, 49 call sites benefit, no API change. Also replace the `eprintln!` banner with `info!`, so it reaches `openprocyon.log` (docs/modding.md says the banner "may not be in the log file").
**Behaviour-preserving?** yes (adds logging only).
**Effort**: S

##### B8-17 [severity: low] [category: rust] Orphaned/misplaced doc comments
**Where**: game/src/data.rs:233-238 (the doc block for `shell_gravity` sits above `burst_cycle` and the actual `shell_gravity` at :247 has none); game/src/crew_arms/posts.rs:20 (`/// Legacy mission-start saves have availability lists but no post assignments.` documents `best`, which is about the best skill); game/src/boarding/grapple.rs:15-16 (two doc lines for different constants stacked on `REEL_PER_TICK`).
**What**: Documentation that describes one item is attached to its neighbour.
**Why**: Docs for PROVISIONAL physics (shell gravity measurement) are exactly what a reader looks for; rustdoc currently shows them on the wrong function.
**Suggestion**: Move the blocks to the right items; keep text.
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-18 [severity: low] [category: rust] Stringly-typed enums parsed repeatedly
**Where**: boarding/odds.rs:13-19 `Skill::parse(&str)` with `_ => Average` (a typo `"Elit"` silently becomes Average); data.rs:510-517 `b["kind"].as_str()` matched against `"NovaMortar"`/`"GravCharge"`; data.rs:520 `decal` `position` into `["Small","Medium","Large"]`; ship `race: String` ("Navy", "Pirate"...) compared by string in `recruit` (posts.rs `d["race"].as_str() != Some(race)`), `Class::race`; `name.starts_with("Ship_")` in docking.rs `dockable_for`.
**What**: Enumerations are carried as strings and matched at use sites with silent fallbacks.
**Why**: Checklist 5.3: parse once at load into enums; a mod typo should error.
**Suggestion**: With the typed docs of B8-02, use `#[derive(Deserialize)] enum Kind`/`Race`/`Decal`/`Skill`, with an `Other(String)` variant where mods may add values (race), and error on unknown ones where they may not. Keep JSON values identical.
**Behaviour-preserving?** yes for valid data.
**Effort**: M

##### B8-19 [severity: low] [category: simplify] Repeated literals: crew role names, gun-family suffixes, sound parameters
**Where**: crew_arms.rs:17-18 (`ROLES` "First mates", `SKILLS`), data/ships.rs:335 (`["Gunners","Captains","First Mates",...]` in a different casing), boarding/odds.rs:69 doc comment listing the same order, `boarding/*` `Sfx { name: "Weapon_GrapHarpoon_Fire".into(), at: ..., vol: 0.7, max: 600.0 }` (ai.rs, fight.rs `"Ship_Boarding_Party"` vol 1.0 max 600), fitting.rs:111 `family()` suffix list `["_Light","_Med","_Medium","_Heavy"]`.
**What**: The eight crew roles are spelled in three places (index order is a contract between `crew_posts`, `data/boarding.json`, `CrewPosts([Vec;8])`, `Strength::ship`). Indices like `totals[6]` (fighters), `caps[4]` (engineers), `self.0[1]`/`[2]` (captain/mate) appear as bare integers in `posts.rs`, `odds.rs` and `boarding.rs` (`matches!(role, 1 | 2)`).
**Why**: Easy to mis-index; `crew_posts` keys are a documented mod-facing schema.
**Suggestion**: `#[repr(usize)] enum Post { Gunners, Captains, FirstMates, Navigators, Engineers, Riggers, Fighters, Lookouts }` with `Post::KEYS`/`ROLES` consts and `Index<Post> for CrewPosts`; use it in `ships.rs` for the `crew_posts` keys. A `Sfx::at(name, pos, vol, max)` constructor removes the repeated struct literals.
**Behaviour-preserving?** yes.
**Effort**: M

##### B8-20 [severity: low] [category: bevy] `dev_board` smoke hook and `Config.script` string checks inside shipped modules
**Where**: game/src/boarding.rs:240-266 (`dev_board`), runs every frame in `Update` and returns on `cfg.script.as_deref() != Some("board")`.
**What**: A `--script board` debug system is part of the boarding plugin (checklist 1.4: early-return instead of run condition, and debug code in the gameplay module).
**Why**: Minor, but every such hook adds a per-frame check and clutters the module that owns the rules.
**Suggestion**: Move it next to the other scripts (`smoke/`) and register it with `.run_if(script_is("board"))`; AGENTS.md says smoke code lives there.
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-21 [severity: low] [category: bevy] Per-frame `status` rebuilds an O(ships^2) map; polling in Update
**Where**: game/src/boarding.rs:177-185 and fight.rs:52-78 (`grappled`, `grappling`, `boarding` each scan all grapples/fighters).
**What**: Every frame, for each ship, three `iter().any` scans.
**Why**: Cheap now; the cost scales with fleet size (v0.2 fleets are 8 slots, fine). Include only if profiling shows it.
**Suggestion**: Build the maps with a single pass, or compute only for ships the options menu shows. Low priority.
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-22 [severity: low] [category: simplify] `#[allow(clippy::type_complexity)]` without reason; `#![allow(deprecated)]` at module level
**Where**: game/src/boarding/ai.rs:14, docking.rs:757, :799, crew_arms.rs:3 (`#![allow(deprecated)] // Bevy 0.20 Interaction`).
**What**: Blanket allows with no `reason`. The `ai_boarding` query tuple is 8-wide.
**Why**: Checklist 5.6. The query types are better as `#[derive(QueryData)]` structs, which also name the fields (`ships.iter()` returns `(e, ship, tf, health, guns, own, ms, pilot)` positional tuples that the body unpacks by position).
**Suggestion**: Replace with `QueryData` structs, or use `#[expect(clippy::type_complexity, reason = "...")]`. For `Interaction`, move the polling into the shared `menu` button helper so `crew_arms` stops depending on a deprecated component (checklist 2.2) and drop the module-wide allow.
**Behaviour-preserving?** yes.
**Effort**: S-M

##### B8-23 [severity: low] [category: modding] Docs: strong reference, but the first mod needs a clearer fast path and the table is dense
**Where**: docs/modding.md:21-43 ("Your first mod"), :111-158 (table with very long cells), :648-667 (Troubleshooting).
**What**: "Your first mod" is good (three files, `--mod`, restart), but (a) `mod.json` in the diagram uses `"name"` only while `tests/test_bundled_mods.py` requires `id`, `name`, `version`, `category`, `description`; (b) it does not show how to see the merged result (`local/overlay/`, mentioned only at :134 and in the table of "Testing a mod"); (c) the main rules table has cells over 600 characters, so the file is hard to scan on a phone or in a diff; (d) the doc says "You can look at `mods/silver-skirmish`" but does not say that `extends` of the same id changes the ship in place vs a new id adds one — it does, in one sentence; make that a short bullet list.
**Why**: A newcomer's first success should take five minutes: copy `mods/silver-skirmish`, change one number, run.
**Suggestion**: Add a 6-line "Hello mod" at the top: copy folder, edit `points`, `make run ARGS='--mod ...'`, open `local/overlay/ships/<Id>/ship.json` to see the result. Split the long table into one bullet per file path. Add a "common mistakes" three-liner linking to Troubleshooting. Add a doc-test (Python) that every `data/...` path in modding.md's merged-files row equals `MERGED` (see B8-05).
**Behaviour-preserving?** yes (docs).
**Effort**: S

##### B8-24 [severity: low] [category: modding] `docs/data-format.md` is one 60-line table of paragraphs; silent-ignore fields and defaults are not listed together
**Where**: docs/data-format.md (single table; rows over 1500 characters for `ships/<Id>/ship.json`, `maps/<id>/map.json`).
**What**: The `ship.json` row mixes required fields, optional fields, defaults, importer provenance and `extends` behaviour in one cell. Defaults live in three places: data/ships.rs (`ring 0.65`, `sight 400`, `core_hp 100`, physics defaults), docs/modding.md "What `ship.json` can leave out", and derive.rs. Unknown fields are accepted and ignored by serde (verified in `data/ships.rs`: no `deny_unknown_fields`), which docs/development.md notes but modding.md does not: a misspelled key (`"sigth"`) does nothing, with no warning.
**Why**: Modders can't tell a typo from a feature.
**Suggestion**: Add a "ship.json keys" table (key, type, required?, default, since) generated or at least checked by a test against the `ShipDoc` struct. Consider an opt-in warning: after parsing, diff the raw key set against `ShipDoc` field names and `warn!` on unknown top-level keys (no behaviour change, keeps forward compatibility). Maintainer approval is needed only if you want `deny_unknown_fields`.
**Behaviour-preserving?** yes (warning only).
**Effort**: S

##### B8-25 [severity: low] [category: fidelity-risk] Docking module header and comments describe the original's behaviour as "read from its code"
**Where**: game/src/docking.rs:7 ("The movement is the original's (read from its code, docs/re/docking-and-orders.md)") and several "Retail:" comments (:600-700).
**What**: AGENTS.md hard rules 3/4 say specs are written in your own words citing observable sources (data files, manual, footage, play), and no decompiled-code descriptions. "Read from its code" points at a source the repo says it must not rely on or describe.
**Why**: Project safety/legal rule; trivially fixed by rewording.
**Suggestion**: Reword to "the original's observed movement (docs/re/docking-and-orders.md)" and check docs/re/docking-and-orders.md for the same phrase (outside this bucket, flag to the docs reviewer).
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-26 [severity: low] [category: rust] Loader panics mid-`Library::load` instead of returning a typed error
**Where**: game/src/data.rs:569-580 (`Library::load`), data/ships.rs:189-191 (`load_class` = `try_load_class(...).unwrap_or_else(|e| panic!("{e}"))`).
**What**: `try_load_class` returns `Result<Class, String>` (good) but `Library::load` still `panic!`s on the first bad ship, so a mod with two broken ships shows one error per launch. `Result<_, String>` has no source error chain.
**Why**: Checklist 5.2. Collecting all errors and reporting them together shortens the fix loop for the modder; and a `ShipLoadError` enum lets the future "skip the bad mod ship and show a banner" be added.
**Suggestion**: `Library::try_load -> Result<Library, Vec<LoadError>>`: collect every ship error with the mod/overlay path, print them all, then exit(2) from `setup`. `load()` stays as the panicking wrapper. A `struct LoadError { file: PathBuf, field: Option<String>, message: String }` replaces the plain `String`.
**Behaviour-preserving?** yes.
**Effort**: S

##### B8-27 [severity: low] [category: simplify] `walk` is `pub` utility in `modding` used for asset index scans; `data::model()` and `audio::sound()` do the same "index by file name" separately
**Where**: game/src/modding.rs:8-14 (`walk`), data.rs:22-32 (`model`), audio.rs:179-182 (sound index; per-map `speech` dirs).
**What**: Two lookups that build a name→path index by walking part of the overlay at first use. The docs say "names must be unique" for sounds and models with silently last-wins behavior (`collect()` into a HashMap, `read_dir` order is OS-dependent, so a duplicate name resolves differently on different filesystems).
**Why**: Determinism and a "one resolver" story. `read_dir` order is unspecified; duplicate names across `models/<folder>/` resolve arbitrarily.
**Suggestion**: Sort the walk output (`files.sort()` once in `walk`) and `warn!` on duplicate file names, then share one `fn index_by_name(dir) -> HashMap<String,String>`. Cheap and removes platform dependence.
**Behaviour-preserving?** yes for unique names.
**Effort**: S

#### Quick wins
- B8-01: stop `merge(…, Null)` from wiping tables; name the failing mod file (about 20 lines + one test).
- B8-16: `try_json` warns on present-but-invalid files with file and serde location.
- B8-03: add the file name to the `extends unknown` panic (the variable `name` is already in scope).
- B8-17: fix the three misplaced doc comments (`shell_gravity`, `best`, `REEL_PER_TICK`).
- B8-12: named constants for `ship.order` 0/2 in docking/boarding.
- B8-09: hoist the two `run_if`s onto `BattleSet` (verify with smoke).
- B8-27: sort `walk` output and warn on duplicate asset names.
- B8-25: reword "read from its code" in docking.rs header.
- B8-23: add a "Hello mod" block and a link to `local/overlay/` at the top of docs/modding.md.
- B8-24: add an unknown-top-level-key `warn!` for `ship.json`.

---

## Appendix: Python tools, tests, scripts (b9)

Overall: the importer pipeline is in good shape. `import_assets.py` has a staged `.importing` directory with an atomic swap, Windows rename retries, numbered progress lines, a single step list, and it is unit-tested. `gamefs.ci_file(s)` gives case-insensitive lookup in one place. `source_text.py` and `extract_wot.py` are strict about provenance and path escape. The hygiene test enforces the hard rules mechanically, and the Makefile and check.sh handle Windows virtualenvs. The ruff set is pinned and explicit. Weaknesses cluster in three areas. (1) Error handling in the exporters is silent: broad `except (KeyError, ValueError, FileNotFoundError): continue` and MSB's silent fallback to a heuristic scanner mean a modded install can import "successfully" with data missing and no message. (2) Inconsistent file encodings and case handling: strict utf-8 in `export_combat.load`, latin1 elsewhere, and some `glob`s that are case-sensitive on Linux. (3) Structural duplication: three or four binary readers, three line-based text parsers, 20+ ad-hoc `sys.argv` entry points with no usage message, module-level scripts that cannot be imported or tested, and JSON written with five different indent styles. I found no rule violations in the tools. Two provenance items need a maintainer decision (B9-18, B9-19).

#### Findings

##### B9-01 [severity: high] [category: robustness] MSB `parse()` silently falls back to a heuristic byte scanner for any file the strict reader rejects
**Where**: tools/msb.py:45-60 (`parse`), fallback `_scan` at :63-100.
**What**: `except (ValueError, struct.error, IndexError): return _scan(d)` swallows the reason and returns a different, lossy parse (only the first attribute, missing elements, per the module's own comment at :132-135).
**Why**: for a modded install (new or edited MSB scene) the importer yields ships or mounts with subtly missing hardpoints and nothing is printed. The docstring says retail "parses to its end", so the scan is only reachable for mods, which is exactly where a message matters.
**Suggestion**: keep the fallback but `warnings.warn(f"{name}: strict MSB read failed ({e}); using the pattern scan, hardpoints may be missing")`. Pass an optional `source` label to `parse`. Make `tool()` in import_assets print the warnings, or collect them and summarise at the end of the import (see B9-03). Optionally add a `strict=True` parameter for tests.
**Behaviour-preserving?**: yes (output unchanged, stderr only)
**Effort**: S

##### B9-02 [severity: high] [category: robustness] `export_combat.load` reads WOT as strict utf-8 and exact case; other readers use latin1 and case-insensitive lookup, and failures are swallowed as "skip"
**Where**: tools/export_combat.py:130-138 (`load` uses `read_source_lines(..., encoding="utf-8")` and `Path("WorldObjects") / f"{name}.wot"`), versus tools/export_maps.py:47-49, tools/export_ship_names.py:24-27, tools/eft.py:93 (`ci_file` + latin1). Callers that swallow: export_island_sounds.py:20, export_bases.py:36, export_boarding.py:63, export_docks.py:50 (`except (KeyError, ValueError, FileNotFoundError[, StopIteration]): continue`).
**What**: `UnicodeDecodeError` is a `ValueError`, and a `FileNotFoundError` from a case difference on Linux is also caught. So a WOT with one accented byte, or `ship_x.wot` vs `Ship_X.wot` on a case-sensitive copy, makes the whole base, island or ship disappear from boarding, docks and island sounds with no output. `ci_files` was written for exactly this (gamefs.py docstring).
**Why**: modded installs must give a useful message. The same file is decoded two different ways in the same import.
**Suggestion**: one `gamefs.read_wot_nodes(root, name)` (ci lookup, one documented encoding, raises an error naming the file and the first parse problems). Stop catching `UnicodeDecodeError` and `FileNotFoundError` silently. At minimum log `print(f"skip {wot.name}: {e}")` in each `continue` branch (it is the pattern export_ships already uses at import_assets.py:153).
**Behaviour-preserving?**: needs-check (output identical for vanilla if utf-8 and latin1 agree on ASCII-only WOTs; verify with compare_assets on a re-import)
**Effort**: M

##### B9-03 [severity: medium] [category: modding] No end-of-import summary of skipped or degraded items
**Where**: tools/import_assets.py:136-172 (`export_ships` prints "skip X" lines mid-stream), `tool(..., quiet=True)` at :50-84 keeps only the last stderr line; the swallow sites of B9-02; tools/export_effects.py:145 (`missing.append`); tools/export_voices.py:55-59 (prints missing).
**What**: warnings are scattered through stdout and not machine-readable. The game's setup screen, which wraps this importer, cannot tell "complete" from "completed with 9 skipped objects".
**Why**: players with community-mod installs report "my ship is missing". A `skipped` list would answer this.
**Suggestion**: a tiny `tools/report.py` with `skip(kind, name, reason)` appending to a module list. `import_assets.main` prints a final "N items skipped" block and writes `import_report.json` into out. This is an additive file and needs maintainer ok because it is a new asset path. Stdout-only is a no-approval first step.
**Behaviour-preserving?**: yes for existing assets (additive output)
**Effort**: M

##### B9-04 [severity: medium] [category: python] Entry points parse `sys.argv` by hand; wrong arity gives a raw `ValueError: not enough values to unpack`
**Where**: tools/export_combat.py:559, export_ship.py:49, export_ship_names.py:31, export_waypoint_art.py:12, export_voices.py:62, export_*.py `export(*sys.argv[1:3])` (animations:65, asteroids:99, bases:104, boarding:77, collision:186, docks:62, island_sounds:38, mesh_detail:63, mesh_effects:93, campaign:402, crew:57, decals:77, hud:127, maps:450, nebulae:154, saves:46, ...), mdb_to_glb.py:331, compare_assets.py:131, import_assets.py:268-285 (hand-rolled loop, then `sys.exit(__doc__)`), package_release.py:163. Contrast with argparse in autoplay, battle_check, bake_sky, bake_surfaces, extract_wot.
**What**: about 30 modules each re-implement `root, out = sys.argv[1:3]`. `import_assets.main` treats `--game` as a value option only if another arg follows, otherwise falls into `startswith("-")` and prints the full docstring.
**Why**: CLI consistency, `--help`, usage on error, and `import_assets` is the one real user-facing CLI (also frozen into the release).
**Suggestion**: (a) convert `import_assets.main` to argparse (`--game`, `--out`, positional ships, `--bake-skies/--bake-surfaces` as sub-commands). Keep the flag spellings so docs/scripts still work. (b) Add `tools/cli.py: root_out_main(fn)` that does argparse with `root` and `out` positionals, and use it in the `__main__` blocks. Internal `tool()` calls would keep passing positional strings. (c) Failures return a one-line "usage: ..." instead of a traceback.
**Behaviour-preserving?**: yes (CLI surface unchanged; no asset change)
**Effort**: M

##### B9-05 [severity: medium] [category: python] Importable-module hygiene: `format_json.py` and `export_waypoint_art.py` run at import time
**Where**: tools/format_json.py:8-12 (no `if __name__` guard, no `main`), tools/export_waypoint_art.py:12-24 (module-level `root, out = sys.argv[1:3]` and loop). Related: fix_alpha_bleed.py:50-55 and ogg.py:40 are guarded but untested entry logic.
**What**: `import tools.format_json` runs the formatter on `sys.argv[1]`; the unit test runner (`python -m unittest`) or ruff-style tooling that imports every module would crash or rewrite files. The importer works only because `runpy` is used. They have no tests.
**Why**: testability and the layering the repo claims. `export_waypoint_art` also uses `glob.glob(root + "/...")` and `f"{out}/hud/tex/..."` (string concatenation), which is not pathlib and assumes `hud/tex` already exists.
**Suggestion**: wrap both in `def main(root, out)` plus a guard. Use `ci_files(Path(root)/"BD_Textures", "HUD_Waypoint*.dds")` and `(out/"hud/tex").mkdir(parents=True, exist_ok=True)`. Add a synthetic-DDS-free test for the un-mixing maths by extracting it into a function.
**Behaviour-preserving?**: yes (the glob becomes case-insensitive, a superset)
**Effort**: S

##### B9-06 [severity: medium] [category: simplify] Binary reading helpers are duplicated across format readers
**Where**: tools/mdb.py:46-58 (`_R` with `take`/`u32`, bounds-checked, raises MdbError); tools/msb.py:26-33 (`_u32`, `_str` bounds-checked, return None) and :104-130 (`_Reader` with `u32/i32/f32/string`, NOT bounds-checked for `string()`); tools/skin.py:93-120, tools/ogg.py:28, tools/export_collision.py:67-80, tools/bake_sky.py:61 and :415-418 (raw `struct.unpack_from` with manual offsets).
**What**: three different reader classes/styles. `msb._Reader.string()` slices past the end and silently truncates (a corrupt length yields a short string and a wrong offset, not an error); `mdb._R.take` checks.
**Why**: a corrupt or modded file gets a clear error from MDB but an opaque `struct.error` or silent misparse from MSB. Duplicate code to maintain.
**Suggestion**: one `tools/binreader.py` (`Reader(data, error=ValueError)` with `take/u32/i32/f32/f32s(n)/string/at_end`, errors naming the offset and a caller-supplied file label). Make `mdb._R` and `msb._Reader` thin users of it (subclass to raise `MdbError`). Convert skin.py's offset arithmetic opportunistically.
**Behaviour-preserving?**: yes for valid files (add MSB/MDB tests with truncated input)
**Effort**: M

##### B9-07 [severity: medium] [category: simplify] Three hand-written line parsers for the same family of text formats, plus regex re-reads of raw WOT text
**Where**: tools/hdt.py:17-56 (nested conditional expression for typed values at :37-48), tools/fft.py:11-42, tools/wot.py (`decode_value`/`parse_lines`), tools/export_ship_names.py:23-27 (re-greps raw WOT text with a regex instead of using `tools.wot`), tools/export_maps.py:27-40 (`strings()`), tools/export_voices.py. hdt.py/fft.py each do `text.replace("\r","").split("\n")` and each is called with latin1 text by different callers.
**What**: typed-value decoding (`Int`/`Float`/`String`/`Bool`) exists in wot.py (strict, with diagnostics) and again, laxer, in hdt.py and fft.py. hdt's nested ternary is hard to read.
**Why**: the same value `"Float 1e-3"` can decode differently per reader, and diagnostics are lost in hdt/fft (they silently ignore unmatched lines).
**Suggestion**: reuse `wot.decode_value` in hdt (keep hdt's skin-key logic). Replace the nested ternary with `{"Int": int, "Float": float, "String": lambda s: s.strip("'")}.get(kind, str)`. Give fft a small dataclass (`Font(height, textures, chars)`). Make `export_ship_names` use `export_combat.load` + `properties()`.
**Behaviour-preserving?**: needs-check (hdt output feeds `hud/hud.json`; re-import and run compare_assets)
**Effort**: M

##### B9-08 [severity: medium] [category: python] "Dict soup" instead of typed records in the parsers and exporter interfaces
**Where**: tools/wot.py (nodes are `dict(source, raw, kind, name, children...)`, diagnostics `issue()` at :22-25 are dicts), tools/msb.py:140-253 (`parse_scene` returns nested dicts although `Node` dataclass exists at :14-23 for the flat form), tools/mdb.py (`Vertex.pos: tuple`, `Model.vertices: list` untyped; `_box_node` returns dict), tools/export_combat.py gun/bullet/section records.
**What**: two parallel representations of an MSB element (`Node` vs dict). Type hints are mostly absent outside source_text.py/extract_wot.py.
**Why**: schema is discoverable only by reading code; typos in keys (`"pivot"`, `"rest_length"`) fail at runtime. The exporters' JSON shape is the modding contract (see B9-14), so typed records would double as its documentation.
**Suggestion**: introduce `@dataclass(frozen=True)` for `MsbElement`, `Clip`, `Diagnostic`, `BoxNode`. Add `tuple[float, float, float]` annotations. Convert `Node` and the dict reader to share one class. Do it reader by reader, with `dataclasses.asdict` at JSON boundaries to keep output identical.
**Behaviour-preserving?**: yes if serialised via the same key order
**Effort**: L

##### B9-09 [severity: medium] [category: robustness] `write_text` / `read_text` without `encoding=` (84 sites) and `.env` read in locale encoding
**Where**: `grep -n "read_text()\|write_text(" tools/*.py` hits 84 lines without `encoding`, e.g. tools/import_assets.py:112 (`read_env` -> `path.read_text()`), :171 (`ships.json` write), export_*.py all JSON writes, compare_assets.py:78, format_json.py:12.
**What**: on Windows (a stated target) the default is the ANSI code page. `json.dumps` defaults to `ensure_ascii=True`, so writes happen to be safe. But `.env` with a non-ASCII path (`C:\Users\Zoë\Games`) is read with the wrong codec, and `format_json` or compare reads of any file written with `ensure_ascii=False` (extract_wot) would break. The same file is read as latin1, utf-8 or locale in different places.
**Why**: correctness on non-English Windows; Python 3.15 will make utf-8 the default but the repo doesn't pin a version.
**Suggestion**: add `encoding="utf-8"` to every JSON/.env read and write (ruff `PLW1514` is preview; alternatively `FURB101/FURB103` already in `FURB`). Optionally a tiny `tools/jsonio.py: read_json(path)` / `write_json(path, obj, style)` that is also the one place for B9-12.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-10 [severity: medium] [category: robustness] Some exporters use case-sensitive `glob` on the install where others use `ci_files`
**Where**: tools/import_assets.py:197-198 (`Sound/SoundEffects`, `Sound/GUI` `.glob("*.ogg")`), :210 (`BD_FlagTextures` `*.tga`), :302 (`WorldObjects` `Ship_*.wot`); export_decals.py:61, export_overlays.py:36, export_environment.py:120, export_campaign.py:231,233,386, export_nebulae.py:122, export_voices.py:30,48, export_waypoint_art.py:13.
**What**: the docstring of gamefs says "files on a case-sensitive copy of the install do not reliably match case", yet directory names and `*.OGG` / `.TGA` suffixes are matched with Python's case-sensitive glob. On Linux with a retail disc copy (many are upper-case, e.g. `WORLD/`), the importer's own `Ship_*.wot` listing (`want` list at import_assets.py:302) finds nothing, and `not (root / "WorldObjects").is_dir()` at :297 refuses the install.
**Why**: the importer is the first thing a Linux/Proton player runs.
**Suggestion**: add `gamefs.ci_dir(root, "WorldObjects")` (case-insensitive subdirectory resolution) and `ci_files` for the pattern. Use it in `main` for the install sanity check and in the loops above.
**Behaviour-preserving?**: yes (superset on case-sensitive systems)
**Effort**: M

##### B9-11 [severity: medium] [category: shell] `tools/check.sh` is POSIX `sh` with `set -e` only, uses `python3` on Windows, and mixes `eval` with composed command strings
**Where**: tools/check.sh:1,8 (`#!/bin/sh`, `set -e`, no `-u`), :17 (`python3 -c ...` although the loop at :13 adds `.venv/Scripts` where only `python.exe` exists on Windows; the Makefile at :6 handles this with `$(PY)`), :31-34 (`run="xvfb-run -a -s '-screen ...' env ..."` then `eval "$run timeout 10 ..."`).
**What**: on Git Bash for Windows `python3` may be the Store stub (the comment at :12 acknowledges it). The `eval` of a string containing quotes is fragile; bash arrays (as shot.sh already uses) avoid it. `cd ..` bookkeeping is repeated (`cd ..` at :22, `(cd .. && ...)` at :44, final `cd ..`).
**Why**: robustness of the one validation gate.
**Suggestion**: `#!/usr/bin/env bash`, `set -euo pipefail`, `PY=$(command -v python3 || command -v python)`, `run=(xvfb-run -a -s "-screen 0 1280x720x24" env WGPU_BACKEND=vulkan)` and `"${run[@]}" timeout 10 target/release/openprocyon`. Use `pushd/popd` or subshells for `game/`. Note: with `pipefail` the smoke's `| grep -q` pipeline status needs care (`timeout` returns 124 by design), so keep `|| true` on the producer or check the grep status explicitly.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-12 [severity: medium] [category: modding] Exported JSON is written in five different styles (indent 0/1/2/None/separators) and not sorted deterministically by design
**Where**: indent counts across tools/*.py: `indent=1` x11 (export_crew:52, docks:57, island_sounds:27, decals:72, mesh_effects:82, saves:41, hud via bar, voices:35, ship:50, ...), `indent=2` x10 (campaign:267/395, nebulae:120, ship_names:33, pack_data:52, bake_*:701-704), `indent=0` (export_mesh_detail.py:58), compact `json.dumps(m)` (export_maps:404/407/446, export_campaign:372/389, export_effects:148/161, export_combat:561, export_hud:116/120, import_assets:171) and `separators=(",",":")` (animations, collision, bases). Only `format_json` post-formats maps/campaign. `sort_keys` is used only in export_voices.py:35 and extract_wot.py.
**What**: the doc says "pretty-printed so it diffs and edits by hand" (docs/data-format.md:3), but ships.json, data/crew.json, `hud/hud.json`, `fx/*.json`, `menu/strings.json` and most per-ship pre-pack files are compact or one-line. Key order depends on insertion order of the exporters, which is deterministic today but undocumented, and compare_assets (by value) hides ordering churn.
**Why**: modders hand-edit these; compact one-line files are painful to edit and diff. A single writer also makes byte-for-byte re-import comparisons possible without compare_assets.
**Suggestion**: one `tools/jsonio.write_json(path, obj, *, compact=False)` using `pack_data.dump` style (indent 2, number lists on one line, trailing newline, `ensure_ascii=False`, `encoding="utf-8"`) for everything that is meant to be edited (docs/data-format.md rows), and compact only for generated geometry/collision (documented as "do not edit"). Final `format_json` pass already exists for maps/campaign; extend its pattern list instead of per-file styles. Output changes whitespace only: run compare_assets (it compares by value, so expect "no differences").
**Behaviour-preserving?**: yes semantically; byte output changes, mod diffs may churn once. Maintainer ok recommended (data format files).
**Effort**: M

##### B9-13 [severity: medium] [category: modding] No schema or version marker for the exported data
**Where**: docs/data-format.md (68 lines, a single wide table); tools/import_assets.py writes no marker (docs/data-format.md:7 says `import.json` is written "by the game's import screen", not by the CLI importer, so a `make assets` import has no version or install path record).
**What**: the doc is thorough on semantics but there is no machine-checkable schema (`docs/` JSON Schema or `data/format_version`), and no record of which importer version produced an asset folder. Mods overlay by relative paths, so an exporter change silently breaks mods with no signal.
**Why**: moddability and "Keep asset paths and JSON layouts stable" (AGENTS.md). Today stability is enforced only by reviewer care.
**Suggestion**: (a) have `import_assets.main` write `import.json {version, importer_format: N}` itself after the swap (the game screen then only adds `game`). Needs maintainer approval since it is a new asset path that the game also writes. (b) Add Rust-independent golden checks: a `tests/test_data_layout.py` that runs on imported assets and asserts required keys per file type (`@needs_assets`, like other tests), listing the contract in one place. (c) Split the giant table in data-format.md by area (ships/maps/hud/sound) so rows are reviewable.
**Behaviour-preserving?**: yes (additive)
**Effort**: M

##### B9-14 [severity: low] [category: modding] Doc/code drift risk: docs/data-format.md points at `tools/build_assets.sh` and `pack_data.py` as the entry points
**Where**: docs/data-format.md:3, "ends with `tools/pack_data.py`, `tools/format_json.py` and `tools/pack_layout.py`", while the real order in import_assets.py:240-250 continues with `export_collision`, `format`, `layout`, `bases`, `base effects` after `pack_data`. tools/pack_data.py:9 says "usage: python3 tools/pack_data.py" (the importer and development.md use `python -m tools.pack_data`; running by path fails because the file does `from tools...` imports? `format_json.py:8` does `from tools.pack_data import dump`, which fails when run as `python3 tools/format_json.py` exactly as its docstring says).
**What**: three module docstrings give a usage line that does not work (`tools/format_json.py:2`, `tools/pack_data.py:9`, `tools/export_waypoint_art.py:4`).
**Why**: contributors copy usage lines.
**Suggestion**: fix the docstrings to `python -m tools.<name> ...`; reword data-format.md:3 to "see `steps()` in tools/import_assets.py for the order".
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-15 [severity: medium] [category: robustness] `models()` and `flags()` are not tolerant of modded installs, unlike ship export
**Where**: tools/import_assets.py:176-190 (`models`: `tool("tools.mdb_to_glb", ...)` non-quiet for fixed `SCENERY` / `SELECTION_INDICATOR_*` / `LOBTARGET_*` names; a missing mesh raises StepFailed and aborts the whole import), :205-211 (`flags`: any unreadable TGA raises inside PIL with a bare traceback and no filename).
**What**: ships have per-ship skip logic (`export_ships`), but these fixed lists abort on the first absent file, and the user sees a traceback from deep in mdb_to_glb (`FileNotFoundError: no X_Layer*.mdb under ...`, the one good message, only if it gets that far).
**Why**: mods that replace or drop an island/indicator mesh stop the import at step 2 of 36.
**Suggestion**: use the existing `quiet=True` mode and print `skip model X: <err>` for optional UI/scenery meshes; wrap each flag conversion with `try/except (OSError, ValueError) as e: print(f"skip flag {f.name}: {e}")`. Which of these are truly required is a maintainer call, so keep SKY/ship hull required.
**Behaviour-preserving?**: yes for vanilla
**Effort**: S

##### B9-16 [severity: medium] [category: simplify] `tool()` re-enters modules through `runpy` + `sys.argv` mutation and swallows tracebacks in quiet mode
**Where**: tools/import_assets.py:50-84.
**What**: every step is a CLI entry (`runpy.run_module(..., run_name="__main__")`) with global `sys.argv` and `warnings` patching, so modules cannot return values or take typed arguments; `export_voices` has three call conventions (`copy`/`table`), `export_effects` takes `*ship_files()`. In quiet mode only the last line of stderr survives (e.g. `ValueError: [{...}]`).
**Why**: `functools.cache` in modules (export_maps.py:43 `mesh_scene`) leaks across steps; failures lose their traceback; the per-step API is stringly typed. It is the reason the entry points in B9-04/B9-05 are all `if __name__` scripts.
**Suggestion**: long-term, give every exporter a `run(root: Path, out: Path)` and let `steps()` call them directly (the frozen importer needs only the imports). Short-term: keep runpy but in quiet mode keep the full traceback in a log file under `out/../import.log` and print its path on failure.
**Behaviour-preserving?**: yes
**Effort**: L (full) / S (log file)

##### B9-17 [severity: low] [category: python] Exceptions with unhelpful messages carrying parse diagnostics as raw dicts
**Where**: tools/export_combat.py:136-137 and tools/eft.py:70-71: `raise ValueError(problems[:3])` (a list of dicts with `source`, `code`, `message`); tools/export_combat.py `next(...)` without default at :299 (`base = next(n for n in nodes.values() if n.parent < 0)`) raising bare `StopIteration` (caught by name in several exporters); tools/mdb_to_glb.py:314 etc.
**What**: a modder sees `ValueError: [{'source': {'path': 'WorldObjects/Ship_X.wot', 'line': 44}, 'code': 'missing_open', ...}]`.
**Why**: "a player with a modded install must get a useful message".
**Suggestion**: add `wot.format_problems(problems)` returning `"WorldObjects/Ship_X.wot:44: Block header requires an opening brace"` lines. Raise `ValueError(f"{path}: ...")`. Use `next(..., None)` and raise `ValueError(f"{scene}: scene has no root node")`.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-18 [severity: medium] [category: modding] Provenance of the committed voice event table needs a licence statement (maintainer decision)
**Where**: tools/export_voices.py:2-5 ("Its grouping comes from the folder layout of the community Unity remake (CL ModPack Voicelines ...); only event and file names are kept") and content/voice/events.json (shipped); tools/mdb.py:1 ("layout per public TPForm source").
**What**: AGENTS.md hard rule 7 requires third-party material in `content/` to be ours or redistributable, with the licence recorded next to the file; rule 4 requires describing behaviour from observable sources. The docstring says the grouping comes from another project's folder layout, but I found no licence note for it in tools/; content/README.md (not in my bucket) may carry it.
**Why**: sharing safety. Names of files and event groupings are likely not copyrightable, but the project's own rule is to record it.
**Suggestion**: confirm `content/README.md` records the source and why only names are kept; if not, add it. Rewrite the mdb.py docstring to say how the layout was determined (observation of retail files) rather than naming an external code base.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-19 [severity: low] [category: python] Hard-rule scan covers only listed suffixes, and the RE-tooling regex has some blind spots
**Where**: tests/test_repo_hygiene.py:18-45 (`GAME_SUFFIXES`, `TEXT_SUFFIXES`), :67-70 (`TOOLING`), :86.
**What**: (a) `GAME_SUFFIXES` lacks `.png/.jpg` (converted textures), `.bmp`, `.ttf` third-party fonts (content/ is allowed), `.twt`/`.tst` are there but `.ckb`, `.dat`, `.sav`, `.bin`, `.mp3`, `.ogv`, `.avi` are not, nor `.zip`/`.7z`/`.pak` archives of game data. (b) `p.read_text(errors="replace")` and `TEXT_SUFFIXES` miss `.cfg`, `.ini`, `.ron`, `.glsl`/`.wgsl`, `.html`, `.csv`, `.sh` is there but extensionless scripts are not. (c) The ADDRESS pattern only matches 0x4..-0x7.. six-digit literals. A `0x0085c3f0` would be missed because of `0{0,2}` handling of `0x00xxxxxx`? It matches up to two leading zeros plus [4-7]..., so `0x00854ac0` (a typical address) is not covered (first digit 8).
**Why**: this test is the only mechanical enforcement of hard rules 1 and 3.
**Suggestion**: extend `GAME_SUFFIXES` with the extra media/archive suffixes (png only outside `content/`, `docs/`, `game/`, `mods/` pictures that are ours, so make it path-scoped instead: any image under `assets/`/`local/` is already forbidden by the git-ignore, so add a check that `git ls-files` contains no path under those). Widen ADDRESS to `0x00?[4-9a-f]` for the typical 0x00400000-0x00ffffff range and keep the 0x7fffff exclusion. Add unit cases to `test_patterns`.
**Behaviour-preserving?**: yes (test only; might flag existing text, check with the run)
**Effort**: S

##### B9-20 [severity: low] [category: python] `tools/mdb.py` code-quality nits
**Where**: tools/mdb.py:25-29 (`import math` inside `normal`), :14-20 (`unk` field name for "packed normal", the comment says what it is), :118-130 (the `t = lambda ...  # noqa: E731`), `_tag_table` swallows `(MdbError, struct.error)` and silently uses the island table (:111-115).
**What**: a lambda assigned to a name with a noqa; an import in a function; a field named `unk` that the docstring has already identified; the same silent-fallback pattern as B9-01 for tag tables of modded meshes.
**Suggestion**: `import math` at module top; rename `unk` to `normal_angles` (check users with `grep -rn "\.unk"`); replace the lambda with `def t(name): return tags.get(name, -1)`; warn when the trailing tag table is present but unreadable.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-21 [severity: low] [category: simplify] `export_effects`/`export_combat` call `ship_files()` and repeated re-reads of the same JSON between steps
**Where**: tools/import_assets.py:232-234 (`ship_files` re-reads ships.json) and later steps re-reading the same files: export_bases.py:20-21, export_campaign.py:318 and :355, export_effects.py:124-138, export_collision.py:153, export_nebulae.py:128, export_overlays.py:28, export_ship_names.py:32 (`json.loads((assets/"ships.json").read_text())`).
**What**: ships.json is written by import_assets then rewritten by export_campaign (:372, "Campaign may append classes", comment at import_assets.py:229-230) while pack_data later deletes the raw per-ship files. The inter-step contract is files on disk with an implicit ordering that only comments describe.
**Why**: reordering steps (the list is the pipeline definition) breaks silently; there is no test that the order is valid.
**Suggestion**: add a `needs=` / `produces=` note per step in `steps()` (or a test asserting `ships.json`, `data/weapons/` exist before the steps that read them using a fake tree). The sorted/deterministic order of `ships.json` is already given by `want`.
**Behaviour-preserving?**: yes
**Effort**: M

##### B9-22 [severity: low] [category: shell] `tools/shot.sh` and `smokes.sh` portability and quoting
**Where**: tools/shot.sh:6 (`realpath -m`, a GNU extension; not on stock macOS), :24-25 (`${frames//,/ }` unquoted word-split loop, fine but `for f in ${frames//,/ }` also glob-expands), :31 (`cargo run --release -- --shot "$out" "$@"` after `cd "$root/game"`: relative game args such as `--mod ../mods/x` are interpreted relative to `game/`, not the caller's cwd, which isn't documented), :9 `set -uo pipefail` without `-e` (intentional because of status handling, but `cd "$root/game"` failure would continue). tools/smokes.sh:8 `export XDG_DATA_HOME; XDG_DATA_HOME=$(mktemp -d)` (fine), the smoke table is a here-doc parsed by `while IFS='|'` later (not read here).
**Why**: robustness, small.
**Suggestion**: `cd "$root/game" || exit 1`; replace `realpath -m` with `python3 -c`-free fallback: `out=$(cd "$(dirname "$1")" 2>/dev/null && pwd)/$(basename "$1")` or document GNU coreutils as a requirement; quote `"${frames//,/ }"` via `IFS=, read -ra list <<<"$frames"`.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-23 [severity: low] [category: python] Python version is not declared or tested; CI only runs ubuntu
**Where**: requirements*.txt (no `python_requires`), .github/workflows/check.yml:20-30 (runs `tools/check.sh` on the runner's default Python, ubuntu-24.04 only; the release workflow pins 3.12 on windows-2022 and ubuntu-22.04 but runs no tests), code uses 3.10+ features (`zip(strict=True)` in eft.py:23 and mdb_to_glb.py:97, walrus in mdb.py:76, `X | Y` types, `Path.is_relative_to` 3.9, `str.removesuffix` 3.9).
**What**: Windows is explicitly supported (Makefile, check.sh comments, `retry()` for rename, `user_dir(windows=...)`), but none of the Python tests run on Windows in CI. The `windows` branches (`user_dir`, `swap_in`, backslash handling) are unit-tested only through injected flags.
**Why**: Windows regressions (path separators in `Path(...).as_posix()` provenance, `os.rename` over an existing folder, case-insensitive FS) are only found by players.
**Suggestion**: add `actions/setup-python@v5` with `python-version: "3.12"` to check.yml and a second matrix job `windows-2022` running only `python -m unittest discover -s tests` plus `ruff` (no cargo). State `Python >= 3.10` in requirements.txt header and docs.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-24 [severity: low] [category: simplify] Small duplicate helpers and dead code candidates
**Where**: `rnd` (pack_data.py:29) vs `rounded` (export_animations.py:17) vs `rnd(out, 4)` use at export_collision.py:180; `pad4` in mdb_to_glb.py:75 (and `-len(blob) % 4` inline at :124); `kv()` in export_combat.py:280-286 (a "k=v,k=v" parser); the Ship name list `SKIRMISH` in pack_data.py:15-23 duplicates `FIRST` in import_assets.py:28-36 (identical seven ships); two copies of the "first = default player ship" ordering.
**What**: the same seven-ship list is maintained twice, with a comment each.
**Why**: adding or removing a retail skirmish ship requires touching two files; they can drift.
**Suggestion**: define `SKIRMISH_SHIPS` once (pack_data or a new `tools/catalog.py`) and import it into `import_assets.FIRST`. Move `rnd` to a shared helper (jsonio, B9-12).
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-25 [severity: low] [category: simplify] Overlong functions that mix concerns
**Where**: tools/export_combat.py (566 lines, the `export` at ~:330-540 builds guns, bullets, sections, engines, wakes, lifeboats in one pass), tools/mdb_to_glb.py `convert` (:117-300, nested closures `add_view`, `material`, many accumulator lists unpacked in a 8-way tuple at :140-149), tools/bake_sky.py (763 lines, the biggest single tool file), tools/export_maps.py:331-450 `main` and tools/export_campaign.py:229-402.
**What**: long linear functions with inner closures; the 8-element tuple-unpack of empty lists is a sign a small `GltfBuilder` class is wanted.
**Why**: testability (the exporter is only covered through large integration tests) and review cost.
**Suggestion**: for `mdb_to_glb`, introduce `class GltfBuilder` (fields `blob, views, accessors, ...`; methods `add_view`, `material`, `add_mesh`, `to_glb`) as a pure refactor, verified by byte-identical GLBs from a re-import (`compare_assets` compares GLB? it compares JSON/images, so compare GLB hashes separately with `cmp`). Split `export_combat.export` into `export_physics/export_guns/export_sections`.
**Behaviour-preserving?**: needs-check (byte compare GLBs and ship JSON)
**Effort**: L

##### B9-26 [severity: low] [category: python] `compare_assets` limits and CLI
**Where**: tools/compare_assets.py:63-90 (images by pixels, JSON by value, other files?), :110-137 (`main(expected, actual, detail=5)` hand-parses `--detail`).
**What**: not read in full; observed: `--detail` is hand-parsed (see B9-04), `.glb`/`.ogg` handling should be confirmed. Since exporter changes are gated by this tool ("Exporter changes: re-import ... compare_assets"), GLB (mesh geometry) and Ogg byte differences matter and the docs promise "compares by meaning".
**Why**: if GLBs are compared by bytes only, a reordering of JSON inside the GLB chunk shows as "differ" with no detail; if they are skipped, mesh changes go unnoticed.
**Suggestion**: confirm and document the policy in the module docstring. For GLB, compare the parsed JSON chunk by value and the BIN chunk by bytes. Convert to argparse with `--detail`.
**Behaviour-preserving?**: yes
**Effort**: S

##### B9-27 [severity: low] [category: python] Tests: reliance on temp dirs via `tempfile` per test and bare `sys.exit` messages as API
**Where**: tests/test_import_assets.py (uses `tempfile.TemporaryDirectory` plus `assertRaises(SystemExit)` and `e.exception.code`), tools/import_assets.py `main` uses `sys.exit("message")` for control flow in `user_assets`, `main`, `swap_in`.
**What**: library-style functions (`user_assets`, `main`) call `sys.exit` with message strings, so tests must catch `SystemExit` and inspect `.code`. There is a clear split: `StepFailed` exists for steps, but input-validation errors use `sys.exit`.
**Why**: embedding the importer (the game's setup screen runs the frozen binary and parses its output) is easier with an `ImportError_`-style exception and a thin `__main__` that converts to an exit code. Current behaviour is fine for the CLI.
**Suggestion**: introduce `class ImportFailed(Exception)` for user-facing errors (no game install, no assets dir, step failure). Only `if __name__ == "__main__"` turns it into `sys.exit(str(e))`. Tests use `assertRaisesRegex`.
**Behaviour-preserving?**: yes (same text and exit status through the guard)
**Effort**: S

##### B9-28 [severity: low] [category: python] `read_env` is a minimal hand parser; `.env` handling inconsistent between Python and Makefile/shell
**Where**: tools/import_assets.py:109-118 (`read_env`: strips quotes, skips `#` lines only when at the line start, no `export KEY=` support, no inline comments), used at :293 (`not FROZEN and read_env(REPO / ".env")`).
**What**: `export PROCYON_GAME_ROOT=...` lines (common when people also `source .env`) are silently ignored (`key` becomes `export PROCYON_GAME_ROOT`), and AGENTS.md says only "put `PROCYON_GAME_ROOT` in `.env`".
**Why**: the symptom is "pass --game" even though the user has set it.
**Suggestion**: `key = key.removeprefix("export ").strip()`; add a test case. Optionally warn when `.env` exists but the key is missing.
**Behaviour-preserving?**: yes (more inputs accepted)
**Effort**: S

##### B9-29 [severity: low] [category: robustness] `package_release.py` hard-codes a repository URL and relies on the checkout layout
**Where**: tools/package_release.py:35 (`https://github.com/<owner>/OpenProcyon` in the PLAY text), :18-23 `IMPORTER_DATA` list of repo files, :44 (`BUNDLED_PYTHON` names).
**What**: a URL inside a generated README is fine for rule 8 (public), but it is hard-coded in code; the IMPORTER_DATA list must be updated when another content file is read by an exporter (the tools resolve `content/...` via `Path(__file__)`, e.g. export_voices.py:20 `EVENTS`, compose_bar.py `BAR`).
**Why**: a new exporter reading another `content/` file works from a checkout and breaks only in the frozen release; test_package_release may cover it (not verified).
**Suggestion**: add a test that greps `tools/*.py` for `content/` paths and asserts each is in `IMPORTER_DATA` (or derive `IMPORTER_DATA` from a `content/` glob), and read the URL from a constant next to a version string.
**Behaviour-preserving?**: yes
**Effort**: S

#### Quick wins

- B9-01: add a `warnings.warn` when MSB falls back to the scan (also mdb tag-table fallback, B9-20).
- B9-02: stop catching `UnicodeDecodeError`/`FileNotFoundError` silently in export_island_sounds, export_bases, export_boarding and export_docks. Print `skip <file>: <reason>`.
- B9-05: guard `format_json.py` and `export_waypoint_art.py` with `main()`, use `ci_files`, and create `hud/tex`.
- B9-09: add `encoding="utf-8"` to JSON and `.env` read and write sites (sed-able; ruff FURB rules help find them).
- B9-11: switch `tools/check.sh` to bash with arrays and `set -euo pipefail`, and use `python` when `python3` is missing.
- B9-14: fix the three docstring usage lines that fail when run by path; correct the step order sentence in docs/data-format.md:3.
- B9-15: make the fixed model lists and flag conversion in `import_assets` skip with a message instead of aborting.
- B9-17: format WOT parse problems as `file:line: message` instead of raising `ValueError([dict, ...])`.
- B9-23: pin Python in check.yml and add a Windows job for `unittest` and `ruff`.
- B9-24: define the seven-ship skirmish list once.
