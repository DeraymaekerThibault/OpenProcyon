"""Reader for Effects/*.eft (EffectStream: particle-system primitives with keyed curves) -> plain JSON-able dicts.

Same block text format as WOT. Conversion rules:
- a block with NumKeys becomes {"curve": [[t, value], ...]} plus active LoopInfo metadata;
- other blocks become dicts of their scalar properties and child blocks (LoopInfo blocks dropped);
- a RenderInfo/MovementInfo block is keyed by the *InfoType property that precedes it (e.g. "ColorFaderChunk");
- repeated block names become lists. 'Primitive' is always a list.
Semantics (what a curve drives) are the consumer's business; this only restructures the text.
"""

from pathlib import Path

from tools.wot import parse_lines
from tools.source_text import SourceLine


def _curve(children):
    ts = [n["value"] for n in children if n["kind"] == "property" and n["name"] == "t"]
    vs = [
        n["value"] for n in children if n["kind"] == "property" and n["name"] == "value"
    ]
    if len(ts) != len(vs):
        raise ValueError("curve with unequal t/value counts")
    out = {"curve": [[t, v] for t, v in zip(ts, vs)]}
    for n in children:
        if n["kind"] == "block" and n["name"] == "LoopInfo":
            loop = _block(n["children"])
            if loop.get("NumberLoops", 0) > 0 and loop.get("EndLoopingT", 0) > loop.get(
                "StartLoopingT", 0
            ):
                out["loop"] = loop
    return out


def _block(children):
    if any(n["kind"] == "property" and n["name"] == "NumKeys" for n in children):
        return _curve(children)
    out, info_type = {}, None
    for n in children:
        if n["kind"] == "property":
            if n["name"] in ("RenderInfoType", "MovementInfoType"):
                info_type = n["value"]
            elif n["name"] in out:
                out[n["name"]] = (
                    out[n["name"]]
                    if isinstance(out[n["name"]], list)
                    else [out[n["name"]]]
                )
                out[n["name"]].append(n["value"])
            else:
                out[n["name"]] = n["value"]
        elif n["kind"] == "block" and n["name"] != "LoopInfo":
            key = (
                info_type if n["name"] in ("RenderInfo", "MovementInfo") else n["name"]
            )
            val = _block(n["children"])
            if n["name"] == "Primitive" or key in out:
                if key not in out:
                    out[key] = []
                elif not isinstance(out[key], list):
                    out[key] = [out[key]]
                out[key].append(val)
            else:
                out[key] = val
    return out


def parse_eft(text, path="<eft>"):
    lines = [
        SourceLine(path, i, t)
        for i, t in enumerate(text.replace("\r", "").split("\n"), 1)
    ]
    nodes, problems = parse_lines(lines)
    if problems:
        raise ValueError(problems[:3])
    stream = next(
        n for n in nodes if n["kind"] == "block" and n["name"] == "EffectStream"
    )
    return _block(stream["children"])


def load(root, name):
    from tools.gamefs import ci_file

    p = ci_file(Path(root) / "Effects", f"{name}.eft")
    return parse_eft(p.read_text(encoding="latin-1"), p.name)
