"""Parser for hud.hdt (in-game HUD definition) -> {component: {skin: {...}}}.

Components are `<hex> Name { ... }` blocks. Per-skin keys look like
`TextureName - NoCursor ( Navy ) String 'x'`, `TextureArea - NoCursor ( Navy ) Rect( l, t, r, b )`,
`TopLeft X ( Navy ) Int 5`. Anything else is kept under skin "_" as raw `key -> value`.
"""

import json
import re
import sys

SKIN = re.compile(r"^(?P<key>.+?)\s*\(\s*(?P<skin>\w+)\s*\)\s*(?P<rest>.*)$")


def parse(text):
    # Components are exactly one tab deep; braces are ignored because the retail file has unbalanced ones.
    comps, cur = {}, None
    for raw in text.replace("\r", "").split("\n"):
        m = re.match(r"^\t([0-9a-f]{8}) (.+?)\s*$", raw)
        if m:
            cur = comps.setdefault(m.group(2), {})
            continue
        s = raw.split("#")[0].strip()
        if cur is None or not s or s in ("{", "}", ")"):
            continue
        m = SKIN.match(s)
        if m:
            key, skin, rest = m["key"].strip(), m["skin"], m["rest"]
            d = cur.setdefault(skin, {})
            r = re.match(
                r"Rect\(\s*(-?\d+),\s*(-?\d+),\s*(-?\d+),\s*(-?\d+)\s*\)", rest
            )
            if r:
                d[key] = [int(x) for x in r.groups()]
            else:
                parts = rest.split(None, 1)
                v = parts[1] if len(parts) > 1 else ""
                d[key] = (
                    v.strip("'")
                    if parts and parts[0] == "String"
                    else (
                        int(v)
                        if parts and parts[0] == "Int"
                        else float(v)
                        if parts and parts[0] == "Float"
                        else v
                    )
                )
        else:
            parts = s.split()
            for i, w in enumerate(parts):
                if w in ("Int", "Float", "String", "Bool"):
                    v = " ".join(parts[i + 1 :]).strip("'")
                    cur.setdefault("_", {})[" ".join(parts[:i])] = (
                        int(v) if w == "Int" else float(v) if w == "Float" else v
                    )
                    break
    return comps


if __name__ == "__main__":
    with open(sys.argv[1], encoding="latin1") as f:
        print(json.dumps(parse(f.read()), indent=1))
