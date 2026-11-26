"""Island ambience: assets/data/island_sounds.json = {island: {sound, distance}} for the islands that declare an ambient loop
(the volcanoes), and the loop's Ogg file next to the other sounds.
usage: python -m tools.export_island_sounds <game_root> <assets_dir>"""

import shutil
from pathlib import Path

from tools import cli, importlog
from tools.export_combat import flat, load
from tools.gamefs import ci_files
from tools.jsonio import write


def export(root, out):
    root, out = Path(root), Path(out)
    islands = {}
    for wot in ci_files(root / "WorldObjects", "Island_*.wot"):
        try:
            f = flat(load(root, wot.stem))
        except (KeyError, ValueError, FileNotFoundError) as e:
            # installs vary (mods); one broken island must not stop the import
            importlog.skipped(f"WorldObjects/{wot.name}", f"{type(e).__name__}: {e}")
            continue
        if f.get("HasAmbientSound") and f.get("AmbientSoundName"):
            islands[wot.stem] = dict(
                sound=f["AmbientSoundName"], distance=f["AmbientSoundMaxDistance"]
            )
    (out / "data").mkdir(exist_ok=True)
    write(out / "data/island_sounds.json", islands)
    (out / "sound").mkdir(exist_ok=True)
    for sound in {i["sound"] for i in islands.values()}:
        src = ci_files(root / "Sound/Dialogue", sound + ".ogg")
        dst = out / "sound" / (sound.lower() + ".ogg")
        if src and not dst.exists():
            shutil.copyfile(src[0], dst)
    print("island sounds:", len(islands), "islands with an ambient loop")


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
