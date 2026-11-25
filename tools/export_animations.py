"""Mesh scene animation clips: assets/data/animations.json = {scene: [{name, duration, motions: [{node, target, channels}]}]}.
`target` is the animated element's name (mesh layer, node or bone); `channels` are six keyframe lists in the order x, y, z position
and yaw, pitch, roll (radians); a keyframe is [time, value, smoothing, tension, continuity, bias, in tangent, out tangent].
Retail: 61 scenes, 65 clips. Ships carry one break-apart clip (ANIMATION00, 2-20 s, plays once), bases a looping windmill spin
(ENVIRONMENT) or crane motion (CRANES), the three animals a swim cycle on their bones. Whether values are offsets from the rest
pose: offsets (docs/re/effects.md). A motion on a mesh layer also carries the layer's `pivot` and rest `rot` (yaw, pitch, roll): the
layer turns about its pivot, at its rest angles plus the clip's (only for a lone leaf layer under unrotated parents, see `export`). usage: python -m tools.export_animations <game_root> <assets_dir>"""

import struct
from pathlib import Path

from tools import cli
from tools.gamefs import ci_files
from tools.jsonio import dump
from tools.msb import parse_scene


def rounded(clip, names, layers):
    return dict(
        name=clip["name"],
        duration=round(clip["duration"], 5),
        motions=[
            dict(
                node=m["node"],
                target=names.get(m["node"]),
                **(
                    dict(
                        pivot=[round(v, 5) for v in layers[m["node"]]["pivot"]],
                        rot=[round(v, 5) for v in layers[m["node"]]["rot"]],
                    )
                    if m["node"] in layers
                    else {}
                ),
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


def lone_leaf_layers(scene):
    """{id: element} of the mesh layers whose pivot and rest angles are exported: no layer under them, no rotated parent, a name
    used once. ponytail: the remake places layers flat (no parent hierarchy) and by name, so crane arms and repeated names keep
    the rest-less pose until the scene hierarchy is built."""
    elements = {e["id"]: e for g in ("nodes", "meshes") for e in scene[g]}

    def turned(i):
        e = elements.get(elements[i]["parent"])
        return bool(e) and (any(abs(v) > 1e-6 for v in e["rot"]) or turned(e["id"]))

    names = [e["name"] for e in scene["meshes"]]
    parents = {e["parent"] for e in scene["meshes"]}
    return {
        e["id"]: e
        for e in scene["meshes"]
        if not turned(e["id"])
        and e["id"] not in parents
        and names.count(e["name"]) == 1
    }


def export(root, out):
    scenes = {}
    for msb in ci_files(Path(root) / "BD_MeshScenes", "*.msb"):
        try:
            scene = parse_scene(msb.read_bytes())
        except (ValueError, struct.error) as e:
            print(
                f"{msb.name}: skipped ({e})"
            )  # unreadable (modded) scene: no animations
            continue
        if scene["animations"]:
            names = {
                e["id"]: e["name"]
                for g in ("nodes", "meshes", "bones")
                for e in scene[g]
            }
            layers = lone_leaf_layers(scene)
            scenes[msb.stem] = [rounded(c, names, layers) for c in scene["animations"]]
    (Path(out) / "data").mkdir(exist_ok=True)
    (Path(out) / "data/animations.json").write_text(dump(scenes), encoding="utf-8")
    print(
        "animations:", sum(map(len, scenes.values())), "clips in", len(scenes), "scenes"
    )


if __name__ == "__main__":
    export(*cli.args(__doc__, 2))
