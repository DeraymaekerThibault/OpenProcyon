import json
import unittest

from tools.export_maps import parse_tst, parse_twt, ship_name, victory_rules
from tools.wot import parse_text

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


RULES = """\t00000010 World Rules
\t{
\t\tRule Name String 'navywin'
\t\t00000001 Condition List
\t\t{
\t\t\tType String 'Group/Unit Vital Section has >,<,= X damage'
\t\t\tGroup/Unit Name String 'Base B'
\t\t\tDamage Percent Float 1.000000
\t\t}
\t\t00000002 Action List
\t\t{
\t\t\tType String 'Team X Wins'
\t\t\tTeam Name String 'IDGS_TPTEAMNAMES_NAVY'
\t\t}
\t\t00000002 Action List
\t\t{
\t\t\tType String 'End Game'
\t\t\tWinner - Custom Message String ID String 'WON'
\t\t}
\t\tRule Name String 'setup'
\t\t00000002 Action List
\t\t{
\t\t\tType String 'Play Music Track'
\t\t}
\t}
\t00000003 Group
\t{
\t\tName String 'Base B'
\t\tWorld Object IDs - Element Int 225
\t}
"""


class MapTests(unittest.TestCase):
    def test_victory_rules_keep_only_end_rules_and_resolve_groups(self):
        v = victory_rules(parse_text(RULES)[0])
        self.assertEqual([r["name"] for r in v], ["navywin"])
        self.assertEqual(v[0]["winner"], "IDGS_TPTEAMNAMES_NAVY")
        self.assertEqual(v[0]["end"]["winner"], "WON")
        self.assertEqual(v[0]["if"][0]["objects"], [225])

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
        twt += "Type String '*State Init* Setup Ship'\nWorld Object ID Int 7\nShip Name String 'RLS X'\nLocalized Ship Name String 'IDGS_X'\nPrimary Ship Bool True\n}"
        m = parse_twt(twt)
        self.assertEqual(
            (m["objects"][0]["id"], m["ship_names"]), (7, {7: ("IDGS_X", "RLS X")})
        )
        self.assertEqual(m["primaries"], [7])
        json.dumps(m)  # export_campaign writes the parse result as is

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


class TileSeams(unittest.TestCase):
    def test_transparent_tile_border_inside_an_image_is_filled(self):
        import numpy as np
        from PIL import Image

        from tools.export_maps import fill_seams

        a = np.zeros((4, 8, 4), np.uint8)
        a[..., :] = (10, 20, 30, 255)
        a[:, 3, 3] = (
            0  # left tile's last column (tile width 4) is transparent, as in the retail tiles
        )
        a[0, :, 3] = (
            0  # a genuinely transparent row along the image edge stays transparent
        )
        out = np.array(fill_seams(Image.fromarray(a), [4], []))
        self.assertTrue((out[1:, 3] == (10, 20, 30, 255)).all())
        self.assertTrue((out[0, :, 3] == 0).all())
