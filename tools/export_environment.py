"""Convert mission environment art and animal settings (not combat ship definitions)."""

import re
import shutil
from pathlib import Path

from tools.export_combat import flat
from tools.export_combat import load as load_wot
from tools.mdb_to_glb import convert
from tools.models import model_path
from tools.wot import properties


def export_environment(root, out, mission):
    root, out = Path(root), Path(out)
    environment = mission["environment"]
    if environment["animals"]:
        whale = flat(load_wot(root, "Animal_SpaceWhale"))
        environment["whale"] = {
            "scene": "SpaceWhale",
            "min": whale["MinExtents"],
            "max": whale["MaxExtents"],
            "sound_interval": [
                whale["ShortestTimeBetweenSounds"],
                whale["LongestTimeBetweenSounds"],
            ],
            "sounds": [whale[f"Sound_{i}"] for i in range(10) if whale[f"Sound_{i}"]],
            "sound_distance": whale["Sound Distance"],
        }
        dst = model_path(out, "SpaceWhale", mkdir=True)
        if not dst.exists():
            # The animated MSB is not supported; export its single retail bind-pose mesh explicitly.
            dst.write_bytes(
                convert(
                    root, "SpaceWhale", files=[root / "BD_Meshes/SpaceWhale_Layer1.mdb"]
                )
            )
    if environment.get("dragons"):
        nodes = load_wot(root, "Animal_SpaceDragon")
        dragon = flat(nodes)
        gun = flat(load_wot(root, "Gun_DragonBreath"))
        # Its one damage section ('material=Dragon Scale,hitpoints=500') is the dragon's strength.
        hp = next(
            int(m[1])
            for p in properties(nodes)
            if p["name"] == "AssociationName"
            for m in [re.search(r"hitpoints=(\d+)", p["value"])]
            if m
        )
        environment["dragon"] = {
            "scene": "SpaceDragon",
            "hp": hp,
            "sight": dragon["Default SightRange"],
            "physics": {
                k: dragon[k]
                for k in (
                    "Mass",
                    "MaxThrust",
                    "MaxSpeed",
                    "RotationalFriction",
                    "MaxAngularAcceleration",
                )
            },
            "sound_interval": [
                dragon["ShortestTimeBetweenSounds"],
                dragon["LongestTimeBetweenSounds"],
            ],
            "sounds": [dragon[f"Sound_{i}"] for i in range(10) if dragon[f"Sound_{i}"]],
            "sound_distance": dragon["Sound Distance"],
            "gun": {
                "bullet": gun["BulletTypeName_Primary"],
                "muzzle": gun["MuzzleSpeed"],
                "reload": gun["TimeBetweenBursts"],
                "range": [
                    gun["EffectiveRange_Range"],
                    gun["LongRange_Range"],
                    gun["MaximumRange_Range"],
                ],
                "dev": [
                    gun["EffectiveRange_AccuracyDeviation"],
                    gun["LongRange_AccuracyDeviation"],
                    gun["MaximumRange_AccuracyDeviation"],
                ],
                "gravity": gun["BulletAffectedByGravity"],
                "sound": gun.get("SoundName"),
            },
        }
        dst = model_path(out, "SpaceDragon", mkdir=True)
        if not dst.exists():
            dst.write_bytes(
                convert(
                    root,
                    "SpaceDragon",
                    files=[root / "BD_Meshes/SpaceDragon_Layer1.mdb"],
                )
            )
    if environment["nebulae"]:
        dst = model_path(out, "OvalNebula", mkdir=True)
        if not dst.exists():
            dst.write_bytes(
                convert(
                    root, "OvalNebula", files=[root / "BD_Meshes/OvalNebula_Layer1.mdb"]
                )
            )
        nebula = flat(load_wot(root, "Terrain_Nebula"))
        environment["nebula_sound"] = nebula["NebulaSoundName"]
    sounds = (
        environment.get("whale", {}).get("sounds", [])
        + environment.get("dragon", {}).get("sounds", [])
        + ([environment["nebula_sound"]] if "nebula_sound" in environment else [])
    )
    if sounds:
        sources = {}
        # Identical names can exist in Dialogue as short/quiet variants; WOT animal calls use SoundEffects.
        for folder in ("SoundEffects", "Dialogue"):
            for path in sorted((root / "Sound" / folder).glob("*.ogg")):
                sources.setdefault(path.stem.lower(), path)
        (out / "sound").mkdir(exist_ok=True)
        for sound in sounds:
            shutil.copyfile(
                sources[sound.lower()], out / "sound" / (sound.lower() + ".ogg")
            )
