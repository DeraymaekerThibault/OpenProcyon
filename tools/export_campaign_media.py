"""Campaign rule media: every speech event and music track the rules use, plus subtitle/end-message strings.
Reads assets/campaign/<mission>.json (tools.export_campaign), its TWT speech list and the install; writes
assets/campaign/<mission>_media.json and copies the Ogg files into assets/sound (lowercase names). Each event records its
sound `file` and talking-head `face`; events whose file is missing get duration null (the game shows their text for a fixed time).
usage: python -m tools.export_campaign_media <game_root> <assets_dir>"""

import json
import shutil
import sys
from pathlib import Path

from tools import ogg
from tools.export_maps import strings

# Maps whose audio is copied into assets/sound (the JSON lists every cue regardless): the 13 campaign missions and both tutorials.
PLAYABLE = {f"mission{n:02d}" for n in range(1, 14)} | {"tutorial", "basic_tutorial"}


def main(root, out):
    root, out = Path(root), Path(out)
    for f in sorted((out / "campaign").glob("*.json")):
        if not f.stem.endswith("_media"):
            export(root, out, f.stem)


def export(root, out, mid):
    names = strings(root)
    mission = json.loads((out / f"campaign/{mid}.json").read_text())
    from tools.export_campaign import (
        speech_events,
    )  # late: export_campaign imports PLAYABLE from here

    events = speech_events((root / mission["source"]).read_text(encoding="latin1"))
    acts = [a for r in mission["rules"] for a in r["actions"]]
    speech, music, messages, ship_names = {}, set(), {}, {}
    for a in acts:
        if a["Type"] == "Play speech event":
            n = a["Speech Event Name"]
            # Rules name an event of the TWT speech list; its sound file and string id may differ from the event name (missions 3+).
            ev = events.get(n, {})
            file = ev.get("file") or n
            src = root / "Sound/Dialogue" / (file + ".ogg")
            keys = (
                [ev.get("text_id")]
                + [f"IDGS_TPSPEECHEVENTS{i:03d}_{file}" for i in range(10)]
                + [f"IDGS_TPSPEECHEVENTTUTORIAL_{file}"]
            )
            text = next((names[k] for k in keys if k in names), "")
            dur = None
            if src.exists():
                dur = ogg.duration(src)
                if mid in PLAYABLE:
                    shutil.copyfile(src, out / "sound" / (file.lower() + ".ogg"))
            speech[n] = dict(text=text, duration=dur, file=file, face=ev.get("face"))
        elif a["Type"] == "Play Special Effect":
            # A dialogue-folder sound (cue or line) played by name; no subtitle/talking head.
            src = root / "Sound/Dialogue" / (a["File Name"] + ".ogg")
            if mid in PLAYABLE and src.exists():
                shutil.copyfile(src, out / "sound" / (a["File Name"].lower() + ".ogg"))
        elif a["Type"] == "Play Music Track" and a["File Name"] != "None":
            music.add(a["File Name"])
        elif a["Type"] == "Set Ship Name" and a["New Ship Name"] in names:
            ship_names[a["New Ship Name"]] = names[a["New Ship Name"]]
        elif a["Type"] == "End Game":
            for k in (
                "Winner - Custom Message String ID",
                "Loser - Custom Message String ID",
            ):
                if a.get(k) in names:
                    messages[a[k]] = names[a[k]]
    for t in sorted(music):
        if mid in PLAYABLE and (root / "Sound/Music" / (t + ".ogg")).exists():
            shutil.copyfile(
                root / "Sound/Music" / (t + ".ogg"),
                out / "sound" / (t.lower() + ".ogg"),
            )
    (out / f"campaign/{mid}_media.json").write_text(
        json.dumps(
            dict(speech=speech, messages=messages, ship_names=ship_names), indent=1
        )
    )
    print(mid, len(speech), "speech events,", len(music), "music tracks")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
