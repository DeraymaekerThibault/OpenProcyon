"""Crew voice lines: an event -> dialogue file table per race, and the retail Ogg files it names.
The table (content/voice/events.json) ships with the code. Its grouping comes from the folder layout of the community Unity
remake (CL ModPack Voicelines/English/<Race>/<event>/<file>.ogg); only event and file names are kept. The audio is always the
retail Sound/Dialogue file of the same name, so any install (vanilla or modded) can be imported without the CL ModPack.
usage: python3 -m tools.export_voices copy <game_root> <out_assets>                        (import step)
       python3 -m tools.export_voices table <game_root> <CL_Data/Voicelines/English> [Race ...]  (regenerate the table)"""

import argparse
import json
import re
import shutil
from pathlib import Path

from tools.jsonio import write

CREW_LINE = re.compile(
    r"[a-z]{2}(gunner|captain|firstmate|helmsman|engineer|rigger|spotter)\d"
)
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
    write(Path(dst), events, sort_keys=True)
    print(sum(len(v) for r in events.values() for v in r.values()), "event entries")


def copy(root, out, events=EVENTS):
    """Copies every file the table names; returns the names this install lacks (a mod may rename dialogue)."""
    root, out = Path(root), Path(out)
    names = {
        n
        for race in json.loads(Path(events).read_text(encoding="utf-8")).values()
        for files in race.values()
        for n in files
    }
    src = {p.stem.lower(): p for p in (root / "Sound/Dialogue").glob("*.ogg")}
    (out / "voice").mkdir(parents=True, exist_ok=True)
    # Every species' crew-post lines too: the game swaps the table's two-letter prefix for the speaker's (game/src/crew.rs).
    for n in {n for n in src if CREW_LINE.match(n)} | (names & src.keys()):
        shutil.copyfile(src[n], out / "voice" / (n + ".ogg"))
    missing = sorted(names - src.keys())
    print(
        len(names & src.keys()),
        "voice files" + (f", missing from this install: {missing}" if missing else ""),
    )
    return missing


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.split("usage:")[0])
    sub = ap.add_subparsers(dest="mode", required=True)
    c = sub.add_parser(
        "copy", help="copy the retail files the event table names (import step)"
    )
    c.add_argument("game_root")
    c.add_argument("out_assets")
    t = sub.add_parser("table", help="regenerate the event table")
    t.add_argument("game_root")
    t.add_argument("voicelines")
    t.add_argument("races", nargs="*")
    a = ap.parse_args()
    if a.mode == "copy":
        copy(a.game_root, a.out_assets)
    else:
        table(a.game_root, a.voicelines, a.races)
