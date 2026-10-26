"""Opt-in checks against a caller-selected local reference; no game values embedded."""

import hashlib
import os
import re
import unittest
from decimal import Decimal
from pathlib import Path

from tools.extract_wot import extract


def walk(nodes):
    for node in nodes:
        yield node
        if node["kind"] == "block":
            yield from walk(node["children"])


@unittest.skipUnless(
    os.environ.get("PROCYON_RUN_REFERENCE_TESTS") == "1",
    "reference catalog check is opt-in (PROCYON_RUN_REFERENCE_TESTS=1)",
)
class ReferenceWotTests(unittest.TestCase):
    def test_all_fields_against_independently_read_source_lines(self):
        self.assertTrue(
            os.environ.get("PROCYON_GAME_ROOT"), "Set PROCYON_GAME_ROOT explicitly"
        )
        root = Path(os.environ["PROCYON_GAME_ROOT"]).resolve(strict=True)
        encoding = os.environ.get("PROCYON_SOURCE_ENCODING", "utf-8")
        catalog = extract(root, encoding=encoding)
        self.assertEqual(
            catalog["summary"]["errors"],
            0,
            "Inspect catalog diagnostics for parse errors",
        )
        checked = 0
        for document in catalog["documents"]:
            path = root / document["path"]
            raw = path.read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), document["sha256"])
            lines = (
                raw.decode(encoding)
                .replace("\r\n", "\n")
                .replace("\r", "\n")
                .split("\n")
            )
            accounted = set()
            for node in walk(document["nodes"]):
                line = node["source"]["line"]
                self.assertTrue(
                    node["raw"] == lines[line - 1],
                    f"Source mismatch at {document['path']}:{line}",
                )
                self.assertEqual(
                    node["source"]["path"], path.resolve().relative_to(root).as_posix()
                )
                self.assertNotIn(line, accounted)
                accounted.add(line)
                if node["kind"] == "block":
                    for key, token in (("open_source", "{"), ("close_source", "}")):
                        location = node[key]
                        self.assertEqual(lines[location["line"] - 1].strip(), token)
                        self.assertNotIn(location["line"], accounted)
                        accounted.add(location["line"])
                elif node["kind"] == "property":
                    checked += 1
                    # Independent lexical checks: do not use the parser's decoder or raw_value.
                    match = re.fullmatch(
                        re.escape(node["name"])
                        + r"\s+"
                        + re.escape(node["type"])
                        + r"\s*(.*)",
                        lines[line - 1].strip(),
                    )
                    self.assertIsNotNone(match)
                    literal = match[1]
                    kind = node["type"]
                    if kind == "String":
                        self.assertTrue(
                            literal.startswith("'") and literal.endswith("'")
                        )
                        expected = literal[1:-1]
                    elif kind == "Int":
                        expected = int(literal, 10)
                    elif kind == "Float":
                        expected = float(Decimal(literal))
                    elif kind == "Bool":
                        self.assertIn(literal, ("True", "False"))
                        expected = literal == "True"
                    elif kind == "Vector3":
                        self.assertTrue(
                            literal.startswith("(") and literal.endswith(")")
                        )
                        expected = [
                            float(Decimal(part.strip()))
                            for part in literal[1:-1].split(",")
                        ]
                        self.assertEqual(len(expected), 3)
                    else:
                        self.fail("Unexpected parsed primitive type")
                    self.assertTrue(
                        node["value"] == expected,
                        f"Value mismatch at {document['path']}:{line}",
                    )
            self.assertEqual(
                accounted,
                {n for n, text in enumerate(lines, 1) if text.strip()},
                f"Unaccounted nonblank lines in {document['path']}",
            )
            self.assertEqual(
                hashlib.sha256(path.read_bytes()).hexdigest(), document["sha256"]
            )
        self.assertEqual(checked, catalog["summary"]["properties"])
        self.assertGreater(checked, 0)


if __name__ == "__main__":
    unittest.main()
