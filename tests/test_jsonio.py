import json
import unittest

from tools.jsonio import dump


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


if __name__ == "__main__":
    unittest.main()
