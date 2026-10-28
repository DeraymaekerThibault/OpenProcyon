"""Island ambience: assets/data/island_sounds.json = {island: {sound, distance}} for the islands that declare an ambient loop
(the volcanoes), and the loop's Ogg file next to the other sounds.
usage: python -m tools.export_island_sounds <game_root> <assets_dir>"""

import json
import shutil
import sys
from pathlib import Path

from tools.export_combat import flat, load
from tools.gamefs import ci_files


def export(root, out):
    root, out = Path(root), Path(out)
    islands = {}
    for wot in ci_files(root / "WorldObjects", "Island_*.wot"):
        try:
            f = flat(load(root, wot.stem))
        except (KeyError, ValueError, FileNotFoundError):
            continue  # installs vary (mods); one broken island must not stop the import
        if f.get("HasAmbientSound") and f.get("AmbientSoundName"):
            islands[wot.stem] = dict(
                sound=f["AmbientSoundName"], distance=f["AmbientSoundMaxDistance"]
            )
    (out / "data").mkdir(exist_ok=True)
    (out / "data/island_sounds.json").write_text(json.dumps(islands, indent=1))
    (out / "sound").mkdir(exist_ok=True)
    for sound in {i["sound"] for i in islands.values()}:
        src = ci_files(root / "Sound/Dialogue", sound + ".ogg")
        dst = out / "sound" / (sound.lower() + ".ogg")
        if src and not dst.exists():
            shutil.copyfile(src[0], dst)
    print("island sounds:", len(islands), "islands with an ambient loop")


if __name__ == "__main__":
    export(*sys.argv[1:3])
