---
name: procyon-loop
description: The OpenProcyon dev loop. Use when asked to check, test, smoke, format or compare a change ("check this", "test X", "smoke for Y", "compare against main"), or before committing or pushing.
---

# OpenProcyon dev loop

Run from the checkout (or worktree) root. Details: `AGENTS.md`, `docs/development.md`.

| Ask | Command | What it does |
|---|---|---|
| "check this" (while iterating) | `tools/check.sh --quick` | Seconds: ruff + hygiene always; rustfmt + clippy if Rust changed, Python tests if Python changed. Trust only the last `GATE OK` line; logs in `local/check/`. |
| "check before push" | `tools/check.sh` | Default tier: ruff, hygiene, Python tests, rustfmt, clippy, Rust tests (asset tests when assets are imported). |
| before push or PR | `tools/check.sh --full` | Default + `fast` build, startup smoke, battle check. Once per push, not per edit. |
| "test X" (one Rust test) | `cargo test --manifest-path game/Cargo.toml --lib <module::path>` | Runs the tests under that path only. Add `-- --ignored` for asset tests (needs an import). |
| "test X" (Python) | `python3 -m unittest tests.test_<name>` or `make test-python` | One test module / all fast parser and exporter tests. |
| "smoke for Y" | `make smoke WHAT=changed` | Runs only the `--script` smokes that your changed files map to (`tools/smokes.sh --changed`). `WHAT=<name regex>` picks by name; `tools/smokes.sh --list` lists them. Needs assets; `PROCYON_RUN=window|software|gpu-hidden` picks how the game runs (default: window with a display, else Xvfb); logs in `local/smokes/`. |
| all smokes | `make smokes ARGS="--jobs 4"` | Every documented smoke, in parallel. |
| a picture | `tools/shot.sh local/x.png --shot-frame 300 [--script S] [--map red_rover]` | One 1280x720 screenshot, fails on a missing or black one. Prints a `run:` line (mode, adapter, fps); compare timings only between runs of one mode. |
| whole-battle sanity | `make battles [ARGS=ambush]` | `tools.battle_check`: short fixed-fleet skirmishes, fails when one stops behaving like a battle (also NaN or far-off positions, runaway entity counts). `ARGS='--twice'` plays each twice, fixed-step and seeded, and fails when the reports differ. No `--batch` mode yet. |
| "compare against main" (behaviour) | `PROCYON_TRACE=local/new.csv` on this checkout, again on main's binary, then `python3 -m tools.tracediff main.csv new.csv` | Per-ship position/velocity/heading/hit-point log and shells (`docs/development.md`). A difference becomes a board row, not more runs. |
| "did my refactor change anything?" | simulation refactor: same seed, `--fixed-step`, `PROCYON_TRACE` on both builds, then `python3 -m tools.tracediff A.csv B.csv`; drawing change: `python3 -m tools.visdiff` | tracediff prints `same` or the first differing row; visdiff renders fixed-step scenes from the merge base and this checkout and prints `same` or `CHANGED` per scene (diffs in `local/visdiff/`). |
| "compare against main" (assets) | `python3 -m tools.compare_assets <dirA> <dirB>` | Compares two imports by meaning (JSON values, image pixels). Re-import into a scratch folder after an exporter change. |
| format | `make fmt` | `cargo fmt`, `ruff format`, `ruff check --fix` over everything. A hook already formats the one file you edit. |

Rules of thumb
- Never `--release` except for a release; never `cargo clean` without `--workspace`.
- Run long Python (`compare_assets`, `battle_check`, `autoplay`, the importer) under `systemd-run --user --scope -q -p MemoryMax=4G -p MemorySwapMax=0 <cmd>`.
- Game runs: scratch `XDG_DATA_HOME=$(mktemp -d)` so `bindings.json` cannot remap smoke keys.
- Work in the checkout you were started in. Do not create a worktree (`EnterWorktree`, `isolation: worktree`, `tools/worktree.sh new`)
  unless the user asks for one: each costs gigabytes of build. Remove one with `tools/worktree.sh rm <path>`.

Hooks
- Shared (`.claude/settings.json`): the command allowlist, and a hook that formats the one file you just edited.
- Opt-in (`.claude/settings.local.example.json`): run `tools/check.sh --quick` at the end of every turn that changed files, and create
  or remove worktrees through `tools/worktree.sh`. To use them, copy the file to `.claude/settings.local.json` (git-ignored), or
  merge its `hooks` into the one you have.
