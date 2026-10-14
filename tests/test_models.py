import unittest
from pathlib import Path

from tools.models import category, model_path

from imported import ASSETS, needs_assets


class ModelLayout(unittest.TestCase):
    def test_category_rules(self):
        self.assertEqual(category("NavyCutterShip"), "ships")
        self.assertEqual(category("gun_Enet_Layer1.glb"), "weapons")
        self.assertEqual(
            model_path("a", "skybox_07").as_posix(), "a/models/sky/skybox_07.glb"
        )

    @needs_assets

    def test_committed_models_sit_in_their_category_and_names_are_unique(self):
        files = list((ASSETS / "models").rglob("*.glb"))
        self.assertGreater(len(files), 100)
        for f in files:
            self.assertEqual(f, model_path(ASSETS, f.name), f)
        self.assertEqual(len({f.name for f in files}), len(files))
        self.assertEqual(
            list(ASSETS.glob("*.glb")), []
        )  # the Rust loader's name index relies on this layout


if __name__ == "__main__":
    unittest.main()
