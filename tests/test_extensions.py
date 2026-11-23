"""Engine extensions stay labelled: the table in docs/modding.md "Engine extensions (not retail)" and the `Not retail:` markers in
game/src agree, the marker has one wording, and the engine-added data keys match docs/data-format.md and the loader."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "game" / "src"
MODDING = ROOT / "docs" / "modding.md"
DATA_FORMAT = ROOT / "docs" / "data-format.md"

MARKER = "Not retail:"
# The mission rule interpreter marks its additions in data (`Origin::Engine`, reported by `--dump-rule-schema`), not with comments.
RULES_EVAL = SRC / "rules" / "eval"
ENGINE_ORIGIN = "Origin::Engine"
# Other ways of saying "not retail" in a comment; the one wording is `Not retail:`. Left out for now: the rule interpreter
# (`rules/eval.rs`, `rules/eval/`), whose additions carry `Origin::Engine`, and files open pull requests are editing.
VARIANT = re.compile(
    r"(?i:\bnot retail\b(?!:)|non-retail|not in the original\b)|\bnot retail:"
)
VARIANT_SKIP = {
    RULES_EVAL,
    SRC / "rules" / "eval.rs",
    SRC / "modding.rs",
    SRC / "theme.rs",
    SRC / "ship" / "attack.rs",
}


def comments(path: Path) -> list[str]:
    """The comment text of each `//`, `///` or `//!` line (inline comments included)."""
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        i = line.find("//")
        if i >= 0:
            out.append(line[i:].lstrip("/!").strip())
    return out


def marked(path: Path) -> bool:
    return any(
        c.startswith(MARKER) or f" {MARKER}" in c or f"({MARKER}" in c
        for c in comments(path)
    )


def is_test(path: Path) -> bool:
    return (
        path.name == "tests.rs"
        or path.name.startswith("tests_")
        or path.stem.endswith("_tests")
    )


def section(text: str, heading: str) -> str:
    start = text.index(f"\n{heading}\n")
    end = text.find("\n## ", start + 1)
    return text[start : end if end >= 0 else None]


def listed() -> set[Path]:
    """Every `game/src/...` path in the extension table (a directory ends in `/`)."""
    table = section(
        MODDING.read_text(encoding="utf-8"), "## Engine extensions (not retail)"
    )
    rows = [
        line
        for line in table.splitlines()
        if line.startswith("| ") and not line.startswith("| Extension")
    ]
    assert rows, "the extension table has no rows"
    return {
        ROOT / p
        for row in rows
        for p in re.findall(r"`(game/src/[^`]+)`", row.split(" | ")[-1])
    }


def rust_files():
    return sorted(p for p in SRC.rglob("*.rs") if not is_test(p))


def rust_list(name: str) -> list[str]:
    m = re.search(
        rf"pub const {name}: &\[&str\] = &\[([^\]]*)\];",
        (SRC / "data.rs").read_text(encoding="utf-8"),
    )
    assert m, f"{name} not found in game/src/data.rs"
    return re.findall(r'"([^"]+)"', m.group(1))


class ExtensionList(unittest.TestCase):
    def test_listed_paths_exist(self):
        for p in listed():
            self.assertTrue(
                p.exists(),
                f"docs/modding.md lists {p.relative_to(ROOT)}, which does not exist",
            )

    def test_every_listed_file_is_marked(self):
        for p in listed():
            if p.is_dir():
                p = p / "mod.rs"
            if p.is_relative_to(RULES_EVAL):
                self.assertIn(
                    ENGINE_ORIGIN,
                    p.read_text(encoding="utf-8"),
                    f"{p.relative_to(ROOT)} has no {ENGINE_ORIGIN}",
                )
            else:
                self.assertTrue(
                    marked(p),
                    f"{p.relative_to(ROOT)} is listed as an engine extension but has no `{MARKER}` comment",
                )

    def test_every_marked_file_is_listed(self):
        docs = listed()
        dirs = [d for d in docs if d.is_dir()]
        for p in rust_files():
            if (
                marked(p)
                and p not in docs
                and not any(p.is_relative_to(d) for d in dirs)
            ):
                self.fail(
                    f"{p.relative_to(ROOT)} has a `{MARKER}` comment but is not in docs/modding.md 'Engine extensions (not retail)'"
                )

    def test_one_wording(self):
        for p in rust_files():
            if any(p == s or p.is_relative_to(s) for s in VARIANT_SKIP):
                continue
            for c in comments(p):
                m = VARIANT.search(c)
                self.assertIsNone(
                    m,
                    f"{p.relative_to(ROOT)}: say `{MARKER}` instead of {m and m.group(0)!r} in: {c}",
                )


class EngineKeys(unittest.TestCase):
    def table_keys(self, source: str) -> set[str]:
        table = section(DATA_FORMAT.read_text(encoding="utf-8"), "## ship.json keys")
        keys = set()
        for line in table.splitlines():
            cells = [c.strip() for c in line.strip("|").split("|")]
            if len(cells) >= 2 and cells[1] == source:
                keys |= set(re.findall(r"`([^`]+)`", cells[0]))
        return keys

    def test_every_key_has_a_source(self):
        table = section(DATA_FORMAT.read_text(encoding="utf-8"), "## ship.json keys")
        for line in table.splitlines():
            cells = [c.strip() for c in line.strip("|").split("|")]
            if line.startswith("| `"):
                self.assertIn(
                    cells[1],
                    ("retail", "engine"),
                    f"ship.json key row without a Source: {line}",
                )

    def test_engine_ship_keys_match_the_docs(self):
        self.assertEqual(set(rust_list("ENGINE_SHIP_KEYS")), self.table_keys("engine"))

    def test_engine_ship_keys_are_read(self):
        loader = (SRC / "data" / "ships.rs").read_text(encoding="utf-8")
        overlay = (SRC / "modding.rs").read_text(encoding="utf-8")
        for k in rust_list("ENGINE_SHIP_KEYS"):
            self.assertTrue(
                re.search(rf"\b{k}: ", loader) or f'"{k}"' in overlay,
                f"ENGINE_SHIP_KEYS has '{k}', which nothing reads",
            )

    def test_engine_gun_keys_are_documented_and_read(self):
        doc = section(DATA_FORMAT.read_text(encoding="utf-8"), "## ship.json keys")
        data = (SRC / "data.rs").read_text(encoding="utf-8")
        for k in rust_list("ENGINE_GUN_KEYS"):
            self.assertIn(
                f"`{k}`",
                doc,
                f"guns.json engine key '{k}' is not named in docs/data-format.md",
            )
            self.assertIn(
                f'g["{k}"]',
                data,
                f"ENGINE_GUN_KEYS has '{k}', which the gun loader does not read",
            )


if __name__ == "__main__":
    unittest.main()
