# Development guide

## Short daily loop

Work from the repository root with the Python virtual environment active. Use `make help` for commands.
The Makefile and `tools/check.sh` use the checkout's `.venv` when there is one, even if it is not active. On Windows, install GNU Make
(`winget install ezwinports.make`) and Git for Windows. `make` then works from PowerShell or Git Bash: the shell scripts run through
Git Bash (`GIT_BASH=<path>` if it is not in the default place), and the screenshot and battle tools use the desktop (`PROCYON_RUN=window`; `software` needs Xvfb).

1. Read the owning module and its callers; pick one complete behaviour or visual batch.
2. Put editable gameplay changes in `mods/`; change exporters only when the retail conversion is wrong.
3. Add a small regression test for new logic. Prefer synthetic parser tests and pure mission-rule tests over long simulations.
4. Run the relevant tests while iterating, then `make check` before committing.
5. For ECS wiring or UI changes, also run a screenshot smoke test. Unit tests do not detect every Bevy query conflict or layout problem. `make smokes [ARGS="--jobs N"]` runs every documented smoke with its arguments (`tools/smokes.sh`; add new smokes to its list with
   a `kind`: `asserts` for a script that logs a PASS line when its checks held (the run fails without it), `visual` otherwise). It builds
   once, runs the binary, plays N at once (default: half the cores; each run has its own display, scratch `XDG_DATA_HOME` and mod overlay)
   and ends with one line (`smokes: 22 ok, 2 FAIL: dock, fleet (...)`); a failure prints its last relevant log lines and the rerun command.
   `make smoke WHAT=changed` runs only the smokes that the files changed against the merge base with origin/main (and uncommitted ones)
   map to; the map is `MAP` in `tools/smokes.py` (combat, ship: battle check + damage, target, course; rules, campaign: briefing-skip,
   lifeboat-m1, storm-m3, hunt-m3; docking, boarding: dock + battle check; classic, panels, menu: pause, fleet, mods; fx, render: fxtest;
   lib.rs, loading, smoke/: everything). `WHAT=<regex>` picks smokes by name.
   `make autoplay [ARGS="1 2"]` (`tools/autoplay.py`) plays campaign missions with the `--script mission` autopilot and prints a
   table per mission: outcome (won, lost, time limit, stuck in NIS, panic), peak entities/particles, slowest frame and how many
   mission rules never fired (named in the log). It finds crashes and soft-locks, not fidelity problems.
   `make battles [ARGS=ambush]` (`tools/battle_check.py [--jobs N]`, part of `make check --full` when assets are imported; stutter is judged only with `--jobs 1`) plays short skirmishes with
   fixed fleets at 16x and fails when one stops behaving like a battle: a side never fires or hits, a computer ship parks, a ship
   grinds on scenery, or the game panics. The game writes one `battle report {json}` line (`PROCYON_REPORT=<game s>`,
   `smoke/report.rs`); read it before adding print statements. When a change touches targeting, steering or what counts as a
   base/ship, add or tighten a scenario there rather than relying on a long manual run.
   `--fire-rule "<rule name>"` (repeatable) and `--start-at x,y` start a mission at a later beat: the named rules' actions run once the world-initialise rules have
   run and the primary ship is placed (`Script::fire_start_rules` in `rules/eval.rs`; debug only, see `docs/modding.md`).
   The developer console (the backquote key, rebindable, or F12, in any battle, skirmish or mission; `rules/console.rs` model and parsing,
   `rules/systems/dev_console.rs` keys and effects, `classic/console.rs` drawing; not retail behaviour) is a one-shot runner for the
   rule vocabulary, not a second command system: a line becomes a rule action (or a condition, for a query) and goes through
   `Script::act` / `Script::cond`, so every action a mission can use works (`act {json}`), and a new action needs nothing here.
   Short commands only build the common ones (`god`, `kill`, `reveal`, `tp`, `set`, `timer`, `task`); `repair` has no rule action and
   touches the ships directly; `fire`, `why` (the rule trace's `Script::explain`, asked once), `flags`, `timers`, `tasks` are the mission-debug commands, and `schema <text>` prints a Type's fields from `rules/eval/schema.rs`, which `act` / `cond` also check their JSON against (`help` lists them; Tab completes the command word, then the arguments: rule, group, point set, flag, timer, task and counter names from the mission, schema Types, and inside act / cond JSON the Type, field names and a field's values; repeated Tab cycles, several matches are listed; a name the mission does not have is refused with the closest one; a name with spaces goes in "quotes" and Tab adds them; the code is rules/console/complete.rs). Mission flow (`rules`, `skip`, `nis end`, `win`, `lose`, `restart`), ships and inspection (`ships`, `inspect`, `primary`, `damage`, `attack`, `stance`, `ally`, `hostile`, `tow`, `untow`), authoring (`where`, `mark`, `show` / `hide` volumes, paths and points, drawn as thin unlit boxes with UI labels because the build has no gizmos: `classic/console.rs`) and camera and sound (`cam free|player|follow`, `hud on|off`, `fps [on|off]`, `say`, `music`; `fps` measures frames from the real-time clock in `rules/console/fps.rs`, saves `Settings.fps` and is drawn by `classic/console.rs`) are in `rules/console/more.rs`; each uses a rule action through `Script::act` where one exists (`skip`, `restart`, the drawing and the free camera ask the ECS side through `Outcome`; `DevView` holds what is shown). `--script console-view --mission 2 --console --dist 1500 --pitch 1.3` is the runtime check for the drawing, the HUD switch and the free camera. `list <kind>` and `find <text>` print the names and ids a mission defines (`rules/console/list.rs`; queries); the output scrolls (`Console::scroll`, 2000 lines; the keys and wheel are in `systems/dev_console.rs`, the markers in `classic/console.rs`). `counters` / `counter <name> <value>` read and set the engine-added counters (`OP Set Counter`), `why` also says when a rule is inactive or waiting in an `OP Wait`, and `spawn` waits for an action that creates ships (none exists). It is
   on in debug builds, and in release builds when the Display option "Developer Console" is on (off by default, saved in `settings.json` as `console`); `--console` or `PROCYON_CONSOLE=1` turn it on for one run without the option. One dispatch serves every singleplayer battle (`systems::dev_console::run`): it works on the campaign mission's script, a skirmish map's `SkirmishScript`, or an empty one (`BareScript`, a battle with no `script.json`), and each command declares what it needs (`console::Needs`, `Command::needs`: nothing, the battle's ships, a rules script, a campaign mission). A command the battle cannot serve is answered with the need (`Command::refusal`) and changes nothing, so it never marks the session as cheated; a ship command in a battle whose ships carry no `MissionShip` first gives them one (`attach_ship_state`). A new command gets its need in `Command::needs` (a test lists them; `help <command>` shows it). `--start-at` / `--fire-rule` reach a skirmish map's script the same way a mission's (`campaign::apply_start`); a map without a script warns that they are ignored. Multiplayer is not implemented (its front-end buttons are not wired and there is no networking code); when it is, the console must be unavailable in it (`Console::available`), as a cheat would be there. `help` lists the commands from one table (`console::TABLE`), which Tab completion and the "did you mean" answer also read. The keys `help` lists under "Keys:" are `console::KEYS`; change it together with `Console::key_with` (a test checks they agree). Any command that changes the game sets
   `rules::console::Cheats::used` for the battle: the campaign points are not recorded then and the screen shows "CHEATS USED". A restart (R, `restart`, N) clears that mark and `god`; leaving the battle or loading another mission
   resets the developer view (`hud`, `cam`, `show`), a restart of the same mission keeps it.
   While it is open the battle is paused and the game sees no keyboard or mouse. Add a command by extending `console::parse`,
   `Command::needs` and `Script::console_run` with a test in `rules/console/tests.rs`; `--script console --mission 1` is the runtime smoke test.
   `PROCYON_TRACE=<file.csv>` (`smoke/trace.rs`) logs every ship (position, velocity, heading, section hit points) and every shell
   once per simulation step, in game axes: the table to lay beside a measurement of the original when a battle plays out
   differently (how close ships come, how often they fire and hit). Stage the same start points with a small mod that replaces
   the map's `map.json`. Ships and shells are numbered by creation order (`SpawnIndex`) and `gt` restarts at 0 when a battle restarts,
   so two runs compare row by row. Every battle logs `battle seed N`; `--seed N` or `PROCYON_SEED=N` replays it (`combat::SimRng`,
   docs/re/tick.md).
   `PROCYON_COMMANDS=<file.jsonl>` records the player's orders (`commands.rs`, so far engine orders, target picks, move, plotted-course and cancel orders, the right-click menu's Stop, Hold, Cloak, Dock, Tow, Board and Grapple, a left-click dock, Backspace's dock and grapple cancels, the Fleet Commands formations, attack and cease fire, P, panic fire and indirect shots), and
   `--replay <file.jsonl>` plays them back with the same scenario arguments, seed and mods (the game names any that differ), also
   from a normal play session: the log keeps each frame's game-time advance and tick count, the replay feeds those to the clock, so
   frame-driven rules and ticks line up as recorded. JSON lines, format 2 (PROVISIONAL; format 1 still reads): a header (format,
   version, seed, scenario arguments, mod folders and, for a battle launched from the skirmish lobby, the lobby's whole setup:
   `skirmish`, `menu/skirmish_setup.rs`), written once the battle has loaded; `frames` ([advance ns, ticks] or [advance ns, ticks,
   repeat]); and `command` lines stamped with the tick (or paused frame) they were applied on, naming objects by kind and
   `SpawnIndex` only (ships, armed bases, rocks, space animals; map scenery without one, such as a base without guns, by its
   place in the map's object list: `SceneryIndex`). The setup holds ids only (map, ship classes, gun keys per weapon bank, crew keys, a picked name as its
   position in the hull's name list) and the computers' draw seed, not their fleets: those are drawn again at launch, so a setup
   replays only on the same data and mods. A lobby battle, set up by hand in the menus or with `--lobby`, replays with `--replay
   <file>` alone: the game opens the lobby, sets it up from the header and presses Launch (`--lobby` next to such a log is refused,
   and the header leaves it out of the arguments). `--lobby` also takes a whole setup as JSON (a spec starting with `{`). Only the
   first battle of a run is recorded; a campaign mission started from the menus is marked `from_menu` and refused (neither the
   arguments nor a setup hold it). Arguments that take a path (`--mod <dir>`, ...) are left out of the header. `battle_check
   --replay` (in `check.sh --full` for `red_rover` and the lobby scenario `border_dispute_full`) records a run of `--script
   commands` and fails when its replay's battle report differs; a lobby scenario replays from the log alone.
   `--fixed-step` (or `PROCYON_FIXED_STEP=1`; `--shot` implies it unless `PROCYON_FIXED_STEP=0`, `gametime.rs::wants_fixed_step`) makes a scripted run reproducible: each frame is one 33 ms tick at 1x whatever
   the frame rate, and the clock-seeded cosmetic draws (ship names, stars) use a fixed seed. Run twice, it gives the same
   pictures and the same trace. `python3 -m tools.tracediff A.csv B.csv` compares two traces (one line per file, then the
   first differing row). `python3 -m tools.visdiff` renders fixed-step scenes from the merge base (a warm worktree,
   `../openprocyon-visdiff-base`) and from this checkout, and prints `same` or `CHANGED` per scene, with diff pictures in
   `local/visdiff/`. Nothing is committed or uploaded. Both builds need `--fixed-step`, so the base must contain #535.
   Two kinds of scenario: a map with the `--ship/--allies/--enemies` shortcut (the human's own fleet; no lobby), and a lobby
   scenario (`--lobby "map=border_dispute;points=1000;limit=10;slots=human,7*computer"`, `menu/lobby_scenario.rs`) that sets the
   skirmish lobby up through the same model as its pull-downs, Auto Assign and Launch button. Only lobby scenarios play allied
   computer slots, full lobbies and large budgets, so a change to skirmish setup or computer fleets needs one. A spec the lobby
   refuses ends the run with status 2. The report marks each ship `computer` (the captain AI's test: only the human's own fleet
   waits for orders) and lists each fleet's victory points spent against its budget; the expectations `patrol`, `fire` and
   `budget` in `tools/battle_check.py` read those.
   Every scenario also judges whole-run sanity from the report's `sanity` block: no ship position that is not finite, none farther
   than `BOUNDS` from the origin, at most `ENTITY_CAP` entities at once (both PROVISIONAL, generous). `resolve` (opt-in) wants the battle
   to end or a ship to sink. `battle_check --twice` plays each scenario twice, fixed-step with seed 1, and fails when the reports differ
   (frame times excepted). Scenarios `red_rover` and `mixed_fleets` add a second map and mixed fleets. There is no `--batch` mode (many
   battles in one process) yet: it needs the headless entry below first. Measured on 2026-11-24: nine `Single<&Window>` systems outside
   `classic/` (`panels.rs`, `view.rs`, `menu/settings.rs`, `options_menu.rs`, `menu.rs`, `campaign.rs`, `smoke/fleet.rs`, `setup.rs`), plus the render-asset and audio setup.

   Headless runs (spike, 2026-10, read from the code, not yet built): `run` adds `DefaultPlugins` and `setup` demands a primary
   window, so the game cannot start without a window and GPU today. The simulation itself is already apart from rendering: it
   runs in `FixedUpdate` at the 33 ms battle tick, and collision shapes, hardpoints and muzzles come from the imported JSON, not
   from the GLB scenes. Known blockers, in order: `setup`/`set_game_speed` take the `Window` (also `view.rs`, `panels.rs`,
   `menu/settings.rs`, `options_menu.rs`); `fx` and `combat::setup` register meshes and the `EffectMaterial` render asset;
   `combat/mod.rs` and `combat/damage.rs` toggle ship parts by scene node `Name`, so ships need their scene children (the GLB
   loads without a GPU, but that is untested); audio needs an output device. First experiment: a `run_headless` entry that uses
   `MinimalPlugins` + asset, state, transform and scene plugins, steps `app.update()` with a manual time strategy, and drops
   `view`, `fx` and `audio`. Anything that then fails is the real list.
6. Review `git diff --check` and `git diff --stat`. Never commit converted assets (`assets/` is git-ignored).

```sh
# No Make dependency required: these are the authoritative underlying commands.
python3 -m tools.import_assets --game <install dir>   # once: convert your own copy of the game into assets/
tools/check.sh
cargo run --profile fast --manifest-path game/Cargo.toml -- --mission 1   # fast: near-release speed, incremental; release is for player builds
python3 -m unittest discover -s tests -p 'test_export_hud.py'
cargo test --manifest-path game/Cargo.toml panels::tests -- --include-ignored  # shared HUD; classic::panels::tests for the retail art
tools/shot.sh local/mission01.png --mission 1 --script briefing-skip --shot-frame 180   # --shot-frame 100,200 saves local/mission01_100.png, _200.png in one run; 1280x720; no late shot unless --late-frame N
tools/shot.sh local/paused.png --map red_rover --script pause --shot-frame 50 --late-frame 0
tools/shot.sh local/pause-restart.png --map red_rover --script pause-restart --shot-frame 90 --late-frame 0
```

`make check` calls `tools/check.sh` (`make check ARGS=--quick` / `--full` pick a tier). The steps, cheapest first: ruff format check and
lint (Python, rules in `ruff.toml`) → hygiene rules (once; the Python tests skip them via `PROCYON_HYGIENE_DONE`) → rustfmt check → strict Clippy → Rust tests; the Python tests run in the background beside those three and are reported just before the final line; `--full` then builds the
game and runs the startup smoke and the battle check.

| Tier | When | Runs |
|---|---|---|
| `--quick` | while iterating | ruff and hygiene; rustfmt + Clippy if anything under `game/` or a `.rs` file changed, the Python tests if a `.py` file changed (against the merge base with `origin/main`, plus uncommitted and untracked files) |
| default | before a commit | every step up to the Rust tests (asset tests with `--include-ignored` when an import exists) |
| `--full` | before a push or PR | default + `fast` game build + startup smoke + battle check; fails without imported assets unless `--allow-partial` |

Each step prints one line (`ok`/`FAIL`, name, seconds) and writes its full output to `local/check/<step>.log`; a failure prints its first
20 relevant lines and the command to rerun. The last line is `GATE OK <tier> <time>`, `GATE FAIL <step> (<log>)` or
`GATE PARTIAL ...` (no imported assets: it names what was skipped). Locally the gate unsets `CI`, which would turn Cargo's
incremental builds off. One module's tests: `cargo test --manifest-path game/Cargo.toml --lib <module::path>`.
Clippy and the tests build in the dev profile (incremental, debug assertions and overflow checks on); the startup smoke, `shot.sh`,
`smokes.sh`, `battle_check` and `autoplay` build the game in the `fast` profile (release speed, incremental) and run
`<target>/fast/openprocyon`. The `release` profile is only for player builds (`release.yml`); `battle_check --profile release`
or `autoplay --profile release` times what players get. Rust tests that need converted
assets are marked `#[ignore = "needs imported assets"]`; the gate runs them with `--include-ignored` when an import exists.
Mark new Rust tests that load imported data with the same ignore reason, including tests that do so through helpers.
Asset-free Rust tests always run. For a manual full run after importing, use `cargo test --manifest-path game/Cargo.toml -- --include-ignored`.
Python tests that need the install or assets skip.
`make fmt` formats Rust and Python (`ruff format`, `ruff check --fix`); it does not rewrite generated JSON. `python3 -m tools.format_json assets` formats asset JSON when needed.
Shared sight smoke: `tools/shot.sh local/sight.png --script sight --allies Ship_Navy_Cutter --enemies Ship_Pirate_Schooner --shot-frame 135 --late-frame 0`; it freezes three ships and asserts group/range/cloak transitions against real radar widgets. This is wiring validation, not a campaign playthrough.

Screenshot tests need a display, or Xvfb + Mesa for the headless helper. Outputs belong in ignored `local/`.
Run modes: `shot.sh`, `battle_check.py` and `autoplay.py` (and through them `smokes.sh`, `visdiff.py`, `check.sh`) launch the game through
`tools/rungame.py`, which reads `PROCYON_RUN`: `window` (the desktop session's GPU), `software` (Xvfb + Mesa lavapipe; CPU-bound)
or `auto`/unset (`window` with `DISPLAY`/`WAYLAND_DISPLAY` or on Windows, else
`software`); anything else is an error. On Linux `WAYLAND_DISPLAY` is cleared for the game in every mode (X11/XWayland: some Wayland sessions
render black), so `window` needs `DISPLAY`. Each run prints one line, e.g. `run: mode=window adapter "..." (DiscreteGpu, Vulkan) 3120 frames
in 41.0 s (76.1 fps)`, from Bevy's `AdapterInfo` log line and the game's `run end: frame N`; check it before comparing timings. With several
software runs at once (`smokes.sh` exports `PROCYON_JOBS`, `battle_check --jobs`), each gets `LP_NUM_THREADS` = cores / jobs unless set.
Under `--fixed-step` vsync is off (one frame is one tick: the refresh rate would cap the run). A screenshot run stops simulating on a frame
fixed by its arguments (the last picture, plus 100 frames for a script) and then waits, game time still, for the writes: same command, same
trace length.
The `pause` / `pause-restart` scripts assert that the non-retail Pause key freezes virtual time and that Pause / restarting resumes it; a scripted run goes on 100 frames after its last picture, which covers frame 70 (use a later `--shot-frame` or `--late-frame` for assertions beyond that).
Every run writes `<user dir>/logs/openprocyon.log` (`~/.local/share/openprocyon/`, `%APPDATA%\openprocyon\`; the run before, or the first 20 MB of a very long one, is `openprocyon.previous.log`): header with version/system/arguments, the console log, panics. Testers attach it to bug reports (`game/src/logging.rs`).

`PROCYON_PROFILE=1 make run ARGS='--mission 1'` logs rolling real frame time and world/FX/mesh/audio counts every five real seconds
and on mission-end changes (`PROCYON_PROFILE` also turns vsync off at every speed; see "Measuring a look"). Fleet smoke: `tools/shot.sh local/fleet.png --map red_rover --allies Ship_Navy_Cutter,Ship_Navy_Cutter,Ship_Navy_Cutter --script fleet --shot-frame 210 --late-frame 260`; it exercises shared keys/widgets, two heights, promotion, formations, restart and menu re-entry. Lifeboat smoke (scripted, needs `--shot` for the frame counter; or run the binary directly with `WAYLAND_DISPLAY=`): `--mission 1 --script lifeboat-m1` (kill Warrior, collect both boats, immediate next level), `lifeboat-m1-fallback --speed 16` (no pickup, ~109 s timers), `--mission 2 --script lifeboat-m2|lifeboat-m2-ladyj`, `--mission 4|9 --script lifeboat-m4|lifeboat-m9`, `--mission 5|6|7|10|12 --script lifeboat-kill`. They kill ships through health and teleport collectors: scripted wiring checks, **not** combat validation. `--mission 3 --script fleet-primary` isolates the real sloop setup rule to check the human-captain handoff; `--mission 5|9 --script fleet-primary` replaces rules with a synthetic primary action to check ECS dispatch, **not** later-mission gameplay. P and brackets now belong to fleet bindings; pause/speed remain on existing HUD/menu controls (Pause key is an explicitly non-retail convenience).
Mission 3 storm smoke: `tools/shot.sh local/m3-storm.png --mission 3 --script storm-m3 --speed 16 --shot-frame 1050 --late-frame 0` isolates the retail storm trigger/entry/release rules and asserts ship carriage (invulnerable ship placed at trigger; not combat validation).
Use `storm-m3-damage` with the same storm arguments to check survival under normal mission protection (no forced invulnerability).
Post-storm hunt/NIS smoke: `tools/shot.sh local/m3-hunt.png --mission 3 --script hunt-m3 --speed 16 --shot-frame 140 --late-frame 0`
checks reveal/patrol without premature hunt, spotting/retreat without engagement, NIS helm halt, fleet launch and intercept ships waiting for their chase trigger; it teleports route endpoints,
so it does not validate the full retreat through scenery or combat. Current visual smoke: `tools/shot.sh local/m3-current.png --mission 3 --script current-m3 --shot-frame 90 --late-frame 0` places the ship on the current and checks two flowing ribbon layers.
Diagnostics are off by default; simulation speed does not scale the frame-time measurement.
Use a scratch `XDG_DATA_HOME` for automated campaign runs if the player's campaign-point save must remain untouched.
Converted assets are found in this order: `PROCYON_ASSETS`, `assets/` in the source checkout (the checkout is found from the working directory or the executable: either must lie inside it), then the
per-user folder `$XDG_DATA_HOME/openprocyon/assets` (Linux default `~/.local/share`; `%APPDATA%\openprocyon\assets` on Windows). Without
any, the game exits with import instructions. `content/` is always layered on top. The tools (tests, `check.sh`, `shot.sh`, `smokes.sh`,
`battle_check`, `autoplay`) use one order, `find_assets()` in `tools/import_assets.py` (`python3 -m tools.import_assets --print-assets` for the shell tools): the game's, with the main checkout's `assets/`
after this checkout's, so a linked worktree uses the main import (never copy `assets/`). They export it as `PROCYON_ASSETS` for the game;
`check.sh` prints `ASSETS: <path>` or `ASSETS: missing`. CLI mod/screenshot paths remain relative to the working directory.

### Measuring a look

To measure what a look costs (a graphics mod, the render profile in `docs/modding.md`), run the same battle twice with `PROCYON_PROFILE=1`, once
without and once with `--mod <dir>`, e.g. `PROCYON_PROFILE=1 make run ARGS='--map red_rover'` and the same with `--mod mods/remaster`. `PROCYON_PROFILE`
sets `PresentMode::AutoNoVsync` at every speed (also after the speed buttons), so `frame_ms` is the real render cost, not the display period.
Compare `frame_ms` (rolling average) and `p99_ms` (99th percentile of the per-frame times since the previous line; a steady 60 fps needs it near 16.7).
The counters next to them say why a look is slower: `lights` (point + spot lights), `draw_entities` (`Mesh3d` entities), `materials`
(`StandardMaterial` plus effect-material assets), `particles` and `mesh_assets`. A second line, `profile gpu_ms`, gives the GPU milliseconds of each render pass
(rolling average, slowest first; `lightprobe_*` are left over from the sky light's first frames). Watch `uniform_allocation` and `bin_unpacking` there: Bevy runs them once per batch set per view
(the camera, the prepass and each shadow cascade), so a material type that is not bindless (a bind group per material) or materials changed every frame show up there and in `frame_ms`
long before the draw passes do. Let each run settle for a few log lines before reading it;
only compare runs on the same machine and scene.

## Battle clock and scheduling

Battle simulation runs in `FixedUpdate` at **0.033 s**, the interval documented in [re/gunnery.md](re/gunnery.md)
and [re/effects.md](re/effects.md). `battle.rs::BattleSet` orders command application → sensors → AI → fleet following →
docking/boarding orders → environmental forces → ship movement → combat → cleanup/accounting → outcome. Keep plugin systems
in the owning stage; use a local chain for dependencies within one feature. Simulation uses fixed `Res<Time>` without per-system
delta clamps. Virtual pause and speed still control this clock. At the original's speeds (up to 5x) one call catches up at most 10 frames
(10 x speed ticks, 0.33 s of game time at normal speed) and drops the rest of the backlog ([re/tick.md](re/tick.md)); above 5x, which the original
does not have, a call advances at most 0.1 s (about 3 ticks) so 16x stays smooth. Single-player mission and skirmish rules are not in the tick:
the `FrameRules` set runs them once per frame in `Update`, after the last tick, also while paused.

Keyboard/mouse/UI capture remains in `PreUpdate`/`Update`. A player order goes through `commands::Orders` (`commands.rs`): the input
system sends a `PlayerCommand` and `commands::apply_tick` applies it first thing in the next tick (`BattleSet::Tick`), or at the end
of the frame while the battle is paused, so it is recorded and replayable. Engine orders and target picks go this way so far; the
other orders still store a command in their resource/component or registered message queue and consume it on a fixed tick
(#369 moves them over one group at a time). Never read `just_pressed` in a simulation system. Frame input may wait until the
next tick. Bevy 0.20's `TimePlugin` gates message maintenance on fixed-schedule execution, retaining commands through zero-tick
frames and fixed output for frame readers. `fixed_tests.rs` verifies both directions, including multi-tick catch-up and no replay.

Presentation remains frame-driven: cameras, HUD, ropes/markers, damage meshes, breakup/debris, particles and audio playback.
Visual RNG is local, separate from combat RNG, so rendering extra frames cannot change damage rolls. Opening briefing speech
keeps its explicit real-time clock, independent of pause/speed; its arrival halt runs per simulation tick. The existing FX
particle interpolation (`tick_acc`) is unchanged.

Ships (lifeboats included), asteroids and shells are drawn between ticks by `interpolate.rs`, so a 60 or 144 Hz display does not
show 30 Hz steps. The simulation keeps reading and writing `Transform` on the tick. `FixedPreUpdate` records the pose the tick starts
from; right after the fixed loop (`RunFixedMainLoopSystems::AfterFixedMainLoop`, before `Update` and transform propagation) each
tracked `Transform` is replaced by `previous.lerp(current, Time::<Fixed>::overstep_fraction())`, and right before the next fixed
loop the simulation's pose is put back. So `FixedUpdate` systems only ever see their own tick's pose, while the orbit camera, wakes,
picking, flags, sails and particles follow the drawn one. The drawn pose lags the simulation by up to one tick (33 ms). Above 1x the
overstep fraction still applies; while virtual time is paused it does not move, which holds the last drawn pose. Entities spawned in
a tick are drawn where they spawned. A `Transform` written by a frame system (a scripted teleport) becomes the new simulation pose and
snaps, and `Sinking` ships are not interpolated (`breakup` owns their pose). `GlobalTransform` is propagated once per frame in
`PostUpdate`, from the drawn pose, but gunnery reads muzzle `GlobalTransform`s on the tick: so the first thing in every tick
(`FixedPreUpdate`, `propagate_sim`) rebuilds the `GlobalTransform`s under each tracked entity whose `Transform` changed since the
last tick from the simulation pose. Shell spawn points therefore do not depend on the frame rate; the cost is one extra walk over
the moved ships' node trees per tick (30 per second), not per frame.

## Layers

The game is one simulation with interchangeable presentations, so every presentation shares every rule and data path and differs
only in what is drawn:

```
shared core + model      ship, combat/, collision, rules/, campaign, docking, boarding, scenario, data, modding, ...
(the `openprocyon` lib)  menu.rs / panels.rs / options_menu.rs / pointer.rs (the one reader of the deprecated `Interaction`): UI state, Actions, marker components, input and their behaviour
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

`lib.rs` re-exports a fixed list of names from `scenario`, `ship` and `view` (grouped by owner) so older code can write `crate::Ship`. Do not add glob re-exports (`pub use module::*`) to `lib.rs`, and do not grow that list for new code: write `crate::module::Name` so the owner is visible at the use site.

| Module (`game/src/`) | Owns | Change here when… |
|---|---|---|
| `lib.rs` | Library root and `run(view)`: application wiring (plugin list, the camera and light `setup`, the lobby scenario); re-exports the old crate-root names | Wiring/ordering systems; feature logic goes in its owning module |
| `cli.rs` | The command line: `--help` text, the one `FLAGS` table (value, scripted, recorded), the parsed `args()` every single-flag reader uses, and the list of `PROCYON_*` environment variables (`ENV_VARS`) | A new flag or environment variable: add it to the table/list and to `HELP` |
| `gametime.rs`, `profile.rs`, `shot.rs`, `paths.rs` | The battle tick, game speed, pause and fixed-step mode; `PROCYON_PROFILE` logging; `--shot` pictures and the run's `Config`; where assets, content and bundled mods are found | Those concerns; `lib.rs` only re-exports them |
| `ship.rs` | `Ship`/`Side` state, movement integration (`step`), skirmish AI | Changing free motion or AI; movement tests live here |
| `alliance.rs` `hostility` | The named answers to "is X hostile to Y" (`by_side`, `by_script`, `pair`, `base`, `friendly_dock`) that boarding, docking and the captain's target check use; gunnery, shells, cloak and crew still compare sides themselves. The test there pins where the side and script answers disagree (three factions) | Asking who is an enemy; do not add a sixth private copy |
| `scenario.rs` | Battle lifecycle: spawn from map/mission, restart, leave, outcome, ship naming | Changing what spawns or how a battle starts/ends |
| `view.rs` | Orbit camera, world mouse/keyboard input, sky and background stars (`docs/re/sky.md`), retail and free camera (`docs/re/camera.md`), F1 debug text, and the shared pick helpers `cursor_ray`, `ray_on_sea`, `project` (cursor ray, sea-plane hit, world to window) | Changing camera or world input |
| `data.rs` | JSON → ship classes, sections, mounts, geometry and shared weapon definitions | Changing simulation data schemas; coordinate the exporter and mod fixtures |
| `geom.rs` | `heading_of`, `angdiff`, `arc_covers`, point-to-segment distance and nearest-point helpers (bodies are the exact expressions the call sites used) | Any new heading, arc or segment-distance code; do not write another copy |
| `combat/` | `mod.rs` health, targets, messages, engines, destruction; `gunnery.rs` gun mounts, turret aim/lead, bursts, ship and tower fire; `shells.rs` shells in flight, torpedo guidance, point defence, hit damage; `hit_test.rs` ray/segment tests against sections; `asteroid.rs` asteroid motion/breakup; `damage.rs` smoke/fires/mesh swaps; `status.rs` HUD-facing gun/section queries; `tests.rs` | Changing combat; report hits/firing/destruction through the existing messages |
| `commands.rs` | Player orders as data (`PlayerCommand`, `Orders`), applied at the start of the next tick (or at the end of a paused frame); `Commander(PlayerId)`, the simulation's mark of a human's command ship (`PlayerShip` is the local view's, and gives player 0 its `Commander`); command-log recording (`PROCYON_COMMANDS`) and `--replay` | Routing another order kind through it (one group per PR, `tracediff` before and after) or changing the log format (ask a maintainer) |
| `collision.rs` | Ship movement substeps, ship/scenery contact, separation and contact velocity; AI collision avoidance (`collision/avoid.rs`) | Changing physical collision; free-motion equations remain in `step` |
| `campaign.rs` | Mission loading (`load_id`, `next`), campaign ship/map setup, campaign points and save files (`CampaignSave`, `list_saves`, `write_save`) | Changing mission initialization or which exported mission is loadable |
| `rules/` | `eval.rs` pure rule evaluator (`Script`, no ECS; `eval/` holds types, state, conditions, actions); `systems.rs` ECS dispatch (`systems/` holds mission AI, presentation, autopilot; `build_world` is the one rule-world builder for mission, skirmish script and dev console, `apply::FxParam` the one effect-environment param, `apply::spawn_voice` the one speech start); `active.rs` `ActiveScript`/`ActiveScriptMut`, the battle's script whether it is a `Mission` or a `SkirmishScript`; `tests.rs` | Implementing a retail condition/action; test the evaluator before ECS wiring; `fixtures.rs` (test-only) holds the shared Unit, world and Script builders of the rule tests; `eval/types.rs` `selects` is the one group-selector rule (`matches` and `Script::answers` call it) |
| `docking.rs` | Dock points and their marker effects, dock routine (wait / approach / docked / return), repair while docked, dock rule hooks, click-to-dock picking | Changing docking/repair or the `Group/Unit Is Docked`-style rules |
| `boarding.rs` | Submodules `boarding/odds.rs` (strengths, casualty odds), `grapple.rs` (harpoon, hook, winch), `fight.rs` (the fight and capture), `ai.rs`, `order.rs` (the board order the mission rule `OP Board Ship` gives). Crew strengths, grapple order/hook/winch, boarding fight and capture, the status bar, AI boarding of bases; `BoardMsg` / `Captured` messages | Changing boarding/grappling or the capture rules (`docs/re/boarding.md`) |
| `crew_arms.rs` | Submodules `crew_arms/posts.rs` (crew data, posts), `fitting.rs` (guns, banks, applying outfits), `view.rs` (screen model). Campaign outfitting model, recruitment, post/bank transactions, live crew/equipment application; `classic/crew_arms.rs` draws it | Changing crew/arms purchases, assignment and their gameplay consumers (`docs/re/scoring-saves.md` §7, `docs/re/crew.md`) |
| `lifeboats.rs` | Lifeboats: `Lifeboat` marker, `ShipLost` message, boat spawn when a flagged ship is destroyed, drift, nearest-ship sphere pickup, pickup sound/points/table, rule-visible owner list; `Abandoned`, the hull left disabled by `OP Abandon Ship` | Changing lifeboat creation/pickup (`docs/re/lifeboats.md`); the creation flag lives on `MissionShip`, the saved table in `rules::Script` |
| `loading.rs` | Battle start gate: `Loading` exists while models, textures, sky pictures and shader pipelines are not ready; the battle is paused, takes no input, starts no sound (music included) and counts no screenshot frames meanwhile, and views draw a cover (`Loading::busy` for a system that waits on an asset of its own) | Changing what a battle waits for before it starts |
| `battle.rs` | The battle wiring that crosses modules: `BattleSet`, `FrameRules`, removal/joining of objects (`Removed`, `Joining`, `tick_start`), the `BattleActive` set (battle screen, no overlay) and the tick and frame chains whose order spans owners; each owner adds the rest in its own `plugin` | Adding a system to the per-tick or per-frame battle chain |
| `smoke/` | Validated `--script` names, fleet/campaign and lifeboat smoke drivers, tender setup hook, `PROCYON_END_SHOT`/`PROCYON_NIS_SHOT` screenshot hooks | Adding a script name or changing scripted checks; keep production simulation in its owning module and preserve system ordering |
| `classic/` | Retail-art views (default): `menu.rs` front-end screens and list scrolling, `widgets.rs` the front-end widget kit (canvas, panels, buttons, lists, scrollbars, pull-downs, fleet bar, info panel) drawn from `theme.rs` / `data/hud/theme.json`, with the hover state swap, `panels.rs` HUD sprites (bar, compass, dial, talking head, weapon panel), `options_menu.rs` right-click menu art and context cursor, `world_hud.rs` ship-name banners, target chevrons and objective indicator, `starmap.rs` star map art, `mission_ui.rs` objectives, speech banners, hints, end text and letterboxes, `boarding_bar.rs` boarding status bar, `crew_arms.rs` outfitting screens, `setup.rs` the first-run import screen (content/ fonts and plain boxes) | Changing how the retail look is drawn; behaviour goes in the shared module |
| `victory.rs` | Skirmish end rules: the map's exported `victory` rules (base destroyed, no ships left, last team), decided from a battle snapshot; `scenario::outcome` feeds it | Changing how a skirmish is won or lost |
| `options_menu.rs` | Right-click menu: opening, items, hit rects, actions and sounds (`OptionsMenuArt` for the views), which context cursor to show (`HoverCursor`: dock / can't / attack) | Adding menu items (Hold and Cloak are implemented; Board and Grapple live in `boarding.rs`) |
| `briefing.rs` | Opening NIS, subtitles and skip behaviour (widgets: `BriefingUi`, `Subtitle`, `Portrait`) | Changing intro timing, camera handoff or skip placement |
| `panels.rs` | HUD contract: widget markers, HUD input, gauge values, radar contacts, status icon, talking-head state, pause and speed buttons; `hud_scale` (window height over `REF_H`), `UiFocus` (the named "UI is blocking this input" predicates, one per call site's set) and `PauseClaim` (one owner's remembered pause) | Adding panel controls: marker + behaviour here, drawing in each view |
| `starmap.rs` | Star map opened by the radar disk (`docs/re/starmap.md`): open/close, sounds and what it shows (`StarmapContents`) | Which icons/labels the map shows; world extent comes from map JSON `world_half` |
| `sprites.rs` | Retail sprite/font access for any view: `HudData` and faction skin choice, `Sprite`/`View` (a trimmed per-sprite PNG and where it sits in its untrimmed box), offsets, 9-slices and their HD scales, `Fonts` | Changing shared art metrics; keep menus (`MENU_SKIN`) separate from battle skin selection |
| `setup.rs` | First run without imported assets: its own small app with the "Import your game" screen state, install discovery (Steam libraries, installed-program entries, Program Files or a Wine prefix) and folder checks, the importer child process and its `[n/total]` progress, then a relaunch of the game | Changing how a player gets from a download to the menu |
| `menu.rs` | Front-end model: screens, `Action`s and their handling, battle entry, the Esc in-game menu (overlay, pauses; Save/Load/Quit), `MenuModel` for the views; `menu/skirmish.rs` lobby slots, races and fleets; `menu/saves.rs` save/load lists and F3/F4 quick save/load; `menu/settings.rs` sound/display options | Changing front-end choices or transitions |
| `authoring.rs` (+ `authoring/`) | The stable surface for tools built on the crate (the editor): asset lookup, overlay into a chosen folder, rule schema, mission check, `defined_names` / `accepted_names`, the published JSON schemas, `check_mod` (`authoring/check_mod.rs`: what `--check-mod` reports about a mod folder), `resolved_ship` (`data/ships/resolved.rs`: the loader's `ships::build` hands back the merged document, so which of `ship.json` and the model wins is decided once, in the loader and `derive::complete`), `RuleReport` / `rule_report_schema` (`authoring/rule_report.rs`: the file `--rule-report` writes; `Script::rule_report` in `rules/eval/state.rs` builds it from the per-rule run counts that also feed `never_fired`, `rules/systems/report.rs` decides when it is written), `sound_duration` (`authoring/sound.rs`: the length of a sound file, decoded with Bevy's own audio decoder, no second one), and `to_json_string` (`authoring/json.rs`: the layout of `tools/jsonio.py`), `keyed_lists` / `extend` / `merge_keyed` (wrappers over `modding::keyed_defaults` / `extend` / `merge_keyed`, the one keyed-merge rule the overlay builder also uses), and re-exports of `scenery_transform` / `game_rotation` (`scenario/spawn.rs`) and `Formation` / `formation_slots` (`ship.rs`) | Thin wrappers over the owning module, except the two files that are new here (the mod checker and the JSON writer, which have no other home); a mission or file check belongs in `rules/check.rs` or the loader and is re-exported |
| `cli.rs` | The flags that print and exit before any window or assets: `--version`, `--help` / `-h`, `--dump-schemas <dir>`, `--check-mod <dir> [--json]` | Add such a flag here; mission-authoring flags (`--check-mission`, `--rule-trace`, `--rule-report`, ...) are in `rules/cli.rs` |
| `modding.rs` | Asset overlays, merge semantics and inheritance | Changing override rules; preserve base assets and test merges |
| `environment.rs` | Shared fixed-tick sight/group/contact snapshot (also refreshed for frame input), nebula rendering/energy/occlusion and whale motion/calls | Changing environment simulation or shared detection (`docs/re/visibility.md`); UI consumers must not invent separate range tests |
| `fx/` (`lib.rs` parsing, `sim.rs` 30 Hz simulation, `draw.rs` quads; also the `EffectStarted` message the render blocks read), `wake.rs`, `flag.rs`, `course.rs`, `audio.rs` | Feature-specific rendering/audio | Changing those presentations; do not put their tuning in `lib.rs` |
| `render/` | Render profile building blocks (not retail), on only with `data/render/look.json`: `mod.rs` profile, camera, sun, sky light; `batch.rs` the shared `QuadBatch` that effects, bursts and tracers build their quads through (used by classic too); `lights.rs` effect lights; `materials.rs` effect/shell gain, lit smoke, effect shadows; `sails.rs` sail material and billow; `rules.rs` the generic `materials` block (any mod shader on matched retail materials); `trails.rs` engine plumes and wake ribbons; their shaders come from the mod | Adding a graphics building block; classic must stay unchanged without its profile key, and its shader uniform goes in docs/modding.md |

`mod.rs` files re-export their children, so `crate::combat::X` / `crate::rules::X` paths are stable. Split further only when the next concrete feature needs a clearer interface—no empty framework or blanket directory reshuffle.
Use explicit system ordering where one system reads another's updates (for example faction selection before panel rebuild).
Tag battle-owned roots for scenario cleanup and test returning to the same ship/map, not just first launch.

## Data and generated assets

- **Ships:** `assets/ships/<Id>/` (stats, hit geometry, hull mesh, names, dock points per ship).
  `data/ships.rs` deserializes ship definitions and hit geometry into private input structs, then converts to the existing
  runtime `Class` types. Keep section/mount insertion order, coordinate/angle conversions and older-mod defaults unchanged.
  Required typed assets report the logical file, resolved source (including replacement mod symlinks), and parse location or
  reference field. Unknown fields remain allowed; present fields with invalid types are rejected rather than silently defaulted.
- **Definitions:** `assets/data/weapons/`, `assets/data/crew.json`, `content/data/hud/bar.json`.
- **Shipped content:** `content/` holds files that ship with the code, never imported: fonts (URW base35), hand-written data (`data/hud/bar.json`) and the voice event table. The game overlays it on the imported assets like a mod (`assets_dir()` in `lib.rs`), so a file there replaces the imported one of the same path.
- **World/mission data:** `assets/maps/<id>/` (map, mission, media, backdrop); export coverage is broader than playable runtime support.
- **Generated presentation:** 3D models in `models/{weapons,scenery,bases,sky,ui}/` (ship hulls are `ships/<Id>/model.glb`) (category from the name: `tools/models.py`; the Rust loader finds them by file name via `data::model`), `hud/`, `menu/`, `fx/`, `flags/`, `sound/<kind>/` (found by name via `audio::sound`; mission-only dialogue in `maps/<id>/speech/`), `fonts/` and waypoint art.
- **Persistent custom edits:** `mods/<name>/` mirrors asset-relative paths. Example: `mods/silver-skirmish/`.
- **Scratch:** `local/` only (git-ignored). Never put screenshots, logs or raw retail inputs anywhere tracked.

Do not change converted asset paths just to make the tree look tidy: JSON, Bevy asset handles and mod overlays reference those paths.
Converted content is never committed: every player and contributor imports their own copy. `python3 -m tools.compare_assets <expected> <actual>`
compares two imports by meaning (JSON by value, images by pixels) to check exporter changes. See [data-format.md](data-format.md) and [modding.md](modding.md).
`data/hud/bar.json` is authored layout configuration; the sprites it names are generated (`hud/sprites/`). Most other retail JSON is regenerated, so edits belong in mods.

## Tool ownership

`tools/` is a Python package used with `python3 -m tools.<name>` from the repository root. Keep names stable for existing callers.

| Group | Files |
|---|---|
| Retail format readers | `source_text.py`, `wot.py`, `mdb.py`, `msb.py`, `hdt.py`, `fft.py`, `eft.py` |
| Mesh conversion | `mdb_to_glb.py`, `preview.py` |
| Content export | `export_ship.py`, `export_combat.py`, `export_maps.py`, `export_campaign.py`, `export_campaign_media.py`, `export_environment.py`, `export_crew.py`, `export_effects.py`, `export_hud.py`, `export_voices.py`, `export_waypoint_art.py`, `export_collision.py`, `export_asteroids.py`, `ogg.py` |
| Shared helpers | `paths.py` (`REPO`), `jsonio.py` (`dump`/`write`: the one JSON layout the exporters write), `rungame.py` (build the game, the run mode `PROCYON_RUN` and its display/graphics settings, scratch data home and environment, logged run and `run:` line: `autoplay.py`, `battle_check.py`, `shot.sh`) |
| Asset preparation | `pack_data.py`, `format_json.py`, `fix_alpha_bleed.py`, `make_slices.py`, `hud_sprites.py` (shared sprite layout) |
| Workflow | `import_assets.py` (the importer; `build_assets.sh` wraps it), `check.sh`, `shot.sh`, `compare_assets.py`, `package_release.py` (player download) |
| Research | `extract_wot.py` (catalog of WOT properties) |

The build pipeline is defined once in `tools/import_assets.py`: export raw ships → HUD/art/media/maps/campaigns/crew/effects → pack ship/weapon data → format JSON.
The steps run in one process (`tool()` runs each module as `python -m` would), so the importer also works frozen: the `release`
workflow (`.github/workflows/release.yml`, `tools/package_release.py`) ships it as `openprocyon-import` next to the release game binary and `content/`.
Raw per-ship exports are temporary inputs to later exporters and removed by packing; do not commit them.
The HUD sprites are cut out, trimmed and alpha-cleaned in `export_hud.py`; the slice strips are made from them, then cleaned. Importing needs a read-only retail install (`--game <dir>` or `PROCYON_GAME_ROOT` in `.env`); the install is never modified.
Python dependencies are in `requirements.txt` (the checks add ruff via `requirements-dev.txt`, release packaging PyInstaller via `requirements-release.txt`); Rust dependencies and resolved versions are in `game/Cargo.toml` and `game/Cargo.lock`.

## What to work on

Priorities are the GitHub milestones (v0.2 skirmish, v0.3 campaign, Later) and the rules in [AGENTS.md](../AGENTS.md) "Workflow":
fidelity work starts from the project board "OpenProcyon fidelity: every rule of the original". Exported assets, visible buttons
and accepted/no-op mission rules do not count as implemented behaviour. Record limitations next to their owning code.

## Documentation map

- [../AGENTS.md](../AGENTS.md): the hard rules and the workflow; [../CONTRIBUTING.md](../CONTRIBUTING.md): ways to help, pull requests.
- [modding.md](modding.md): making, testing and sharing mods, the overlay, weapons, sounds, the render profile (its first table indexes every doc); [missions.md](missions.md): writing a custom mission; [data-format.md](data-format.md): the asset layout.
- [re/](re/): the original's behaviour as specifications (rules, numbers, sources), one file per system.
- [wot-format.md](wot-format.md): WOT file format notes.
