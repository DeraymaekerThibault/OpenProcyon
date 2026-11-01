import json
import tempfile
import unittest
from pathlib import Path

from test_msb_scene import scene

from tools.export_animations import export as export_animations
from tools.export_mesh_effects import export


class MeshSceneExports(unittest.TestCase):
    def run_export(self, fn):
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "BD_MeshScenes").mkdir(parents=True)
            (root / "WorldObjects").mkdir()
            (root / "WorldObjects/Island_Fire.wot").write_text(
                "Mesh Scene Name String 'Fire'\n"
                "MeshName String 'Smoke1'\n"
                "AttributeName String 'PlayEffect'\n"
                "DescriptorName String 'default'\n"
                "AssociationName String 'Island_Smoke'\n"
            )
            out.mkdir()
            (root / "BD_MeshScenes/Fire.msb").write_bytes(scene())
            fn(root, out)
            return {
                p.name: json.loads(p.read_text()) for p in (out / "data").glob("*.json")
            }

    def test_every_attribute_of_a_node_is_an_effect_point(self):
        got = self.run_export(export)["mesh_effects.json"]
        pos = [1.0, 2.0, 3.0]
        self.assertEqual(
            got["Fire"],
            [
                dict(
                    kind="PlayEffect",
                    name="Smoke1",
                    descriptor="default",
                    effect="Island_Smoke",
                    pos=pos,
                ),
                dict(
                    kind="VolcanoSmoke",
                    name="Smoke1",
                    descriptor="Smoke1",
                    effect=None,
                    pos=pos,
                ),
            ],
        )

    def test_clips_keep_their_keyframes_and_name_their_target(self):
        (clip,) = self.run_export(export_animations)["animations.json"]["Fire"]
        self.assertEqual((clip["name"], clip["duration"]), ("ANIMATION00", 2.0))
        (motion,) = clip["motions"]
        self.assertEqual((motion["node"], motion["target"]), (0, "Smoke1"))
        self.assertEqual(
            motion["channels"][0],
            [
                [0.0, 0.0, 2, 0.0, 0.0, 0.0, 0.0, 0.0],
                [2.0, 5.0, 2, 0.0, 0.0, 0.0, 0.0, 5.0],
            ],
        )


if __name__ == "__main__":
    unittest.main()
