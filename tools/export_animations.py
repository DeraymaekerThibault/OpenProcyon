"""Mesh scene animation clips: assets/data/animations.json = {scene: [{name, duration, motions: [{node, target, channels}]}]}.
`target` is the animated element's name (mesh layer, node or bone); `channels` are six keyframe lists in the order x, y, z position
and yaw, pitch, roll (radians); a keyframe is [time, value, smoothing, tension, continuity, bias, in tangent, out tangent].
Retail: 61 scenes, 65 clips. Ships carry one break-apart clip (ANIMATION00, 2-20 s, plays once), bases a looping windmill spin
(ENVIRONMENT) or crane motion (CRANES), the three animals a swim cycle on their bones. Whether values are offsets from the rest
pose or absolute poses is not settled (docs/re/effects.md). usage: python -m tools.export_animations <game_root> <assets_dir>"""

import json
import sys
from pathlib import Path

from tools.gamefs import ci_files
from tools.msb import parse_scene


def rounded(clip, names):
    return dict(
        name=clip["name"],
        duration=round(clip["duration"], 5),
        motions=[
            dict(
                node=m["node"],
                target=names.get(m["node"]),
                channels=[
                    [
                        [round(v, 5) if isinstance(v, float) else v for v in key]
                        for key in channel
                    ]
                    for channel in m["channels"]
                ],
            )
            for m in clip["motions"]
        ],
    )


def export(root, out):
    scenes = {}
    for msb in ci_files(Path(root) / "BD_MeshScenes", "*.msb"):
        scene = parse_scene(msb.read_bytes())
        if scene["animations"]:
            names = {
                e["id"]: e["name"]
                for g in ("nodes", "meshes", "bones")
                for e in scene[g]
            }
            scenes[msb.stem] = [rounded(c, names) for c in scene["animations"]]
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/animations.json").write_text(
        json.dumps(scenes, separators=(",", ":"))
    )
    print(
        "animations:", sum(map(len, scenes.values())), "clips in", len(scenes), "scenes"
    )


if __name__ == "__main__":
    export(*sys.argv[1:3])
