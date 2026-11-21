# Contributing

Thanks for helping. OpenProcyon only stays shareable if every contribution respects the hard rules in [AGENTS.md](AGENTS.md):
no game content, nothing taken from the game's executable (code, addresses, or how to analyse it), and behaviour written
in your own words. A pull request that breaks them is closed, however good the rest is.

## Ways to help

- **Report retail behaviour.** Play the original game and write down what you see as rules and numbers: speeds, ranges, timings,
  which ship does what and when. Items marked `PROVISIONAL` in `docs/re/*.md` and in the code, and issues labelled *needs retail measurement*, list what we still guess. Give the source (mission or skirmish
  map, difficulty, ship, what you did) so someone else can repeat it. Keep your videos and screenshots to yourself: describe
  them, don't upload them.
- **Play-test campaign missions 2-13.** They load but are largely untested ([docs/re/campaign.md](docs/re/campaign.md)).
  Report where the remake differs from the original: triggers, objectives, enemy behaviour, crashes.
- **Use your own saves.** Your retail save files can be read with this project's tools in `tools/`. Report what you learn
  from them as rules and numbers; never commit the files.
- **Make a mod.** Ships, maps, missions, sounds and art are files in a mod folder of your own, no engine change needed:
  [docs/modding.md](docs/modding.md), "Sharing a mod". Open an issue when a mod needs something the engine cannot do yet.
- **Code.** Pick a backlog row or issue, add the behaviour to the module that owns it ([docs/development.md](docs/development.md)),
  and mark anything you have not checked against the original `PROVISIONAL`.

## Pull requests

1. Branch off `main`, one topic per pull request.
2. Run `tools/check.sh` (format, Clippy, Rust and Python tests, hygiene rules). It must pass. Without imported assets the Rust
   tests only compile; CI runs the same way.
3. Non-trivial changes need a regression test. Changes to ECS systems or the UI also need a smoke run (`make smokes`, or
   `tools/shot.sh` with the matching `--script`).
4. Spec changes go into `docs/re/*.md`, citing what they come from (the retail data files, the manual, footage, play).

The full workflow, and what to ask a maintainer before doing (new dependencies, changed asset paths or JSON layouts, files in
`content/`), is in [AGENTS.md](AGENTS.md); module ownership in [docs/development.md](docs/development.md).
