import re
import unittest
from pathlib import Path

from tools import import_assets, pack_data
from tools.skirmish_ships import SKIRMISH_SHIPS

TOOLS = Path(__file__).resolve().parents[1] / "tools"


class SkirmishShips(unittest.TestCase):
    def test_importer_and_packer_share_one_list(self):
        self.assertEqual(import_assets.FIRST, SKIRMISH_SHIPS)
        self.assertEqual(pack_data.SKIRMISH, set(SKIRMISH_SHIPS))
        self.assertEqual(len(SKIRMISH_SHIPS), len(set(SKIRMISH_SHIPS)))

    def test_no_second_copy_of_the_list(self):
        # a ship name written out in either tool would be a second list that can drift
        for name in ("import_assets.py", "pack_data.py"):
            text = (TOOLS / name).read_text()
            self.assertIsNone(re.search(r'"Ship_(Navy|Pirate)_\w+"', text), name)


if __name__ == "__main__":
    unittest.main()
