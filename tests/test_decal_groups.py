import unittest

from tools.export_decals import materials


class DecalGroups(unittest.TestCase):
    def test_surface_groups_follow_the_originals_table(self):
        everywhere = ["Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon"]
        self.assertEqual(materials("burn2_Small"), everywhere)
        self.assertEqual(materials("hole4_Medium"), ["Wood", "Reinforced", "Cloth"])
        self.assertEqual(materials("dent7_Small"), ["Wood", "Reinforced"])
        self.assertEqual(materials("SteelDecal1_Small"), ["Iron"])
        self.assertEqual(materials("sail_hole1_Small"), ["Cloth"])
        self.assertEqual(materials("wall dmg 1_Large"), ["Stone"])

    def test_unregistered_textures_are_skipped(self):
        for name in ("engine_part_Small", "rudder_dent_Small", "hole_damage60_1_Small"):
            self.assertIsNone(materials(name))


if __name__ == "__main__":
    unittest.main()
