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


# Simulation modules (#369 phase 2): what they decide is the same on every machine of a lock-step match, so they never read the
# local player's view state: the view's ship marker (`PlayerShip`), the local fleet and selection (`Fleets.local`/`selected`) or the
# old global target (`combat::Target`). A human's ship is `commands::Commander`, a human player is `commands::Players`.
SIMULATION = (
    "ship.rs", "ship/", "combat/", "collision.rs", "collision/", "docking.rs", "boarding.rs", "boarding/",
    "scenario/outcome.rs", "rules/", "cloak.rs", "dragons.rs", "lifeboats.rs", "blackhole.rs", "nebula.rs", "victory.rs",
    "environment.rs", "crew_arms.rs", "crew_arms/", "alliance.rs", "commands.rs",
)  # fmt: skip
VIEW_STATE = re.compile(
    r"\bPlayerShip\b|\bcombat::Target\b|\bfleets\b[^;\n]{0,40}?\.(?:local|selected)\b"
)
# The functions in those modules that may: the local player's input, the view's own systems, dev and smoke hooks acting as the
# local player, the marker's own bookkeeping, and the no-table fallbacks of unit-test worlds. (file, function): why.
VIEW_STATE_OK = {
    ("alliance.rs", "update"): "the alliance roster panel's rows (view)",
    (
        "alliance.rs",
        "click",
    ): "the roster panel's clicks; alliance changes are not commands yet (phase 3)",
    ("boarding.rs", "dev_board"): "dev hook --script board",
    (
        "boarding.rs",
        "status",
    ): "the boarding menu's state for the local player's ship (view)",
    (
        "boarding/fight.rs",
        "handle_messages",
    ): "the local ship's crew line (cosmetic generator)",
    ("boarding/fight.rs", "fights"): "the local ship's crew lines (cosmetic generator)",
    (
        "boarding/grapple.rs",
        "hook_flight",
    ): "the local ship's 'grappled' line (cosmetic generator)",
    ("boarding/grapple.rs", "cancel_grapple_order"): "the local player's Backspace",
    ("cloak.rs", "cloak_smoke"): "smoke hook",
    ("combat/asteroid.rs", "asteroid_demo"): "dev hook",
    ("combat/mod.rs", "draw"): "the view's target marker",
    ("commands.rs", "commander_set"): "the marker follows the local Commander",
    ("commands.rs", "commander_removed"): "the marker follows the local Commander",
    ("crew_arms.rs", "screen"): "the campaign crew screen (view)",
    ("docking.rs", "cancel_input"): "the local player's Backspace",
    ("environment.rs", "sensor_smoke"): "smoke hook",
    (
        "rules/systems/dev_console.rs",
        "run",
    ): "the developer console acts as the local player",
    ("ship.rs", "is_human_ship"): "the fallback of a world without a players table",
    ("ship.rs", "promote"): "only the local player's selection follows a P",
    ("ship.rs", "reconcile_fleets"): "the local selection stays on a live ship",
}


def view_state_reads(path: Path):
    """(function, text) of each view-state read in `path` outside tests, `use` lines and the marker's own definition."""
    code = code_without_tests(path)
    code = re.sub(r"^\s*use [^;]*;", "", code, flags=re.MULTILINE)
    code = re.sub(r"pub struct PlayerShip;", "", code)
    for m in VIEW_STATE.finditer(code):
        fns = re.findall(r"\bfn (\w+)", code[: m.start()])
        yield (fns[-1] if fns else "-"), m[0]


class Layers(unittest.TestCase):
    def test_simulation_does_not_read_the_local_players_view_state(self):
        for path in sorted(SRC.rglob("*.rs")):
            rel = path.relative_to(SRC).as_posix()
            if rel.endswith("tests.rs") or not any(
                rel == s or (s.endswith("/") and rel.startswith(s)) for s in SIMULATION
            ):
                continue
            for fn, text in view_state_reads(path):
                self.assertIn(
                    (rel, fn),
                    VIEW_STATE_OK,
                    f"{rel} `{fn}` reads {text!r}: simulation keys on commands::Commander / Players, not the local view",
                )

    def test_the_view_state_guard_finds_a_read(self):
        code = "fn decide(q: Query<&Ship, With<PlayerShip>>) {}\nfn other(fleets: Res<Fleets>) { let o = fleets.local.clone(); }"
        found = [
            re.findall(r"\bfn (\w+)", code[: m.start()])[-1]
            for m in VIEW_STATE.finditer(code)
        ]
        self.assertEqual(found, ["decide", "other"])

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
