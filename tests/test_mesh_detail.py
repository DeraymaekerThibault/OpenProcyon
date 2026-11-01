import unittest

from tools.export_mesh_detail import lod_lookup, parse

DMT = """Mesh Detail Settings Chunk Int 2
Mesh Name String ''
00000005 MeshDetailSetting Chunk
{
\tLOD Level Info Chunk Array Int 1
\tMax Camera Distance Float 0.000000
\tMesh Detail Percentage Float 1.000000
\tCamera Distance To Not Draw Mesh Float 0.000000
\tCollision LOD Level Index Int 0
}
Mesh Name String 'HULL_Layer1'
00000007 MeshDetailSetting Chunk
{
\tLOD Level Info Chunk Array Int 2
\tMax Camera Distance Float 0.000000
\tMesh Detail Percentage Float 1.000000
\tMax Camera Distance Float 1000.000000
\tMesh Detail Percentage Float 0.200000
\tCamera Distance To Not Draw Mesh Float 2500.000000
\tCollision LOD Level Index Int 1
}
"""


class MeshDetail(unittest.TestCase):
    def test_levels_no_draw_distance_and_collision_level(self):
        m = parse(DMT)
        self.assertEqual(m[""]["collision_lod"], 0)
        hull = m["HULL_Layer1"]
        self.assertEqual(hull["levels"], [[0.0, 1.0], [1000.0, 0.2]])
        self.assertEqual((hull["no_draw"], hull["collision_lod"]), (2500.0, 1))

    def test_collision_level_lookup_ignores_case_and_defaults(self):
        lod = lod_lookup(parse(DMT))
        self.assertEqual((lod("hull_layer1"), lod("OTHER_Layer1")), (1, 0))


if __name__ == "__main__":
    unittest.main()
