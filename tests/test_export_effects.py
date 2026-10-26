import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.eft import parse_eft
from tools.export_effects import export_morph_meshes
from tools.models import model_path


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
