"""Presentation layers stay separate (docs/development.md "Layers"): shared code never reaches into a view, views never into each other."""

import re
import unittest
from pathlib import Path

SRC = Path(__file__).resolve().parents[1] / "game" / "src"
VIEWS = ("classic",)  # other presentations live in their own crates


def uses(path: Path, view: str) -> bool:
    # `crate::classic`, `classic::plugin`, `use crate::classic::{...}` ...; comments are ignored.
    code = "\n".join(line.split("//")[0] for line in path.read_text().splitlines())
    return re.search(rf"\b{view}::", code) is not None


# Building UI widgets is drawing: shared code may only tag, fill and show widgets a view spawned. Allowed: the retail-art
# helpers every view uses (sprites.rs) and the F1 debug text (lib.rs).
WIDGETS = re.compile(
    r"\bNode \{|\bImageNode::new|\bText::new|\bBackgroundColor\(|\bTextFont \{|\bText2d\b"
)
DRAWS_OK = {Path("sprites.rs"), Path("lib.rs")}


def code_without_tests(path: Path) -> str:
    """Code without comments and without `#[cfg(test)] mod ... { }` blocks (tests may build widgets to drive systems)."""
    code = "\n".join(line.split("//")[0] for line in path.read_text().splitlines())
    while (
        start := re.search(r"#\[cfg\(test\)\]\n(pub )?mod \w+ \{", code)
    ) is not None:
        start = start.start()
        depth, i = 0, code.index("{", start)
        while True:
            depth += {"{": 1, "}": -1}.get(code[i], 0)
            if depth == 0:
                break
            i += 1
        code = code[:start] + code[i + 1 :]
    return code


class Layers(unittest.TestCase):
    def test_shared_code_does_not_build_widgets(self):
        for path in SRC.rglob("*.rs"):
            rel = path.relative_to(SRC)
            if rel.parts[0] in VIEWS or rel == Path("main.rs") or rel in DRAWS_OK:
                continue
            found = WIDGETS.search(code_without_tests(path))
            self.assertIsNone(
                found,
                f"{rel} builds a UI widget ({found and found[0]}); spawn it in a view",
            )

    def test_shared_code_does_not_use_a_view(self):
        for path in SRC.rglob("*.rs"):
            rel = path.relative_to(SRC)
            if rel.parts[0] in VIEWS or rel == Path(
                "main.rs"
            ):  # main.rs picks the view
                continue
            for view in VIEWS:
                self.assertFalse(uses(path, view), f"{rel} uses {view}/")

    def test_views_do_not_use_each_other(self):
        for view in VIEWS:
            for other in VIEWS:
                if other == view:
                    continue
                for path in (SRC / view).rglob("*.rs"):
                    self.assertFalse(
                        uses(path, other), f"{path.relative_to(SRC)} uses {other}/"
                    )


if __name__ == "__main__":
    unittest.main()
