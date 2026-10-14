import json
import unittest
import tempfile
from pathlib import Path
from unittest.mock import patch

from tools.export_campaign import parse_mission
from tools.models import model_path
from tools.export_environment import export_environment
from tools.export_nebulae import weather_damage
from tools.wot import parse_lines
from tools.source_text import SourceLine

from imported import ASSETS, needs_assets


class EnvironmentExportTests(unittest.TestCase):
    def test_weather_damage_keeps_each_hazards_own_material_table(self):
        fields = ("Damage Lower Bound", "Damage Upper Bound", "Chance of Critical Damage", "Chance of Fire", "Initial Fire Strength")
        blocks = []
        for hazard, lo in (("Lightning", 20), ("Wind", 5), ("Meteor", 10)):
            lines = [f"00000048 {hazard} Damage Potential", "{"]
            for material in ("Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon"):
                lines.extend(f"  {key}: {material} Float {value}" for key, value in zip(fields, (lo, lo + 2, 0.2, 0.1, 3)))
            lines.append("}")
            blocks.extend(lines)
        nodes, problems = parse_lines([SourceLine("synthetic.wot", i, line) for i, line in enumerate(blocks, 1)])
        self.assertFalse(problems)
        tables = weather_damage(nodes)
        self.assertEqual(tables["wind"]["Cloth"], [5, 7, 0.2, 0.1, 3])
        self.assertEqual(tables["lightning"]["Wood"][:2], [20, 22])
        self.assertEqual(tables["meteor"]["Iron"][:2], [10, 12])

    def test_animal_sound_effects_take_precedence_over_dialogue_variants(self):
        with tempfile.TemporaryDirectory() as folder:
            root, out = Path(folder) / "retail", Path(folder) / "assets"
            out.mkdir()
            for directory in ("SoundEffects", "Dialogue"):
                (root / "Sound" / directory).mkdir(parents=True)
            (root / "Sound/SoundEffects/Animal_WhaleCall01.ogg").write_bytes(
                b"full call"
            )
            (root / "Sound/Dialogue/Animal_WhaleCall01.ogg").write_bytes(
                b"quiet variant"
            )
            definition = {
                "MinExtents": [-1, -1, -1],
                "MaxExtents": [1, 1, 1],
                "ShortestTimeBetweenSounds": 3,
                "LongestTimeBetweenSounds": 10,
                "Sound Distance": 400,
                **{
                    f"Sound_{i}": "Animal_WhaleCall01" if i == 0 else ""
                    for i in range(10)
                },
            }
            mission = {"environment": {"animals": [{}], "nebulae": []}}
            with (
                patch("tools.export_environment.load_wot", return_value=[]),
                patch("tools.export_environment.flat", return_value=definition),
                patch(
                    "tools.export_environment.convert", return_value=b"converted mesh"
                ),
            ):
                export_environment(root, out, mission)
            self.assertEqual(
                (out / "sound/animal_whalecall01.ogg").read_bytes(), b"full call"
            )
            self.assertEqual(mission["environment"]["whale"]["sound_interval"], [3, 10])

    def test_animal_ids_groups_and_cloud_point_sets_are_preserved(self):
        mission = parse_mission("""00000000 World Rules
{
}
\tID Int 205
\tType String 'Animal_SpaceWhale'
\t00000014 State
\t{
\t\tPosition Vector3( 1.0, 2.0, 3.0 )
\t\tOrientation Matrix33( 1,0,0,0,1,0,0,0,1 )
\t}
00000003 Group
{
    Name String 'Whales 2'
    World Object IDs - Element Int 205
}
00000014 World Point Sets Vector - Element
{
    Name String 'Clarke'
    00000009 World Points - Element
    {
        World Point Magnitude Float 381.0
        00000005 World Point Basis
        {
            Position Vector3( 10.0, 20.0, 4.0 )
        }
    }
}
""")
        env = mission["environment"]
        self.assertEqual(env["animals"][0]["id"], 205)
        self.assertEqual(env["animals"][0]["group"], "Whales 2")
        self.assertEqual(env["animals"][0]["pos"], [1, 2, 3])
        self.assertEqual(
            env["point_sets"]["Clarke"], [{"pos": [10, 20, 4], "magnitude": 381}]
        )

    @needs_assets

    def test_mission01_environment_assets_and_retail_groups_exist(self):
        mission = json.loads((ASSETS / "maps/mission01/mission.json").read_text())
        env = mission["environment"]
        self.assertEqual([a["id"] for a in env["animals"]], [205, 206, 207, 217])
        self.assertEqual(
            [a["group"] for a in env["animals"]], ["Whales"] * 3 + ["Whales 2"]
        )
        self.assertGreater(env["point_sets"]["Clarke"][0]["magnitude"], 380)
        self.assertTrue(model_path(ASSETS, "SpaceWhale").is_file())
        self.assertTrue(model_path(ASSETS, "OvalNebula").is_file())
        for sound in env["whale"]["sounds"] + [env["nebula_sound"]]:
            self.assertTrue(list(ASSETS.glob(f"sound/*/{sound.lower()}.ogg")), sound)
