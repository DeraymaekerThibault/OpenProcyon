"""All inputs in this module are authored synthetic text, not copied game data."""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from links import symlink_or_skip

from tools.extract_wot import extract, main
from tools.source_text import SourceLine
from tools.wot import analyze, parse_lines, properties


def parse(text):
    return parse_lines(
        [
            SourceLine("synthetic.wot", n, line)
            for n, line in enumerate(text.splitlines(), 1)
        ]
    )


def definition(name, factory, fields=""):
    return (
        f"Type String '{name}'\nDefinition String 'SYNTHETIC'\n"
        f"FactoryType String '{factory}'\n00000000 ConstData\n{{\n{fields}}}\n"
    )


class ParserTests(unittest.TestCase):
    def test_types_unknown_keys_duplicates_and_nested_provenance(self):
        nodes, errors = parse("""Type String 'Synthetic'
00000009 Novel Block
{
  Unfamiliar Float -1.25e2
  Repeated Int +7
  Repeated Int -2
  Switch Bool False
  Position Vector3(1, -2.5, 3e1)
  Display String ID String 'not a game label'
  Empty String ''
  Literal String '{ # String nested }'
  Path String 'a\\b'
  # synthetic comment
}
""")
        self.assertEqual(errors, [])
        fields = list(properties(nodes))
        self.assertEqual(
            [f["value"] for f in fields],
            [
                "Synthetic",
                -125.0,
                7,
                -2,
                False,
                [1.0, -2.5, 30.0],
                "not a game label",
                "",
                "{ # String nested }",
                "a\\b",
            ],
        )
        self.assertEqual(fields[1]["source"], {"path": "synthetic.wot", "line": 4})
        self.assertEqual(fields[1]["raw_value"], "-1.25e2")
        self.assertEqual(fields[6]["name"], "Display String ID")
        self.assertEqual(nodes[1]["count_token"], "00000009")
        self.assertEqual(nodes[1]["close_source"]["line"], 14)
        self.assertEqual(nodes[1]["children"][-1]["kind"], "comment")

    def test_unsupported_type_and_invalid_values_are_retained_and_reported(self):
        for text in (
            "NewThing Matrix4 (unknown)",
            "Bad Float nan",
            "Bad Float 1e999",
            "Bad Bool maybe",
            "Bad Int 1.5",
            "Bad String 'unterminated",
            "Bad Vector3(1,2)",
            "Bad Float 2 trailing",
        ):
            with self.subTest(text=text):
                nodes, errors = parse(text)
                self.assertEqual(nodes[0]["raw"], text)
                self.assertEqual(nodes[0]["kind"], "unparsed")
                self.assertEqual(len(errors), 1)
                self.assertEqual(errors[0]["source"]["line"], 1)

    def test_malformed_braces_and_headers_report_errors_without_losing_fields(self):
        cases = [
            ("}\n", "unexpected_close"),
            ("{\n", "unexpected_open"),
            ("00000001 Block\n", "missing_open"),
            ("00000001 Block\nX Int 3", "missing_open"),
            ("00000001 Block\n{\nX Int 3", "unclosed_block"),
        ]
        for text, code in cases:
            with self.subTest(code=code, text=text):
                nodes, errors = parse(text)
                self.assertIn(code, [e["code"] for e in errors])
                if "X Int" in text:
                    self.assertEqual(next(properties(nodes))["value"], 3)

    def test_comments_and_blank_lines_between_header_and_open_brace(self):
        nodes, errors = parse("00000001 Synthetic\n\n# comment\n{\nX Int 8\n}")
        self.assertEqual(errors, [])
        self.assertEqual(next(properties(nodes))["source"]["line"], 5)

    def test_classification_uses_factory_not_filename_or_type_prefix(self):
        for factory, kind in (
            ("ShipCustomInfoFactory", "ship"),
            ("GunCustomInfoFactory", "gun"),
            ("ProjectilePhysics", "projectile"),
            ("TorpedoPhysics", "projectile"),
            ("MinePhysics", "projectile"),
            ("UnrecognizedFactory", "other"),
        ):
            with self.subTest(factory=factory):
                nodes, errors = parse(
                    definition("Ship_MisleadingSyntheticName", factory)
                )
                identity, classification, _ = analyze(nodes, errors)
                self.assertEqual(errors, [])
                self.assertEqual(classification["kind"], kind)
                self.assertEqual(identity["source"]["line"], 1)
                if kind != "other":
                    self.assertEqual(classification["evidence"][0]["source"]["line"], 3)

    def test_conflicting_classification_and_missing_duplicate_identity(self):
        nodes, errors = parse(
            "FactoryType String 'ShipCustomInfoFactory'\nFactoryType String 'GunCustomInfoFactory'"
        )
        _, classification, _ = analyze(nodes, errors)
        self.assertEqual(classification["kind"], "other")
        self.assertEqual(
            {e["code"] for e in errors}, {"object_type", "ambiguous_category"}
        )
        nodes, errors = parse("Type String 'A'\nType String 'B'")
        identity, _, _ = analyze(nodes, errors)
        self.assertIsNone(identity)
        self.assertEqual(errors[0]["code"], "object_type")

    def test_mount_association_and_projectile_roles_preserve_sources(self):
        nodes, errors = parse(
            definition(
                "SyntheticMount",
                "ShipCustomInfoFactory",
                """00000003 SyntheticMountBlock
{
AttributeName String 'GunPlacement'
DescriptorName String 'synthetic-side'
AssociationName String 'SyntheticGun,opaque,uninterpreted'
}
""",
            )
        )
        _, _, links = analyze(nodes, errors)
        self.assertEqual(errors, [])
        self.assertEqual(links[0]["target"], "SyntheticGun")
        self.assertEqual(links[0]["arguments_raw"], "opaque,uninterpreted")
        self.assertEqual(links[0]["source"]["line"], 10)
        self.assertEqual(links[0]["descriptors"][0]["value"], "synthetic-side")
        nodes, errors = parse(
            definition(
                "SyntheticGun",
                "GunCustomInfoFactory",
                "BulletTypeName_Primary String 'SyntheticProjectile'\nBulletTypeName_Secondary String 'NoBulletType'\n",
            )
        )
        _, _, links = analyze(nodes, errors)
        self.assertEqual(
            [r["target"] for r in links], ["SyntheticProjectile", "NoBulletType"]
        )

    def test_ambiguous_empty_and_invalid_reference_fields_are_reported(self):
        for fields, code in (
            ("AttributeName String 'GunPlacement'\n", "ambiguous_mount"),
            (
                "AttributeName String 'GunPlacement'\nAssociationName String ',rest'\n",
                "empty_mount",
            ),
            ("BulletTypeName_Primary Int 4\n", "reference_type"),
        ):
            with self.subTest(code=code):
                nodes, errors = parse(
                    definition("Synthetic", "GunCustomInfoFactory", fields)
                )
                analyze(nodes, errors)
                self.assertIn(code, [e["code"] for e in errors])


class CatalogTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.parent = Path(temporary.name)
        self.root = self.parent / "inputs"
        self.objects = self.root / "WorldObjects"
        self.objects.mkdir(parents=True)

    def add(self, filename, name, factory, fields=""):
        path = self.objects / filename
        path.write_text(definition(name, factory, fields), encoding="utf-8")
        return path

    def test_deterministic_catalog_case_insensitive_extension_and_read_only(self):
        path = self.add(
            "ignored-prefix.WoT", "S", "ShipCustomInfoFactory", "Novel Int 17\n"
        )
        before = path.read_bytes()
        catalog = extract(self.root)
        self.assertEqual(catalog, extract(self.root))
        self.assertEqual(catalog["summary"]["categories"], {"ship": 1})
        self.assertEqual(catalog["summary"]["errors"], 0)
        self.assertEqual(path.read_bytes(), before)
        self.assertEqual(list(self.objects.iterdir()), [path])
        self.assertEqual(len(catalog["documents"][0]["sha256"]), 64)

    def test_relationship_resolution_missing_wrong_kind_sentinel_and_duplicates(self):
        self.add("p.wot", "P", "ProjectilePhysics")
        self.add(
            "g.wot",
            "G",
            "GunCustomInfoFactory",
            "BulletTypeName_Primary String 'P'\nBulletTypeName_Secondary String 'NoBulletType'\n",
        )
        self.add(
            "g2.wot",
            "G2",
            "GunCustomInfoFactory",
            "BulletTypeName_Primary String 'Absent'\nBulletTypeName_Secondary String 'G'\n",
        )
        catalog = extract(self.root)
        self.assertEqual(
            catalog["summary"]["relationships"],
            {"missing": 1, "resolved": 1, "sentinel": 1, "wrong_kind": 1},
        )
        self.add("duplicate.wot", "P", "ProjectilePhysics")
        catalog = extract(self.root)
        self.assertEqual(catalog["summary"]["relationships"]["ambiguous"], 1)
        self.assertEqual(catalog["summary"]["errors"], 2)

    def test_unreadable_encoding_and_bad_syntax_do_not_hide_other_files(self):
        (self.objects / "bad.wot").write_bytes(b"\xff")
        (self.objects / "empty.wot").write_bytes(b"")
        self.add("good.wot", "Good", "ShipCustomInfoFactory")
        catalog = extract(self.root)
        self.assertEqual(catalog["summary"]["files"], 3)
        self.assertEqual(catalog["summary"]["errors"], 2)
        self.assertEqual(
            {d["code"] for d in catalog["diagnostics"]}, {"file_read", "object_type"}
        )
        self.assertTrue(all(d["source"]["path"] for d in catalog["diagnostics"]))

    def test_empty_scan_is_not_a_success(self):
        with self.assertRaisesRegex(ValueError, "No WOT"):
            extract(self.root)

    def test_symlink_escape_is_reported_without_reading_target(self):
        outside = self.parent / "outside.txt"
        outside.write_text("not an allowed input", encoding="utf-8")
        symlink_or_skip(self, self.objects / "escape.wot", outside)
        catalog = extract(self.root)
        self.assertEqual(catalog["summary"]["errors"], 1)
        self.assertEqual(catalog["documents"][0]["nodes"], [])

    def cli(self, output):
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            return main(["--game-root", str(self.root), "--output", str(output)])

    def test_cli_creates_catalog_but_refuses_overwrite_and_unsafe_destinations(self):
        self.add("synthetic.wot", "S", "ShipCustomInfoFactory")
        output = self.parent / "catalog.json"
        self.assertEqual(self.cli(output), 0)
        before = output.read_bytes()
        self.assertEqual(json.loads(before)["summary"]["files"], 1)
        self.assertEqual(self.cli(output), 2)
        self.assertEqual(output.read_bytes(), before)
        self.assertEqual(self.cli(self.root / "catalog.json"), 2)
        self.assertFalse((self.root / "catalog.json").exists())
        self.assertEqual(self.cli(Path(__file__).parent / "should-not-exist.json"), 2)

    def test_cli_writes_diagnostic_catalog_and_returns_one_for_parse_errors(self):
        (self.objects / "bad.wot").write_text("unknown syntax", encoding="utf-8")
        output = self.parent / "partial.json"
        self.assertEqual(self.cli(output), 1)
        self.assertGreater(json.loads(output.read_text())["summary"]["errors"], 0)


if __name__ == "__main__":
    unittest.main()
