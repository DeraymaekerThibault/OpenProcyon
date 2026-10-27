import os
import tempfile
import unittest


class DestructRecordTests(unittest.TestCase):
    def test_parses_effect_times_and_velocity(self):
        from tools.export_combat import destruct_record

        self.assertEqual(
            destruct_record("{ Some_Chunk 1.5 1.6 ( 1.0 -2.0 3.0 ) }"),
            dict(fx="Some_Chunk", start=1.5, end=1.6, vel=[1.0, -2.0, 3.0]),
        )
        self.assertIsNone(destruct_record("{ none 0 2 ( 1 2 3 ) }")["fx"])
        self.assertIsNone(destruct_record(None))
        self.assertIsNone(destruct_record("{ broken }"))


@unittest.skipUnless(os.environ.get("PROCYON_GAME_ROOT"), "needs PROCYON_GAME_ROOT")
class LocalCombatExportTests(unittest.TestCase):
    def test_base_mounts_stay_on_their_own_towers(self):
        from tools.export_combat import export

        with tempfile.TemporaryDirectory() as tmp:
            j = export(os.environ["PROCYON_GAME_ROOT"], "Base_SmugglersCamp", tmp)
        sections = {s["layer"]: s for s in j["sections"]}
        for gun in j["guns"]:
            lo, hi = sections[gun["parent_layer"]]["aabb"]
            self.assertTrue(lo[0] <= gun["pos"][0] <= hi[0])
            self.assertTrue(lo[1] <= gun["pos"][1] <= hi[1])
            self.assertLess(abs(gun["pos"][2] - hi[2]), 1.0)

    def test_cutter(self):
        from tools.export_combat import export

        j = export(
            os.environ["PROCYON_GAME_ROOT"], "Ship_Navy_Cutter", tempfile.mkdtemp()
        )
        self.assertEqual(len(j["guns"]), 10)
        self.assertEqual(len(j["sections"]), 6)
        self.assertEqual(j["core_hp"], 200.0)
        self.assertEqual(j["scene"], "NavyCutterShip")
        chunks = [s["destruct"] for s in j["sections"]]
        self.assertTrue(all(c and c["end"] >= c["start"] for c in chunks))
        self.assertTrue(
            any(c["fx"] for c in chunks) and any(not c["fx"] for c in chunks)
        )
        debris = {s["label"]: s["debris"] for s in j["sections"]}
        self.assertIsNone(debris["Port_Mid_Hull_wood"], "mid hulls break off nothing")
        self.assertTrue(debris["Mast_2_cloth"]["model"].endswith(".glb"))
        self.assertGreater(
            debris["Mast_2_cloth"]["pivot"][2], 10.0, "the mast piece pivots high"
        )
        self.assertEqual(j["physics"]["MaxSpeed"]["value"], 8.055555)
        # mounts on a hull layer record which section carries them; the rear mount sits on the stern
        rear = next(g for g in j["guns"] if g["label"] == "Rear_LightMount")
        self.assertEqual(rear["parent_layer"], "NavyCutterShip_Layer2")
        self.assertEqual({g["gun"] for g in j["guns"]}, set(j["gun_defs"]))
        for s in j["sections"]:
            self.assertGreater(len(s["tris"]), 0)
            self.assertEqual(len(s["tris"]) % 9, 0)
        for b in j[
            "bullets"
        ].values():  # per-material damage must satisfy lower <= upper
            for lo, hi, crit, _fire, _strength in b["damage"].values():
                self.assertLessEqual(lo, hi)
                self.assertTrue(0 <= crit <= 1)
        # sounds: engine loops per order, gun fire, per-material impact pools (empty WOT slots dropped)
        self.assertEqual(
            j["engine_snd"],
            ["Engine_NAVY_MED", "Engine_NAVY_FULL", "Engine_NAVY_EMERGFULL"],
        )
        self.assertEqual(
            j["ring"], 0.65
        )  # Navy Cutter: selection ring = 65 % of the ship's radius
        self.assertEqual(
            j["gun_defs"]["Gun_CannonBall_Med"]["SoundName"], "Weapon_Cannon_FireMed"
        )
        med = j["bullets"]["Bullet_CannonBall_Med"]
        self.assertEqual(
            set(med["impact_snd"]),
            {"Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon"},
        )
        self.assertEqual(
            med["impact_snd"]["Iron"],
            [f"Weapon_Cannon_Impact_MetalMed0{i}" for i in (1, 2, 3)],
        )


@unittest.skipUnless(os.environ.get("PROCYON_GAME_ROOT"), "needs PROCYON_GAME_ROOT")
class CaseInsensitiveAssetTests(unittest.TestCase):
    def test_fastfrigate_mixed_case_scene(self):
        # WOT says NavyFastfrigateShip, files are NavyFastFrigateShip_*: Windows-era case mismatch
        from tools.export_combat import export

        j = export(
            os.environ["PROCYON_GAME_ROOT"], "Ship_Navy_FastFrigate", tempfile.mkdtemp()
        )
        self.assertGreater(len(j["sections"]), 0)


if __name__ == "__main__":
    unittest.main()
