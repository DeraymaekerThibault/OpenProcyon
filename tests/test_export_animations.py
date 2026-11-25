"""Which mesh layers carry their pivot and rest angles in animations.json (synthetic scenes, no game data)."""

import unittest

from tools.export_animations import lone_leaf_layers


def el(i, parent, name, rot=(0.0, 0.0, 0.0)):
    return {"id": i, "parent": parent, "name": name, "rot": list(rot)}


class LoneLeafLayers(unittest.TestCase):
    def test_only_lone_leaves_under_unrotated_parents(self):
        scene = {
            "nodes": [el(0, -1, "root"), el(1, 0, "turned", (0.5, 0, 0))],
            "meshes": [
                el(2, 0, "Base_Layer1"),
                el(
                    3, 2, "WindMill_Layer2", (-0.57, 0, 0)
                ),  # leaf under an unrotated layer: kept
                el(4, 1, "Arm_Layer1"),  # under a rotated node: dropped
                el(5, 0, "Twin_Layer1"),
                el(6, 0, "Twin_Layer1"),  # repeated name: both dropped
            ],
        }
        self.assertEqual(sorted(lone_leaf_layers(scene)), [3])


if __name__ == "__main__":
    unittest.main()
