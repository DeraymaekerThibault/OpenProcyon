"""Lifeboat class metadata (docs/re/lifeboats.md §6.1): the explicit flag, boat counts and pickup spheres in the imported assets."""

import json
import unittest

from tools.export_combat import lifeboat_fields

from imported import ASSETS, needs_assets

SHIPS = ASSETS / "ships"


def ship(name):
    return json.loads((SHIPS / name / "ship.json").read_text())


@needs_assets
class LifeboatMetadataTests(unittest.TestCase):
    def test_four_race_lifeboats_carry_the_flag_and_mission9_longboat_does_not(self):
        for race in ("Navy", "Civilian", "Pirate", "Procyon"):
            j = ship(f"Ship_{race}_Lifeboat")
            self.assertTrue(j["is_lifeboat"], race)
            self.assertEqual((j["response"], j["lifeboats"]), ("Lifeboat", 0), race)
        longboat = ship("Ship_Navy_Lifeboat_Mission9")
        self.assertFalse(
            longboat["is_lifeboat"]
        )  # a combat story ship despite its name
        self.assertEqual(longboat["response"], "Ship")

    def test_boat_counts_and_spheres(self):
        self.assertEqual(
            [
                ship(n)["lifeboats"]
                for n in (
                    "Ship_Pirate_Schooner",
                    "Ship_Pirate_Sloop",
                    "Ship_Procyon_Dreadnought",
                )
            ],
            [2, 1, 0],
        )
        self.assertEqual(
            ship("Ship_Navy_Lifeboat")["sphere"],
            {"center": [0.0, 0.0, 1.0], "radius": 2.0, "authored": True},
        )
        self.assertEqual(ship("Ship_Navy_Cutter")["sphere"]["radius"], 12.0)
        derived = ship("Ship_Pirate_Lifeboat")[
            "sphere"
        ]  # no authored sphere: derived from the box (provisional)
        self.assertFalse(derived["authored"])
        self.assertGreater(derived["radius"], 1.0)

    def test_every_ship_has_the_fields(self):
        for f in SHIPS.glob("*/ship.json"):
            j = json.loads(f.read_text())
            self.assertTrue(
                {"lifeboats", "is_lifeboat", "response", "sphere"} <= j.keys(),
                f.parent.name,
            )

    def test_sphere_falls_back_to_extents_then_section_boxes(self):
        sections = [{"aabb": [[-1, -2, 0], [1, 2, 2]]}]
        a = lifeboat_fields(
            {"MinExtents": [-1, -1, -1], "MaxExtents": [1, 1, 1]}, sections
        )["sphere"]
        self.assertAlmostEqual(a["radius"], 3**0.5)
        b = lifeboat_fields({}, sections)
        self.assertEqual(
            (b["lifeboats"], b["is_lifeboat"], b["response"], b["sphere"]["center"]),
            (0, False, "Ship", [0.0, 0.0, 1.0]),
        )


if __name__ == "__main__":
    unittest.main()
