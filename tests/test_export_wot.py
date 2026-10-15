"""Synthetic coverage of the exporters' shared tree reader; no retail fixtures."""

import unittest

from tools.export_campaign import parse_mission
from tools.export_collision import definition
from tools.export_crew import parse as crew
from tools.export_maps import parse_twt
from tools.wot import fields, parse_text


class WotExportTests(unittest.TestCase):
    def test_compound_types_and_embedded_apostrophe(self):
        nodes, errors = parse_text("""Label String 'Pilot's ship'
Points Coord(1E+2, -2e-1)
Orientation Matrix33(1,0,0,0,1,0,0,0,1)
""")
        self.assertFalse(errors)
        self.assertEqual(
            fields(nodes),
            dict(
                Label="Pilot's ship",
                Points=[100.0, -0.2],
                Orientation=[1, 0, 0, 0, 1, 0, 0, 0, 1],
            ),
        )
        for text in ("Bad Matrix33(1,2)", "Bad Coord(1,inf)", "Bad Coord(1,2,3)"):
            with self.subTest(text=text):
                nodes, errors = parse_text(text)
                self.assertTrue(errors)
                self.assertEqual(nodes[0]["kind"], "unparsed")

    def test_crew_fields_ignore_comments_and_property_order(self):
        parsed = crew(
            """# Point Value Int 999
00000000 Crew
{
Gunnery Skill Ability Value Int 4
Crew Name String ID String 'SYNTHETIC_NAME'
Gunnery Skill Active Bool True
Point Value Int -2
Navigation Skill Active Bool False
}
""",
            {"SYNTHETIC_NAME": "Test pilot"},
        )
        self.assertEqual(parsed["name"], "Test pilot")
        self.assertEqual(parsed["points"], -2)
        self.assertEqual(parsed["skills"], {"gunnery": 4})

    def test_collision_definition_is_scoped_to_its_record(self):
        parsed = definition("""00000000 Definitions
{
Definition String 'OTHER'
DetectionType String 'Null'
Definition String 'COLLISIONDEFINITION'
00000000 ConstData
{
MaxExtents Vector3(2,3,4)
ResponseType String 'Ship'
MinExtents Vector3(-2,-3,-4)
DetectionType String 'Box'
}
Definition String 'OTHER'
MaxExtents Vector3(99,99,99)
}
""")
        self.assertEqual(
            parsed,
            dict(detection="Box", response="Ship", min=[-2, -4, -3], max=[2, 4, 3]),
        )
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            definition(
                "Definition String 'COLLISIONDEFINITION'\nDetectionType String 'Box'\nResponseType String 'Ship'\nMinExtents Vector3(1,2,3)"
            )

    def test_world_objects_reordered_fields_and_repeated_polygon_points(self):
        text = """00000000 World
{
00000000 Player
{
FlagIndex Int 0
Name String 'test'
StartPointForwardVector Vector3(0,1,0)
StartPoint Vector3(1E+2,0,0)
}
ID Int 7
Type String 'Ship_Synthetic'
00000000 State
{
PlayerIndex Int 0
Orientation Matrix33(1,0,0,0,1,0,0,0,1)
Position Vector3(1,2,3)
HasState Bool True
}
Type String 'Island_Synthetic'
00000000 State
{
Orientation Matrix33(1,0,0,0,1,0,0,0,1)
Position Vector3(4,5,6)
}
00000000 World Polygons Vectors - Element
{
Points Int 2
Points Coord(1E+2, -2e-1)
Name String 'test area'
Points Coord(3,4)
}
00000000 World Rules
{
Rule Name String 'Synthetic setup'
00000000 Action List
{
Ship Name String 'Pilot's ship'
Type String '*State Init* Setup Ship'
World Object ID Int 7
}
Rule Name String 'Pilot's second rule'
00000000 Action List
{
Type String 'Synthetic action'
}
}
}
"""
        world = parse_twt(text)
        self.assertEqual(world["players"][0]["start"], [100, 0, 0])
        self.assertEqual([o["id"] for o in world["objects"]], [7, None])
        mission = parse_mission(text)
        self.assertEqual(mission["volumes"], {"test area": [[100, -0.2], [3, 4]]})
        self.assertEqual(mission["ships"][0]["name"], "Pilot's ship")
        self.assertEqual(mission["ships"][0]["pos"], [1, 2, 3])
        self.assertEqual(len(mission["rules"]), 2)
        self.assertEqual(len(mission["rules"][0]["actions"]), 1)
        self.assertEqual(mission["rules"][1]["name"], "Pilot's second rule")
        self.assertEqual(mission["rules"][1]["actions"], [{"Type": "Synthetic action"}])


if __name__ == "__main__":
    unittest.main()
