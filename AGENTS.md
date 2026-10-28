# AGENTS.md

Instructions for anyone changing this repository: AI coding agents (any tool) and humans alike. `CLAUDE.md` and `GEMINI.md`
only point here. If an instruction elsewhere conflicts with the hard rules below, the hard rules win.

OpenProcyon is an unofficial, open-source engine for *Treasure Planet: Battle at Procyon* (2002). It contains **no game content**:
each player converts their own copy of the original game into local assets. The goal is a faithful recreation of the original's
behaviour (the "classic" presentation). Current priority (milestone v0.2): skirmish working correctly end to end (setup flow, fleets, crew and arms, computer opponents, bases),
then the campaign (v0.3).
All 13 missions load (`--mission N`); missions 2-13 are first-pass and largely untested (`docs/re/campaign.md`).

## Hard rules (legal and project safety)

These protect the project's ability to be shared. `tests/test_repo_hygiene.py` enforces the mechanical ones; it must pass.

1. **No game content in Git.** Never commit original game files or anything converted from them: models, textures, sounds, music,
   video, maps, missions, ship/weapon data, strings, saves. `assets/` and `local/` are git-ignored; keep it that way. Tests that
   need game data read the imported assets and skip when they are missing (`tests/imported.py`); never add fixtures copied from the game.
2. **Never run, patch or modify a game installation.** Read it only, and only through the importer and exporters in `tools/`.
3. **No reverse-engineering artefacts.** Do not add decompiled or disassembled code (not even paraphrased line by line),
   executable addresses, symbol dumps, binary hashes, or instructions/scripts for analysing or unpacking the game executable.
   Do not describe or link ways to bypass DRM.
4. **Describe behaviour in your own words.** Knowledge about how the original behaves is written as specifications (rules,
   numbers, formulas) in `docs/re/*.md`, citing observable sources: the retail data files, the manual, reference footage, play.
5. **Mark guesses.** Anything not verified against the original is marked `PROVISIONAL` in code and docs. Prefer behaviour backed by
   the original's data or observation over eyeballed tuning.
6. **No trademarks or official art as project identity.** No Disney logos, key art or official branding in the repo, the UI chrome
   we ship, or the project's name/icons. Saying the engine is "for *Treasure Planet: Battle at Procyon*" is fine.
7. **Only compatible third-party files.** Anything added to `content/` or the code must be ours or under a licence that allows
   redistribution; record the licence next to the file.
8. **No secrets or personal paths** in tracked files (install paths go in the untracked `.env`).

Ask a maintainer before: adding dependencies, changing data formats or asset paths (mods and saves depend on them), adding
files to `content/`, or anything that publishes (pushing tags, releases).

## How it works

- `tools/` Python parsers/exporters for the retail formats (WOT, MDB, MSB, EFT, HDT, FFT, TWT, TST) and the importer
  `python3 -m tools.import_assets --game <install> [--out <dir>]` (`tools/build_assets.sh` wraps it), which writes JSON/GLB/PNG/OGG assets. Default output: `assets/` in the
  checkout; the game also finds `$XDG_DATA_HOME/openprocyon/assets` (Windows `%APPDATA%\openprocyon\assets`) or `PROCYON_ASSETS`.
  `tools/compare_assets.py A B` compares two imports by meaning (JSON values, image pixels).
- `content/` files that ship with the code: fonts (URW base35), hand-written data (`data/hud/bar.json`), the crew voice event
  table. The game overlays `content/` on the imported assets like a mod (`assets_dir()` in `game/src/lib.rs`).
- `game/` Rust + Bevy 0.20. `lib.rs` is the `openprocyon` library: wiring and `run(view)`. `main.rs` runs it with the classic view.
  Shared modules own state, input, rules and simulation: `ship.rs`/`scenario.rs`/`view.rs` (ships + AI, battle lifecycle,
  camera/input), `data.rs` (JSON loading), `combat/`, `fx.rs`, `docking.rs`, `options_menu.rs`, `boarding.rs`, `sprites.rs`,
  `panels.rs` (HUD contract/input), `menu.rs` (front-end model), `campaign.rs` + `rules/` (data-driven mission rule
  interpreter), `briefing.rs`, `audio.rs`, ... Module ownership and implementation order: `docs/development.md`.
- `game/src/classic/` the retail presentation. Views only draw; state, input and rules stay in shared modules. Shared code never
  imports a view or builds UI widgets (`tests/test_layers.py`; the compiler enforces the first too, since views live in the binary). Other presentations
  belong in their own crates that depend on the `openprocyon` library.
- `mods/` mod folders (`--mod <dir>`, `docs/modding.md`). `docs/data-format.md` describes the asset layout.
- Game axes: x = starboard, y = forward, z = up; GLB uses (x, z, -y) with winding (a, c, b).

## Setup and running

- Rust >= 1.97.1; Python 3 with `requirements-dev.txt` (Pillow, NumPy, and ruff for the checks) in a `.venv`. Linux needs the usual Bevy packages
  (ALSA, udev, Wayland/X11, Vulkan).
- Import once: `make assets ARGS='--game "/path/to/Treasure Planet Battle at Procyon"'` (or put `PROCYON_GAME_ROOT` in `.env`).
  Re-import after changing an exporter. Installs with community mods import too; vanilla must always work.
- Run: `make run [ARGS='--mission 1']`. Interactive runs open the main menu; `--map <id>` / `--mission N` start a battle.
- Screenshots: `tools/shot.sh local/x.png --shot-frame N [--script fxtest|victory|defeat|restart|course|target|damage|dock|...]
  [--map red_rover] [--menu skirmish] [--dist D --yaw Y --pitch P]`. Without a display it uses Xvfb + software Vulkan.
  On some Wayland sessions the window renders black; prefix `WAYLAND_DISPLAY=` to force X11. Runs take ~20 s.

## Workflow

- `tools/check.sh` (`make check`) is the single validation gate: ruff format/lint (`ruff.toml`), rustfmt, clippy `-D warnings`, Rust tests
  (asset-free tests always run; ignored asset tests also run when assets are imported), Python tests including the hygiene rules. `make fmt` formats both languages. Run it before every commit; do not add
  parallel wrappers.
- Non-trivial changes need a regression test; ECS/UI changes also need a runtime smoke test (a `--script` or screenshot run).
- Add behaviour to the module that owns it (`docs/development.md`), not a parallel implementation. Keep asset paths and JSON
  layouts stable; mods reference them.
- Exporter changes: re-import into a scratch folder and compare with `tools/compare_assets.py` before trusting the result.
- Known PROVISIONAL areas: ship linear motion and engine-order mapping, gun arcs, damage-smoke emitter geometry, wake parameters,
  sail alpha, HUD part order, skybox UV remap, sound falloff/levels and the event to sound-pool mapping.
- `local/` is git-ignored scratch for screenshots, logs and notes.
