"""Auditable parser for the observed line-oriented WOT subset, not engine semantics."""

import math
import re

from tools.source_text import SourceLine

HEADER = re.compile(r"([0-9]{8,})\s+(.+)")
TYPE_TOKEN = re.compile(r"\s+(String|Int|Float|Bool|Vector3|Matrix33|Coord|Colour)\b")
NUMBER = re.compile(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?")
FACTORIES = {
    "ShipCustomInfoFactory": "ship",
    "GunCustomInfoFactory": "gun",
    "ProjectilePhysics": "projectile",
    "TorpedoPhysics": "projectile",
    "MinePhysics": "projectile",
}


def issue(diagnostics, source, code, message, severity="error"):
    diagnostics.append(
        dict(source=source, code=code, message=message, severity=severity)
    )


def decode_value(type_name, raw):
    if (
        type_name == "String"
        and raw.startswith("'")
        and raw.endswith("'")
        and len(raw) >= 2
    ):
        return raw[
            1:-1
        ]  # No escape syntax has been established; keep backslashes literal.
    if type_name == "Bool" and raw in ("True", "False"):
        return raw == "True"
    if type_name == "Int" and re.fullmatch(r"[+-]?[0-9]+", raw):
        return int(raw)
    if type_name == "Float" and NUMBER.fullmatch(raw):
        value = float(raw)
        if math.isfinite(value):
            return value
    if (
        type_name in ("Vector3", "Matrix33", "Coord", "Colour")
        and raw.startswith("(")
        and raw.endswith(")")
    ):
        parts = raw[1:-1].split(",")
        if (
            len(parts)
            == {"Vector3": 3, "Matrix33": 9, "Coord": 2, "Colour": 4}[type_name]
        ):
            return [decode_value("Float", part.strip()) for part in parts]
    raise ValueError("Unsupported or invalid typed value")


def parse_lines(lines: list[SourceLine]):
    nodes, diagnostics = [], []
    stack = [dict(children=nodes)]
    pending = None
    for line in lines:
        text = line.text.strip()
        if not text:
            continue
        source = dict(path=line.path, line=line.line)
        node = dict(source=source, raw=line.text)
        if text.startswith("#"):
            stack[-1]["children"].append(dict(node, kind="comment"))
            continue
        if pending is not None:
            if text == "{":
                pending["open_source"] = source
                stack.append(pending)
                pending = None
                continue
            issue(
                diagnostics,
                pending["source"],
                "missing_open",
                "Block header requires an opening brace",
            )
            pending = None
        if text == "{":
            issue(
                diagnostics,
                source,
                "unexpected_open",
                "Opening brace has no block header",
            )
            block = dict(node, kind="block", name=None, count_token=None, children=[])
            stack[-1]["children"].append(block)
            stack.append(block)
            continue
        if text == "}":
            if len(stack) == 1:
                issue(
                    diagnostics,
                    source,
                    "unexpected_close",
                    "Closing brace has no open block",
                )
                nodes.append(dict(node, kind="unparsed"))
            else:
                stack.pop()["close_source"] = source
            continue
        header = HEADER.fullmatch(text)
        if header:
            pending = dict(
                node, kind="block", count_token=header[1], name=header[2], children=[]
            )
            stack[-1]["children"].append(pending)
            continue
        candidates = list(TYPE_TOKEN.finditer(text))
        for token in candidates:
            raw_value = text[token.end() :].strip()
            try:
                value = decode_value(token[1], raw_value)
            except ValueError:
                continue
            node.update(
                kind="property",
                name=text[: token.start()],
                type=token[1],
                value=value,
                raw_value=raw_value,
            )
            break
        else:
            node["kind"] = "unparsed"
            issue(
                diagnostics,
                source,
                "invalid_property" if candidates else "unsupported_line",
                "Line retained verbatim; no supported name/type/value form matched",
            )
        stack[-1]["children"].append(node)
    if pending is not None:
        issue(
            diagnostics,
            pending["source"],
            "missing_open",
            "Block header at end of file",
        )
    for block in stack[1:]:
        issue(
            diagnostics, block["source"], "unclosed_block", "Block has no closing brace"
        )
    return nodes, diagnostics


def parse_text(text, source="<text>"):
    return parse_lines(
        [SourceLine(source, i, line) for i, line in enumerate(text.splitlines(), 1)]
    )


def fields(nodes):
    return {n["name"]: n["value"] for n in nodes if n["kind"] == "property"}


def blocks(nodes, name):
    for n in nodes:
        if n["kind"] == "block":
            if n["name"] == name:
                yield n
            yield from blocks(n["children"], name)


def records(nodes, key):
    """Split sibling properties/blocks at a repeated record-start property."""
    record = []
    for node in nodes:
        if node["kind"] == "property" and node["name"] == key:
            if record:
                yield record
            record = [node]
        elif record:
            record.append(node)
    if record:
        yield record


def objects(nodes):
    """A world placement is a sibling Type property followed by its State block."""
    pending = {}
    for node in nodes:
        if node["kind"] == "property":
            pending[node["name"]] = node["value"]
        elif node["kind"] == "block":
            if node["name"] == "State" and "Type" in pending:
                state = fields(node["children"])
                if "Position" in state and "Orientation" in state:
                    yield dict(
                        type=pending["Type"],
                        id=pending.get("ID"),
                        pos=state["Position"],
                        rot=state["Orientation"],
                        player=state.get("PlayerIndex"),
                    )
                pending = {}
            else:
                yield from objects(node["children"])


def properties(nodes):
    """Visit every property in order, including repeated names and unknown keys."""
    for node in nodes:
        if node["kind"] == "property":
            yield node
        elif node["kind"] == "block":
            yield from properties(node["children"])


def analyze(nodes, diagnostics):
    top = [node for node in nodes if node["kind"] == "property"]
    types = [
        node for node in top if node["name"] == "Type" and node["type"] == "String"
    ]
    first_source = nodes[0]["source"] if nodes else None
    if len(types) != 1 or not types[0]["value"]:
        issue(
            diagnostics,
            first_source,
            "object_type",
            "Expected exactly one nonempty top-level Type String",
        )
    evidence = [
        node
        for node in top
        if node["name"] == "FactoryType"
        and node["type"] == "String"
        and node["value"] in FACTORIES
    ]
    kinds = {FACTORIES[node["value"]] for node in evidence}
    category = next(iter(kinds)) if len(kinds) == 1 else "other"
    if len(kinds) > 1:
        issue(
            diagnostics,
            first_source,
            "ambiguous_category",
            "Conflicting factory-based classifications",
        )
    classification = dict(
        kind=category,
        evidence=[dict(value=n["value"], source=n["source"]) for n in evidence],
    )
    identity = (
        dict(value=types[0]["value"], source=types[0]["source"])
        if len(types) == 1
        else None
    )
    relationships = []
    if category == "gun":
        for field in properties(nodes):
            if field["name"] in ("BulletTypeName_Primary", "BulletTypeName_Secondary"):
                if field["type"] != "String":
                    issue(
                        diagnostics,
                        field["source"],
                        "reference_type",
                        "Projectile reference must be a String",
                    )
                else:
                    relationships.append(
                        dict(
                            kind="projectile",
                            role=field["name"],
                            target=field["value"],
                            source=field["source"],
                        )
                    )

    def mounts(children):
        fields = [n for n in children if n["kind"] == "property"]
        attributes = [n for n in fields if n["name"] == "AttributeName"]
        if any(n["value"] == "GunPlacement" for n in attributes):
            associations = [n for n in fields if n["name"] == "AssociationName"]
            if (
                len(attributes) != 1
                or len(associations) != 1
                or associations[0]["type"] != "String"
            ):
                issue(
                    diagnostics,
                    attributes[0]["source"],
                    "ambiguous_mount",
                    "GunPlacement requires one String AssociationName",
                )
            else:
                association = associations[0]
                target, separator, rest = association["value"].partition(",")
                if not target.strip():
                    issue(
                        diagnostics,
                        association["source"],
                        "empty_mount",
                        "GunPlacement has no target",
                    )
                else:
                    relationships.append(
                        dict(
                            kind="mounted_gun",
                            target=target.strip(),
                            arguments_raw=rest if separator else "",
                            source=association["source"],
                            attribute_source=attributes[0]["source"],
                            descriptors=[
                                dict(value=n["value"], source=n["source"])
                                for n in fields
                                if n["name"] == "DescriptorName"
                            ],
                        )
                    )
        for node in children:
            if node["kind"] == "block":
                mounts(node["children"])

    mounts(nodes)
    return identity, classification, relationships
