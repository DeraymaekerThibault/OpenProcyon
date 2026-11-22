#!/usr/bin/env python3
"""Writes the Type tables of docs/rule-types.md from the rule schema.

    openprocyon --dump-rule-schema schema.json
    python tools/gen_rule_types.py schema.json          # or: --bin path/to/openprocyon

Everything between `<!-- generated:begin -->` and `<!-- generated:end -->` is rebuilt from the schema (names, fields, kinds, defaults,
required, origin) and the hand-written explanations in tools/rule_types_notes.json (group order, group intros, one note per Type).
Text outside the markers is kept. Fails when a Type has no note or no group, or a note names a Type that no longer exists.
Standard library only; output is deterministic.
"""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "docs" / "rule-types.md"
NOTES = ROOT / "tools" / "rule_types_notes.json"
BEGIN, END = "<!-- generated:begin -->", "<!-- generated:end -->"

HEAD = """# Mission rule Types

Reference of every condition and action `Type` a `mission.json` rule can use, for mod authors. How rules fit together (folder
layout, the keys of `mission.json`, a worked example) is in `docs/missions.md`; the engine-added extensions are introduced in
`docs/modding.md`. The tables below are generated from the rule schema (`--dump-rule-schema`, `game/src/rules/eval/schema.rs`) by
`tools/gen_rule_types.py`: do not edit between the markers, edit `tools/rule_types_notes.json` and re-run the script.

## Conventions

- A rule is `{"name", "Run Once", "Is Active", "conditions": [...], "actions": [...]}`. All conditions must hold; actions run in order.
  Every condition or action is an object with a `"Type"` and the fields listed below. Field names are exact, spaces, odd capitals
  and typos included.
- **Booleans are the strings `"TRUE"` / `"FALSE"`.** A JSON `true` reads as FALSE. Only the rule field `Is Active` and the task keys
  take JSON booleans.
- **Numbers must be JSON numbers.** A number written as a string reads as 0. Fields marked as a fraction (0..1) are fractions:
  damage, throttle, vulnerability and volume are `0.5`, never `50`.
- A missing field reads as its default. A field marked required is a mistake when absent: its default is a silent no-op or a condition
  that never holds. `--check-mission` reports it.
- A group spec is `Group` (a group of the mission, or an owner when no group has the name) or `Group,Unit` (one named ship).
  A misspelled name has no members: conditions on it are false, actions on it do nothing. `Player0 Group` always means the human player.
- `Equivalence` is `Greater Than`, `Less Than` or `Equal To`; anything else is false. `Equal To` on a measured number is rarely useful.
- Event conditions (kills, captures, entering a volume once per unit) count events, and are true when **more than** the quota has
  been seen since the rule last fired. A quota of 0 therefore means "at least one".
- An unknown `Type` is logged once, a condition is then false and an action does nothing.
- Origin: **retail** Types come from the original game's rule editor. **engine** Types were added by OpenProcyon
  and start with `OP `; the original ignores or rejects them.
- "ignored" in a field's description means the retail editor exports it but the engine never reads it.
- Things written PROVISIONAL are not verified against the original.

"""


def esc(t):
    return t.replace("|", "\\|")


def field_cell(f):
    kind = f["kind"]
    if kind == "enum":
        kind = "one of " + ", ".join("`%s`" % v for v in f.get("values", []))
    bits = [kind]
    if "range" in f:
        lo, hi = f["range"]
        bits.append(
            "fraction %g..%g" % (lo, hi)
            if (lo, hi) == (0.0, 1.0)
            else "range %g..%g" % (lo, hi)
        )
    if not f["used"]:
        bits.append("ignored")
    elif f["required"]:
        bits.append("required")
    if f["default"] not in ("", None):
        bits.append("default `%s`" % f["default"])
    if f.get("soft"):
        bits.append("a broken reference is only a warning")
    return "`%s` (%s)" % (esc(f["name"]), "; ".join(bits))


def render(schema, notes):
    types = {t["name"]: t for t in schema["types"]}
    if len(types) != len(schema["types"]):
        sys.exit("duplicate Type names in the schema")
    grouped = [n for g in notes["groups"] for n in g["types"]]
    problems = []
    problems += ["no group for Type: " + n for n in sorted(set(types) - set(grouped))]
    problems += [
        "group names unknown Type: " + n for n in sorted(set(grouped) - set(types))
    ]
    problems += [
        "Type listed in two groups: " + n
        for n in sorted({n for n in grouped if grouped.count(n) > 1})
    ]
    problems += [
        "no note for Type: " + n for n in sorted(set(types) - set(notes["notes"]))
    ]
    problems += [
        "note for unknown Type: " + n for n in sorted(set(notes["notes"]) - set(types))
    ]
    if problems:
        sys.exit("\n".join(problems))
    out = []
    for g in notes["groups"]:
        out.append("## " + g["title"] + "\n")
        out.append(g["intro"] + "\n")
        out.append("| Type | Kind | Origin | Fields | What it does |")
        out.append("|---|---|---|---|---|")
        for n in g["types"]:
            t = types[n]
            kind = "condition" if t["role"] == "condition" else "action"
            if t["handler"] == "victory":
                kind += " (end rule)"
            fields = "<br>".join(field_cell(f) for f in t["fields"]) or "none"
            out.append(
                "| `%s` | %s | %s | %s | %s |"
                % (esc(n), kind, t["origin"], fields, esc(notes["notes"][n]))
            )
        out.append("")
    return "\n".join(out)


def main(argv):
    if len(argv) == 3 and argv[1] == "--bin":
        tmp = Path(tempfile.mkdtemp()) / "schema.json"
        subprocess.run([argv[2], "--dump-rule-schema", str(tmp)], check=True)
        src = tmp
    elif len(argv) == 2:
        src = Path(argv[1])
    else:
        sys.exit(__doc__)
    schema = json.loads(src.read_text(encoding="utf-8"))
    notes = json.loads(NOTES.read_text(encoding="utf-8"))
    body = render(schema, notes)
    block = BEGIN + "\n" + body + END + "\n"
    if DOC.exists() and BEGIN in (old := DOC.read_text(encoding="utf-8")):
        a, rest = old.split(BEGIN, 1)
        tail = rest.split(END, 1)[1].lstrip("\n") if END in rest else ""
        text = a + block + ("\n" + tail if tail else "")
    else:
        text = HEAD + block
    DOC.write_text(text, encoding="utf-8", newline="\n")
    print("wrote %s (%d Types)" % (DOC, len(schema["types"])))


if __name__ == "__main__":
    main(sys.argv)
