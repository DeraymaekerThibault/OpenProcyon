import json
import tempfile
import unittest
from pathlib import Path

from imported import ASSETS, needs_assets

from tools.pack_layout import pack


def put(p, v=None, raw=None):
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(raw if raw is not None else json.dumps(v))


class PackLayoutTests(unittest.TestCase):
    def test_ships_and_maps_get_one_folder_each(self):
        with tempfile.TemporaryDirectory() as t:
            a = Path(t)
            for s in ("Ship_A", "Ship_A_Feather"):  # a variant sharing A's hull
                put(a / f"data/ships/{s}.json", {"scene": "AShip", "entity": s})
                put(a / f"data/geometry/{s}.json", {})
            (a / "models/ships").mkdir(parents=True)
            (a / "models/ships/AShip.glb").write_bytes(b"glb")
            put(a / "data/ship_names.json", {"Ship_A": ["Alpha"]})
            put(
                a / "data/docks.json",
                {"Ship_A": {"points": [], "tender": True}, "Base_X": {"points": []}},
            )
            put(a / "ships.json", ["Ship_A", "Ship_A_Feather"])
            put(a / "maps/index.json", [{"id": "duel"}])
            put(a / "maps/duel.json", {"backdrop": "Map_Duel"})
            put(a / "maps/mission01.json", {"backdrop": "Map_Duel"})
            put(a / "campaign/mission01.json", {"rules": []})
            put(a / "campaign/mission01_media.json", {})
            put(a / "campaign/duel_script.json", {"rules": []})
            put(a / "menu/map_duel.png", raw="png")
            pack(a)
            pack(a)  # idempotent
            self.assertEqual(
                json.loads((a / "ships/Ship_A_Feather/ship.json").read_text())["model"],
                "Ship_A",
            )
            self.assertTrue((a / "ships/Ship_A/model.glb").is_file())
            self.assertFalse((a / "ships/Ship_A_Feather/model.glb").exists())
            self.assertEqual(
                json.loads((a / "ships/Ship_A/names.json").read_text()), ["Alpha"]
            )
            self.assertEqual(
                list(json.loads((a / "data/docks.json").read_text())), ["Base_X"]
            )
            self.assertFalse(
                (a / "ships.json").exists() or (a / "maps/index.json").exists()
            )
            self.assertTrue(
                json.loads((a / "maps/duel/map.json").read_text())["skirmish"]
            )
            self.assertNotIn(
                "skirmish", json.loads((a / "maps/mission01/map.json").read_text())
            )
            self.assertTrue(
                (a / "maps/mission01/mission.json").is_file()
                and (a / "maps/mission01/media.json").is_file()
            )
            # a skirmish map's own script sits beside it without making it a mission
            self.assertTrue((a / "maps/duel/script.json").is_file())
            self.assertFalse((a / "maps/duel/mission.json").exists())
            self.assertTrue(
                (a / "maps/duel/backdrop.png").is_file()
                and (a / "maps/mission01/backdrop.png").is_file()
            )

    def test_sounds_are_sorted_by_kind_and_mission_speech_goes_to_its_map(self):
        with tempfile.TemporaryDirectory() as t:
            a = Path(t)
            put(a / "maps/mission04.json", {})
            put(
                a / "campaign/mission04_media.json",
                {"speech": ["M04JIM01", "NIS_SolarStorm"]},
            )
            for n in (
                "weapon_x.ogg",
                "engine_navy_med.ogg",
                "m04jim01.ogg",
                "m04jim99.ogg",
                "m05shared.ogg",
                "hud_open.ogg",
                "odd.ogg",
                "nis_solarstorm.ogg",
            ):
                put(a / "sound" / n, raw="ogg")
            put(a / "maps/mission05.json", {"speech": ["M05SHARED"]})
            put(a / "campaign/mission05_media.json", {"speech": ["M05SHARED"]})
            put(a / "maps/mission06.json", {"speech": ["M05SHARED"]})
            pack(a)
            self.assertTrue(
                (a / "sound/weapons/weapon_x.ogg").is_file()
                and (a / "sound/ships/engine_navy_med.ogg").is_file()
            )
            self.assertTrue(
                (a / "sound/ui/hud_open.ogg").is_file()
                and (a / "sound/odd.ogg").is_file()
            )
            self.assertTrue(
                (a / "maps/mission04/speech/m04jim01.ogg").is_file()
                and (a / "maps/mission04/speech/m04jim99.ogg").is_file()
            )
            self.assertTrue(
                (a / "sound/speech/m05shared.ogg").is_file()
            )  # named by two missions
            # No speech prefix, but a mission names it: still that mission's dialogue.
            self.assertTrue((a / "maps/mission04/speech/nis_solarstorm.ogg").is_file())

    @needs_assets
    def test_committed_folders_are_complete(self):
        ships = list(ASSETS.glob("ships/*/ship.json"))
        self.assertGreater(len(ships), 60)
        for f in ships:
            j = json.loads(f.read_text())
            self.assertTrue(
                (ASSETS / "ships" / j["geometry"] / "geometry.json").is_file(), f
            )
            if not f.parent.name.startswith(
                "Base_"
            ):  # armed bases draw their scenery GLB (models/bases)
                self.assertTrue(
                    (ASSETS / "ships" / j["model"] / "model.glb").is_file(), f
                )
        for f in ASSETS.glob("maps/*/map.json"):
            self.assertTrue((f.parent / "backdrop.png").is_file(), f)
        self.assertFalse((ASSETS / "ships.json").exists())
        self.assertEqual(
            list((ASSETS / "sound").glob("*.ogg")), []
        )  # everything sits in a kind folder


if __name__ == "__main__":
    unittest.main()
