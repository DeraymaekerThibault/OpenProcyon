import unittest
from tools.export_maps import parse_twt, parse_tst, ship_name

TWT = """\tWorldNameID String 'IDGS_X'
\tMustAssembleFleet Bool True
\tTeam List - Size Int 1
\t00000003 Team List - Element
\t{
\t\tTeam Name ID String 'IDGS_TPTEAMNAMES_NAVY'
\t\tRace Int 0
\t\tRace Lock Bool True
\t}
\t00000032 Player
\t{
\t\tName String 'navy'
\t\tStartPoint Vector3( -1.5, 2.0, 0.000000 )
\t\tStartPointForwardVector Vector3( 0.0, 1.0, 0.0 )
\t\tRace Int 0
\t\tTeamIndex Int 0
\t\tFormationType Int 2
\t\tFlagIndex Int 0
\t}
\tSkybox mesh name String 'skybox_09_Layer1'
\tType String 'Island08'
\t00000014 State
\t{
\t\tHasState Bool False 
\t\tPosition Vector3( 729.1, 423.6, 0.000000 )
\t\tOrientation Matrix33( 1, 0, 0, 0, 1, 0, 0, 0, 1 )
\t\tPlayerIndex Int 3
"""


class MapTests(unittest.TestCase):
    def test_twt(self):
        m = parse_twt(TWT)
        self.assertTrue(m["assemble"])
        self.assertEqual(m["sky"], "skybox_09_Layer1")
        self.assertEqual(m["players"][0]["start"], [-1.5, 2.0, 0.0])
        self.assertEqual(m["players"][0]["formation"], 2)
        self.assertEqual(m["objects"][0]["type"], "Island08")
        self.assertEqual(m["objects"][0]["pos"], [729.1, 423.6, 0.0])
        self.assertEqual(m["objects"][0]["player"], 3)
        self.assertEqual(
            m["teams"], [{"name_id": "IDGS_TPTEAMNAMES_NAVY", "race": 0, "lock": True}]
        )
        self.assertEqual(m["players"][0]["race"], 0)

    def test_ship_name_prefers_the_localized_string(self):
        text = {"IDGS_N": "RLS Mercy"}
        self.assertEqual(ship_name(text, ("IDGS_N", "scripted")), "RLS Mercy")
        self.assertEqual(ship_name(text, ("", "scripted")), "scripted")
        self.assertIsNone(ship_name(text, None))

    def test_setup_ship_names(self):
        twt = "\n\tID Int 7\n\tType String 'Ship_Navy_Frigate'\n\t00000014 State\n\t{\n\t\tHasState Bool False\n\t\tPosition Vector3( 1, 2, 3 )\n\t\tOrientation Matrix33( 1, 0, 0, 0, 1, 0, 0, 0, 1 )\n\t\tPlayerIndex Int 0\n\t}\n"
        twt += "Type String '*State Init* Setup Ship'\nWorld Object ID Int 7\nShip Name String 'RLS X'\nLocalized Ship Name String 'IDGS_X'\n}"
        m = parse_twt(twt)
        self.assertEqual(
            (m["objects"][0]["id"], m["ship_names"]), (7, {7: ("IDGS_X", "RLS X")})
        )

    def test_tst(self):
        els = parse_tst(
            "ScreenElementType Int 1\nImplName String 'Quit'\nEnabled Bool True\nReferenceArea Rect( 1, 2, 3, 4 )\t# l,t,r,b\nText String 'IDGS_Q'\n"
        )
        self.assertEqual(
            els,
            [{"impl": "Quit", "enabled": True, "rect": [1, 2, 3, 4], "text": "IDGS_Q"}],
        )


if __name__ == "__main__":
    unittest.main()
