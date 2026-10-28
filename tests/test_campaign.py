import json
import unittest

from imported import ASSETS, needs_assets

from tools.export_campaign import parse_mission, speech_events
from tools.models import model_path


class CampaignTests(unittest.TestCase):
    def test_rule_order_and_repeated_actions(self):
        parsed = parse_mission("""00000001 World Rules
{
 Rule Name String 'init'
 Run Once Bool True
 00000001 Condition List
 {
  Type String 'World Initialize'
 }
 00000002 Action List
 {
  Type String 'Set Flag Action'
  Flag Name String 'one'
 }
 00000002 Action List
 {
  Type String 'Set Flag Action'
  Flag Name String 'two'
 }
 Rule Name String 'next'
 00000001 Condition List
 {
  Type String 'Enter Volume'
 }
}
""")
        self.assertEqual([r["name"] for r in parsed["rules"]], ["init", "next"])
        self.assertEqual(
            [a["Flag Name"] for a in parsed["rules"][0]["actions"]], ["one", "two"]
        )
        self.assertEqual(parsed["rules"][0]["line"], 3)

    def test_crew_and_arms_pools_and_speech_presentation(self):
        text = """\tWorld Crew List - Size Int 2
\tWorld Crew List - Element String 'Crew_A'
\tWorld Crew List - Element String 'Crew_B'
\tWorld Arms List - Size Int 1
\tWorld Arms List - Element String 'Gun_X'
\tPlayEndMovie Bool True
\t00000001 World Rules
\t{
\t}
\t00000002 Speech Event List - Element
\t{
\t\tName String 'Hello'
\t\tSound FileName String 'M01X'
\t\tText Color Colour( 1.0, 0.5, 0.0, 1.0 )
\t\tTalkingHeadLocation Int 1
\t\tSpeakerID String 'IDGS_TPSPEAKERNAMES_JIM'
\t}
"""
        m = parse_mission(text)
        self.assertEqual(m["crew_pool"], ["Crew_A", "Crew_B"])
        self.assertEqual(m["arms_pool"], ["Gun_X"])
        self.assertTrue(m["play_end_movie"])
        ev = speech_events(text)["Hello"]
        self.assertEqual((ev["color"], ev["head"]), ([1.0, 0.5, 0.0, 1.0], 1))
        self.assertEqual(ev["speaker"], "IDGS_TPSPEAKERNAMES_JIM")

    def test_unclosed_script_rejected(self):
        with self.assertRaises(ValueError):
            parse_mission("00000001 World Rules\n{\n")

    @needs_assets
    def test_committed_mission_dependencies(self):
        mission = json.loads((ASSETS / "maps/mission01/mission.json").read_text())
        self.assertEqual(len(mission["ships"]), 11)
        primary = [s for s in mission["ships"] if s["primary"]]
        self.assertEqual(
            [(s["id"], s["name"], s["type"]) for s in primary],
            [(231, "RLS Comet", "Ship_Navy_Torpedo")],
        )
        self.assertEqual(len(mission["rules"]), 136)
        registry = {f.parent.name for f in ASSETS.glob("ships/*/ship.json")}
        guns = json.loads((ASSETS / "data/weapons/guns.json").read_text())
        for ship in mission["ships"]:
            self.assertIn(ship["type"], registry)
            data = json.loads(
                (ASSETS / "ships" / ship["type"] / "ship.json").read_text()
            )
            self.assertTrue((ASSETS / "ships" / data["model"] / "model.glb").is_file())
            self.assertTrue(
                (ASSETS / "ships" / data["geometry"] / "geometry.json").is_file()
            )
            for gun in data["guns"].values():
                self.assertIn(gun["gun"], guns)
        for bullet in json.loads(
            (ASSETS / "data/weapons/bullets.json").read_text()
        ).values():
            if bullet.get("glb"):
                self.assertTrue(model_path(ASSETS, bullet["glb"]).is_file())
        for part in ["COMPASS", "ARROW"]:
            self.assertTrue(
                model_path(ASSETS, f"SELECTION_INDICATOR_{part}_SMALL").is_file()
            )
        map_ = json.loads((ASSETS / "maps/mission01/map.json").read_text())
        self.assertTrue(model_path(ASSETS, map_["sky"].split("_Layer")[0]).is_file())
        for obj in map_["objects"]:
            self.assertTrue(model_path(ASSETS, obj["scene"]).is_file())
        self.assertEqual(len(mission["points"]), 11)
        self.assertEqual(len(mission["tasks"]), 10)
        self.assertEqual(len(mission["paths"]), 21)
        self.assertEqual(
            [s["name"] for s in mission["opening_speech"]],
            ["M01AMELIA01", "M01AMELIA01A", "M01AMELIA02", "M01AMELIA02A"],
        )
        for cue in mission["opening_speech"]:
            self.assertGreater(cue["duration"], 0)
            self.assertTrue(cue["text"])
            self.assertTrue(
                (
                    ASSETS / "maps/mission01/speech" / (cue["name"].lower() + ".ogg")
                ).is_file()
            )
        self.assertIn("Montressor Archipelago", mission["landmarks"])
        self.assertTrue(
            list(ASSETS.glob("sound/*/" + mission["handoff_music"].lower() + ".ogg"))
        )
