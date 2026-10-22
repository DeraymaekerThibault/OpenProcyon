# OpenProcyon

An unofficial, open-source engine for *Treasure Planet: Battle at Procyon* (2002), aiming to recreate the original game
faithfully on modern systems.

**You need your own copy of the original game.** OpenProcyon contains no game content: an importer converts the files from
your installation into a local asset folder, and the engine runs on those. This project is not affiliated with or endorsed by
Disney or the original developers.

## Status

Early development. Skirmish battles on the retail maps work; all 13 campaign missions load, Mission 1 is the current focus and
missions 2-13 are first-pass. See `docs/fidelity-backlog.md`.

## Getting started

Requirements: Rust 1.97.1 or newer, Python 3 with `pip install -r requirements.txt` (Pillow, NumPy), and on Linux the usual
Bevy system packages (ALSA, udev, Wayland/X11, Vulkan).

```sh
# 1. Convert your copy of the game (read-only; takes a minute or two). Works without bash, e.g. from PowerShell.
python3 -m tools.import_assets --game "/path/to/Treasure Planet Battle at Procyon"
# 2. Play
make run                      # main menu
make run ARGS='--mission 1'   # straight into Mission 1
```

The import goes to `assets/` in this checkout. Pass `--out <dir>` to put it elsewhere; the game also looks in
`~/.local/share/openprocyon/assets` (Windows: `%APPDATA%\openprocyon\assets`) or `PROCYON_ASSETS`.
Installs with community mods can be imported too.

## How to help

You don't need to write code. Playing the original and reporting exact behaviour (speeds, ranges, timings, what the computer
captains do) settles the guesses still marked `PROVISIONAL`, and missions 2-13 need play-testing. See
[CONTRIBUTING.md](CONTRIBUTING.md) for what to report and how.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md) first: it is the guide for human contributors and AI coding assistants alike, and its hard rules
(no game content, no decompiled code or executable addresses, behaviour described in your own words) are not negotiable.
`tools/check.sh` must pass before every commit. Architecture and module ownership: [docs/development.md](docs/development.md).
Modding: [docs/modding.md](docs/modding.md).

## Credits

Crew voice-line grouping derived from the community CL ModPack's voice-line layout (event and file names only).
Fonts: URW base35 (Z003, Nimbus Sans), AGPL with font exception.

## License

To be decided before the repository is made public.
