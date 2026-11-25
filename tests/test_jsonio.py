import json
import tempfile
import unittest
from pathlib import Path

from tools.jsonio import dump, write


class JsonIoTest(unittest.TestCase):
    def test_indent_two_number_lists_on_one_line(self):
        v = {"a": [1, 2.5, 3], "b": {"c": "x", "d": [[1, 2], [3, 4]]}}
        text = dump(v)
        self.assertTrue(text.endswith("}\n"))
        self.assertIn('  "a": [1, 2.5, 3],', text)
        self.assertIn("[1, 2]", text)
        self.assertEqual(json.loads(text), v)

    def test_sort_keys(self):
        self.assertLess(dump({"b": 1, "a": 2}, sort_keys=True).index('"a"'), 10)

    def test_write_is_dump_in_a_new_folder(self):
        v = {"b": [1, 2], "a": "é"}
        with tempfile.TemporaryDirectory() as tmp:
            for kwargs in ({}, {"sort_keys": True}):
                path = Path(tmp) / "x/y/z.json"
                write(path, v, **kwargs)
                # write_text writes the platform's line endings (CRLF on Windows), as the call sites it replaced did.
                self.assertEqual(path.read_text(encoding="utf-8"), dump(v, **kwargs))


if __name__ == "__main__":
    unittest.main()
