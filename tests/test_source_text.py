import tempfile
import unittest
from pathlib import Path

from links import symlink_or_skip

from tools.source_text import SourceLine, read_source_lines


class SourceTextTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.parent = Path(self.temporary.name)
        self.root = self.parent / "inputs"
        self.root.mkdir()

    def read(self, content: bytes, *, encoding="utf-8"):
        source = self.root / "nested" / "synthetic.txt"
        source.parent.mkdir(exist_ok=True)
        source.write_bytes(content)
        result = read_source_lines(
            self.root, Path("nested/synthetic.txt"), encoding=encoding
        )
        self.assertEqual(source.read_bytes(), content)
        self.assertEqual(list(source.parent.iterdir()), [source])
        return result

    def test_synthetic_fixture_preserves_every_line_and_provenance(self):
        fixture_root = Path(__file__).parent / "fixtures"
        result = read_source_lines(
            fixture_root, Path("synthetic-source.txt"), encoding="utf-8"
        )
        self.assertEqual(
            result,
            [
                SourceLine(
                    "synthetic-source.txt",
                    1,
                    "Synthetic text fixture; not original game content or a WOT grammar.",
                ),
                SourceLine("synthetic-source.txt", 2, ""),
                SourceLine("synthetic-source.txt", 3, "  label = café"),
                SourceLine("synthetic-source.txt", 4, "label = repeated"),
            ],
        )

    def test_physical_newlines_blank_lines_and_no_final_newline(self):
        for ending in (b"\n", b"\r\n", b"\r"):
            with self.subTest(ending=ending):
                self.assertEqual(
                    self.read(ending.join([b"same", b"", b"same"])),
                    [
                        SourceLine("nested/synthetic.txt", 1, "same"),
                        SourceLine("nested/synthetic.txt", 2, ""),
                        SourceLine("nested/synthetic.txt", 3, "same"),
                    ],
                )

    def test_empty_file_has_no_lines(self):
        self.assertEqual(self.read(b""), [])

    def test_final_newline_does_not_add_a_phantom_line(self):
        self.assertEqual(self.read(b"\n"), [SourceLine("nested/synthetic.txt", 1, "")])
        self.assertEqual(
            self.read(b"a\n"), [SourceLine("nested/synthetic.txt", 1, "a")]
        )

    def test_whitespace_and_non_newline_unicode_separators_are_preserved(self):
        text = "\t café  \u2028still the same physical line\x0b"
        self.assertEqual(
            self.read(text.encode()), [SourceLine("nested/synthetic.txt", 1, text)]
        )

    def test_encoding_is_explicit_and_bom_handling_is_caller_selected(self):
        self.assertEqual(self.read(b"caf\xe9", encoding="latin-1")[0].text, "café")
        content = b"\xef\xbb\xbfsynthetic"
        self.assertEqual(self.read(content, encoding="utf-8-sig")[0].text, "synthetic")
        self.assertEqual(
            self.read(content, encoding="utf-8")[0].text, "\ufeffsynthetic"
        )

    def test_invalid_encoding_bytes_are_not_silently_replaced(self):
        with self.assertRaises(UnicodeDecodeError):
            self.read(b"\xff")

    def test_missing_file_and_missing_root_fail(self):
        with self.assertRaises(FileNotFoundError):
            read_source_lines(self.root, Path("absent.txt"), encoding="utf-8")
        with self.assertRaises(FileNotFoundError):
            read_source_lines(self.root / "absent", Path("file.txt"), encoding="utf-8")

    def test_non_directory_root_and_directory_input_fail(self):
        source = self.root / "synthetic.txt"
        source.write_text("synthetic", encoding="utf-8")
        with self.assertRaises(NotADirectoryError):
            read_source_lines(source, Path("file.txt"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "regular file"):
            read_source_lines(self.root, Path("."), encoding="utf-8")

    def test_absolute_input_is_rejected_even_inside_root(self):
        source = self.root / "synthetic.txt"
        source.write_text("synthetic", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "relative"):
            read_source_lines(self.root, source, encoding="utf-8")

    def test_parent_escape_and_symlink_escape_are_rejected(self):
        outside = self.parent / "outside.txt"
        outside.write_text("synthetic", encoding="utf-8")
        link = self.root / "link.txt"
        symlink_or_skip(self, link, outside)
        for relative in (Path("../outside.txt"), Path("link.txt")):
            with (
                self.subTest(relative=relative),
                self.assertRaisesRegex(ValueError, "outside"),
            ):
                read_source_lines(self.root, relative, encoding="utf-8")

    def test_internal_symlink_reports_actual_source_path(self):
        source = self.root / "synthetic.txt"
        source.write_text("synthetic", encoding="utf-8")
        symlink_or_skip(self, self.root / "alias.txt", source)
        self.assertEqual(
            read_source_lines(self.root, Path("alias.txt"), encoding="utf-8"),
            [SourceLine("synthetic.txt", 1, "synthetic")],
        )


if __name__ == "__main__":
    unittest.main()
