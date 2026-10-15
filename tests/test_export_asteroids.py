import json
import unittest

from imported import ASSETS, needs_assets

from tools.export_asteroids import definition, validate
from tools.export_campaign import parse_mission
from tools.models import model_path


class AsteroidExportTests(unittest.TestCase):
    def test_definition_preserves_slots_and_rejects_invalid_strength(self):
        text = """FactoryType String 'AsteroidCustomInfoFactory'
Mesh Scene Name String 'AsteroidLarge'
Max Hit Points Int 50
Mass Float 40000
Radius Float 15
ExplosionEffect String 'ShipExplosion_Medium'
""" + "\n".join(
            f"WorldObject To Create Upon Death {i} String '{'Asteroid_Med' if i == 2 else ''}'"
            for i in range(10)
        )
        d = definition(text)
        self.assertEqual((d["hp"], d["mass"], d["radius"]), (50, 40000, 15))
        self.assertEqual(d["children"][2], "Asteroid_Med")
        self.assertEqual(d["children"][:2], ["", ""])
        self.assertIsNone(
            definition(
                text.replace("AsteroidCustomInfoFactory", "ShipCustomInfoFactory")
            )
        )
        with self.assertRaises(ValueError):
            definition(text.replace("Max Hit Points Int 50", "Max Hit Points Int 0"))
        for defs in [{"a": {"children": ["a"]}}, {"a": {"children": ["missing"]}}]:
            with self.assertRaises(ValueError):
                validate(defs)

    @needs_assets
    def test_named_group_ids_are_exported(self):
        parsed = parse_mission("""00000000 World Rules
{
}
00000001 Group
{
 Name String 'Montressor Asteroids'
 World Object IDs - Element Int 12
}
""")
        self.assertEqual(parsed["object_groups"], {12: "Montressor Asteroids"})
        objects = json.loads((ASSETS / "maps/mission01/map.json").read_text())[
            "objects"
        ]
        self.assertEqual(sum(o.get("group") == "Asteroid Belt" for o in objects), 24)
        self.assertEqual(
            sum(o.get("group") == "Montressor Asteroids" for o in objects), 20
        )

    @needs_assets
    def test_committed_breakup_models_and_effects(self):
        defs = json.loads((ASSETS / "data/asteroids.json").read_text())
        validate(defs)
        for kind, hp, children in [
            ("Asteroid_Large", 50, ["Asteroid_Med"] * 2),
            ("Asteroid_Med", 30, ["Asteroid_Small"] * 3),
            ("Asteroid_Small", 10, []),
        ]:
            self.assertEqual(defs[kind]["hp"], hp)
            self.assertEqual([c for c in defs[kind]["children"] if c], children)
        effects = json.loads((ASSETS / "fx/index.json").read_text())
        for d in defs.values():
            self.assertTrue(model_path(ASSETS, d["scene"]).exists())
            self.assertIn(d["effect"], effects)
            self.assertTrue((ASSETS / "fx" / (d["effect"] + ".json")).exists())
        fx = json.loads((ASSETS / "fx/ShipExplosion_Medium.json").read_text())
        self.assertEqual(
            [p["NumberParticlesAtStart"] for p in fx["Primitive"]], [7, 1, 15]
        )
