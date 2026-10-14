"""Crew voice lines: an event -> dialogue file table per race, and the retail Ogg files it names.
The table (content/voice/events.json) ships with the code. Its grouping comes from the folder layout of the community Unity
remake (CL ModPack Voicelines/English/<Race>/<event>/<file>.ogg); only event and file names are kept. The audio is always the
retail Sound/Dialogue file of the same name, so any install (vanilla or modded) can be imported without the CL ModPack.
usage: python3 -m tools.export_voices copy <game_root> <out_assets>                        (import step)
       python3 -m tools.export_voices table <game_root> <CL_Data/Voicelines/English> [Race ...]  (regenerate the table)"""

import json
import shutil
import sys
from pathlib import Path

EVENTS = Path(__file__).resolve().parents[1] / "content/voice/events.json"


def table(root, voicelines, races, dst=EVENTS):
    root, voicelines = Path(root), Path(voicelines)
    events = {}
    for race in sorted(
        p for p in voicelines.iterdir() if p.is_dir() and (not races or p.name in races)
    ):
        ev = events[race.name] = {}
        for d in sorted(p for p in race.iterdir() if p.is_dir()):
            names = sorted(
                f.stem.lower()
                for f in d.glob("*.ogg")
                if (root / "Sound/Dialogue" / f.name).exists()
            )
            if names:
                ev[d.name] = names
    Path(dst).write_text(json.dumps(events, indent=1, sort_keys=True))
    print(sum(len(v) for r in events.values() for v in r.values()), "event entries")


def copy(root, out, events=EVENTS):
    """Copies every file the table names; returns the names this install lacks (a mod may rename dialogue)."""
    root, out = Path(root), Path(out)
    names = {n for race in json.loads(Path(events).read_text()).values() for files in race.values() for n in files}
    src = {p.stem.lower(): p for p in (root / "Sound/Dialogue").glob("*.ogg")}
    (out / "voice").mkdir(parents=True, exist_ok=True)
    for n in names & src.keys():
        shutil.copyfile(src[n], out / "voice" / (n + ".ogg"))
    missing = sorted(names - src.keys())
    print(len(names & src.keys()), "voice files" + (f", missing from this install: {missing}" if missing else ""))
    return missing


if __name__ == "__main__":
    mode, *args = sys.argv[1:]
    if mode == "copy":
        copy(*args)
    else:
        table(args[0], args[1], args[2:])
