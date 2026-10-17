# Contributing

Thanks for helping. OpenProcyon only stays shareable if every contribution respects the hard rules in [AGENTS.md](AGENTS.md):
no game content, nothing taken from the game's executable (code, addresses, or how to analyse it), and behaviour written
in your own words. A pull request that breaks them is closed, however good the rest is.

## Ways to help

- **Report retail behaviour.** Play the original game and write down what you see as rules and numbers: speeds, ranges, timings,
  which ship does what and when. Rows marked `PROVISIONAL` in [docs/fidelity-backlog.md](docs/fidelity-backlog.md) and in
  `docs/re/*.md`, and issues labelled *needs retail measurement*, list what we still guess. Give the source (mission or skirmish
  map, difficulty, ship, what you did) so someone else can repeat it. Keep your videos and screenshots to yourself: describe
  them, don't upload them.
- **Play-test campaign missions 2-13.** They load but are largely untested ([docs/re/campaign.md](docs/re/campaign.md)).
  Report where the remake differs from the original: triggers, objectives, enemy behaviour, crashes.
- **Use your own saves.** Your retail save files can be read with this project's tools in `tools/`. Report what you learn
  from them as rules and numbers; never commit the files.
- **Code.** Pick a backlog row or issue, add the behaviour to the module that owns it ([docs/development.md](docs/development.md)),
  and mark anything you have not checked against the original `PROVISIONAL`.

## Pull requests

1. Branch off `main`, one topic per pull request.
2. Run `tools/check.sh` (format, Clippy, Rust and Python tests, hygiene rules). It must pass. Without imported assets the Rust
   tests only compile; CI runs the same way.
3. Non-trivial changes need a regression test. Changes to ECS systems or the UI also need a smoke run (`make smokes`, or
   `tools/shot.sh` with the matching `--script`).
4. Spec changes go into `docs/re/*.md`, citing what they come from (the retail data files, the manual, footage, play).
5. Ask before adding dependencies, changing asset paths or JSON layouts (mods and saves depend on them), or adding files to
   `content/` (they need a redistributable licence, recorded next to the file).
