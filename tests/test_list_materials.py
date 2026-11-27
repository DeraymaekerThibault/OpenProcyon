import json
import struct
import tempfile
import unittest
from pathlib import Path

from tools import list_materials


def glb(path, names):
    body = json.dumps({"materials": [{"name": n} for n in names]}).encode()
    body += b" " * (-len(body) % 4)
    path.write_bytes(
        b"glTF"
        + struct.pack("<II", 2, 20 + len(body))
        + struct.pack("<I4s", len(body), b"JSON")
        + body
    )


class ListMaterials(unittest.TestCase):
    def test_reads_names_from_a_glb(self):
        with tempfile.TemporaryDirectory() as d:
            glb(Path(d, "a.glb"), ["Gattling", "deck"])
            self.assertEqual(
                list_materials.glb_materials(Path(d, "a.glb")), ["Gattling", "deck"]
            )

    def test_the_shipped_remaster_rules_sort_the_known_misfires(self):
        # Invented names with the shapes the rules had to get right (no game content).
        look = json.loads(list_materials.LOOK.read_text(encoding="utf-8"))["surfaces"]
        rules = look["rules"]

        def kind(name):
            found = list_materials.classify(look, name)
            return rules[found]["match"][0] if isinstance(found, int) else found

        self.assertEqual(kind("x_carronade_1"), "metal")
        self.assertEqual(kind("x_plasma"), "metal")
        self.assertEqual(kind("x_wood_engine"), "wood", "wood wins over engine")
        self.assertEqual(kind("x_crater"), "rock", "crate must not claim crater")
        self.assertEqual(kind("x_island_trails"), "rock", "rail must not claim trails")
        self.assertEqual(kind("x_sail"), "skipped")
        self.assertEqual(kind("zzz"), "default")

    def test_groups_by_winning_rule(self):
        look = {"skip": ["sail"], "rules": [{"match": ["a"]}, {"match": ["b"]}]}
        self.assertEqual(
            list_materials.group(look, ["xa", "xb", "xa", "sail", "q"]),
            {0: ["xa"], 1: ["xb"], "skipped": ["sail"], "default": ["q"]},
        )
