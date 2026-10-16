# Development guide

## Short daily loop

Work from the repository root with the Python virtual environment active. Use `make help` for commands.

1. Read the owning module and its callers; pick one complete behaviour or visual batch.
2. Put editable gameplay changes in `mods/`; change exporters only when the retail conversion is wrong.
3. Add a small regression test for new logic. Prefer synthetic parser tests and pure mission-rule tests over long simulations.
4. Run the relevant tests while iterating, then `make check` before committing.
5. For ECS wiring or UI changes, also run a screenshot smoke test. Unit tests do not detect every Bevy query conflict or layout problem.
6. Review `git diff --check` and `git diff --stat`. Never commit converted assets (`assets/` is git-ignored).

```sh
# No Make dependency required: these are the authoritative underlying commands.
tools/build_assets.sh --game <install dir>   # once: convert your own copy of the game into assets/
tools/check.sh
cargo run --release --manifest-path game/Cargo.toml -- --mission 1
python3 -m unittest discover -s tests -p 'test_trim_hud_rects.py'
cargo test --release --manifest-path game/Cargo.toml panels::tests  # shared HUD; classic::panels::tests for the retail art
tools/shot.sh local/mission01.png --mission 1 --script briefing-skip --shot-frame 180 --late-frame 0
tools/shot.sh local/paused.png --map red_rover --script pause --shot-frame 50 --late-frame 0
tools/shot.sh local/pause-restart.png --map red_rover --script pause-restart --shot-frame 90 --late-frame 0
```

`make check` calls `tools/check.sh`: ruff format check and lint (Python, rules in `ruff.toml`) → rustfmt check → strict Clippy → Rust release tests → Python tests. Rust tests that need converted
assets only run when an import exists (otherwise the tests are compiled but not run); Python tests that need the install or assets skip.
`make fmt` formats Rust and Python (`ruff format`, `ruff check --fix`); it does not rewrite generated JSON. `python3 -m tools.format_json assets` formats asset JSON when needed.
Shared sight smoke: `WAYLAND_DISPLAY= tools/shot.sh local/sight.png --script sight --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0`; it freezes three ships and asserts group/range/cloak transitions against real radar widgets. This is wiring validation, not a campaign playthrough.

Screenshot tests need a display, or Xvfb + Mesa for the headless helper. Outputs belong in ignored `local/`.
The `pause` / `pause-restart` scripts assert that the non-retail Pause key freezes virtual time and that Pause / restarting resumes it; let the run finish past frame 70, not just write its screenshot.
`PROCYON_PROFILE=1 make run ARGS='--mission 1'` logs rolling real frame time and world/FX/mesh/audio counts every five real seconds
and on mission-end changes. Fleet smoke: `tools/shot.sh local/fleet.png --map red_rover --allies Ship_Navy_Cutter,Ship_Navy_Cutter,Ship_Navy_Cutter --script fleet --shot-frame 210 --late-frame 260`; it exercises shared keys/widgets, two heights, promotion, formations, restart and menu re-entry. Lifeboat smoke (scripted, needs `--shot` for the frame counter; run the binary directly with `WAYLAND_DISPLAY=`): `--mission 1 --script lifeboat-m1` (kill Warrior, collect both boats, immediate next level), `lifeboat-m1-fallback --speed 16` (no pickup, ~109 s timers), `--mission 2 --script lifeboat-m2|lifeboat-m2-ladyj`, `--mission 4|9 --script lifeboat-m4|lifeboat-m9`, `--mission 5|6|7|10|12 --script lifeboat-kill`. They kill ships through health and teleport collectors: scripted wiring checks, **not** combat validation. `--mission 3 --script fleet-primary` isolates the real sloop setup rule to check the human-captain handoff; `--mission 5|9 --script fleet-primary` replaces rules with a synthetic primary action to check ECS dispatch, **not** later-mission gameplay. P and brackets now belong to fleet bindings; pause/speed remain on existing HUD/menu controls (Pause key is an explicitly non-retail convenience).
Mission 3 storm smoke: `tools/shot.sh local/m3-storm.png --mission 3 --script storm-m3 --speed 16 --shot-frame 1050 --late-frame 0` isolates the retail storm trigger/entry/release rules and asserts ship carriage (invulnerable ship placed at trigger; not combat validation).
Use `storm-m3-damage` with the same storm arguments to check survival under normal mission protection (no forced invulnerability).
Post-storm hunt/NIS smoke: `tools/shot.sh local/m3-hunt.png --mission 3 --script hunt-m3 --speed 16 --shot-frame 140 --late-frame 0`
checks reveal/patrol without premature hunt, spotting/retreat without engagement, NIS helm halt, fleet launch and intercept ships waiting for their chase trigger; it teleports route endpoints,
so it does not validate the full retreat through scenery or combat. Current visual smoke: `tools/shot.sh local/m3-current.png --mission 3 --script current-m3 --shot-frame 90 --late-frame 0` places the ship on the current and checks two flowing ribbon layers.
Diagnostics are off by default; simulation speed does not scale the frame-time measurement.
Use a scratch `XDG_DATA_HOME` for automated campaign runs if the player's campaign-point save must remain untouched.
Converted assets are found in this order: `PROCYON_ASSETS`, `assets/` in the source checkout (regardless of the working directory), then the
per-user folder `$XDG_DATA_HOME/openprocyon/assets` (Linux default `~/.local/share`; `%APPDATA%\openprocyon\assets` on Windows). Without
any, the game exits with import instructions. `content/` is always layered on top. CLI mod/screenshot paths remain relative to the working directory.

## Layers

The game is one simulation with interchangeable presentations, so every presentation shares every rule and data path and differs
only in what is drawn:

```
shared core + model      ship, combat/, collision, rules/, campaign, docking, boarding, scenario, data, modding, ...
(the `openprocyon` lib)  menu.rs / panels.rs / options_menu.rs: UI state, Actions, marker components, input and their behaviour
        ▲
classic/  (this binary)  retail-art views
```

- Shared code is the `openprocyon` library (`lib.rs`); the classic view lives in the binary (`main.rs`), which passes its view plugin to
  `openprocyon::run`. Other presentations live in their own crates that depend on the library and call `openprocyon::run(view_plugin)`.
  The compiler enforces that shared code cannot reach a view; there is no style flag to branch on.
- **Views only draw.** A view spawns widgets tagged with the shared markers (`menu::Action`, `panels::PanicButton`, `GunButton`, `Contact`, `HudRoot`, ...) or reacts to shared descriptions (`options_menu::OptionsMenuArt`), and reads shared state (`MenuModel`, `HeadState`, `gauge_value`, `Radar`). Clicks, pausing, saving and every gameplay effect live in the shared modules, so every view behaves the same.
- Shared code never names `classic::` and never builds UI widgets (`Node`, `ImageNode`, `Text`, ...; `tests/test_layers.py` enforces
  both, allowing only the retail-art helpers in `sprites.rs` and the F1 debug text). Shared code may set the text, art and visibility
  of widgets a view spawned with a shared marker. If a view needs something new, add it to the shared model (state, marker or helper), not a branch in the view.
- Ordering seams: front-end views build in `menu::BuildMenu` (before `menu::RunButtons`); HUD views build before `panels::HudSet` and animate after it.
- Gameplay changes follow the same rule, cheapest first: tuning and assets as a mod folder (`mods/`, overlay system); behaviour that data cannot express as an alternative system another crate registers, while the faithful one stays the default and unedited. Classic behaviour stays pinned by the existing tests.
- Assets: `assets/` is exactly what the importer produces from the player's own copy; nothing rewrites it. `content/` holds the files that ship with the code.

## Runtime ownership

Keep the existing module boundaries. New files should own a real feature, not just hide a long function.

| Module (`game/src/`) | Owns | Change here when… |
|---|---|---|
| `lib.rs` | Library root and `run(view)`: application wiring, CLI `Config`, time scale/pause, profiling and the screenshot driver | Wiring/ordering systems; feature logic goes in its owning module |
| `ship.rs` | `Ship`/`Side` state, movement integration (`step`), skirmish AI | Changing free motion or AI; movement tests live here |
| `scenario.rs` | Battle lifecycle: spawn from map/mission, restart, leave, outcome, ship naming | Changing what spawns or how a battle starts/ends |
| `view.rs` | Orbit camera, world mouse/keyboard input, sky, F1 debug text | Changing camera or world input |
| `data.rs` | JSON → ship classes, sections, mounts, geometry and shared weapon definitions | Changing simulation data schemas; coordinate the exporter and mod fixtures |
| `combat/` | `mod.rs` health, targets, messages, engines, destruction; `gunnery.rs` gun mounts, turret aim/lead, bursts, ship and tower fire; `shells.rs` shells in flight, torpedo guidance, point defence, hit damage; `hit_test.rs` ray/segment tests against sections; `asteroid.rs` asteroid motion/breakup; `damage.rs` smoke/fires/mesh swaps; `status.rs` HUD-facing gun/section queries; `tests.rs` | Changing combat; report hits/firing/destruction through the existing messages |
| `collision.rs` | Ship movement substeps, ship/scenery contact, separation and contact velocity | Changing physical collision; free-motion equations remain in `step` |
| `campaign.rs` | Mission loading (`load_id`, `next`), campaign ship/map setup, campaign points and save files (`CampaignSave`, `list_saves`, `write_save`) | Changing mission initialization or which exported mission is loadable |
| `rules/` | `eval.rs` pure rule evaluator (`Script`, no ECS); `systems.rs` ECS dispatch, mission AI, UI, music, NIS camera; `tests.rs` | Implementing a retail condition/action; test the evaluator before ECS wiring |
| `docking.rs` | Dock points and their marker effects, dock routine (wait / approach / docked / return), repair while docked, dock rule hooks, click-to-dock picking | Changing docking/repair or the `Group/Unit Is Docked`-style rules |
| `boarding.rs` | Crew strengths, grapple order/hook/winch, boarding fight and capture, the status bar, AI boarding of bases; `BoardMsg` / `Captured` messages | Changing boarding/grappling or the capture rules (`docs/re/boarding.md`) |
| `crew_arms.rs` | Campaign outfitting model, recruitment, post/bank transactions, live crew/equipment application; `classic/crew_arms.rs` draws it | Changing crew/arms purchases, assignment and their gameplay consumers (`docs/re/scoring-saves.md` §7, `docs/re/crew.md`) |
| `lifeboats.rs` | Lifeboats: `Lifeboat` marker, `ShipLost` message, boat spawn when a flagged ship is destroyed, drift, nearest-ship sphere pickup, pickup sound/points/table, rule-visible owner list | Changing lifeboat creation/pickup (`docs/re/lifeboats.md`); the creation flag lives on `MissionShip`, the saved table in `rules::Script` |
| `smoke/` | Validated `--script` names, fleet/campaign and lifeboat smoke drivers, tender setup hook, `PROCYON_END_SHOT`/`PROCYON_NIS_SHOT` screenshot hooks | Adding a script name or changing scripted checks; keep production simulation in its owning module and preserve system ordering |
| `classic/` | Retail-art views (default): `menu.rs` front-end screens and their hover art and list scrolling, `panels.rs` HUD sprites (bar, compass, dial, talking head, weapon panel), `options_menu.rs` right-click menu art and context cursor, `world_hud.rs` ship-name banners, target chevrons and objective indicator, `starmap.rs` star map art, `mission_ui.rs` objectives, speech banners, hints, end text and letterboxes, `boarding_bar.rs` boarding status bar, `crew_arms.rs` outfitting screens | Changing how the retail look is drawn; behaviour goes in the shared module |
| `options_menu.rs` | Right-click menu: opening, items, hit rects, actions and sounds (`OptionsMenuArt` for the views), which context cursor to show (`HoverCursor`: dock / can't / attack) | Adding menu items (Hold and Cloak are implemented; Board and Grapple live in `boarding.rs`) |
| `briefing.rs` | Opening NIS, subtitles and skip behaviour (widgets: `BriefingUi`, `Subtitle`, `Portrait`) | Changing intro timing, camera handoff or skip placement |
| `panels.rs` | HUD contract: widget markers, HUD input, gauge values, radar contacts, status icon, talking-head state, pause and speed buttons | Adding panel controls: marker + behaviour here, drawing in each view |
| `starmap.rs` | Star map opened by the radar disk (`docs/re/starmap.md`): open/close, sounds and what it shows (`StarmapContents`) | Which icons/labels the map shows; world extent comes from map JSON `world_half` |
| `sprites.rs` | Retail sprite/font access for any view: `HudData` and faction skin choice, sprite rects/offsets, HD sheet scales, 9-slices, `image`, `Fonts` | Changing shared art metrics; keep menus (`MENU_SKIN`) separate from battle skin selection |
| `menu.rs` | Front-end model: screens, `Action`s and their handling, battle entry, the Esc in-game menu (overlay, pauses; Save/Load/Quit), `MenuModel` for the views; `menu/skirmish.rs` lobby slots, races and fleets; `menu/saves.rs` save/load lists and F3/F4 quick save/load; `menu/settings.rs` sound/display options | Changing front-end choices or transitions |
| `modding.rs` | Asset overlays, merge semantics and inheritance | Changing override rules; preserve base assets and test merges |
| `environment.rs` | Shared frame-boundary sight/group/contact snapshot, nebula rendering/energy/occlusion and whale motion/calls | Changing environment simulation or shared detection (`docs/re/visibility.md`); UI consumers must not invent separate range tests |
| `fx.rs`, `wake.rs`, `flag.rs`, `course.rs`, `audio.rs` | Feature-specific rendering/audio | Changing those presentations; do not put their tuning in `lib.rs` |

`mod.rs` files re-export their children, so `crate::combat::X` / `crate::rules::X` paths are stable. Split further only when the next concrete feature needs a clearer interface—no empty framework or blanket directory reshuffle.
Use explicit system ordering where one system reads another's updates (for example faction selection before panel rebuild).
Tag battle-owned roots for scenario cleanup and test returning to the same ship/map, not just first launch.

## Data and generated assets

- **Ships:** `assets/ships/<Id>/` (stats, hit geometry, hull mesh, names, dock points per ship).
- **Definitions:** `assets/data/weapons/`, `assets/data/crew.json`, `content/data/hud/bar.json`.
- **Shipped content:** `content/` holds files that ship with the code, never imported: fonts (URW base35), hand-written data (`data/hud/bar.json`) and the voice event table. The game overlays it on the imported assets like a mod (`assets_dir()` in `lib.rs`), so a file there replaces the imported one of the same path.
- **World/mission data:** `assets/maps/<id>/` (map, mission, media, backdrop); export coverage is broader than playable runtime support.
- **Generated presentation:** 3D models in `models/{weapons,scenery,bases,sky,ui}/` (ship hulls are `ships/<Id>/model.glb`) (category from the name: `tools/models.py`; the Rust loader finds them by file name via `data::model`), `hud/`, `menu/`, `fx/`, `flags/`, `sound/<kind>/` (found by name via `audio::sound`; mission-only dialogue in `maps/<id>/speech/`), `fonts/` and waypoint art.
- **Persistent custom edits:** `mods/<name>/` mirrors asset-relative paths. Example: `mods/example/`.
- **Scratch:** `local/` only (git-ignored). Never put screenshots, logs or raw retail inputs anywhere tracked.

Do not change converted asset paths just to make the tree look tidy: JSON, Bevy asset handles and mod overlays reference those paths.
Converted content is never committed: every player and contributor imports their own copy. `tools/compare_assets.py <expected> <actual>`
compares two imports by meaning (JSON by value, images by pixels) to check exporter changes. See [data-format.md](data-format.md) and [modding.md](modding.md).
`data/hud/bar.json` is authored layout configuration; its sprite crops are generated. Most other retail JSON is regenerated, so edits belong in mods.

## Tool ownership

`tools/` is a Python package used with `python3 -m tools.<name>` from the repository root. Keep names stable for existing callers.

| Group | Files |
|---|---|
| Retail format readers | `source_text.py`, `wot.py`, `mdb.py`, `msb.py`, `hdt.py`, `fft.py`, `eft.py` |
| Mesh conversion | `mdb_to_glb.py`, `preview.py` |
| Content export | `export_ship.py`, `export_combat.py`, `export_maps.py`, `export_campaign.py`, `export_campaign_media.py`, `export_environment.py`, `export_crew.py`, `export_effects.py`, `export_hud.py`, `export_voices.py`, `export_waypoint_art.py`, `export_collision.py`, `export_asteroids.py` |
| Asset preparation | `pack_data.py`, `format_json.py`, `fix_alpha_bleed.py`, `make_slices.py`, `trim_hud_rects.py` |
| Workflow | `build_assets.sh` (the importer), `check.sh`, `shot.sh`, `compare_assets.py` |
| Research | `extract_wot.py` (catalog of WOT properties) |

The build pipeline is defined once in `tools/build_assets.sh`: export raw ships → HUD/art/media/maps/campaigns/crew/effects → pack ship/weapon data → format JSON.
Raw per-ship exports are temporary inputs to later exporters and removed by packing; do not commit them.
HUD trimming/crops run before alpha-bleed cleanup. Importing needs a read-only retail install (`--game <dir>` or `PROCYON_GAME_ROOT` in `.env`); the install is never modified.
Python dependencies are in `requirements.txt` (the checks add ruff via `requirements-dev.txt`); Rust dependencies and resolved versions are in `game/Cargo.toml` and `game/Cargo.lock`.

## Implementation order

1. **Finish functional HUD controls:** one complete control at a time (state, input, gameplay handler, feedback and tests). Artwork alone is not implementation.
2. **Polish and validate Mission 1:** it has been completed manually end-to-end. Resolve the reported FPS/torpedo/docking/towing/nebula issues (`docs/re/campaign.md`, "Next"), and verify defeat, restart and return-to-menu. Do not treat the earlier pirate-fight stall as an outstanding blocker without a new reproduction.
3. **Fidelity passes:** compare behaviour and screenshots against retail evidence; retain explicit PROVISIONAL labels for guesses.
4. **Expand playable campaigns:** only after the Mission 1 path works. Exported Missions 2–13 must not be presented as supported gameplay yet.

Keep this list about outcomes, not speculative systems. Use [fidelity-backlog.md](fidelity-backlog.md) as the consolidated original-game gap checklist: pick a stable ID and acceptance scenario before implementing, then update its status and the owning `docs/re/` evidence. Record limitations next to their owning code. Exported assets, visible buttons and accepted/no-op mission rules do not count as implemented behaviour.
Never publish, upload or attach converted retail content anywhere (issues, CI artifacts, releases).

## Documentation map

- [fidelity-backlog.md](fidelity-backlog.md): current consolidated fidelity gaps and acceptance criteria.
- [faithful-remake.md](faithful-remake.md): original direction, evidence and historical implementation milestones; use the backlog for current gap status.
- [data-format.md](data-format.md): editable schema and export limits.
- [modding.md](modding.md): overlays, inheritance and HD art.
- [re/campaign.md](re/campaign.md), [re/movement.md](re/movement.md), [re/gunnery.md](re/gunnery.md), [re/effects.md](re/effects.md), [re/sound.md](re/sound.md): evidence behind implementation.
- [re/scenery-placement.md](re/scenery-placement.md): island/base height, authored tilt and unresolved native scene/matrix conventions.
- [re/visibility.md](re/visibility.md): sight range/group spotting, native occlusion, current consumer gaps and acceptance cases.
- [reference-video.md](reference-video.md): visual comparison notes.
- [wot-format.md](wot-format.md): WOT file format notes.
