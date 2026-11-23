# A fast, enjoyable feedback loop for OpenProcyon

Review date: 2026-11-23. Baseline: branch `ccr-da74beda-l25trq` (`main` 0.2.0 plus the code review).
Scope: `tools/check.sh`, the Makefile, CI, Rust profiles and tests, the Python tests, smoke and screenshot tooling, worktrees, disk use, and the agent instructions.

> **The strategy in one sentence:** compile the way you edit (incremental dev and `fast` profiles instead of `--release`), check in tiers that match what changed (seconds per edit, about 30 s per commit, the full gate once per push or PR), and give every worktree and agent a warm, bounded build plus short, machine-readable results.

This document has three parts:
- **Part 1** gives the measurements: where the time goes today and what the proposed setup gives.
- **Part 2** is the strategy: tiers, profiles, worktrees and disk, smokes and regression checks, agent ergonomics, and a prioritised plan.
- **Part 3** is the appendix: the researched checklist and four detailed area reviews (R1 gate and CI, R2 Rust build and tests, R3 smokes and screenshots, R4 worktrees and agents).

The method was the same as for the code review. One research pass produced a checklist (Rust 1.100 and Cargo release notes, Bevy 0.20.1 fast-compile setup and CI, nextest, sccache, rust-cache, Claude Code hooks). Four Sonnet reviewers then read their area against that checklist. **I measured the build myself** in this container, and I checked the reviewers' key claims against the code.

---

## Part 1: Measurements

Machine: this cloud container (4 cores, 15 GB RAM, ext4, so no reflink). Toolchain: **Rust 1.100.0** (the container updated past the project's 1.97.1 minimum; numbers on 1.97 should be close, but see the caveats). Bevy 0.20.0 from `Cargo.lock`. There were no imported assets, so the 263 asset tests, the smokes and the battle check could not run here.
- **How measured:** each warm number is a rebuild after `touch`ing one module. A leaf module (`flag.rs`) and a core module (`ship.rs`) gave the same times, which is expected: the crate is a single compilation unit for incremental purposes.
- **Proposed setup without changing the repo:** the dev profile used `CARGO_PROFILE_DEV_DEBUG=line-tables-only`. The `fast` profile was defined on the command line: `--config profile.fast.inherits="release"`, plus `incremental=true`, `codegen-units=256` and `debug="line-tables-only"`.

### What one edit costs

| Step | Today (`--release`) | Proposed | Speed-up |
|---|---|---|---|
| Lint: `cargo clippy --all-targets -D warnings` | 19 s | **3.8 s** (dev) | 5× |
| Test binaries: `cargo test --no-run` | **2 min 49 s** | **12-14 s** (dev) | 12-14× |
| Game binary for smokes and shots | **2 min 7 s** | **8-9 s** (`fast`) | 14× |
| Run asset-free tests (≈630 run, 263 ignored) | ~1-2 s | ~2 s | same |
| Python tests + ruff + hygiene | ~4.5 s | ~4.5 s | same |
| **Full gate's compile per edit** (clippy + test build + game build) | **≈ 5 min 15 s** | **≈ 25 s** | **≈ 12×** |
| `check.sh --fast` compile per edit | ≈ 3 min 8 s | ≈ 17 s | ≈ 11× |

### Cold builds (new worktree, `cargo clean`, toolchain update)

| Build | Cold |
|---|---|
| `clippy --release` | 1 min 24 s |
| `test --release --no-run` | 9 min 11 s |
| `build --release` (after the test build) | +19 s |
| `build` (dev, full debuginfo) | 13 min 56 s |
| `clippy` (dev, line-tables-only) | 2 min 25 s |
| `test --no-run` (dev, line-tables-only) | 12 min 4 s |
| `build --profile fast` | 10 min 34 s |

A cold start costs 10-15 minutes in every profile on 4 cores. The cure is never building cold, not a faster profile: warm a new worktree from an existing target directory (section 2.3).

### Disk

| Tree | Size |
|---|---|
| `target/release` (clippy + tests + game) | 2.6 GB |
| `target/debug` with Cargo's default dev debuginfo | ≈ 12 GB. The dev game binary alone is **2.5 GB**, against 118 MB for release. |
| `target/debug` with `line-tables-only` (clippy + tests) | ≈ 6-9 GB, of which `incremental/` is 5.8 GB |
| `target/fast` (game binary) | 4.8 GB, of which `incremental/` is 1.4 GB |
| **This container after both rounds** | **23 GB in one worktree** |

That is the "disk explodes with worktrees" problem in one line: several profiles × debuginfo × incremental × N worktrees. Section 2.3 brings a worktree to one or two bounded trees.

### Where the time goes today
- **Every step builds in release.** `tools/check.sh:19,21,30` runs clippy, test and build with `--release`. Release has incremental off, opt-level 3 and 16 codegen units, so **any one-line edit recompiles all 67k lines at full optimisation, twice** (the lib test binary and the game binary), plus a separate clippy pass. The tuned `[profile.dev]` (`game/Cargo.toml:22-26`) is never used by the gate, `shot.sh`, `smokes.sh`, `battle_check` or `make run` (R1-01, R2-01, R4-01).
- **`--fast` saves little.** It skips only the startup smoke and battle check (about 1-2 minutes). It still pays the 3-minute compile (R1-02).
- **Smokes and battles cost per process.** `make smokes` starts 24 game processes one after another (about 8 minutes), each paying for `cargo run`, Xvfb, asset loading and shader pipeline compiles on software Vulkan. The battle check starts five more (R3-01 to R3-03).
- **Python is not the bottleneck.** 196 tests run in 4.4 s, and ruff takes under 0.1 s (R1, measured).

---

## Part 2: Strategy

### 2.1 Profiles: compile the way you edit (largest gain, smallest change)

| Profile | Used by | Settings |
|---|---|---|
| `dev` (exists) | clippy, unit tests, `make run` while iterating | add `debug = "line-tables-only"`; keep `opt-level = 1` and dependencies at 3 |
| `fast` (new) | anything that runs the game for checking: startup smoke, `shot.sh`, `smokes.sh`, `battle_check`, `autoplay` | `inherits = "release"`, `incremental = true`, `codegen-units = 256`, `debug = "line-tables-only"` |
| `release` (unchanged) | player downloads, the release workflow, and perhaps a final pre-release battle check | unchanged |

```toml
# game/Cargo.toml (maintainer approval: profile change)
[profile.dev]
opt-level = 1
debug = "line-tables-only"   # backtraces keep file:line; full debuginfo made the dev binary 2.5 GB

[profile.dev.package."*"]
opt-level = 3
debug = false                # dependencies: no debuginfo (unmeasured; expected to shrink target/debug further)

[profile.fast]               # smokes, screenshots, battle check: near-release speed, incremental rebuilds (measured 8-9 s)
inherits = "release"
incremental = true
codegen-units = 256
debug = "line-tables-only"

[lib]
doctest = false              # no doc examples exist; skip the rustdoc pass on every `cargo test`
```

- **Tool updates.** The tools then use `--profile fast` and `target/fast/openprocyon`. Point `battle_check.py` (already reads `CARGO_TARGET_DIR`), `autoplay.py` (hard-coded `game/target/release`, `autoplay.py:20` ✔ verified) and `shot.sh` and `smokes.sh` at it. Have them run the built binary directly instead of `cargo run` (R3-03).
- **Lints.** Keep clippy's `-D warnings` for now. Later move to `[lints]` in `Cargo.toml` plus `CARGO_BUILD_WARNINGS=deny` (stable since 1.97), which does not invalidate the build cache (R2-07).
- **Optional.** A `dev` Cargo feature, `dev = ["bevy/dynamic_linking"]`, which is never on by default, never in CI and never in release. Bevy calls it the biggest compile-time saver, and it makes every test binary smaller (R2-03). I did not measure it; try it after the profile change.
- **Caveats:**
  - **(a) Debug checks.** Dev-profile tests turn on `debug_assertions` and overflow checks, which the release-only gate never ran. Expect a few **new test failures** at first; they are real bugs that were hidden, so this counts in the change's favour.
  - **(b) Long simulations.** Long asset tests (for example `combat/tests.rs:288` with 1200 updates per gun mount, or `fx/sim/tests.rs:121` with 9000 ticks; R2-08) run slower at opt-level 1. If a few become too slow, mark them, or run the asset tests under `fast` (`cargo test --profile fast -- --ignored`).
  - **(c) `fast` runtime speed.** With 256 codegen units the game runs a few percent slower than release. That's fine for smokes, but measure it once on a battle before trusting the stutter threshold in `battle_check`.

### 2.2 Tiers: check what changed, at the moment it matters
These are flags inside the one gate (`tools/check.sh`), which keeps the AGENTS.md rule "the single validation gate … no parallel wrappers". AGENTS.md needs a one-line edit to say the gate has tiers (R1-02, R4-06).

| Tier | When | What | Measured or estimated cost |
|---|---|---|---|
| **0: on edit** (hook or habit) | every save | `rustfmt` / `ruff format` on the changed file; nothing else | < 1 s |
| **1: `check.sh --quick`** | while iterating, many times | ruff, hygiene, `cargo clippy` (dev); only parts for what changed (docs only → hygiene; Python only → ruff + Python tests; Rust → clippy) | **≈ 5 s** warm |
| **2: `check.sh`** (default, before a commit) | each commit | tier 1 + `cargo test` (dev) + asset tests if imported + Python tests; prints an `ASSETS: found/missing` line | **≈ 30 s** warm without asset tests (+ their run time) |
| **3: `check.sh --full`** | before a push or PR, and in a merge queue | tier 2 + `fast` build + startup smoke + battle check + the smokes mapped to the changed modules (2.4) | ≈ 1-3 min with parallel smokes |
| **4: CI** | PR and main | lint job (seconds) → clippy and tests (dev, asset-free) → optional Windows check | see 2.6 |

Rules that make the tiers trustworthy:
- **Hygiene always runs.** It is a 0.6 s test, so it belongs in tier 1 for every change. My own docs commit earlier in this session broke the gate (three lines of the code review matched the hygiene regexes), and the old gate would only have caught that after a 5-minute Rust compile. Fixed in `586f4af`.
- **No green partial run.** A partial run must never look green. Without assets the gate today exits 0 with only a `NOTE` on stderr (R1-07, R4-02). Print a final line such as `GATE: PARTIAL (no assets: 263 Rust asset tests, smoke, battle check skipped)` and make tier 3 fail without assets unless `--allow-partial` is given.
- **Short output.** Use a `step()` wrapper: one line per step with its time, full logs to `local/check/<step>.log`, and on failure the first 20 relevant lines plus the exact rerun command (R1-03, R3-08).
- **Cheap checks first.** Run ruff, hygiene and Python tests before any Rust compile, so the cheap failures show in seconds (R1-03).
- **One-module tests.** Document `cargo test --lib <module::path>` as the single-test command (R2-04).

### 2.3 Worktrees and disk: one warm, bounded build per agent

**The problems:**
- **Cold builds.** A fresh worktree builds cold: 10-15 minutes before the first useful feedback.
- **Disk.** Each worktree grows to 15-23 GB with today's profiles.
- **Silent skips.** In a linked worktree only `shot.sh` falls back to the main checkout's `assets/`. `tests/imported.py`, `check.sh`, `battle_check` and `autoplay` do not, so the gate silently skips the asset tests and still passes, when the import lives in the checkout. ✔ verified: `tests/imported.py:_find` checks `PROCYON_ASSETS`, then `<this checkout>/assets`, then the per-user folder, but never the main worktree (R4-02).

**Recommended setup (maintainer approval: a new `tools/worktree.sh`, a lifecycle helper rather than a second gate):**
1. **Assets: one copy, shared.** Either import once into the per-user folder, which every tool already finds, or have all tools share `shot.sh`'s main-checkout fallback. The best place for it is one Python helper that `tests/imported.py`, `battle_check` and `autoplay` import, while `check.sh` and `shot.sh` set `PROCYON_ASSETS`. Never copy `assets/` into a worktree.
2. **Warm start by copying, not by sharing.** `tools/worktree.sh new <branch>` does three things:
   - runs `git worktree add`;
   - runs `cp -a --reflink=auto <main>/game/target/{debug,fast} <wt>/game/target/`, then deletes the copied `incremental/` directories;
   - links or creates `.venv`.

   On btrfs or XFS the copy is near-instant and nearly free on disk. On ext4 (this container) or NTFS it is a real copy of about 3-6 GB, still far faster than 10-15 minutes of compiling. `-a` keeps modification times, so dependencies stay fresh and only the workspace crate rebuilds (R2-06, R4-03).
3. **Don't share one `CARGO_TARGET_DIR` between active agents.** It has one lock per profile, so agents queue behind each other, and their binaries overwrite each other. `build.build-dir` sharing for dependencies becomes worth testing once the minimum Rust is ≥ 1.100 (build-dir layout v2) (R4-04).
4. **No sccache by default.** It cannot cache the incremental workspace crate or linked binaries, so it does not help the edit loop. It is only worth it for CI dependency churn (R4-05).
5. **Cleanup:**
   - `tools/worktree.sh rm <wt>` deletes `<wt>/game/target`, then runs `git worktree remove`.
   - `tools/worktree.sh gc` deletes `incremental/` directories older than N days, removes target dirs of worktrees that no longer exist, prunes `local/smokes` and `local/check` PNGs and logs older than 14 days, and runs `cargo clean --workspace` on any target over a size cap (for example 8 GB).
   - Git has no worktree-remove hook, so the script is the hook. Claude Code's `WorktreeCreate`/`WorktreeRemove` hooks can call it for worktrees that Claude Code itself creates.
6. **Isolate parallel runs.** Every scripted run gets its own `XDG_DATA_HOME` (logs, settings, saves), as `smokes.sh` and `autoplay.py` already do. Extend that to `shot.sh` and `battle_check` (R4-08).

**Expected per worktree:**
- **Disk:** about 4-7 GB (dev with line-tables-only plus `fast`, incremental pruned), against 15-23 GB today.
- **First feedback:** under a minute after `worktree.sh new` on a reflink filesystem, a few minutes on ext4, against 10-15 minutes today.

### 2.4 Smokes, screenshots and regression checks: fewer processes, real verdicts

**Faster:**
1. **Built binary (S).** Run the built binary, not `cargo run`, and build once with `--profile fast` (R3-03).
2. **Parallel (S).** `smokes.sh --jobs N` and `battle_check --jobs N`, one Xvfb display and one scratch data folder per job, capped at cores/2 because software Vulkan is CPU-bound (R3-02). The stutter check in `battle_check` is load-sensitive, so judge it only with `--jobs 1`.
3. **One process (M).** A `--batch <file>` mode plays many scenarios in one process, reusing the existing leave, enter and restart paths and the Loading gate, so startup and shader compiles are paid once instead of 24 times (R3-01). Things to fix first: the `exit(0)` in the screenshot driver, the per-system `Local`s and process-wide statics that a restart doesn't reset (the code review's theme 1 again), and the panicking `assert!`s, which should become reported failures.
4. **Replace the 10 s sleep.** The startup smoke runs a fixed `timeout 10`. Replace it with a self-quitting `--menu main --shot-frame N` run (R3-09).

**More trustworthy:**
1. **Fixed step (M).** Scripted runs follow real frame time today. `TimeUpdateStrategy::ManualDuration` is used only in unit tests (✔ verified: `flag.rs:375`, `dragons.rs:330,354`), so scripted runs, battle reports and screenshots are not reproducible. Add a fixed-step, seeded mode for `--script` / `--lobby` runs: fixed frame time, seeded RNG (code-review theme 1), and frames counted after Loading. This one change makes tight battle thresholds and image comparison possible (R3-04, R3-11).
2. **Verdicts, not just "not black" (S).** About 8 scripts assert something; the rest only check that the picture isn't black. Add a `kind` column (asserts / visual) to `smokes.sh`, and make an asserting script fail if its final PASS line is missing (R3-07).
3. **Image comparison without committing game content (M).** `tools/visdiff.py`:
   - builds the merge-base binary and the branch binary (both `fast`, each in its own worktree target, so the merge-base build stays warm across runs);
   - renders the same fixed-step scripts with both into `local/visdiff/`;
   - compares them with NumPy and Pillow, which are already dependencies;
   - prints one line per scene: `same` or `CHANGED 3.1% px, bbox …, diff local/visdiff/x_diff.png`.

   Nothing is stored in Git or uploaded, which keeps hard rule 1. An SSIM library would be a new dependency, so start with a per-pixel delta plus a changed-area threshold (R3-05).
4. **Trace diff (S).** A no-dependency `tools/tracediff.py` compares two `PROCYON_TRACE` CSVs (main against branch) for refactors that must not change behaviour. It is the cheapest strong regression test for simulation refactors like the ones in the code review (R3-10).
5. **More battle invariants (M).** Add: the battle ends or ships sink within the time limit, no NaN or out-of-bounds positions, a bounded entity count, and the same seed giving the same report. Add more maps and mixed fleets. `FIRE_BY` and `SPEND` stay PROVISIONAL until measured (R3-10).
6. **Headless simulation (L, later).** Drop rendering for `battle_check` and `autoplay`. `docs/development.md` already lists the `Window` coupling as the blocker (R3-06).

**Change → smoke map (summary; full table in R3).** A table maps the area you changed to the checks to run:
- `combat/`, `collision/`, `ship*` → battle check + `damage`, `target`, `course`
- `rules/`, `campaign` → `briefing-skip`, `lifeboat-m1`, `storm-m3`, `hunt-m3`
- `docking`, `boarding` → `dock` + battle check
- `classic/`, `panels`, `menu/` → `pause`, `fleet`, `mods` + a visdiff of the menu and HUD scenes
- `fx/`, `render/` → `fxtest` + visdiff
- `tools/` exporters → Python tests + `compare_assets` on a scratch re-import

`make smoke WHAT=changed` reads `git diff --name-only` against this map.

### 2.5 Agent ergonomics: make the fast path the obvious path
- **AGENTS.md wording (R4-06).** Today "run it before every commit" plus a slow `--fast` pushes agents to run the full gate repeatedly. Proposed wording:
  > Iterate with `tools/check.sh --quick` (about 5 s), commit after `tools/check.sh` (about 30 s), run `tools/check.sh --full` once before you push or open a PR. Run one test with `cargo test --lib <module::path>`, a smoke with `make smoke WHAT=changed`. Never build `--release` except for a release; never `cargo clean` without `--workspace`.
- **A `.claude/` project config (R4-07; new directory, maintainer approval).** Full sketch in R4:
  - **Skill:** a `procyon-loop` skill that maps "check this", "test X", "smoke for Y" and "compare against main" to the commands above, with their short output.
  - **Hooks:**
    - a `PostToolUse` hook on `Edit|Write` that runs `rustfmt` or `ruff format` on that one file;
    - a `Stop` hook that runs only `check.sh --quick` and reports in under 20 lines;
    - `WorktreeCreate`/`WorktreeRemove` hooks that call `tools/worktree.sh new` / `rm`.
  - **Permissions:** an allowlist for the read-only and check commands, so agents don't stop for approval prompts.
  - **What to avoid:** never run the full suite from a per-edit hook.
- **Machine-readable results.** Every tool ends with one summary line: `GATE OK 31s` / `GATE FAIL clippy (local/check/clippy.log)`, `smokes: 22 ok, 2 FAIL: dock, fleet`, `visdiff: 1 CHANGED: hud_bar`. Agents read the line, open the log only when something fails, and rerun only what failed.
- **CI cannot mask local incremental builds.** Cargo ≥ 1.99 turns incremental off when `CI` is set. If an agent sandbox sets `CI=true`, local builds silently lose incremental. The gate should `unset CI` or force `CARGO_INCREMENTAL=1` for local tiers. This needs a check, because the project's minimum toolchain is 1.97.

### 2.6 CI
- **Use the profile everyone else uses (R1-05).** CI tests release with opt-level 0, which no developer runs. Moving everything to the dev profile makes CI build exactly what tier 2 builds. Then drop the env overrides in `check.yml:13-15`.
- **Split jobs (R1-06).** First a `lint` job (ruff, hygiene, fmt: seconds, no compile), then `clippy + test` (dev, asset-free). Use `paths:` filters so a docs-only or Python-only PR skips Rust. Run a Windows `check + test` job once per PR or in a merge queue. Today Windows is first built only at release time.
- **Cache hygiene (R1-05).** Use `Swatinem/rust-cache` with `save-if: github.ref == 'refs/heads/main'`, so every PR stops writing a large cache. Pin the toolchain to the `rust-version` instead of the floating `@stable`, or test both.
- **Nextest (optional, R2-05).** cargo-nextest is a per-developer and CI tool, not a crate dependency. It gives 1.4-3.4× faster test execution, process-per-test isolation (useful with the two mutable `Mutex` statics, `boarding/odds.rs:150` and `render/rules.rs:130`), retries, and `rerun` of failures only. `check.sh` falls back to `cargo test` when it isn't installed. Low priority: on these numbers, running tests takes seconds and compiling is the cost.

### 2.7 Prioritised plan

| # | Change | Gain | Effort | Approval |
|---|---|---|---|---|
| P0-1 | Gate, Makefile and tools use `dev` (clippy, test) and a new `fast` profile (anything that runs the game) instead of `--release`; `line-tables-only`; `doctest = false` | compile per edit **5 min → 25 s**; disk roughly halved | S | `Cargo.toml` profiles |
| P0-2 | `check.sh` tiers (`--quick` / default / `--full`); cheap steps first; one-line step summaries with logs in `local/check/`; `GATE: PARTIAL` line | about 5 s inner loop; no silent partial passes | S | AGENTS.md one-line edit |
| P0-3 | One asset lookup with the main-worktree fallback for tests, `check.sh`, `battle_check` and `autoplay`; `autoplay` honours the target dir | worktrees really test | S | – |
| P0-4 | AGENTS.md wording for tiers, the single-test command and "no `--release`" | agents stop over-running the gate | S | AGENTS.md |
| P1-1 | `tools/worktree.sh new/rm/gc` (reflink warm copy, cleanup, size cap) | first feedback in a new worktree **10-15 min → about 1-3 min**; disk **15-23 GB → about 4-7 GB** per worktree | S-M | new tool |
| P1-2 | Smokes and battles run the prebuilt `fast` binary with `--jobs N`; replace the startup sleep | smokes **about 8 min → about 2-3 min** on 4 cores | S | – |
| P1-3 | `make smoke WHAT=changed` (change → smoke map); asserting scripts must reach PASS | the right smokes, real verdicts | S | – |
| P1-4 | CI: lint job, `paths:` filters, `save-if` on main, toolchain pin, a Windows job per PR or queue | faster, cheaper, broader CI | S-M | workflow change |
| P1-5 | `.claude/` skill, hooks and allowlist | fast path by default for Claude Code agents | S | new directory |
| P2-1 | Fixed-step, seeded mode for scripted runs (needs the code review's theme 1, determinism) | reproducible smokes and battles | M | – |
| P2-2 | `tools/visdiff.py` (merge-base vs branch, NumPy/Pillow) and `tools/tracediff.py` | visual and simulation regression checks without goldens in Git | M | – |
| P2-3 | `--batch` mode: many scenarios in one process | smokes about 1 min; needs the restart-safety fixes | M | – |
| P3 | `dynamic_linking` dev feature, cargo-nextest, a headless simulation mode, `build.build-dir` sharing after a move to Rust ≥ 1.100 | further gains, each to be measured | M-L | some |

### Caveats
- **Toolchain.** The measurements used Rust 1.100, not the pinned 1.97.1. The container's `rustup update` moved it. The relative gains come from profile settings that have been stable for years. The absolute numbers come from a 4-core container; a modern 8-16-core desktop will be roughly 2-3× faster everywhere.
- **Not run here.** No assets exist here, so I could not measure the asset tests, smokes, battle check, or the runtime speed of the `fast` profile.
- **Mixed dev measurements.** The dev-profile disk number mixes the two measurement rounds (cleaned up by file time where possible). Trust the direction more than the second digit.
- **Approval.** Every P0 item touches `Cargo.toml`, `check.sh` or AGENTS.md, so per AGENTS.md they need a maintainer's go-ahead. None changes game behaviour or data formats.

---

## Part 3: Appendix
- L0. The checklist given to every reviewer
- R1. Validation gate and CI
- R2. Rust build configuration and test suite
- R3. Smoke, screenshot and regression tooling
- R4. Worktrees, disk and the AI-agent workflow

The appendices are the reviewers' own reports. R2 and R4 worked from estimates, because cargo was reserved for my measurements. Where their estimates differ from Part 1, Part 1's measured numbers win.


---

## L0. Review checklist
### Fast dev/test loop checklist (researched 2026-11; Rust stable 1.100, Bevy 0.20.1; project pins Rust >= 1.97.1, Bevy 0.20)

#### 1. Compile speed
1.1 Linker: LLD is rustc's default on x86_64 Linux since 1.90 (link 7x faster incremental vs bfd). No config needed. mold/wild faster still but opt-in per developer (~/.cargo/config.toml); don't commit a linker CI/Windows lacks. Windows: `linker = "rust-lld.exe"` (Bevy config_fast_builds.toml).
1.2 `bevy/dynamic_linking` behind a dev-only feature: "most impactful compile-time decrease" (Bevy). Never ship it. Windows caveats (dep opt-level needed, PATH for doc tests). Shrinks every test binary too.
1.3 Profiles: Bevy official `[profile.dev] opt-level=1` + `[profile.dev.package."*"] opt-level=3`. Add `debug="line-tables-only"` (or 0) to dev: 20-40% faster dev builds, up to 30-40% faster incremental, ~2x smaller target. Keep incremental=true and default codegen-units for own crate.
1.4 NEVER run tests/clippy in `--release`: release = incremental off, codegen-units 16, opt-level 3 → every edit recompiles the crate fully optimised. `clippy --release` builds a third, unshared artifact set. If fast code is needed for sim tests/smokes use ONE custom profile: `[profile.fast] inherits="release" incremental=true codegen-units=256 debug="line-tables-only"`. Each profile is a separate target tree (disk).
1.5 check vs clippy vs test: clippy shares deps with `cargo check`, not with test/build (metadata vs codegen). Gate running clippy then test compiles the crate twice — unavoidable; inner loop should run only `cargo check` (or clippy) and only build/test when executing.
1.6 Deny warnings without cache busting: `RUSTFLAGS=-Dwarnings` invalidates all artifacts; Rust 1.97 stabilised `build.warnings` / `CARGO_BUILD_WARNINGS=deny` which doesn't. Prefer `[lints]` + CARGO_BUILD_WARNINGS over `-- -D warnings` (verify fingerprint effect).
1.7 Nightly-only (opt-in per dev, conflicts with stable contract): cranelift (~30% faster codegen for own crate), -Zthreads (up to ~40% clean check, 3-5% incremental), -Zshare-generics, -Zcache-proc-macros.

#### 2. Worktrees
2.1 Shared CARGO_TARGET_DIR across concurrently active worktrees: one lock per target/<profile> ("Blocking waiting for file lock") serialises agents; final binaries overwrite each other; pre-1.100 layout had hash bugs exacerbated by shared build dirs (fixed by build-dir layout v2 in 1.100). Don't share target dirs between active agents on < 1.100.
2.2 `build.build-dir` (stable 1.91) splits intermediates from final artifacts; templates {workspace-root}, {cargo-cache-home}, {workspace-path-hash}. Per-worktree build-dir, or (>=1.100) a shared one for deps after measuring lock contention.
2.3 sccache: helps deps across worktrees/CI/fresh clones (SCCACHE_BASEDIRS for path-independence); never caches bin/dylib/proc-macro or incremental crates → no help in the edit-rebuild loop.
2.4 Warm a new worktree by `cp -a --reflink=auto main/game/target new/game/target` (near-free on btrfs/XFS, full copy on ext4); -a preserves mtimes so deps stay fresh; delete incremental/ in the copy. Don't symlink/hardlink active target dirs.

#### 3. Disk
3.1 Debuginfo + incremental ≈ 3x size (cargo's own: 3.15 GB dev incr+debug vs 1.0 GB non-incr no-debug). A Bevy game with several profiles (dev, release, clippy-release, test) is often 10+ GB per worktree.
3.2 Rules: line-tables-only, one dev + at most one fast profile, no --release checks, dynamic_linking.
3.3 Cleaning: ~/.cargo auto-GC stable since 1.88 (not target/). `cargo clean --workspace` (1.93) removes only own crates. Deleting target/*/incremental frees much. cargo-sweep for age-based. Target GC still unstable.
3.4 No git post-worktree-remove hook: wrap removal in a script that deletes target/ (and build-dir); Claude Code WorktreeCreate/WorktreeRemove hooks can warm/clean (WorktreeRemove only for hook-created worktrees).

#### 4. Tests
4.1 cargo-nextest: 1.4-3.4x faster execution, process per test (isolation, good for Bevy/global statics), retries, slow-test timeouts, filtersets, partitions, JUnit, `rerun` failed only, test-groups (max-threads for GPU smokes). No doctests (run `cargo test --doc` separately or `doctest=false`).
4.2 Few test binaries (each tests/*.rs links the whole lib + Bevy); unit tests in src/ are one binary.
4.3 Asset tests: keep #[ignore] / a nextest group; separate `--run-ignored only` step.
4.4 Selection: map changed paths → crate/filter; Python-only change skips Rust; agents run the single relevant test.
4.5 ECS tests without GPU: App + MinimalPlugins + app.update(); world.run_system_once; TimeUpdateStrategy::ManualDuration / FixedTimesteps(n) for deterministic stepping.

#### 5. Smoke / visual regression
5.1 Headless: Xvfb + mesa lavapipe (Bevy's own CI); WGPU_BACKEND=vulkan. Or render-to-texture headless app (examples/app/headless_renderer.rs).
5.2 Screenshot: `Screenshot::primary_window()` + `save_to_disk`. `bevy_dev_tools::ci_testing` (feature bevy_ci_testing, CI_TESTING_CONFIG RON) gives fixed_frame_time + frame-numbered Screenshot/NamedScreenshot/Custom/AppExit events — same idea as a custom --script/--shot-frame harness.
5.3 Determinism: fixed frame time, fixed seeds, wait N frames after load; software Vulkan stable per Mesa/CPU only → compare with tolerance.
5.4 Image diff: image-compare crate (SSIM/RMS/hybrid + diff map); gate e.g. SSIM < 0.99 or >X% pixels over delta; write diff images to local/.
5.5 Goldens without committing copyrighted content: (a) baseline rendered on the fly from merge-base binary vs branch binary, (b) goldens only in untracked local/ per machine, (c) commit goldens only for asset-free scenes. Never upload to CI artifacts/hosted diff services.
5.6 Many smokes in ONE process (winit allows one EventLoop per process → run scenarios sequentially in one App with state reset, or headless apps); startup/pipeline compile on lavapipe dominates short smokes. Parallel smokes: several Xvfb displays capped by CPU (lavapipe is CPU-bound).

#### 6. CI
6.1 Swatinem/rust-cache: caches deps only (drops workspace crates, incremental), keyed by job/rustc/lock/env (CARGO_*/RUST* env changes keys); 10 GB repo quota; shared-key across jobs; save only on main.
6.2 Bevy CI: CARGO_INCREMENTAL=0, debug 0. Cargo 1.99 disables incremental by default when CI env var is set — beware agents' sandboxes setting CI=true.
6.3 Split jobs: no-compile checks (fmt/ruff/hygiene, seconds) / clippy / tests (nextest, partitions) / optional smoke; paths filters (docs/Python-only skip Rust); cancel-in-progress; merge_group for a merge queue; Windows check+test once per PR or in queue.

#### 7. AI-agent loop
7.1 One cheap command per question: compile? `cargo check --message-format short` (CARGO_BUILD_WARNINGS=allow while fixing); my test? `cargo nextest run <filter>`; lint once before commit. Never --release, never clean without --workspace.
7.2 Short machine-readable failures: summaries (file:line: msg), full logs to local/…log with path; nextest `--failure-output final --success-output never --status-level fail`; smokes print "saved X, SSIM Y, diff at Z".
7.3 Tiered gates: fast pre-commit (fmt, clippy, unit tests of what changed, hygiene); full (asset tests, smokes) before push/PR or in CI/merge queue. Tell agents not to rerun the full gate after every edit; rerun only what failed.
7.4 One warm build per agent; identical fixed profile/feature set across agents so copies/caches stay valid.
7.5 Disk budget per worktree; delete target on worktree removal; prune incremental/ past a size cap.
7.6 Claude Code hooks: PostToolUse (Edit|Write) → rustfmt the file / cheap check; Stop → fast gate only; WorktreeCreate/Remove → warm/clean target; async hooks for slow work; never full suite per write. A project skill/Makefile target wrapping "check", "test X", "smoke Y" with summarised output is the main lever.

---

## Appendix: Validation gate and CI

#### Assessment

The gate is a single, understandable script with good instincts: one command, ruff pinned (`requirements-dev.txt:2`), an explicit ruff rule set (`ruff.toml:2`), asset-free tests always running, asset tests behind `#[ignore = "needs imported assets"]` plus a skip helper on the Python side (`tests/imported.py`), the gate itself tested with shims (`tests/test_check_gate.py`), a `--fast` mode, concurrency cancel and "branches are checked via PR" in CI. Python is not the bottleneck: measured here, `python3 -m unittest discover -s tests` = 196 tests in **~4.4 s** (27 skipped, no assets here), `ruff check .` 0.08 s, `ruff format --check .` 0.03 s, the slowest single Python module ~0.08 s. So every second of real waiting is in the Rust half.

The problem is structural, not Python-side:

1. **Everything runs in `--release`** (`check.sh:19,21,30,42`). `[profile.release]` has no `incremental`, so every edit to the 67k-line crate recompiles the whole crate with opt-level 3 and 16 codegen units; clippy `--release` is a separate artifact set from test `--release`. The `[profile.dev]` (opt-level 1, deps opt 3, `game/Cargo.toml:~24-28`) that was tuned for fast iteration is never used by the gate.
2. **`--fast` only drops the two smokes** (~2 min of the full run per the header, `check.sh:3`). It still runs clippy + the full 895-test suite with `--include-ignored` + all Python, so it is not a fast inner loop; it is a "slightly cheaper commit gate". There is no tier below it and no changed-files selector.
3. **Output is unbounded.** `set -e` with raw tool output: a clippy failure or a test failure prints the full cargo log (hundreds of lines with Bevy). No summary line, no log file, no step timing.
4. **One more unrelated trap found while measuring:** the hygiene test currently fails in this tree because an untracked/committed `docs/review/2026-11-code-review.md:2639` contains text the RE-tooling regex matches. Any doc a review agent writes can break the gate; that is the gate working as designed, but the failure shows how a docs-only change pays the whole pipeline for a 4 s check.

#### Current pipeline

Order is exactly `tools/check.sh`. "Imported" = `assets/ships` exists (`check.sh:16`).

| # | Step | What | Profile | Est. cost (warm / cold) | Rebuilds? |
|---|------|------|---------|------------------------|-----------|
| 1 | `ruff format --check .` | format | n/a | 0.03 s | no |
| 2 | `ruff check .` | lint | n/a | 0.08 s | no |
| 3 | python3 -c ... ASSETS | detect import | n/a | ~0.2 s (imports tools.import_assets) | no |
| 4 | `cargo fmt --check` | rustfmt | n/a | ~1 s | no |
| 5 | `cargo clippy --release --all-targets -- -D warnings` | lint, metadata-only build of lib, bin, unit tests | release (non-incr, opt3 flags) | edit: whole-crate recheck (tens of s); cold: Bevy deps check, minutes | Deps: shared w/ nothing else (clippy metadata is not reused by test/build). Own crate: full |
| 6 | `cargo test --release [-- --include-ignored]` | compile + run 895 `#[test]` (263 `#[ignore]` asset tests), lib unit-test binary + bin test + doctests | release | edit: **full opt-3 codegen of 67k lines twice** (normal lib rlib for bin/doctests, and `cfg(test)` lib) plus rustdoc doctest compile; minutes. Run time probably small | Yes: lib compiled 2x (rlib + test harness), unavoidable under cargo test, but at opt 3 non-incremental it is the dominant cost |
| 7 | (full only) `cargo build --release` | bin for startup smoke | release | link only after 6 (normal lib reused) but `strip=true` + fat link of 100 MB binary ~ tens of s | link |
| 8 | startup smoke `timeout 10 target/release/openprocyon` under xvfb | detect B0001 / panics at start | release | 10 s + Xvfb/lavapipe pipeline compile | no |
| 9 | `python3 -m tools.battle_check` | skirmish scenarios at 16x, 10-40 s each | release | ~1-2 min | no |
| 10 | `python3 -m unittest discover -s tests` | 196 tests | n/a | 4.4 s | no (it is last, so a Python failure is discovered only after the minutes above) |

Notes: no assets -> steps 7-9 and the ignored tests are skipped and a NOTE is printed (`check.sh:43`). `--fast` with assets: 1-6 + 10 then exit (`check.sh:20-27`); `--fast` without assets is identical to the full run (the flag is ignored in the `else` branch, which is fine since nothing more would run).

CI (`.github/workflows/check.yml`): one job on ubuntu-24.04, runs `tools/check.sh` with env `CARGO_PROFILE_RELEASE_OPT_LEVEL=0`, `CARGO_PROFILE_RELEASE_DEBUG=false`, `CARGO_INCREMENTAL=0`; no assets, so steps 1-6 and 10 only.

#### Findings

##### R1-01 [impact: high] [effort: M] The gate builds and tests in `--release` with a non-incremental, opt-3 profile
**Where:** `tools/check.sh:19,21,30,42`; `game/Cargo.toml` `[profile.release]` (only `strip = true`); `[profile.dev]` opt-level 1 + `"*"` opt 3 is defined but unused by the gate.
**What:** clippy, tests and the smoke build all use release. Release disables incremental compilation and uses 16 CGUs at opt 3. Every one-line edit therefore recompiles the whole `openprocyon` lib (twice for tests: rlib + test harness) with full optimisation.
**Why:** checklist 1.4: never run tests/clippy in `--release`; the correct fix is a dedicated profile. The only legitimate reason to want optimised code is the simulation-heavy tests and the battle check/smokes (16x battles, lavapipe). Pure unit tests (most of the 895) need none of it. Note also release has `debug-assertions` and `overflow-checks` off, so the two `debug_assert` in the tree never fire in the gate, and arithmetic overflow wraps instead of failing in tests.
**Suggestion:** add (maintainer-visible change, `game/Cargo.toml`) `[profile.fast] inherits = "release"; incremental = true; codegen-units = 256; debug = "line-tables-only"; strip = false` and move steps 5-7 to `--profile fast`; keep real `--release` for the player binary and `make run`. Even better for tests: run unit tests under the existing `dev` profile (deps already opt 3, own crate opt 1, incremental on, debug-assertions on). Measure both on the cargo timing run; likely candidate is `cargo test` (dev) + `cargo build --profile fast` only for the smokes. Update `tools/battle_check.py:GAME` (hard-coded `release/openprocyon`) and `tools/shot.sh`/`smokes.sh`/`autoplay.py` to honour one `PROFILE` variable so smokes and the gate share one binary.
**Expected gain:** edit-compile cycle from "whole crate at opt 3" to incremental: typically 3-10x for the test step after the first build; clippy shares nothing either way. Also catches overflow / debug_assert bugs.
**Risks:** a profile name change affects every doc/script path `target/release/` (`check.sh:34`, battle_check, shot.sh, docs). Dev-profile tests run slower for sim-heavy ones (opt 1 own code); keep those under `fast`. The profile change must be approved (Cargo.toml). Local and CI profile then must match, see R1-05.

##### R1-02 [impact: high] [effort: S] No tier below "everything": `--fast` is not fast and nothing is changed-files aware
**Where:** `tools/check.sh:8-9,20-27`; `AGENTS.md:71-73`; `docs/development.md:13`.
**What:** the only knob is `--fast`, which still runs clippy over all targets plus the full 895-test suite with `--include-ignored` plus all Python tests. There is no `--python`, `--rust`, `--docs`, no test filter pass-through, no `git diff --name-only` selector. A docs-only change, a Python exporter tweak and a Bevy system change all pay the same Rust cost, and doc says "run the relevant tests while iterating" with the only mechanism being typed cargo/unittest incantations (`docs/development.md:63-65`).
**Why:** checklist 4.4, 7.1, 7.3.
**Suggestion:** evolve `check.sh` (rule-compatible, one gate with modes): `tools/check.sh [--quick|--fast|--full] [--changed] [-- <cargo test filter>]`. `--changed` maps `git diff --name-only $(git merge-base HEAD origin/main)` + working tree: only `*.md`/`docs/**` -> hygiene test + ruff only; only `tools/**`/`tests/**`/`*.py` -> ruff + unittest (the matching `tests/test_<tool>.py` first); `game/**` -> add `cargo fmt --check` + `cargo clippy` + `cargo test <module filter>` where module = the `game/src/<mod>/` path (e.g. `combat/` -> `combat::`); `content/`, `mods/`, `Cargo.lock`, `check.sh` itself -> escalate to full. Always print the tier chosen and what was skipped. See "Proposed tiers".
**Expected gain:** docs/Python-only: ~5 s instead of minutes; single-module Rust change: one clippy + a filtered test run instead of 895 tests. Largest quality-of-life win per effort.
**Risks:** a change in a shared module (`ship.rs`, `data.rs`) has far-reaching effects; selector must be conservative (unknown path -> full) and the commit/CI tiers still run everything. Needs AGENTS.md wording: "single gate with tiers" (the rule says no parallel wrappers; this is one script with modes, so it fits, but `make check ARGS=...` docs should be updated).

##### R1-03 [impact: medium] [effort: S] Failure output is raw and unbounded; fast checks come last
**Where:** `tools/check.sh` as a whole (`set -e`, no capture); order lines 14-46.
**What:** each step streams its full output. A cargo test failure prints compile logs, all test output and a panic backtrace; a Python failure prints the whole unittest trace (the hygiene failure here printed a `Lists differ` diff of several lines plus a 100-char assertion). There is no `[n/m] clippy ... 41 s OK` progress line, no log path, no total time, and no final "FAILED at step X, rerun with: ..." line. The 4 s Python suite runs *after* minutes of Rust, so a trivially broken Python test is reported last, and `check.sh` ordering gives no cheapest-first fail-fast.
**Why:** checklist 7.2.
**Suggestion:** wrap each step in a tiny `step "name" cmd...` shell function that (a) redirects full output to `local/check/<step>.log`, (b) on success prints `ok  <name>  <secs>s`, (c) on failure prints `FAIL <name> (<secs>s)`, the last ~30 lines or a grep'd summary (`error[`/`warning:` lines with file:line via `--message-format short`, `failures:` + test names for cargo test, `FAIL:`/`ERROR:` lines for unittest) and the log path, then exits 2. Use `cargo test ... -- -q`, `cargo clippy --message-format short`. Add `CARGO_TERM_COLOR=never`. Put ruff, hygiene (`python3 -m unittest tests.test_repo_hygiene tests.test_layers tests.test_check_gate`, <1 s) before clippy. Keep `GITHUB_ACTIONS` mode streaming full logs (or `::group::`).
**Expected gain:** agent context per failure from several thousand lines to ~30; fail-fast on the cheap checks saves minutes on the many commits that fail on a Python/hygiene/format error.
**Risks:** swallowed output can hide a real cause; always keep the log path in the message. Gate shim tests (`test_check_gate.py`) assert exact call strings (`"cargo test --release"`), so they need updating with the profile and flags.

##### R1-04 [impact: medium] [effort: M] Rust tests are one big binary at release opt; no nextest, no filtering convention, asset tests all-or-nothing
**Where:** `check.sh:21` (`cargo test --release -- --include-ignored`); 895 `#[test]`, 263 `#[ignore]` across `game/src/**` (no `game/tests/`, so one lib test binary plus the 11-line `main.rs` binary compiled again for tests).
**What:** with assets, the gate runs all 895 including 263 asset tests every time. The lib is compiled as rlib + test harness + doctest. There is no `[lib] doctest = false`, no `cargo test --lib`, no `--bins` exclusion. A change to the CLI one-liner main still recompiles.
**Why:** checklist 4.1-4.3.
**Suggestion:** (a) `cargo test --lib` (skips bin test and doctests; add `doctest = false` to `[lib]` if no doctests exist, `grep` shows none obviously used) — saves one rustdoc compile of the lib. (b) Gate tier: edit tier runs asset-free tests only (`cargo test --lib <filter>`), commit tier adds `--include-ignored`. (c) Optional, **needs maintainer approval (new dev tool)**: `cargo-nextest` with `.config/nextest.toml` (`failure-output = "final"`, `status-level = "fail"`, test-group for asset tests), nextest `--run-ignored all`; process-per-test isolation suits Bevy global state; 1.4-3.4x faster run, `--failure-output final` gives short output, `cargo nextest run -E 'test(combat::)'` selector. Install check in `check.sh` with graceful fallback to `cargo test` so CI/Windows without it still work.
**Expected gain:** (a) tens of seconds per run; (b) a large part of the test run time when assets are present; (c) execution time 1.4-3.4x plus better output.
**Risks:** nextest has no doctests (fine if none); extra tool for contributors/Windows; fallback keeps the gate portable.

##### R1-05 [impact: medium] [effort: S] CI profile differs from local, so CI proves less and shares nothing with local; cache is saved from every PR
**Where:** `.github/workflows/check.yml:12-15,26-28`.
**What:** CI overrides `CARGO_PROFILE_RELEASE_OPT_LEVEL=0` and `DEBUG=false`, so it checks a profile (release flags, opt 0, no debug assertions) that nobody runs locally; clippy lint results can differ little, but cfg/codegen-dependent lints and overflow behaviour are identical to neither dev nor release. The Swatinem cache key includes env, so these overrides fine-tune the cache for CI alone (good for CI cost, no harm), but `Swatinem/rust-cache` has no `save-if`, so every PR run writes a multi-GB Bevy cache; with a 10 GB repo quota this evicts the main cache (checklist 6.1). `CARGO_INCREMENTAL: 0` is redundant for release (release is non-incremental) but needed if the gate moves to a dev/fast profile. `push: main` plus `pull_request` runs the full gate twice for a merge to main.
**Suggestion:** once R1-01 lands, set CI to the same named profile as local with only the env that really matters (`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_FAST_DEBUG=false` if kept) and `save-if: ${{ github.ref == 'refs/heads/main' }}`; add `shared-key: check` and `cache-on-failure: true`. Add `paths-ignore: ["docs/**","**.md"]` for the Rust job only if a separate cheap job still runs hygiene (a docs change can break hygiene — it just did here, so do not skip hygiene). Prefer `CARGO_BUILD_WARNINGS=deny` over `-- -D warnings`/RUSTFLAGS if cache key churn is an issue (clippy `-D warnings` after `--` does not change the fingerprint, so current form is fine; verify before changing).
**Expected gain:** warm CI cache hit rate on PRs (saved from main only), fewer evictions; faster feedback.
**Risks:** cache from main only means the first PR after a Cargo.lock change builds cold.

##### R1-06 [impact: medium] [effort: M] CI is one serial job; no Windows or asset-free smoke coverage until release
**Where:** `check.yml` (single `check` job); `release.yml:21-31` is the only Windows job and triggers on tags/dispatch only.
**What:** fmt, ruff, hygiene, clippy and tests run sequentially in one VM, so a ruff/hygiene failure is reported after the full Bevy compile. Windows is only built at release; AGENTS.md and the Makefile/check.sh clearly try to support Windows (`.venv/Scripts`, Git Bash), but a Windows break (path separators, `cfg(unix)` `wayland-sys` dep) first appears when a tag is pushed. No job runs the startup smoke even though lavapipe/Xvfb is available on ubuntu runners (asset-free menu start could be smoke-tested: the menu may need assets, check).
**Suggestion:** split into jobs: `lint` (ruff, hygiene, layers, check_gate unittest, rustfmt; ~30 s, no Rust deps, runs first, `needs` for the heavy job so it fails fast), `rust` (clippy + test, cached), optional `windows` (`cargo check --lib` or `cargo test --lib` only, `save-if` main, on `pull_request` with label or on `merge_group`/nightly `schedule` to bound cost). Keep it one script: jobs call `tools/check.sh --lint-only` / `--rust-only` (tier flags from R1-02) rather than copying commands into YAML.
**Expected gain:** failures on cheap checks within ~1 min instead of ~15-30 min; Windows regressions found at PR/nightly time.
**Risks:** Windows runners are 2x billed and Bevy compile is slow; use scheduled/label-triggered, not every push.

##### R1-07 [impact: medium] [effort: S] Gate misses things the human/agent relies on it for
**Where:** `check.sh`, `check.yml` (no assets).
**What:** What it catches: format/lint, unused code/clippy, hygiene rules (no game content, no addresses), layering (`tests/test_layers.py`), all asset-free Rust tests, Python parser tests with synthetic data, startup panic/B0001 (local with assets only), battle-level invariants (local with assets only), the gate script's own logic. What it misses: (1) **in CI, everything that needs assets**: 263 Rust asset tests, startup smoke, battle check, 13 of the Python test modules that use `needs_assets`, and 27 Python skips even here — so CI is green while the real tests never ran; the local full gate is the only place, and a contributor without assets (or an agent in a sandbox without the import) gets only the "NOTE: ... skipped" line on stderr and exit 0 — a silent pass that looks like green. (2) Debug assertions and overflow checks (release profile). (3) Rendering regressions: `tools/smokes.sh` (24 smokes) and `shot.sh` are not part of any gate; screenshots are never compared (UI changes rely on a human looking). (4) Windows. (5) Bevy B0001 query conflicts only if a smoke starts all plugins; the 10 s menu start does not enter a battle, so a battle-only system conflict (the very thing the comment `check.sh:28` cites) is found only by `battle_check`, which also needs assets.
**Suggestion:** make the skip loud and explicit in the machine-readable summary (`SKIPPED: asset-tests(263) startup-smoke battle-check — gate is PARTIAL`) and exit non-zero or a distinct code (`--require-assets`) for the commit tier on a machine that has assets configured; add a battle-entering asset-free headless smoke if one can be built from synthetic data (a tiny fixture map generated by a test, no game content); add `make smokes` to the push/"full" tier, not the commit tier; start image diffing per checklist 5.4-5.5 with untracked goldens (needs approval for any new tooling). Document in AGENTS.md which tier is "green enough to commit".
**Expected gain:** fewer false greens; regressions in rendering/battles caught before review.
**Risks:** more local time in the full tier; goldens differ across Mesa versions (use tolerance, local-only).

##### R1-08 [impact: low] [effort: S] Python test suite: fast and clean, but unselective and not isolated from the tree
**Where:** `tests/*.py` (46 modules, ~4.5k lines, 196 tests, ~4.4 s), `tests/imported.py`.
**What:** whole suite is cheap, so splitting is not worth it for speed; the issues are: `python3 -m unittest discover -s tests` is invoked in two places and from `make test-python`, with no `-v`/`--failfast`/`-q` and no parallelism (nothing needed). Test runs import the whole `tools` package via `tests/imported.py` (`from tools.import_assets import user_dir`) just to decide skipping — `check.sh:16` does this once more for detection, costing an interpreter start each. The hygiene test scans every tracked/doc file (`test_repo_hygiene.py:110-125`), so any markdown written into `docs/` by an agent (as `docs/review/2026-11-code-review.md` did) fails it; the failure message does not say how to fix it (it prints the line, which is fine). Per-module timing shows no slow outlier (all <0.1 s).
**Suggestion:** add `--failfast` and `-q` in the fast/quick tiers, `python3 -m unittest -q tests.test_repo_hygiene tests.test_layers tests.test_check_gate` as the "static" step run immediately after ruff; make `--changed` run `tests/test_<name>.py` for changed `tools/<name>.py` (naming is consistent: test_wot, test_msb, test_hdt, ...).
**Expected gain:** minor in time, helps clarity of failures; mainly enables R1-02.
**Risks:** none.

##### R1-09 [impact: low] [effort: S] `check_gate` test covers script wiring but will need to follow every gate change; no end-to-end self check
**Where:** `tests/test_check_gate.py:14-80`.
**What:** the shim approach (fake `ruff`, `python3`, `cargo`, `timeout`) is a good idea and cheap, but it asserts literal command strings (`"cargo test --release"` line 54-57), so each of R1-01..R1-04 must update it in lock-step; add tests for the new tier selection and for the "partial gate" message.
**Suggestion:** assert on semantic properties (a step order list, "clippy before test", "python tests present", "smoke only when imported and not fast") rather than exact flags; parametrize the profile name.
**Expected gain:** keeps the gate refactor safe. **Risks:** none.

##### R1-10 [impact: low] [effort: S] No dependency-pinned reproducibility of tool versions beyond ruff
**Where:** `.github/workflows/check.yml:23` uses `dtolnay/rust-toolchain@stable` while `Cargo.toml` pins `rust-version = "1.97.1"`; `release.yml` also `@stable`.
**What:** CI floats with stable; a new stable clippy lint can turn CI red without any code change (and local is on whatever rustup gave). `actions/checkout@v4`, `setup-python` unpinned by SHA.
**Suggestion:** pin the toolchain via `rust-toolchain.toml` (maintainer approval: new file affecting every contributor) or at least `toolchain: 1.97.1` in both workflows, bump deliberately. Python version: CI check job relies on the runner's system Python; `release.yml` pins 3.12; make `check.yml` use `setup-python` with the same pin and `cache: pip`.
**Expected gain:** deterministic CI; fewer surprise red builds. **Risks:** a pinned toolchain needs periodic bump.

#### Proposed tiers

All inside `tools/check.sh` (the "single gate" rule holds; this relaxes only the implied "one fixed sequence" reading and needs a one-line AGENTS.md edit saying the gate has tiers).

| Tier | When | Command | Does | Target time |
|------|------|---------|------|-------------|
| 0 static | after every edit (a Claude Code `Stop`/PostToolUse hook) | `tools/check.sh --quick` | ruff format/lint (or `rustfmt` on the touched file), hygiene+layers+gate tests, plus `cargo check --message-format short` only if `game/**` changed | 1-3 s Python/docs, ~10-30 s Rust (incremental `check`) |
| 1 module | while iterating | `tools/check.sh --changed` or `tools/check.sh -- combat::` | tier 0 + clippy + `cargo test --lib <module filter>` (dev/fast profile, incremental) + matching `tests/test_<tool>.py` | seconds-1 min |
| 2 commit | before every commit | `tools/check.sh` (today's `--fast`) | all of ruff/fmt/clippy, full Rust tests incl. asset (`--include-ignored`), full Python, summary + PARTIAL notice if assets missing | a few min |
| 3 push | before push / PR, UI or ECS changes | `tools/check.sh --full` | tier 2 + startup smoke + battle_check + `make smokes` subset | + ~2-4 min |
| 4 CI | PR | `lint` job (tier 0 + static) then `rust` job (tier 2 asset-free), Windows `cargo check` nightly/label | cached, `save-if` main | ~10 min cold, ~5 warm |

Per kind of change, cheapest correct gate: docs only -> hygiene unittest (`tests.test_repo_hygiene`, <1 s) + ruff; Python only (tools/tests) -> ruff + the one `tests/test_<tool>.py`, then full Python suite (4 s) at commit; one Rust module -> `cargo clippy` + `cargo test --lib <module>::` then tier 2 at commit; exporter change -> tier 2 plus re-import to a scratch dir and `python3 -m tools.compare_assets` (per AGENTS.md; not automated in the gate — consider adding as a `--full` step when `tools/export_*.py` changed); UI/ECS change -> tier 3 including the relevant `make smokes ARGS=<name>`.

#### Quick wins

1. (S) Add `step()` wrapper with timing, logs in `local/check/`, one-line ok/FAIL summaries, and run ruff + hygiene/layers/gate unittests before any cargo step (R1-03). Pure shell, no approval needed.
2. (S) Print an explicit `GATE PARTIAL: skipped asset tests, startup smoke, battle check` summary line (stderr already has a NOTE; make it the last line and tied to exit-code option) (R1-07).
3. (S) `cargo test --lib` (and `doctest = false`) instead of bare `cargo test` (R1-04a).
4. (S) Add `--changed`/`--quick` modes for docs-only and Python-only changes (R1-02); mapping can start with just those two categories.
5. (S) CI: `save-if` main on rust-cache, pin toolchain `1.97.1`, split a `lint` job before the heavy job (R1-05, R1-06, R1-10).
6. (M, approval) A named `fast` profile with incremental + codegen-units 256 for gate and smokes (R1-01); measure against the cargo timing run; then drop the CI env overrides.
7. (M, approval) `cargo-nextest` with fallback (R1-04c).
8. Note for maintainers: the currently failing hygiene test is caused by `docs/review/2026-11-code-review.md:2639` (text matched by the RE-tooling pattern); fix the text or the doc before the next commit gate.

---

## Appendix: Rust build configuration and test suite

#### Assessment

The repo already has the *right dev profile on paper* and then does not use it. `game/Cargo.toml:22-26` sets
`[profile.dev] opt-level = 1` and `[profile.dev.package."*"] opt-level = 3` (the Bevy-recommended pair), but every
validation, test, smoke and run path uses `--release`: `tools/check.sh:19` (`clippy --release`), `:21`/`:42`
(`test --release`), `:30` (`build --release`), `Makefile` `run`, `tools/shot.sh:23,25`, `tools/smokes.sh:39`,
`docs/development.md:63,65,74`. Release is `incremental = false`, 16 codegen units, opt-level 3
(`Cargo.toml:29-30` only adds `strip = true`), so each edit to the 67k-line `openprocyon` crate triggers a full
optimised recompile of that crate, and `cargo test` does this for up to three crate units (lib, lib-test, bin-test; the bin
test links the lib as a dependency). CI already works around this with `CARGO_PROFILE_RELEASE_OPT_LEVEL=0`,
`CARGO_INCREMENTAL=0` (`.github/workflows/check.yml:13-15`), which is itself evidence the owner knows release is the
wrong profile for checks; the local gate never got the same treatment.

No `.cargo/config.toml`, no `rust-toolchain.toml`, no `build.rs`, no `[features]`, no `[lib] doctest = false`, no
`[lints.rust]`, no `build.build-dir`, no per-worktree target policy. Toolchain here is rustc 1.100.0 (LLD is
already the default linker on x86_64 Linux, `lld`/`clang` exist, `mold`, `sccache`, `cargo-nextest` do not). Cargo.lock has 558
packages (6236 lines). `game/target` is 2.3 GB for the release tree alone (one worktree); at 4 cores / 15 GB RAM, parallel
agents with full opt-3 builds will contend hard for CPU.

Tests: 895 `#[test]`, 263 `#[ignore = "needs imported assets"]` (so ~632 asset-free). 151 call sites do `Library::load(assets_dir())`
(a full scan of every `ships/<id>/` folder + weapons, `data.rs:569-581`), uncached, per test. All ECS tests use
`MinimalPlugins` (+`AssetPlugin`); no test opens a window or GPU (grep for `DefaultPlugins|WinitPlugin|RenderPlugin`
found only non-test code). Global statics are almost all read-only `OnceLock` caches, so nextest is compatible.
I could not measure anything (no cargo, no assets in this checkout), so timings below are structural estimates, flagged as such.

#### Findings

##### R2-01 [impact: high] [effort: S] Stop running check, clippy and test in `--release`; use dev profile (already tuned) for the inner loop
**Where:** `tools/check.sh:19,21,30,42`; `Makefile:run`; `tools/shot.sh:23,25`; `tools/smokes.sh:39`; `docs/development.md:63-65,74`; `Cargo.toml:22-30`.
**What:** Everything compiles the crate at opt-level 3 with incremental off. The dev profile with opt1 + deps at opt3 exists but is only used by a bare `cargo run`/`cargo test`.
**Why:** Per-edit cost is a full fat-opt rebuild of one 67k-line crate, repeated for lib, lib-test, bin-test (and clippy `--release` builds a separate third artifact set that is shared with nothing; `clippy --release` metadata is shared with `cargo check --release` only).
Dev with incremental recompiles only changed CGUs (typically seconds to ~20 s vs minutes; common Bevy numbers: opt-level 1 own code + opt-level 3 deps is ~2-5x faster per edit than release for a crate this size; I did not measure here).
**Suggestion:**
- `cargo check`/`cargo clippy --all-targets` (dev profile, no `--release`) as the inner-loop and gate lint step. Clippy shares dep artifacts with `cargo check`, but not with `cargo test`, so the gate that runs clippy then test compiles the crate twice; unavoidable, but both are incremental in dev.
- `cargo test` (dev; the `test` profile inherits dev) for unit tests.
- Keep `--release` only for: `make run` for players/perf work, `cargo build --release --locked` in release.yml, and (if the battle check proves too slow in dev) a `fast` profile (R2-02).
- Add `debug = "line-tables-only"` to dev (see Proposed Cargo config): ~20-40% faster dev builds, roughly halves target size; backtraces still have file:line.
**Expected gain:** biggest single item. Edit-to-test goes from "recompile 3 crate units at opt 3" to "incremental recompile of the touched CGUs". Likely minutes to well under a minute per iteration; confirm with the timing run that is in progress (compare `cargo test --release` vs `cargo test` after a one-line edit in `combat/tests.rs`).
**Risks:** (1) Behavioural drift: float results can differ between opt levels only if code relies on UB/fused ops; Rust does not fuse FMA, so numeric results are deterministic across opt levels; low risk. (2) Long simulation tests run slower in dev (see Slowest tests); own code is opt1 and Bevy is opt3, so should be acceptable; verify and move any outliers behind `--release` or `#[ignore]`. (3) First dev build compiles Bevy at opt3 (one-time, cached per worktree; see R2-06 for warming). (4) `check.sh` smoke currently runs `target/release/openprocyon`; switching it to the dev binary needs `cargo build` and a path change (or keep release for the smoke only; the smoke is not in `--fast`).

##### R2-02 [impact: medium] [effort: S] One extra profile `fast` for the things that really need optimised own code
**Where:** `tools/check.sh:30-40` (startup smoke + `tools.battle_check`, ~1-2 min), `tools/smokes.sh`, `shot.sh`.
**What:** Battle check, autoplay and long smokes run the real game for minutes of sim; own-crate opt1 may throttle them, and wall-clock matters since they can run at `--speed 16`.
**Suggestion:** `[profile.fast] inherits = "release"; incremental = true; codegen-units = 256; debug = "line-tables-only"; strip = "none"` (profile in the Proposed Cargo config). Use `cargo build --profile fast` + `target/fast/openprocyon` for smokes/battle_check/shot.sh. It has separate artifacts (another ~1 GB+ per worktree), so only create it if measurement shows dev (opt1 own + opt3 Bevy) is too slow for the smokes. Start by just trying the dev binary for smokes: dev already has Bevy at opt 3 which is where most frame time goes (render, ECS), so it may be good enough and then you need no extra tree.
**Expected gain:** edit-to-smoke rebuild incremental instead of full; smoke runtime near release.
**Risks:** extra disk; two paths to document. `profile.release.strip = true` is inherited; override to keep the profile fast to link.

##### R2-03 [impact: medium] [effort: S] `dynamic_linking` as a dev-only feature
**Where:** `game/Cargo.toml:10` (bevy with `default-features = false`, no `[features]` table).
**What / Suggestion:** add `[features] dev = ["bevy/dynamic_linking"]`. Use `cargo test --features dev` / `cargo run --features dev`; wrap in Makefile/`tools/dev.sh` (e.g. `FEATURES ?= dev`). Never in `default`, never enabled in `.github/workflows/*` or `tools/package_release.py` (the release job runs `cargo build --release --locked`, release.yml:42, so is untouched as long as the feature is not default).
**Why:** Bevy calls this the most impactful compile-time reduction: Bevy becomes a dylib so only the 67k-line crate relinks; every test binary (lib-test and bin-test each statically link all of Bevy today) shrinks and links much faster.
**Expected gain:** link step per test binary from many seconds to ~1 s on top of R2-01; biggest effect on `cargo test` where two big test executables are relinked on each edit.
**Risks:** (a) Windows: needs the dylib dir on PATH when launching the binary directly (cargo run/test sets it), the 65535 exported-symbol limit has bitten Bevy projects (Bevy documents working configurations; opt-level for deps is required, which dev already has). Treat as Linux/macOS dev feature, Windows contributors can skip it. (b) Running `target/debug/openprocyon` directly outside cargo needs `LD_LIBRARY_PATH`; `tools/shot.sh` uses `cargo run`, so OK; `check.sh` direct-execs the binary (release, no feature) so unaffected. (c) `--features dev` changes the fingerprint: all agents must use the same feature set or they will rebuild each other's caches (R2-06). (d) Dynamic linking and `wayland-sys dlopen` (`Cargo.toml:15`) are independent, no interaction. (e) Not a new dependency (existing crate feature), but it is a manifest feature: mention to the maintainer.

##### R2-04 [impact: medium] [effort: S] Disable doc-tests; make "run one module's tests" a documented one-liner
**Where:** `game/Cargo.toml` (no `[lib] doctest = false`); `docs/development.md:65`.
**What:** `cargo test` always ends with a rustdoc doc-test pass over the lib. I found no ``` examples in `game/src` (grep for doc comments with fences returned nothing), so that pass finds nothing, but rustdoc still has to parse and analyse the whole lib plus deps; in release it also runs in the release dep config. Doc-test pass with `--include-ignored` also does nothing useful.
**Suggestion:** add `[lib] doctest = false` (and a `[[bin]]` is not needed; bins have no doc-tests). Document in `docs/development.md` and AGENTS.md: `cargo test --lib <module path>` (e.g. `cargo test --lib combat::tests`, `--lib rules::`, `--lib menu::tests`) which also skips the bin-test binary; add `-- --include-ignored` for the asset ones; `classic::` tests live in the bin and need `--bin openprocyon`. Today only `panels::tests` is shown as an example (development.md:65), and it omits `--lib`, so it also compiles and runs the bin tests (22 tests in `classic/`).
**Expected gain:** seconds per test run (rustdoc pass); less for filtered runs, mainly that `--lib` avoids linking/running the bin test binary.
**Risks:** none; adding a real doc example later would need doctest re-enabled.

##### R2-05 [impact: medium] [effort: M] cargo-nextest for the test step (optional, per-developer tool)
**Where:** `tools/check.sh:21,42`; test suite statics listed below.
**What:** nextest runs each test in its own process, parallel across all cores, prints per-test time (the only stable-toolchain way to find the slow tests), supports `--run-ignored all` (equals `--include-ignored`), `--failure-output final`, `--retries`, `rerun` of failures, test groups (`max-threads = 1` for any GPU/Xvfb test, none exist among unit tests today), and filterset like `-E 'test(/^combat::/)'`.
**Compatibility check:** global state is benign: read-only caches `OnceLock` in `data.rs:25`, `sprites.rs:106,111,274,283`, `crew.rs:124`, `menu.rs:353`, `theme.rs:249`, `lib.rs:91` (ASSETS path cached from `PROCYON_ASSETS`), all idempotent. Mutable globals: `boarding/odds.rs:150` `MODDED` Mutex registry (written by `Library::load -> boarding::register`, `data.rs:575-577`), `render/rules.rs:130` `SHADERS` Mutex, and the test-only `VIEW_STATICS` lock (`rules/console/more/tests.rs:224`) that serialises tests which touch view statics (`view.rs:522` `LOOK`). Under plain `cargo test` those are shared across threads (order-dependent hazard; the existing lock shows they have already been bitten); under nextest each test is a fresh process, which removes that whole class. `modding.rs:29` `ACTIVE` and `rules/eval/diag.rs:20` `OPTIONS` OnceLocks are set from env/args: also fine per process. `smoke/mod.rs:214,267` caches env reads: fine.
**Cost:** each process repeats `Library::load` (the 151 call sites already do that per test today, uncached, so no loss) plus process start (small; the dylib from R2-03 loads quickly).
**Suggestion:** document `cargo nextest run --lib` as the recommended test command when installed; keep `cargo test` as the fallback in `check.sh` (`command -v cargo-nextest`). Add `.config/nextest.toml` with `[profile.default] slow-timeout = { period = "30s" }, fail-fast = false`, `[profile.default] status-level = "fail"`. Nextest does not run doc-tests (moot after R2-04). Needs maintainer OK as a dev tool (not a crate dependency, nothing in Cargo.lock changes).
**Expected gain:** 1.4-3x on test execution (Library::load-heavy tests are CPU-bound so parallelise well), plus per-test timing visibility and a failure-only summary suited to agent output.
**Risks:** one more tool for contributors to install; CI install step (`taiki-e/install-action`).

##### R2-06 [impact: high] [effort: S] Worktrees: no target-dir, warm-up or cleanup policy
**Where:** no `.cargo/config.toml`; `tools/shot.sh:9` only handles `assets/` for fresh worktrees ("use the main checkout's import"), nothing for `game/target`.
**What:** each worktree starts with an empty `game/target` (558 crates incl. all of Bevy at opt 3, several minutes on 4 cores) and then accumulates ~2.3 GB per profile. Several agents building at once on 4 cores / 15 GB RAM will thrash.
**Suggestion:** (1) a `tools/worktree-warm.sh` (or a Claude Code `WorktreeCreate` hook): `cp -a --reflink=auto <main>/game/target <new>/game/target` then `rm -rf target/*/incremental`; `-a` preserves mtimes so dependencies stay fresh (check `cargo build` prints no "Compiling bevy"). Dev profile only, with one fixed feature set, so copies stay valid. (2) Do not share one `CARGO_TARGET_DIR` among concurrent agents (single lock per profile dir serialises them, binaries overwrite each other, unless rustc >= 1.100 build-dir v2 is used and measured). (3) A cleanup wrapper that deletes `game/target` on worktree removal. (4) Add `build.jobs`/`CARGO_BUILD_JOBS=2` per agent when 3+ run in parallel to avoid the OOM linker spike (rustc and LLD at dev debug level are fine at 15 GB; the risk is mostly CPU starvation).
**Expected gain:** a new agent skips the 5-15 minute cold dependency build (estimate, 558 crates; confirm with the timing run).
**Risks:** ext4 has no reflink so the copy is a real 2+ GB copy (still much faster than building); stale `incremental/` must be dropped.

##### R2-07 [impact: medium] [effort: S] Lint policy: `[lints]` + `CARGO_BUILD_WARNINGS=deny` instead of `-- -D warnings`
**Where:** `tools/check.sh:19` (`cargo clippy --release --all-targets -- -D warnings`), `Cargo.toml:17-20` (`[lints.clippy]` only).
**What:** warnings are denied via clippy args at gate time only. During iteration agents either see warnings in noise or have no way to say "deny only at the end".
**Suggestion:** rust-version is 1.97.1, so `CARGO_BUILD_WARNINGS=deny` (stable, `build.warnings`) is available. In `check.sh`: `CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets` (no `--`), and leave warnings as warnings during development; inner loop uses `CARGO_BUILD_WARNINGS=allow`. Unlike `RUSTFLAGS=-Dwarnings` it does not change dependency fingerprints. The existing `-- -D warnings` is a clippy-driver arg and also does not rebuild deps, so this is a modest ergonomic/consistency gain, not a speed gain; verify with two consecutive clippy runs that nothing recompiles when only the env var changes. Keep `[lints.clippy]` and optionally add `[lints.rust] unused = "warn"`-style shared config so `cargo check` and clippy agree.
**Expected gain:** small time; mainly clearer "fix warnings at the end" semantics and one fewer flag to get wrong.
**Risks:** verify fingerprint behaviour in your cargo version (1.100 here) before relying on it; CI uses `dtolnay/rust-toolchain@stable`, fine.

##### R2-08 [impact: medium] [effort: M] `Library::load` per test (151 call sites) and several multi-thousand-tick sims
**Where:** `data.rs:569-581`; call sites e.g. `combat/mod.rs:210`, `panels.rs:685`, `audio.rs:355`, `blackhole/tests.rs:133,155`, `combat/lob/tests.rs:37,40,82,126,157` (the last file loads twice inside one test, at 37 and 40).
**What:** every asset-requiring test re-reads and parses JSON/geometry for every ship class in `ships/` (a few dozen ships, with geometry.json) plus weapons, and re-registers boarding points. Asset tests are 263 of 895 and they dominate wall-clock (only run when assets exist, i.e. in the full gate and for agents with an import).
**Suggestion:** a test-only shared fixture: `fn shared_library() -> &'static Library` with `OnceLock`, giving `.clone()` (is `Library` Clone? `Class` appears owned data; if not, tests take `&Library` and wrap into `Arc`/`insert_resource(lib.clone())`). Only valid for tests that do not mutate or point at a modded root. Under nextest this saving disappears (process per test), so choose: nextest for isolation, or shared fixture with plain `cargo test`. Measure first (`cargo nextest run --lib -- --run-ignored all` prints the per-test time).
**Expected gain:** unknown without timings; if a load is ~50-200 ms it is 151 x that, summed across threads (7-30 s CPU), i.e. likely a few seconds wall. Low-to-medium. Do not do before measuring.
**Risks:** shared mutable global (`boarding::MODDED` is written by `register`) makes the order of tests matter; fixture must register once.

##### R2-09 [impact: medium] [effort: M-L] Compile-time hotspots and crate splitting: an honest cost/benefit
**Where:** `game/src/lib.rs` (704 lines, 49 `mod`), biggest files `combat/tests.rs` 1763, `combat/gunnery.rs` 1238, `rules/tests.rs` 1171, `ship.rs` 1118, `classic/panels.rs` 1097. 273 `add_systems` calls; two `macro_rules!` only (`render/batch.rs:11`, `smoke/mod.rs:13`); no proc-macros of our own, no `build.rs`. Own code is ~62k lines in the lib, ~5.3k in `classic/` (in the bin).
**What:** Rust compiles one crate as ~16 CGUs; incremental in dev reuses unchanged CGUs, but every edit still re-runs type check/borrowck on the whole crate (query-based, incremental) and re-codegens the modules' CGUs plus the Bevy generic instantiations (`Query`/`Res` system params, `add_systems` tuples) which are monomorphised into the lib. Large `add_systems((... ,..))` tuples and big schedule-wiring functions in `lib.rs` are the usual slow items. `--all-targets` builds the lib twice (normal + test cfg), so every non-test edit is analysed and codegenned twice (plus once more through the bin test).
**Splitting the crate:** AGENTS.md's layering (shared lib, `classic` in the bin) already isolates the view: editing `classic/` rebuilds only the 5k-line bin. The remaining 62k-line lib is one unit. A cut that would help is a leaf crate that does not depend on the rest, but candidates are tangled: `rules/eval` (2.9k lines, "pure, no ECS" per development.md) still `use bevy::prelude::*`, `crate::game_to_bevy`, `crate::Nearest`, `crate::campaign::Mission` (`rules/eval.rs:1-7`), so extraction needs an interface cut; `fx/sim`, `ship::step`, `collision` geometry are similar. A Cargo workspace also affects every doc path, `include_str!`/`assets_dir()` source-checkout lookup (`lib.rs:91-110`), tooling (`tools/check.sh` `cd game`, `tests/test_layers.py`, CI `workspaces: game`) and the "Module ownership" doc, and Cargo.lock/`rust-cache` config.
**Verdict:** do not split now. First do R2-01/R2-03/R2-04, then run `cargo build --timings` (and `cargo +nightly rustc -Zself-profile` only if wanted) on a no-op-edit rebuild; if the lib's frontend time stays above ~15-20 s after the profile fix, split out a Bevy-free "rules + data model" crate as the first cut (it also makes the rules tests, ~1.2k+ lines in `rules/tests.rs`, runnable in ~seconds without linking Bevy). The expected benefit of a split is real only for the leaf crate's own tests and for parallelism across crates; incremental already does most of the per-file work. Cost: high churn on 67k lines of `crate::` paths, and merge conflicts with parallel agents mid-flight.
**Risks:** a mid-flight crate split collides with every open worktree.

##### R2-10 [impact: low] [effort: S] Release profile and CI are fine; keep them separate
**Where:** `Cargo.toml:28-30`, `.github/workflows/check.yml:13-15,26`.
**What:** release: no LTO deliberately (documented, bigger binaries), `strip = true`; good. CI check job already uses opt-level 0 and `CARGO_INCREMENTAL=0` and `Swatinem/rust-cache` with `workspaces: game`. Once the local gate switches to dev, CI should run the same commands on the dev profile, with `CARGO_PROFILE_DEV_DEBUG=0` (no debuginfo in CI) so that local and CI check the same artifacts class; drop the `CARGO_PROFILE_RELEASE_*` workaround. Caveat from R2: if `CI=true` is set in agents' sandboxes, Cargo >= 1.99 disables incremental by default; set `CARGO_INCREMENTAL=1` explicitly in the agent env/Makefile.
**Risks:** none significant; CI cache key changes once.

##### R2-11 [impact: low] [effort: S] `.cargo/config.toml` and toolchain pinning
**Where:** absent.
**What/Suggestion:** a committed `game/.cargo/config.toml` with only things valid on every platform: `[build] incremental` default, `[term] ...`. Not a linker line (LLD is already the Linux default since 1.90; mold/wild are per-developer in `~/.cargo/config.toml`, and a committed linker breaks CI/Windows). Do NOT add `rust-toolchain.toml` pinning stable: `rust-version = "1.97.1"` already states the floor; a pin conflicts with the `dtolnay/rust-toolchain@stable` CI and the project's stable-only contract (needs maintainer approval if wanted). Nightly options (`-Zshare-generics`, cranelift, `-Zthreads`) stay opt-in per developer.
**Risks:** none for the stable-only suggestion.

##### R2-12 [impact: low] [effort: S] Test binary/fixtures structure
**Where:** all tests are unit tests inside `game/src`; there is no `game/tests/` directory.
**What:** that is the cheap structure for link time (one lib-test binary + one bin-test binary with 22 classic tests). Do not add `tests/*.rs` integration binaries (each would link the whole lib plus Bevy again). Bin tests: 22 tests compile and link a second big executable on every `cargo test`; consider moving classic tests behind `cargo test --lib` only in the inner loop (R2-04) and full run only in the gate.
**Risks:** none.

#### Proposed Cargo config

```toml
# game/Cargo.toml additions

[features]
# Developer-only: Bevy as a shared library, so edits relink only this crate. Never in `default`; release.yml and CI do not enable it.
# Windows: skip it (or see Bevy's dynamic-linking notes); `cargo run/test --features dev` sets the library path.
dev = ["bevy/dynamic_linking"]

[lib]
doctest = false          # no doc examples exist; skips the rustdoc pass in `cargo test`

[profile.dev]
opt-level = 1
debug = "line-tables-only"   # file:line backtraces, ~20-40% faster builds, ~half the target size

[profile.dev.package."*"]
opt-level = 3
debug = false

# Optional: only if smokes / battle_check are too slow on the dev profile.
[profile.fast]
inherits = "release"
incremental = true
codegen-units = 256
debug = "line-tables-only"
strip = "none"

# [profile.release] unchanged (strip = true, no LTO: documented choice)
```

Command mapping (all without `--release` unless stated):

| Purpose | Command | Profile |
|---|---|---|
| compile errors only | `cargo check --message-format short` (add `--features dev`) | dev, shares with clippy |
| lint gate | `CARGO_BUILD_WARNINGS=deny cargo clippy --all-targets` | dev |
| one module | `cargo test --lib combat::tests [-- --include-ignored]` or `cargo nextest run --lib -E 'test(/^combat::/)' --run-ignored all` | dev/test |
| full tests (gate) | `cargo test [-- --include-ignored]` | dev/test (not shared with clippy) |
| smokes, battle_check, shots | `cargo build --profile fast` then `target/fast/openprocyon`, or dev binary if fast enough | fast (optional) |
| play / players / CI release | `cargo run --release`, `cargo build --release --locked` | release |

Per-developer (not committed): `~/.cargo/config.toml` with mold or wild linker; `CARGO_INCREMENTAL=1` if `CI=true` leaks into sandboxes.

#### Slowest/most expensive tests

All estimates are from structure (no timings available; no assets, no cargo run). Confirm with `cargo nextest run --lib --run-ignored all` which prints per-test time.

1. `collision/tests.rs:205` loop `for frame in 0..3636` with a full `sail_app` `app.update()` per frame plus `Library::load` (asset test).
2. `fx/sim/tests.rs:121` `docking_marker_particles_are_bounded_over_a_long_mission`: 9000 `update_fx` ticks of a particle emitter (asset test); `fx/sim/tests.rs:102,144` 1800 ticks each.
3. `combat/tests.rs:288`: 1200 `app.update()` per gun mount, inside a loop over every mount of a class (`for mount in &lib.classes[class].mounts`), so 1200 x number of mounts updates; shells query per tick (`:288-292`); the likely slowest unit test in the repo. Others in that file: 600-tick loops (`:522,:582`), 300/400/200-tick loops (`:267,324,351,379,427,476,627,1657`).
4. `blackhole/tests.rs:218` 3000 `tick()` with an `all()` assertion over points each tick; `nebula/tests.rs:117` 3000 ticks and `:63,100` 300 each.
5. `collision/tests.rs:167` 1500-update and `:141` 300-update tests (plus 609: 200).
6. `rules/tests.rs:869` 1000-iteration tutorial camera objective test; `rules/tests_ext.rs:395` 120.
7. `lifeboats/tests.rs:54` 200 updates; `combat/lob/tests.rs` loads the Library 5 times (lines 37, 40, 82, 126, 157).
8. Every asset test pays the `Library::load` tax (151 call sites, e.g. `panels.rs:685`, `audio.rs:355`, `combat/mod.rs:210`); the files with the most `#[ignore]` asset tests are `combat/tests.rs` (44), `rules/tests.rs` (33), `menu/tests.rs` (27), `ship/movement_tests.rs` (19), `crew_arms/tests.rs` (18), `collision/tests.rs` (18).
Which genuinely need optimisation: probably none at the "must be release" level, because Bevy itself is opt 3 in dev; the 3000-9000-tick pure-Rust loops (`blackhole`, `nebula`, `fx/sim`) are the only ones where own-code opt-level 1 vs 3 could matter (maybe 2-4x slower than release for tight loops, still seconds); if one is over ~5 s in dev, mark it `#[ignore = "slow in dev; cargo test --release"]` rather than moving everything back to release.

Not in the unit tests but the real slow items of the gate (`check.sh:29-40`): the startup smoke (10 s plus Xvfb + lavapipe pipeline compile) and `tools.battle_check` (1-2 min); `--fast` already skips them.

#### Quick wins

1. Drop `--release` from `tools/check.sh` lines 19, 21, 42 (and `docs/development.md:63,65,74` examples); keep release only for play/release builds. Add `debug = "line-tables-only"` to `[profile.dev]` (`Cargo.toml:22`). One edit, largest effect.
2. Add `[lib] doctest = false` to `game/Cargo.toml`.
3. Document in AGENTS.md/development.md: `cargo test --lib <module::path>` for one module (add `-- --include-ignored` for asset tests; `--bin openprocyon classic::` for classic) and `cargo check --message-format short` as the "does it compile" command; say "do not run the full `make check` after each edit".
4. Add the `dev` feature (`bevy/dynamic_linking`) and use it in the Makefile via a `FEATURES` variable.
5. Install `cargo-nextest` (dev tool, no repo change) and use `cargo nextest run --lib` for per-test timing to confirm the slow list above; then decide on R2-08.
6. Warm new worktrees by `cp -a --reflink=auto` of the dev `target/` (delete `incremental/`); delete `game/target` when a worktree is removed (R2-06).
7. Replace `-- -D warnings` by `CARGO_BUILD_WARNINGS=deny` in `check.sh` after verifying no rebuild.
8. Change CI to the same dev-profile commands (`CARGO_PROFILE_DEV_DEBUG=0`) and remove the `CARGO_PROFILE_RELEASE_*` workaround (`check.yml:13-15`).

Needs maintainer approval: new `[features]`/`[lib]` keys and profile changes in `Cargo.toml` (manifest changes), cargo-nextest as a recommended tool, any committed `.cargo/config.toml` or `rust-toolchain.toml`, and any crate split. No new crate dependency is required by any suggestion.

---

## Appendix: Smoke, screenshot and regression tooling

#### Assessment

The harness is good in spirit: one `--script` enum with a typo test (`game/src/smoke/mod.rs:13-45`), frame counters that wait
for the Loading gate (`game/src/lib.rs:600-605`, "the count waits while it loads"), a logged battle report with whole-battle
invariants (`tools/battle_check.py`), and software-Vulkan fallback in `tools/shot.sh`. The weaknesses are in the loop around it.

1. **Every scenario is a fresh process on the release profile, run strictly serially.** `smokes.sh` spawns 24, `battle_check` 5,
   `check.sh` 1 more (10 s sleep) plus the battle check. Nothing is shared or parallel.
2. **Mixed self-judging.** About 8 scripts assert (fleet 77 asserts, lifeboat PASS lines, console, pause, sight); the rest
   (damage, victory, defeat, restart, course, target, dock, fxtest, mods, impacts, fire, shells) only prove "a picture exists and
   is not black" (`tools/shot.sh:38-41`: max channel < 12). There is **no image comparison anywhere**, so a visual regression is
   found only if an agent opens the PNG and looks (expensive for the agent: tokens, plus it is subjective).
3. **Timing is wall-clock-coupled.** Frames count only post-Loading, but sim time per frame comes from real frame time
   (`Time<Virtual>` with `max_delta`, `lib.rs:454-470`); there is no `TimeUpdateStrategy::ManualDuration` in the real app
   (it only appears in unit tests: `fixed_tests.rs:8`, `combat/tests.rs:1009`). So frame N is a different game moment on lavapipe
   at 8 fps than on a GPU at 144 fps, battles at `--speed 16` are not reproducible run to run, and pixels cannot be compared exactly.
4. **Output and selection are agent-hostile**: logs go to `local/smokes/*.log`, the only live output is `ok/FAIL name`, there is
   no "which smoke for my change" map (this report adds one below), and an agent has no cheap way to see "what changed visually".

#### Current cost model

Per process (shot.sh path): `cargo run --release` (fingerprint check, 0.5-2 s warm; a full optimised rebuild of the crate if any
`.rs` changed, since `[profile.release]` has only `strip = true`, `game/Cargo.toml:29-30`: opt-level 3, no incremental) ->
`xvfb-run -a` (X server start ~1 s) -> Bevy window + wgpu on lavapipe -> asset load (JSON/GLB/PNG, large) -> shader pipeline
compile on CPU (`loading.rs`: SETTLE = 10 quiet frames, up to GIVE_UP = 20 s) -> N frames -> `SCRIPT_TAIL` 100 extra frames for
scripted runs (`lib.rs:592-595`) -> `std::process::exit` -> Python/PIL black check (a third process, `shot.sh:33-37`).
The docs' "~20 s each" is therefore mostly **fixed startup** (Xvfb + assets + pipeline compile on lavapipe), not the scenario.

| Entry point | Processes | Rough serial wall time (docs' own numbers) |
|---|---|---|
| `make smokes` | 24 game runs (+24 `cargo run` + 24 PIL) | 24 x ~20 s = ~8 min (lifeboat-* run to frame 3000 or PASS) |
| `make battles` | 5 | `seconds`/16 of game time at best (120-300 game s = 8-19 s) + startup; docs say 10-40 s each, ~1-2 min total |
| `check.sh` (full) | clippy, `cargo test --release -- --include-ignored`, `cargo build --release` (third artifact set for the same crate), 10 s startup smoke, battle_check | docs: ~2 min of it is smoke + battles |
| `make autoplay` | 13, default `--jobs 1` | game-time-bound, long (limit 3600 game s each at speed 16 = minutes each) |

What dominates: (a) startup x process count, (b) in `check.sh`/`battle_check` a fixed `timeout 10` for a menu run that is
only looking for a panic, (c) the release crate rebuild whenever a file changed (`battle_check.py:196-198` and `smokes.sh:46`
both call `cargo build --release`, so a stale binary is silently rebuilt; each agent in a separate worktree has its own target dir).

#### Findings

##### R3-01 [impact: high] [effort: M] Run many scenarios in one process ("smoke batch")
**Where:** `game/src/lib.rs:600-635` (`screenshot()` ends with `std::process::exit(0)`); `tools/smokes.sh:11-35`; `tools/battle_check.py:187-203`.
**What:** every smoke is one process because `Config { script, shot, shot_frame, ... }` is parsed once and `exit(0)` terminates the run.
**Why:** the game already owns battle reset (`scenario::restart`, `leave_battle`, `enter_battle`, `resume_scenario`, `lib.rs:56`; scripts
`restart`, `pause-restart`, `ingame-*`, `missions-play` exercise it) and `Loading` re-gates each battle start. Startup (window, asset
index, pipeline cache) is paid once; later battles reuse compiled pipelines and loaded GLB/textures (Bevy asset cache), so scenario
2..N should cost only their own frames, maybe 2-5 s each instead of ~20 s.
**Suggestion:** add `--batch <file>` (one scenario per line, same args as smokes.sh). `Config` becomes `Resource<Queue>`: when the
current scenario is "over" (`run_is_over`), save its pictures, log `SMOKE <name> PASS|FAIL <reason>`, reset `cfg.frame = 0`, swap
`cfg.script/shot/shot_frames`, call `leave_battle` then `enter_battle` with the next args, and exit when the queue is empty.
Assertions currently use `assert!`/panic (`fleet.rs`, `lifeboats.rs`); in batch mode catch them with `std::panic::catch_unwind`
around the system or convert to `Result` -> per-scenario FAIL instead of killing the batch. Group batches by "same
map/mission" to avoid reload, and fall back to one-process-per-scenario for scripts that change the menu screen or `--mod`.
**Expected gain:** `make smokes` ~8 min -> ~2-3 min; battle_check 5 starts -> 1. Lowest risk first step: batch only scripts of
the form `--map X --script Y` (pause, sight, fxtest, victory, defeat, course, target, damage, restart) and keep mission/lifeboat ones separate.
**Risks:** state leaking between scenarios (Local<> in systems such as `fire`'s `source`, `impacts`' `done`, `console`'s `submitted`
are per-system `Local`s that never reset: a second use in one process would silently skip); `static SHOTS_SAVED`/`OnceLock` env
reads (`mod.rs` `screenshot_hooks`, `autoplay_end`, `LIMIT`) are process-global; `ended` rules; a panic aborts the whole batch
unless caught. Needs a maintainer decision on touching `Config` (it is a shared wiring struct).

##### R3-02 [impact: high] [effort: S] Run independent smokes in parallel (jobs)
**Where:** `tools/smokes.sh:36-47` serial `while read`; `tools/autoplay.py` already has `--jobs` (`ThreadPoolExecutor`); `battle_check.py` is serial.
**What / Why:** each process is startup-bound and lavapipe uses all cores for pipeline compile but is mostly idle while waiting on
asset IO and during short menu frames; `autoplay.py` already proved separate `PROCYON_OVERLAY` dirs make parallel runs safe. `xvfb-run -a` picks a free display, so no collisions.
**Suggestion:** port `smokes.sh` to `tools/smokes.py` (shares `display_wrapper`, `silent_data_home` from `autoplay.py`) with `--jobs N`
(default `min(4, cpu_count//4)`), per-run `XDG_DATA_HOME` and `PROCYON_OVERLAY`, `LP_NUM_THREADS=2` (lavapipe thread cap) per run so
the processes do not fight, and the same flags for `battle_check`. Print results as they finish.
**Expected gain:** 2.5-3.5x on a 8-16 core machine for smokes; battles 5 scenarios in parallel ~ time of the longest.
**Risks:** lavapipe is CPU-bound and battle_check's stutter invariant (`STUTTER = 10` frames > 50 ms, `battle_check.py:41`) will false-fail under load, so judge stutter only with `--jobs 1` or on a quiet machine (mark `no-stutter` in parallel mode). Several worktree agents on one machine already share the CPU; measure first.

##### R3-03 [impact: high] [effort: S] Use the built binary, not `cargo run`, and stop rebuilding implicitly
**Where:** `tools/shot.sh:24-26` (`cargo run --release`), `tools/smokes.sh:46` (build once, then 24 `cargo run`), `tools/battle_check.py:196` and `autoplay.py:78` (build, then run `target/release/openprocyon`; they hard-code `game/target/release/openprocyon`, ignoring `CARGO_TARGET_DIR` in autoplay, honoring it in battle_check).
**What:** `cargo run` re-evaluates the fingerprint (0.3-2 s with Bevy's dependency graph, more over a slow FS) per run, and rebuilds whatever is stale. Fine for the first run, wasteful for 23 more.
**Suggestion:** one helper `tools/lib_game.sh|py` that resolves the binary path via `cargo metadata`/`CARGO_TARGET_DIR`, builds once (`cargo build --release -q`), prints the build time, and everything else execs the binary with `cwd=game/` and the asset env. `shot.sh` gets `--no-build`/`PROCYON_BIN` (if set, skip cargo). Honors assets path discovery in the worktree. Also lets the "baseline" binary (R3-05) be passed in.
**Expected gain:** 1-2 s per smoke (~30-50 s per `make smokes`), and removes a surprising source of hidden minutes (a silent full release rebuild inside a smoke run).
**Risks:** low. Keep `cargo run` fallback when no binary exists.

##### R3-04 [impact: high] [effort: M] Deterministic fixed-step mode for scripted runs
**Where:** `lib.rs:454-470` (virtual-time `max_delta`/speed), `lib.rs:431` (`Time::<Fixed>` = 33 ms), no `TimeUpdateStrategy` in the app, `screenshot()` counts frames (`cfg.frame += 1`).
**What:** in a shot/script/report run, sim time depends on real frame time. A frame-300 shot is "after ~10 s of sim at 60 fps" but "after fewer ticks" at lower fps with `max_delta` clamps (`MAX_CATCH_UP_FRAMES`), and `--speed 16` is explicitly "capped by the frame rate" (autoplay docstring). Battle reports differ run to run (the owner notes flakiness risk via `STUTTER`/`FIRE_BY` PROVISIONAL), AI/RNG-driven outcomes differ, pixels differ.
**Suggestion:** under `--shot`/`PROCYON_REPORT`/`PROCYON_AUTOPLAY` (or a new `--deterministic`) insert `TimeUpdateStrategy::ManualDuration(Duration::from_millis(33))` (one fixed tick per frame, speed-scaled by running k updates... or by setting the manual duration to `33 ms * speed`, which is what `max_frame_step` caps today) and seed RNG from `--seed` (check that all `rand` uses go through one seeded resource; grep for `thread_rng`/`rand::` first). Then "frame N" == "sim tick N" on any machine; speed is limited only by how fast frames render, which with a software renderer is the bottleneck, see R3-06 for skipping rendering.
**Expected gain:** reproducible battle reports (a flaky smoke becomes a real signal), exact-frame screenshots, makes golden images possible (R3-05), makes `battle_check` thresholds tight instead of "PROVISIONAL, tighten later".
**Risks:** Bevy `TimeUpdateStrategy` changes `Time<Real>` too, so wall-clock-based code (Loading `GIVE_UP`, profiling `frame_ms`, `Report.frames`) changes meaning: gate it to batch/shot modes only. Audio/animation advance with virtual time; fine. Open question for maintainers: whether game logic uses an un-seeded RNG.

##### R3-05 [impact: high] [effort: M] Image-diff regression check with on-the-fly baselines, no goldens in Git
**Where:** nothing today: `shot.sh:38-41` only checks non-black; `tests/test_repo_hygiene.py` forbids tracked pictures.
**What:** visual regressions (HUD, FX, ship damage, UI layout) are found only by a human/agent opening PNGs.
**Suggestion (needs R3-04 for exact repeatability; works approximately without it for static UI/menu shots):**
`tools/visdiff.py` (maintainer-approval: Python dependency, see Risks):
1. `tools/visdiff.py base=<merge-base-sha-or-local-binary> [names...]`: builds the merge-base once into a separate
   `CARGO_TARGET_DIR` (`git worktree add local/base <sha>` + build; cached by sha under `local/visdiff/<sha>/openprocyon`),
   or reuses `PROCYON_BASE_BIN`.
2. Render the same scenario list with the baseline binary and the work-tree binary, both via the batch runner (R3-01), into
   `local/visdiff/<sha>/<name>.png` and `local/visdiff/head/<name>.png`. Baselines are cached per (sha, scenario, asset import hash), so
   repeated runs on the same base only render the head side. Goldens live only in `local/` (git-ignored), never in CI artifacts.
3. Compare with NumPy only (already in `requirements-dev.txt`): per-pixel abs diff after a 3x3 blur; report `changed %` of pixels with delta > 16/255 and the
   bounding box of the changed region. SSIM via `scikit-image` or the Rust `image-compare` crate would be nicer but requires approval; start with the NumPy metric which needs no new dependency.
4. Output one line per scenario and write only on change a diff side-by-side to `local/visdiff/diff/<name>.png`:
   `visdiff: damage  CHANGED 3.1% px, bbox x420-690 y300-410 (diff: local/visdiff/diff/damage.png)` and `unchanged` lines folded to one summary.
   Exit non-zero over a threshold (default 0.5% px), but treat it as informative: an intentional visual change is expected to differ.
5. An agent then gets "diff at <path>, bbox" and opens only the one diff image (a 3-panel base | head | diff PNG), instead of viewing 24 pictures.
**Expected gain:** automatic detection of unintended visual side effects of unrelated changes (the project's main failure mode, per battle_check's own header: "one feature quietly changes another"); agents look at 0-2 images rather than all.
**Risks:** lavapipe output is stable per Mesa version/CPU only: baseline and head must come from the same machine within one run (that is why baselines are rendered on the fly). Particles/RNG/time make nearly every battle shot nondeterministic until R3-04; start with scripts that freeze state (`sight`, `damage`, `pause`, `mods`, menu shots, `impacts`, `fire`) and use a large tolerance. New Python dependency (scikit-image/opencv/image-compare) = **maintainer approval**; NumPy+Pillow are already required. Hard rule 1: pictures stay under `local/`, never uploaded; the script must refuse to write outside `local/`.

##### R3-06 [impact: medium] [effort: M] Headless simulation mode for battle_check and autoplay (skip rendering)
**Where:** `battle_check.py` and `autoplay.py` run the full renderer under Xvfb, but read only the log; the only render-dependent data is `frame_ms` stutter.
**What:** `PROCYON_REPORT` needs ECS state, not pixels. Frames cost lavapipe CPU time that dominates "speed 16" runs (effective speed is "capped by the frame rate").
**Suggestion:** a `--headless-sim` run (window hidden, or the render plugins disabled like `bevy::DefaultPlugins` minus `RenderPlugin`/`WinitPlugin` with `ScheduleRunnerPlugin`), driven by `TimeUpdateStrategy::ManualDuration` (R3-04), running updates as fast as the CPU goes. View-layer systems (classic/) are not needed; shared modules are, which matches the layering rule ("views only draw").
**Expected gain:** a battle of 300 game seconds in a few wall seconds (hundreds x real time instead of 16 x capped by lavapipe fps), autoplay of 13 missions from hours to minutes, no Xvfb at all. Because `tools/check.sh` currently runs battle_check last at ~1-2 min, this is the biggest piece for it.
**Risks:** meshes/collision data may be loaded through render-world assets (GLB), requiring the `AssetPlugin` + glTF loader without GPU; some systems depend on `Camera`/`Window` queries (`set_game_speed` takes `Window`; the docs say the `Window` coupling is a known blocker: `docs/development.md:51` "Known blockers... `setup`/`set_game_speed` take the `Window`"). Keep the current windowed path for stutter/FPS measurement. Larger refactor, so after R3-01..04.

##### R3-07 [impact: medium] [effort: S] Many smokes only prove "not black"; add assertions or demote them
**Where:** `tools/smokes.sh:12-35`; scripts with no assertion (examples: `shells.rs:7` "Visual check only; nothing is asserted", `impacts`, `fire`, `animals`, `stormflash`, `allstop` in `smoke/mod.rs`).
**What:** `smokes.sh` reports `ok` for these as long as the process exits 0 with a non-black picture; a panic is already a fail, but wrong behavior is not.
**Suggestion:** label the list in `smokes.sh` with a kind column: `assert` (self-judging; must log `smoke::<name>: PASS`) / `visual` (picture only; the compare in R3-05 is the check). Make the **assert kind mandatory** to print a PASS line: today `smokes.sh:44` accepts exit 0 OR a PASS grep, so an assert-script that exits 0 early without ever reaching its assert (e.g. `run_is_over` fires before frame 130 for `fleet`, `SCRIPT_TAIL` = 100 frames) would show ok. Make assertion scripts also print one final `smoke <name>: PASS (checks: N)`; fail when absent. Add cheap invariants to the visual ones: entity counts from the `PROCYON_PROFILE` line (`particles > 0` for fire/fxtest, `Fx` count for impacts), selected HUD widgets present, `ended` outcome for victory/defeat (`cfg.ended` already exists in `lib.rs:617`).
**Expected gain:** fewer false greens; failures name the missed expectation.
**Risks:** low; some assertions touch Bevy queries that are brittle to wiring changes (that is the point).

##### R3-08 [impact: medium] [effort: S] Make failure output short and actionable for agents
**Where:** `smokes.sh:43-47` prints `FAIL name (log: ...)` only; `shot.sh` prints saved files and timing; `battle_check.py` summaries are good (one line per scenario) but `check.sh` shows nothing of a failed battle other than the final `exit` message.
**Suggestion:** on FAIL print the last 15 lines that match `panicked at|ERROR|WARN|assert|FAIL|PASS` of the log (not the whole log) and the log path; always a final single-line summary with total elapsed, `N passed M failed: names`, and the exact rerun command (`tools/smokes.sh '^fleet$'`). Add `make smoke NAME=fleet` (alias for the regex filter, shown by `make help`). `battle_check.py`: also print the exact rerun command and the worst offender for stutter (log line). Add a `--json` for machine reading? Not needed yet.
**Expected gain:** agent reads 10 lines instead of a 3000-line log; less token cost, fewer reruns.
**Risks:** none.

##### R3-09 [impact: medium] [effort: S] `check.sh` startup smoke is a fixed 10 s sleep and builds a third artifact set
**Where:** `tools/check.sh:19,31-37`: `cargo clippy --release --all-targets` (metadata check artifacts), `cargo test --release -- --include-ignored` (test-profile artifacts at opt-level 3), `cargo build --release` (the bin), then `timeout 10 target/release/openprocyon` (fixed 10 s even when the menu is up after 3).
**What:** per checklist 1.4, `--release` for clippy+test+build means three compile passes, each fully optimised and non-incremental, for every full gate; the startup smoke always costs 10 s plus `battle_check` re-runs `cargo build --release` again (no-op when current).
**Suggestion:** (a) startup smoke exits early: add `PROCYON_QUIT_AFTER_FRAMES=N` (or reuse `--shot` with `--menu main --shot-frame 120`, which already exists and quits itself, plus the black-picture check) instead of `timeout 10`; that is deterministic and typically 3-5 s. (b) fold the startup smoke into the batch (R3-01) as its first scenario. (c) a `--changed` mode (see Change -> smoke map) so agents run only the relevant smokes pre-commit and the full list in CI/merge queue. (d) the build-profile items (single `fast` profile, no `--release` for tests) belong to the build-area report; here only note that `tools/check.sh` is the biggest cost center agents hit and each smoke path inherits the release-build cost.
**Expected gain:** ~5 s per gate from (a); more from (c).
**Risks:** low.

##### R3-10 [impact: medium] [effort: M] Battle invariants are thin and thresholds are guesses
**Where:** `battle_check.py:44-67` (SCENARIOS), `evaluate()`; `smoke/report.rs`.
**What exists:** panic, "engage/fight" (fired, hit, first burst <= 60 s), "patrol" (sailed >= 300), "fire" (FIRE_BY = 240, PROVISIONAL), "budget" (SPEND = 0.6, PROVISIONAL), grinding > 10 s, stutter (>10 frames over 50 ms). 5 scenarios, 4 maps, one fleet (Navy cutter vs pirates). Outcome is measured only at the report timeout; not judged on who won.
**Missing invariants (cheap, from data the report already has or can add):**
- Battle ends: at least one side reaches zero ships within the limit in a fleet-vs-fleet scenario, or at least `n` ships were sunk; a sim that "stalls" (nothing sinks, nobody progresses) currently passes `engage` if a few bursts are fired.
- Conservation/sanity: no NaN/inf position or health (`report` loops over ships); positions inside map bounds; hit points never increase; speed within class max; shell count returns to ~0 at battle end; entity count does not grow unbounded (`PROFILE` already logs `entities`/`particles`: autoplay reports peaks but battle_check does not use them).
- Determinism check: same seed twice gives identical report JSON (needs R3-04); also run the `PROCYON_TRACE` csv hash for "unchanged" in refactors that must not change behavior (see Quick wins).
- Coverage: more maps and mixed-class fleets (frigates, galleons, big ships, tenders, bases), lobby scenario with a `slots=human,7*computer` already exists; add boarding/docking/lifeboat/cloak/storm in-battle scenarios since each smoke checks the feature in isolation but battle_check never combines them.
- Win condition: victory rules (`victory.rs`) end the battle with the expected outcome when a base is killed (`script victory`/`defeat` only take pictures).
**Flakiness risks:** outcomes depend on wall-clock frame rate and un-seeded RNG until R3-04; stutter thresholds depend on machine load (parallel runs, other agents) and false-fail; `timeout=seconds+120` hides slow-hang vs crash (both "no battle report"); the log regex `battle report (\{.*\})` is greedy but single-line, fine.
**Suggestion:** add 3-4 invariants above to `evaluate()`; add a `--seed`; keep a few scenarios where the human fleet has orders (so "fight" checks both sides); record expected ranges only after N real runs (document the measurement in `docs/re`).
**Risks:** each new invariant needs a pass baseline on the real engine first; mark guesses `PROVISIONAL` (hard rule 5).

##### R3-11 [impact: medium] [effort: S] Frame-number semantics are fragile when scripts and pictures disagree
**Where:** `lib.rs:567-635`, `smoke/mod.rs` (`fire`: "Use thresholds: the screenshot counter can advance before or after this system in a frame", `impacts`: `cfg.shot_frame.saturating_sub(12).max(30)`), docs line 80: "a scripted run goes on 100 frames after its last picture, which covers frame 70 (use a later `--shot-frame` ...)".
**What:** scripts time themselves off `cfg.frame` that `screenshot()` increments in `Update` while scripts run in `PreUpdate`/`Update` (ordering varies), `SCRIPT_TAIL` is a global magic number, and a script that asserts after frame `last_shot+100` is silently never judged (documented only in prose). `lifeboat-*` "log PASS and quit before their picture frame" is handled by a grep special case in `smokes.sh:44`.
**Suggestion:** give each script a declared `expected_frames` / completion contract: the harness provides `SmokeResult` resource (Pending/Pass/Fail(String)); scripts set it; `run_is_over` waits for Pass/Fail for scripts that judge themselves (with a hard frame cap = FAIL "never concluded"). Removes the grep special case, `SCRIPT_TAIL` and the "assertion never reached" blind spot.
**Expected gain:** every self-judging smoke ends the moment it has a verdict (faster; `lifeboat-*` already does it by hand) and a smoke that never reaches its assertion fails loudly.
**Risks:** touches all assert-scripts (mechanical).

##### R3-12 [impact: low] [effort: S] Process hygiene in the scripts
**Where:** `smokes.sh:36`: the `timeout 600` per smoke is generous; `shot.sh:22` cds into `game/`; `battle_check.py:196`/`autoplay.py:78` both re-run cargo build; `autoplay.py:20` ignores `CARGO_TARGET_DIR` (battle_check honors it): in a worktree with `CARGO_TARGET_DIR` set to a shared directory autoplay would run a stale or missing binary.
**Suggestion:** share one `tools/game_bin.py` (binary path + build + env + display wrapper) between battle_check, autoplay, a new smokes.py; set `timeout` per smoke from its kind (visual 90 s, lifeboat 300 s) so a hung run fails in 1.5 min instead of 10; kill the whole process group on timeout (xvfb-run children can linger).
**Expected gain:** small but removes real worktree footguns.

#### Proposed smoke architecture

Three tiers, selected by what changed (see map):

1. **Tier 0, seconds, no game process (default pre-commit):** `cargo test` of the module under change (`cargo test --release <module>` or nextest filter), Python tests for tools, ECS tests with `MinimalPlugins` + `ManualDuration` (already the pattern in `fixed_tests.rs`). Extend this: where a smoke currently asserts something that does not need pixels (sight transitions, fleet logic, lifeboat ownership, storm trigger), move/duplicate the assertion into an app-level test that runs the same system set without a window. This is the cheapest way to make "smoke" cheap and is where the largest durable gain is: the smoke then only needs to prove wiring/pixels.
2. **Tier 1, 10-40 s total, one process:** `tools/smokes.py --batch <changed|names>` = one `openprocyon --batch <list>` process, deterministic stepping (R3-04), `--jobs 1..N` splitting the list into N processes (one per Xvfb) for parallelism. Output: one line per scenario `PASS|FAIL|CHANGED`, failures with the 10 relevant log lines, `visdiff` summary against the merge-base binary for scenarios marked visual (cache by sha).
3. **Tier 2, minutes, before push or in the merge queue:** all smokes, `battle_check` in headless-sim mode (R3-06) with more scenarios and seeds, autoplay nightly (`--jobs 2`), full `check.sh`.

Interfaces to build, in order: `tools/game_bin.py` (R3-03) -> `tools/smokes.py` with kinds and parallel jobs (R3-02, R3-07, R3-08) -> `--batch` in the game (R3-01) -> `--deterministic` (R3-04) -> `tools/visdiff.py` (R3-05) -> headless sim (R3-06).

Agent-facing commands (to document in AGENTS.md, replacing the prose lists): `make smoke WHAT=changed` (maps `git diff --name-only <merge-base>` through the table below to scenario names and prints "running: a, b, c"), `make smoke WHAT=fleet`, `make visdiff`, `make battles`.

#### Change -> smoke map

(derived from `docs/development.md:175-200` ownership table and the smokes.sh list; to be encoded in `tools/smokes.py`)

| Changed path | Run |
|---|---|
| `tools/*.py` only | `make test-python` (no game) |
| `docs/`, `README`, `content/` text | `tests/test_repo_hygiene.py` only |
| `game/src/ship.rs`, `scenario/`, `combat/` (gunnery, shells, hit_test, damage), `victory.rs` | `cargo test combat`, then `make battles` (ambush, bayles_stand), `damage`, `target`, `victory`, `defeat`, `fxtest` |
| `ship.rs` AI/steering, `environment.rs`, `asteroid.rs` | `make battles` (all 5; grinding/patrol invariants), `course`, `sight` |
| `fx/`, `render/`, `nebula/`, `cloak.rs`, `classic/` drawing | visual: `fxtest`, `fire`, `impacts`, `damage`, `stormflash`, `current-m3`, `animals`, then `visdiff` |
| `panels.rs`, `menu*.rs`, `options_menu.rs`, `classic/` UI | `pause`, `pause-restart`, `mods`, `ingame*`, `menu-*`, `fleet*`, `briefing-skip` + startup smoke |
| `fleet`/`rules/skirmish`, lobby model, `menu/lobby_scenario.rs` | `fleet`, `fleet-*`, `make battles border_dispute_full` |
| `docking.rs`, `boarding.rs` | `dock`, `dock-click`, `dock-menu`, `board` |
| `campaign.rs`, `rules/` (evaluator/systems/console), `lifeboats.rs`, `briefing.rs` | `cargo test rules`, `lifeboat-*`, `fleet-primary-m3`, `storm-m3`, `hunt-m3`, `console`, `briefing-skip`, then `autoplay <affected mission>` |
| `loading.rs`, `lib.rs` wiring, `smoke/`, system ordering, plugin/resource additions | startup smoke + whole `make smokes` (Bevy B0001 conflicts only appear at run time) |
| `tools/export_*.py` (assets) | re-import to scratch + `compare_assets`, then smokes for the touched asset class (ships -> `damage`, hud -> `fleet`/`pause`, effects -> `fxtest`, maps -> `battles`) |

#### Quick wins

1. `smokes.sh`/`shot.sh`: build once, run the binary (`PROCYON_BIN`/`--no-build`): saves 1-2 s x 24 (R3-03). S.
2. Add `--jobs` to smokes/battle_check (R3-02); set `LP_NUM_THREADS` per run. S-M. Re-check the stutter invariant under parallelism.
3. Replace the 10 s startup `timeout` with `--menu main --shot-frame 120` (self-quitting), or a `PROCYON_QUIT_FRAMES` env (R3-09). S.
4. `smokes.sh` kind column + mandatory PASS line for asserting scripts; failure prints only the last matching log lines + rerun command (R3-07, R3-08). S.
5. A written Change -> smoke map in AGENTS.md (table above) plus a `make smoke WHAT=changed` that only prints the plan first. S.
6. `PROCYON_TRACE` doubles as a determinism and refactor check today: for pure refactors, run a fixed battle with `PROCYON_TRACE` before/after (with R3-04, the CSVs must be identical); a 10-line `tools/trace_diff.py` prints "first divergence at gt=..., ship=..., field=...". No new dependency, no game content stored (CSV stays in `local/`). S.
7. `autoplay.py:20` honor `CARGO_TARGET_DIR` like `battle_check.py:21-24` (shared helper). S.
8. Keep new dependencies out of the first iteration: NumPy+Pillow suffice for the first visdiff (SSIM crates/libs only after maintainer approval).

---

## Appendix: Worktrees, disk and the AI-agent workflow

#### Assessment

Facts observed (read-only; this sandbox has one checkout, no `assets/`, no `.venv`, 4 cores, rustc 1.100.0):
- No `.claude/`, no `.cargo/config.toml`, no `[profile.*]` beyond `dev` (opt 1, deps 3) and `release` (strip). Nothing in the repo uses the `dev` profile: gate, shot.sh, battle_check, autoplay, smokes and `make run` all use `--release`. So every edit rebuilds the 67k-line crate at opt-level 3 with incremental off, and `clippy --release --all-targets` plus `cargo test --release` plus `cargo build --release` are three artifact sets (check-metadata, test bins, game bin) in one tree.
- `game/target` here is 2.3 GB for a partly built release tree (deps only so far). A fully warmed tree (release build + release test binaries + clippy-release metadata, Bevy 0.20 deps) is realistically 6-9 GB; adding a dev tree would add 3-6 GB with debuginfo (not measured; estimate from checklist 3.1). So N worktrees = N x ~7 GB, which is the "disk explodes".
- A fresh worktree has no `game/target`: first useful feedback = full Bevy dependency build in release. Estimated 4-8 min on 4 cores for `cargo check`-class feedback, 8-15 min to first `check.sh` (clippy build, test build, game build serially, each recompiling the crate at opt 3). I did not measure it (a timing run is in progress elsewhere).
- Assets: not duplicated by git (`/assets/` ignored), but NOT shared automatically either. Only `tools/shot.sh` falls back to the main checkout's `assets/` (via `git --git-common-dir`, exports `PROCYON_ASSETS`). `tests/imported.py`, `game/src/lib.rs::base_assets()`, `tools/check.sh`, `battle_check.py`, `autoplay.py`, `smokes.sh` and `make run` honour only `PROCYON_ASSETS`, the checkout's `assets/`, then `~/.local/share/openprocyon/assets`. In a fresh worktree `check.sh` therefore computes `imported=0`, silently skips ignored asset tests, startup smoke and battle check, and ends with only a NOTE line. An agent can believe the gate passed. Or it re-imports (needs the game install, minutes, a full duplicate of assets).
- `local/` is per worktree and git-ignored; here it is 400 KB, but screenshots (1280x720 PNG, ~1-2 MB each), `local/battle_check/*.log` (one log per scenario per run, overwritten by name) and `local/overlay` grow with use. Not a primary cost.
- The user log dir (`~/.local/share/openprocyon/logs/openprocyon.log` + `.previous.log`, up to 2 x 20 MB) is shared by all worktrees and all parallel runs: bounded (~40 MB), but concurrent smokes clobber each other's log, which hurts diagnosis more than disk. Same for `settings.json` and campaign saves (docs already say to use a scratch `XDG_DATA_HOME` for autoplay).
- Importer staging (`assets.importing`, `assets.replaced`) is ignored and transient but doubles assets size briefly; only matters if someone imports per worktree.
- Instructions: AGENTS.md says "Run it [check.sh] before every commit", development.md step 4 says tests while iterating then `make check`. There is a `--fast` mode, but it still runs `clippy --release --all-targets` and the full release test run with asset tests, and `--fast` is described as a fallback, not the default. No "check what I changed", no single-test command, no per-module smoke map. The `docs/development.md` loop section is one very long paragraph of console/battle-check detail, hard for an agent to extract commands from. AGENTS.md has the rule "do not add parallel wrappers", which constrains how to add tiers (see Risks below): tiers should be flags of `tools/check.sh`, not new scripts.
- CI already sets `CARGO_PROFILE_RELEASE_OPT_LEVEL=0`, `CARGO_INCREMENTAL=0`, `debug=false`: CI proves the gate works at opt 0. Locally the opposite is paid.

#### Findings

##### R4-01 [impact: high] [effort: S] Inner loop and gate compile at opt-level 3 with incremental off
**Where:** `tools/check.sh` (clippy/test/build all `--release`), `game/Cargo.toml` profiles, `tools/shot.sh`, `battle_check.py`, `autoplay.py`, `smokes.sh`, Makefile `run`.
**What:** Every cargo invocation of a developer or agent is `--release`. Release has incremental off and opt 3, so any one-line edit recompiles the whole 67k-line crate optimised, three times (clippy, test, build) per gate. The `dev` profile (opt 1, deps opt 3) is defined but unused.
**Why:** Biggest single lever; checklist 1.4/1.5. Only the run-the-game tools need speed; compile/lint/unit tests do not.
**Suggestion:** (a) Tier 0 compile question = `cargo check` (dev, shares nothing with release, small). (b) Lint = `cargo clippy` in dev profile (shares deps with check). (c) Unit tests = `cargo test` in dev (deps already opt 3 via the existing profile; add `debug = "line-tables-only"`). (d) Keep ONE fast profile for anything that runs the game: `[profile.fast] inherits="release" incremental=true codegen-units=256 debug="line-tables-only" strip=false`, used by shot.sh, battle_check, smokes, autoplay (they would pass `--profile fast`, binary at `target/fast/openprocyon`, so also update the `battle_check.py` and `autoplay.py` binary path). Keep `--release` only for `package_release`/release workflow. Verify sim tests that rely on timing/float behaviour are identical at opt 1 vs 3 (floating point is the same in Rust without fast-math; run the suite once both ways).
**Expected gain:** edit-to-compile-error from "full release crate rebuild" (tens of seconds to minutes) to a few seconds for check, ~10-20 s for incremental clippy/test; disk drops about 30-40% (no clippy-release set, line-tables-only). Needs measurement by the timing run.
**Risks:** dev vs release behavioural differences (overflow checks and debug_assert fire in dev: arguably good, but may expose existing assumptions); smokes at opt 1 may run slower than at opt 3 (hence `fast` with opt 3). Changing profiles in `game/Cargo.toml` is not a dependency or data-format change, but flag it to a maintainer; and the CI env vars `CARGO_PROFILE_RELEASE_*` would need the equivalents.

##### R4-02 [impact: high] [effort: S] Assets are silently not found in linked worktrees (gate "passes" while skipping)
**Where:** `tools/shot.sh` lines 8-13 (only place with fallback); `tests/imported.py::_find`; `tools/check.sh` (`imported=` probe); `game/src/lib.rs::base_assets`; `battle_check.py`, `autoplay.py`, `smokes.sh`.
**What:** Only shot.sh resolves the main checkout's assets. Everything else needs `PROCYON_ASSETS` set by hand or a per-user import.
**Why:** An agent in a worktree runs `make check`, gets green with asset tests, startup smoke and battle check skipped (only a NOTE on stderr). Or re-imports into the worktree (duplicate GBs and minutes).
**Suggestion:** One resolution, in one place. Cheapest and zero config: make the importer's per-user folder the dev default too: import once with `python3 -m tools.import_assets --game ...` to `$XDG_DATA_HOME/openprocyon/assets`; `base_assets()` and `imported.py` already look there, so every worktree works with no variables. Check that `player_assets()`/`setup::imported` version check does not force the import screen for a dev build (if it does, that is the reason to prefer option b). Option b: move shot.sh's git-common-dir logic into a tiny sourced snippet used by `check.sh`, `Makefile` (`export PROCYON_ASSETS ?=`) and `tests/imported.py` (the Python function can run `git rev-parse --git-common-dir`). Also make check.sh print `ASSETS: <path>` or `ASSETS: NONE, asset tests, startup smoke, battle check SKIPPED` as the first line and, with `--require-assets`, fail.
**Expected gain:** removes silent partial gates; no per-worktree assets (saves the full size of assets/, I could not measure it here, expect hundreds of MB to a few GB, since models, PNG and OGG for 13 missions).
**Risks:** a mod/exporter change tested by one agent re-importing into the shared dir would affect every worktree: exporter work must use `--out local/assets-x` plus `PROCYON_ASSETS` (AGENTS.md already says "scratch folder"). Assets are read-only in normal use; the game writes no file into assets/ (confirm: `modding::overlay` builds in `local/overlay` or temp, and autoplay notes "each run builds its own asset overlay" so parallel runs need distinct dirs).

##### R4-03 [impact: high] [effort: S] No worktree lifecycle: cold target per worktree, no cleanup
**Where:** repo-wide (no `.claude/`, no script); `.gitignore` has `/game/target/` only.
**What:** Each worktree builds Bevy deps from scratch (4-8 min) and keeps 6-9 GB forever. Removing a worktree (`git worktree remove`) deletes `game/target` only if the agent/harness does; leaked branches, pruned worktrees and `ccr-*` dirs leave orphans.
**Why:** The "disk explodes" cause. Checklist 2.4, 3.4.
**Suggestion:** see "Recommended worktree setup": a single `tools/worktree.sh {new,rm,gc}` is a lifecycle helper rather than a parallel check wrapper, but it is a new script, so ask the maintainer (AGENTS.md "no parallel wrappers" targets validation). Alternative with zero new scripts: `.claude/settings.json` WorktreeCreate/WorktreeRemove hooks calling inline commands.
**Expected gain:** new worktree first feedback from ~5-8 min to ~30-90 s (reflink warm copy of deps; or shared deps via build-dir), disk per worktree from ~7 GB toward 1-3 GB (see tables).
**Risks:** warm copy of a stale base target makes the first build recompile changed crates only (fine). `cp --reflink` is only cheap on btrfs/XFS/APFS; on ext4 it is a full copy (still faster than building, and disk equal to today). Windows: no reflink; skip the warm step.

##### R4-04 [impact: medium] [effort: S] Shared CARGO_TARGET_DIR is the wrong default on locks; a shared dep cache is not available on 1.97
**Where:** `battle_check.py` honours `CARGO_TARGET_DIR` (good), but `autoplay.py` hardcodes `game/target/release/openprocyon`, so a shared or relocated target dir breaks it; shot.sh uses `cargo run` so it follows the env.
**What:** With one shared `CARGO_TARGET_DIR`, cargo takes one file lock per `target/<profile>`: parallel agents queue ("Blocking waiting for file lock"), final binaries overwrite each other (agent A's smoke runs agent B's binary: wrong results that look like regressions), and on < 1.100 shared build dirs had hash/layout bugs.
**Why:** Rust minimum is 1.97.1 in `Cargo.toml` but 1.100 is current stable and installed here.
**Suggestion:** Do not share the whole target dir. Per worktree `target`, optionally on 1.100+ split with `build.build-dir`: shared deps/intermediates only if lock contention is measured to be acceptable (agents mostly edit, not build, simultaneously; but lock is held for the whole `cargo` call including test run). Recommended: per-worktree target + reflink warm (R4-03). Fix `autoplay.py` to use the same `CARGO_TARGET_DIR`/profile logic as battle_check (small shared helper in `tools/`).
**Expected gain:** correct results under parallel agents; no cross-agent binary clobber.
**Risks:** if the maintainer wants a hard 1.97 floor in CI/MSRV, build-dir features beyond 1.91 stable are fine (build-dir stable since 1.91); layout v2 only on 1.100: make shared build-dir opt-in for developers (`~/.cargo/config.toml`), not committed.

##### R4-05 [impact: medium] [effort: S] sccache gives little here
**Where:** n/a.
**What:** sccache caches registry dependencies compiled with the same flags/paths (SCCACHE_BASEDIRS). It does not cache the workspace crate (incremental/bin) nor proc-macros/build scripts/dylibs.
**Why:** The cost that remains after warm copy is the 67k-line `openprocyon` crate and test/game links, which sccache cannot help; the deps are already covered by reflink copy.
**Suggestion:** Skip sccache for local worktrees. Consider it only for non-reflink filesystems (ext4/Windows) as an opt-in in `~/.cargo/config.toml` (`build.rustc-wrapper="sccache"`), which also needs `CARGO_INCREMENTAL=0` for the cached crates. Not a repo change.
**Expected gain:** deps cold build 5 min -> ~1 min on ext4 hosts with a primed cache. Small relevance on btrfs/XFS.
**Risks:** extra tool; cache invalidates with rustc upgrades (1.97 vs 1.100 contributors have separate entries).

##### R4-06 [impact: high] [effort: S] Instructions push the full gate too often; no cheap commands exist for "what I changed"
**Where:** `AGENTS.md` Workflow ("Run it before every commit"), `docs/development.md` step 4-5, `CONTRIBUTING.md` PR step 2, `Makefile` help.
**What:** The only tiers are `check` and `check --fast`; `--fast` still runs clippy release `--all-targets` and all tests including ignored (all test code in one crate = one test binary compile, then every test). No path to run one test, one module's tests, or "ruff on changed Python only". Commit-time gate = ~5-10 min plus the 1-2 min battle check in full mode. Agents that commit often (and run the gate each time) burn most wall-clock there.
**Why:** Checklist 7.1/7.3: one cheap command per question, full gate once before push.
**Suggestion:** Document three tiers and make them Make targets (Makefile targets are not "parallel wrappers" in the sense of AGENTS.md, but ask: they must call `tools/check.sh` flags, not reimplement):
- `make quick` = `tools/check.sh --quick`: `cargo check --message-format short` + `ruff` on changed files + `cargo fmt --check` + hygiene python test only. Seconds.
- `make t T=<filter>` = `cargo test <filter>` (dev profile) with `--include-ignored` if assets resolve; prints only failures.
- `make check` (`--fast` today) before commit; `make check` full before push/PR, or leave full to CI+merge queue and `make smokes` only for modules touched (map below).
- Path-to-smoke map in docs (e.g. `game/src/classic/** -> make shot ...`, `boarding* -> script ...`) so agents run one smoke, not all (`tools/smokes.sh` already accepts a filter name: `make smokes ARGS=fleet`).
**Expected gain:** typical edit cycle from minutes to seconds; the full gate once per PR.
**Risks:** AGENTS.md says `check.sh` is the single validation gate and "do not add parallel wrappers": adding `--quick` to the same script keeps that true, but wording should change from "before every commit" to "tiers"; maintainer approval needed for the policy edit.

##### R4-07 [impact: medium] [effort: S] No `.claude/` project config (hooks, skills, permissions)
**Where:** repo root (absent).
**What:** No shared hooks, skill or permission allowlist; each agent rediscovers commands from a 30-line paragraph in development.md. Fresh sessions prompt for routine commands (cargo, tools/*.sh).
**Why:** Checklist 7.6.
**Suggestion:** Commit `.claude/settings.json` (shared, no secrets; hard rule 8 satisfied) with: permission allowlist for `cargo check/test/clippy/fmt`, `tools/*.sh`, `make *`, `python3 -m tools.*`, `git worktree *`; PostToolUse rustfmt/ruff on edited file; Stop hook running `tools/check.sh --quick` only; WorktreeCreate warm; plus `.claude/skills/procyon-loop/SKILL.md` (sketch below). `.claude/` is not game content, but it is a new tracked directory: ask a maintainer (hard-rule-adjacent: only tooling config). GEMINI.md/CLAUDE.md point to AGENTS.md; keep the single source of truth there and have the skill link to it rather than duplicate.
**Expected gain:** fewer prompts; format drift never reaches the gate; agents pick the cheap command first.
**Risks:** hooks must be fast and non-fatal; a Stop hook that fails blocks the agent forever, so use `exit 0` with a message unless the quick tier fails, and cap runtime.

##### R4-08 [impact: medium] [effort: S] Parallel runs share one user dir (log, settings, saves) and one `local/overlay`-style scratch
**Where:** `game/src/logging.rs` (user dir logs), `campaign::user_dir`, `docs/development.md` (scratch XDG advice), `autoplay.py`, `battle_check.py` (`local/battle_check/<name>.log`).
**What:** Disk-bounded (~40 MB) but parallel agents overwrite each other's `openprocyon.log` and settings; battle_check logs overwrite by name per worktree (fine, worktree-local).
**Suggestion:** `tools/shot.sh`, `battle_check`, `autoplay`, `smokes` should default `XDG_DATA_HOME` to `local/xdg` per worktree (note: this breaks the "assets in per-user folder" lookup of R4-02 option a; so set `PROCYON_ASSETS` explicitly and keep XDG scratch, or give assets a separate env). Print the log path in the failure summary.
**Expected gain:** correct logs for the right worktree; players' saves not touched by agents.
**Risks:** interacts with R4-02; decide both together. Windows `%APPDATA%` equivalent is different.

##### R4-09 [impact: medium] [effort: M] Test layout: one lib test binary compiled in release, asset tests mixed in
**Where:** `game/src` (66,820 lines; no `game/tests/`; tests in-crate), `check.sh`.
**What:** One test binary links Bevy; a test change relinks it. Full test run executes all tests; selection by name is possible but undocumented. cargo-nextest not used (adding it = dependency of tooling: ask).
**Suggestion:** document `cargo test <module>::tests`; optional nextest (process-per-test good for Bevy statics; test-groups to throttle GPU smokes). Python-only changes skip Rust (`git diff --name-only` map in `--quick`).
**Expected gain:** seconds per targeted test; 1.4-3.4x faster full test execution with nextest.
**Risks:** nextest has no doctests; adds a dev tool dependency (maintainer approval).

##### R4-10 [impact: low] [effort: S] `.venv` per worktree
**Where:** AGENTS.md setup ("a `.venv`"), Makefile/check.sh use the checkout's `.venv` when present.
**What:** Pillow + NumPy + ruff is ~100-150 MB per venv and 30-60 s to create. Not the problem; but a missing venv makes check.sh fail with "ruff not found".
**Suggestion:** Let check.sh/Makefile fall back to a venv in the main checkout (same git-common-dir trick) or to `~/.cache/openprocyon/venv`; or `uv`/`ruff` on PATH. Symlinking a venv is fragile (absolute shebangs); prefer fallback lookup.
**Expected gain:** a few GB across many worktrees, 1 min per new worktree.
**Risks:** low.

##### R4-11 [impact: low] [effort: S] `.gitignore` and stale-dir hygiene
**Where:** `.gitignore`.
**What:** Ignores `/assets/`, `/local/`, `/game/target/`, `.venv/`, `assets.importing`, `assets.replaced`. Missing: `/.claude/settings.local.json`, `/.claude/worktrees/` (if the harness puts worktrees inside the repo), `/target/` if a root workspace is ever added, `local/` is covered. Nothing prunes `local/`.
**Suggestion:** add those entries; `gc` script prunes `local/` PNGs older than 14 days.
**Risks:** none.

#### Recommended worktree setup

Decision table (disk is per concurrent worktree, estimates, not measured):

| Strategy | Disk / worktree | First feedback | Contention | Verdict |
|---|---|---|---|---|
| Today (cold, `--release` x3 trees) | 6-9 GB | 5-8 min check, 10-15 min gate | none | baseline |
| Shared CARGO_TARGET_DIR | ~0 extra | ~0 (warm) | file lock per profile; binaries clobbered | no |
| Per-worktree target, reflink warm (btrfs/XFS) | 0.5-1 GB real after divergence (shared extents) | 30-90 s | none | recommended |
| Same, ext4/Windows (full copy) | 3-4 GB with trimmed profiles | 1-2 min (copy) | none | acceptable |
| `build.build-dir` shared across worktrees (1.100+) | ~0.3 GB + shared | 30-60 s | lock only while building | later experiment |
| sccache | deps cache only | ~1-2 min | none | low value |

Profile trimming (R4-01) shrinks every row by ~30-40%.

Proposed `game/Cargo.toml` additions (maintainer decision):
```toml
[profile.dev]
opt-level = 1
debug = "line-tables-only"        # keep incremental default

[profile.dev.package."*"]
opt-level = 3
debug = false

# The one profile for anything that runs the game (smokes, battle check, autoplay, screenshots).
[profile.fast]
inherits = "release"
incremental = true
codegen-units = 256
debug = "line-tables-only"
strip = false
```
Plus optionally `game/.cargo/config.toml` (committed, no linker!):
```toml
[build]
# warnings as errors without invalidating the cache (Rust 1.97+); set per run: CARGO_BUILD_WARNINGS=deny
```
Developers add `mold`/`wild` and `bevy/dynamic_linking` locally in `~/.cargo/config.toml` or via a dev-only feature `dev = ["bevy/dynamic_linking"]` passed by Make (`cargo check --features dev`): dynamic linking also shrinks the test binary link time; never in release or CI.

`tools/worktree.sh` (new file; needs maintainer approval because of the "no parallel wrappers" rule: it is lifecycle, not validation, and calls `tools/check.sh` for nothing):
```sh
#!/bin/sh
# usage: tools/worktree.sh new <branch> [dir] | rm <dir> | gc
set -e
main=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
case "$1" in
new)
  dir=${3:-$main/../openprocyon-wt-$(echo "$2" | tr / -)}
  git -C "$main" worktree add -b "$2" "$dir" origin/main 2>/dev/null || git -C "$main" worktree add "$dir" "$2"
  if [ -d "$main/game/target" ]; then
    # reflink = near-free on btrfs/XFS; -a keeps mtimes so deps stay fresh; incremental state is not reusable across paths
    cp -a --reflink=auto "$main/game/target" "$dir/game/target" 2>/dev/null || true
    rm -rf "$dir"/game/target/*/incremental
  fi
  # assets: shared via PROCYON_ASSETS fallback (R4-02), venv: shared lookup (R4-10)
  echo "ready: $dir  (cd there; make quick)" ;;
rm)
  rm -rf "$2/game/target"                      # git worktree remove leaves ignored files: delete the big dir first
  git worktree remove --force "$2" && git worktree prune ;;
gc)
  git worktree prune
  # stale: worktrees whose last commit/checkout is older than 14 days and clean
  git worktree list --porcelain | awk '/^worktree /{print $2}' | while read -r w; do
    [ "$w" = "$main" ] && continue
    [ -d "$w/game/target" ] && [ -n "$(find "$w" -maxdepth 1 -name .git -mtime +14)" ] && echo "stale: $w ($(du -sh "$w/game/target" | cut -f1))"
  done
  find "$main/../" -maxdepth 3 -path '*/game/target/*/incremental' -mtime +7 -prune -exec rm -rf {} + 2>/dev/null || true
  find "$main/local" "$main"/../openprocyon-wt-*/local -name '*.png' -mtime +14 -delete 2>/dev/null || true ;;
esac
```
The base `$main/game/target` should be built with the same profiles/toolchain agents use (`cargo check`, `cargo test --no-run`, `cargo build --profile fast`). Keep the main checkout as the "warm donor" and refresh it after merges (a nightly `cargo build --profile fast` there).

Expected per worktree with this setup (btrfs/XFS): create 5-20 s, first `make quick` 20-60 s (only crate rebuild), first `make check` ~2-4 min, disk ~0.5-1.5 GB divergence (shared extents until written; `du` reports the apparent size, use `btrfs fi du`/`compsize`). On ext4: 3-4 GB, copy 20-60 s.

Where `rm` is not available: Claude Code `WorktreeRemove` only fires for hook-created worktrees, so the harness-created `.claude/worktrees/*` need the `gc` script or a `SessionEnd`-style cleanup; a cron of `tools/worktree.sh gc` on the developer machine covers the rest.

#### Recommended agent instructions

Proposed AGENTS.md replacement for the Workflow gate bullet (maintainer approval needed; keeps single gate):

> - `tools/check.sh` (`make check`) is the single validation gate, with three speeds. While editing: `make quick` (`--quick`: `cargo check`, rustfmt, ruff on changed files, hygiene; seconds) and `make t T=<test name or module>` for the one test you are changing. Before a commit: `tools/check.sh --fast` once (clippy in the dev profile, all Rust and Python tests). Before opening or updating a PR, and after touching ECS wiring, input or UI: the full `tools/check.sh` plus the one smoke for what you changed (`make smokes ARGS=<name>`, map in docs/development.md). Never rerun the whole gate after each edit and never run cargo with `--release` for checks; rerun only the step that failed. Do not `cargo clean` (use `tools/worktree.sh gc`). Read the printed `ASSETS:` line: a gate that says assets were not found has not run the asset tests.
> - Work in a git worktree (`tools/worktree.sh new <branch>`), not by switching branches; remove it with `tools/worktree.sh rm` when the PR is merged. Builds are per worktree; assets, venv and the cargo registry are shared.

Proposed `.claude/skills/procyon-loop/SKILL.md` (skeleton):
```
---
name: procyon-loop
description: Fast edit-check-test-smoke loop for OpenProcyon. Use before running any cargo, check.sh or smoke command in this repo.
---
1. Compile question: `make quick`. Show only the first error with file:line.
2. Test I changed: `make t T=<module::tests::name>`; add --include-ignored only when ASSETS: is found.
3. Before commit: `tools/check.sh --fast`. Full gate only before PR.
4. Smoke for module -> command: (table: combat/ship/scenario -> make battles ARGS=<name>; classic/*, panels, menu -> make shot with the matching --script; rules/campaign -> make smokes ARGS=<name>; tools/*.py -> make test-python).
5. Never --release for checks, never cargo clean, never import assets into a worktree; logs live in local/, summarise failures (file:line + 5 lines), do not paste whole logs.
```
Also a `.claude/commands/` file is unnecessary; a skill is enough.

`.claude/settings.json` sketch (committed; no personal paths):
```json
{
  "permissions": { "allow": ["Bash(cargo check:*)", "Bash(cargo test:*)", "Bash(cargo clippy:*)", "Bash(cargo fmt:*)",
    "Bash(make quick)", "Bash(make t:*)", "Bash(make check:*)", "Bash(tools/check.sh:*)", "Bash(tools/shot.sh:*)",
    "Bash(tools/worktree.sh:*)", "Bash(git worktree:*)", "Bash(python3 -m tools.*)"] },
  "hooks": {
    "PostToolUse": [{ "matcher": "Edit|Write", "hooks": [{ "type": "command",
      "command": "f=$(jq -r .tool_input.file_path); case \"$f\" in *.rs) rustfmt --edition 2024 \"$f\" ;; *.py) ruff format \"$f\" ;; esac; exit 0" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "tools/check.sh --quick 2>&1 | tail -20; exit 0", "timeout": 120 }] }],
    "WorktreeCreate": [{ "hooks": [{ "type": "command", "command": "tools/worktree.sh warm \"$(jq -r .path)\"" }] }]
  }
}
```
(The rustfmt hook must honour `game/rustfmt.toml`; run `rustfmt --config-path game` rather than the bare invocation. WorktreeCreate hooks replace the default worktree creation; verify the hook contract before relying on it. Stop hook exit 0 so it informs instead of blocks.)

#### Quick wins

1. Make asset discovery one thing: either import once into the per-user folder or share shot.sh's git-common-dir fallback with check.sh, Makefile, tests/imported.py; print an `ASSETS:` line in check.sh (R4-02). Removes the silent skipped-gate trap.
2. Reword AGENTS.md/development.md: full gate once per PR, `--fast` per commit, `cargo check` and single tests while editing; list the one-liners at the top of development.md rather than in the long paragraph (R4-06).
3. Delete `target/` on worktree removal and run a periodic `gc` (R4-03); reflink warm copy of the main target when creating one.
4. Add `debug = "line-tables-only"` to dev and a `fast` profile; switch clippy/test to dev and smokes/battles to `fast` (R4-01), after one run proving results identical.
5. Fix `autoplay.py` to honour `CARGO_TARGET_DIR` like battle_check; give parallel runs separate `XDG_DATA_HOME` scratch (R4-04, R4-08).
6. Add `.claude/skills/procyon-loop` and a permission allowlist (R4-07); PostToolUse formatter hook.

Needs maintainer approval: any edit to `game/Cargo.toml` profiles, new `.claude/` and `tools/worktree.sh`, AGENTS.md policy wording, nextest/sccache/mold as documented dependencies, `check.sh` flag semantics (`--quick`). Within AGENTS.md "do not add parallel wrappers": implement tiers as flags/targets of `tools/check.sh` and the Makefile, not new gate scripts; `worktree.sh` is lifecycle only.
