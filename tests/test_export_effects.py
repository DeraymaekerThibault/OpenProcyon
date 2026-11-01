import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tests.imported import ASSETS, needs_assets
from tools.eft import parse_eft
from tools.export_effects import EXTRAS, export_morph_meshes
from tools.models import model_path


class AmbientEffects(unittest.TestCase):
    def test_mesh_scene_effects_are_exported(self):
        for name in (
            "FireStack",
            "Crater_Smoke",
            "Island_Smoke",
            "SmokeStack_Smoke",
            "Lighthouse",
            "bouy",
            "VolcanoSmoke",
            "volcanoSparks",
        ):
            self.assertIn(name, EXTRAS)


class MorphMeshes(unittest.TestCase):
    def test_exports_both_authored_endpoints_through_existing_converter(self):
        source = "RepairMesh_Small_Down_Layer1"
        destination = "RepairMesh_Small_up_Layer1"
        effect = {
            "Primitive": [
                {
                    "CommonPropertiesChunk": {
                        "MeshChunk": {
                            "UseMorph": True,
                            "SourceMeshName": source,
                            "DestinationMeshName": destination,
                        }
                    }
                }
            ]
        }
        with (
            tempfile.TemporaryDirectory() as out,
            patch(
                "tools.export_effects.ci_file",
                side_effect=lambda folder, name: folder / name,
            ),
            patch(
                "tools.export_effects.convert", return_value=b"converted"
            ) as converter,
        ):
            export_morph_meshes("retail", out, effect)
            for name in (source, destination):
                converter.assert_any_call(
                    "retail", name, files=[Path("retail/BD_Meshes") / (name + ".mdb")]
                )
                self.assertEqual(model_path(out, name).read_bytes(), b"converted")
            self.assertEqual(converter.call_count, 2)

    def test_curve_flags_keys_only_values(self):
        text = """00000001 EffectStream
{
00000002 ParticleRateInterpolator
{
UseValuesAtKeysOnly Bool True
NumKeys Int 1
t Float 0
value Float 1
}
}
"""
        self.assertTrue(parse_eft(text)["ParticleRateInterpolator"]["keys_only"])
        plain = text.replace("Bool True", "Bool False")
        self.assertNotIn("keys_only", parse_eft(plain)["ParticleRateInterpolator"])

    def test_curve_keeps_active_loop_metadata_and_omits_disabled_loops(self):
        text = """00000001 EffectStream
{
00000002 SizeInterpolatorChunk
{
NumKeys Int 2
t Float 0
value Float 1
t Float 2.5
value Float 5
00000003 LoopInfo
{
StartLoopingT Float 0
EndLoopingT Float 2.5
NumberLoops Int 9999999
}
}
}
"""
        curve = parse_eft(text)["SizeInterpolatorChunk"]
        self.assertEqual(curve["curve"], [[0.0, 1.0], [2.5, 5.0]])
        self.assertEqual(
            curve["loop"],
            {"StartLoopingT": 0.0, "EndLoopingT": 2.5, "NumberLoops": 9999999},
        )
        self.assertNotIn(
            "loop",
            parse_eft(text.replace("NumberLoops Int 9999999", "NumberLoops Int 0"))[
                "SizeInterpolatorChunk"
            ],
        )

    def test_non_morph_primitives_do_not_export_meshes(self):
        with patch("tools.export_effects.convert") as converter:
            export_morph_meshes(
                "retail",
                "unused",
                {
                    "Primitive": [
                        {"CommonPropertiesChunk": {"MeshChunk": {"UseMorph": False}}},
                        {},
                    ]
                },
            )
            converter.assert_not_called()


@needs_assets
class WeaponEffectsAreExported(unittest.TestCase):
    def test_every_bullet_hit_and_muzzle_effect_exists(self):
        # Base and island guns (heavy carronade) are exported after the stock ships; their shells were invisible without this.
        index = set(json.loads((ASSETS / "fx/index.json").read_text()))
        wdir = ASSETS / "data/weapons"
        names = set()
        for rec in json.loads((wdir / "bullets.json").read_text()).values():
            names |= {rec.get("bullet_fx"), rec.get("hit_fx")}
        for rec in json.loads((wdir / "guns.json").read_text()).values():
            names.add(rec.get("MuzzleFlashEffect"))
        names -= {None, "", "none", "NoEffect"}
        self.assertEqual(sorted(names - index), [])
