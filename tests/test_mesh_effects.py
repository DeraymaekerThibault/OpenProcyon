import json
import tempfile
import unittest
from pathlib import Path

from test_msb import node

from tools.export_mesh_effects import export


class MeshEffects(unittest.TestCase):
    def test_only_effect_nodes_are_listed_with_their_descriptor_and_position(self):
        data = (
            node(1, -1, "root", [0] * 6)
            + node(2, 1, "Smoke1", [1, 2, 3, 0, 0, 0], "PlayEffect", "default")
            + node(3, 1, "Gun", [0] * 6, "GunPlacement", "Port_X")
            + node(4, 1, "SmokeNode4", [4, 5, 6, 0, 0, 0], "VolcanoSmoke", "SmokeNode4")
        )
        with tempfile.TemporaryDirectory() as d:
            root, out = Path(d, "game"), Path(d, "out")
            (root / "BD_MeshScenes").mkdir(parents=True)
            out.mkdir()
            (root / "BD_MeshScenes/Fire.msb").write_bytes(data)
            export(root, out)
            got = json.loads((out / "data/mesh_effects.json").read_text())
        self.assertEqual(
            got["Fire"],
            [
                dict(
                    kind="PlayEffect",
                    name="Smoke1",
                    descriptor="default",
                    pos=[1, 2, 3],
                ),
                dict(
                    kind="VolcanoSmoke",
                    name="SmokeNode4",
                    descriptor="SmokeNode4",
                    pos=[4, 5, 6],
                ),
            ],
        )


if __name__ == "__main__":
    unittest.main()
