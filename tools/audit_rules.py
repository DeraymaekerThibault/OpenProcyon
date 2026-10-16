#!/usr/bin/env python3
"""Rule-vocabulary coverage: every condition/action Type used by shipped maps/missions vs what rules/eval.rs interprets.
Classes: UNHANDLED (warns), NO-OP (accepted, does nothing), OK (has logic)."""

import collections
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[1]
src = (root / "game/src/rules/eval.rs").read_text()
lits = set(re.findall(r'"([^"\n]+)"', src))
m = re.search(r"// Setup actions the remake does not need.*?=> \{\}", src, re.DOTALL)
noop = set(re.findall(r'"([^"\n]+)"', m.group(0))) if m else set()
noop.add("*State Init* Setup Nebula")  # empty arm, handled in environment.rs
use = collections.defaultdict(lambda: collections.defaultdict(int))
for p in sorted((root / "assets/maps").glob("*/mission.json")) + sorted(
    (root / "assets/maps").glob("*/map.json")
):
    d = json.loads(p.read_text())
    for r in d.get("rules", []):
        for kind in ("conditions", "actions"):
            for x in r.get(kind, []):
                use[(kind[0], x.get("Type", "?"))][
                    p.parent.name.replace("mission", "M")
                ] += 1
rows = {"UNHANDLED": [], "NO-OP": []}
for (k, t), maps in sorted(use.items()):
    cls = (
        "OK" if t in lits and t not in noop else ("NO-OP" if t in noop else "UNHANDLED")
    )
    if cls != "OK":
        rows[cls].append(
            f"{'cond' if k == 'c' else 'act '} {t}  [{', '.join(f'{m}x{n}' for m, n in sorted(maps.items()))}]"
        )
total = len(use)
for c, entries in rows.items():
    print(f"## {c} ({len(entries)} of {total} distinct types used)")
    print("\n".join(entries))
