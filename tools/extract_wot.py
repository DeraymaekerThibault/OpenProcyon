"""Run as: python -m tools.extract_wot --game-root ROOT --output /outside/catalog.json"""

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import sys

from tools.source_text import read_source_lines
from tools.wot import analyze, issue, parse_lines, properties


def extract(root: Path, *, encoding="utf-8"):
    root = root.resolve(strict=True)
    objects = (root / "WorldObjects").resolve(strict=True)
    if not objects.is_dir() or not objects.is_relative_to(root):
        raise ValueError("WorldObjects must be a directory inside the supplied root")
    files = sorted(p for p in objects.rglob("*") if p.suffix.lower() == ".wot")
    if not files:
        raise ValueError("No WOT files found in WorldObjects")
    documents, diagnostics = [], []
    for path in files:
        relative = path.relative_to(root)
        document = dict(
            path=relative.as_posix(),
            sha256=None,
            nodes=[],
            identity=None,
            classification=dict(kind="other", evidence=[]),
            relationships=[],
        )
        try:
            lines = read_source_lines(root, relative, encoding=encoding)
            document["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
            nodes, problems = parse_lines(lines)
            # File-level diagnostics still identify empty/malformed input files.
            identity, classification, relationships = analyze(nodes, problems)
            for problem in problems:
                if problem["source"] is None:
                    problem["source"] = dict(path=relative.as_posix(), line=None)
            document.update(
                nodes=nodes,
                identity=identity,
                classification=classification,
                relationships=relationships,
            )
            diagnostics.extend(problems)
        except (OSError, UnicodeError, ValueError) as error:
            issue(
                diagnostics,
                dict(path=relative.as_posix(), line=None),
                "file_read",
                str(error),
            )
        documents.append(document)

    index = defaultdict(list)
    for document in documents:
        if document["identity"]:
            index[document["identity"]["value"]].append(document)
    for name, matches in sorted(index.items()):
        if len(matches) > 1:
            for match in matches:
                issue(
                    diagnostics,
                    match["identity"]["source"],
                    "duplicate_identity",
                    f"Object Type {name!r} is declared by multiple files",
                )
    for document in documents:
        for link in document["relationships"]:
            matches = index.get(link["target"], [])
            expected = "gun" if link["kind"] == "mounted_gun" else "projectile"
            if link["kind"] == "projectile" and link["target"] == "NoBulletType":
                status = "sentinel"
            elif not matches:
                status = "missing"
            elif len(matches) != 1:
                status = "ambiguous"
            elif matches[0]["classification"]["kind"] != expected:
                status = "wrong_kind"
            else:
                status = "resolved"
            link["resolution"] = dict(
                status=status, paths=[match["path"] for match in matches]
            )
            if status not in ("resolved", "sentinel"):
                issue(
                    diagnostics,
                    link["source"],
                    "reference_" + status,
                    f"{link['kind']} target {link['target']!r}: {status}",
                    severity="warning",
                )

    return dict(
        schema_version=1,
        encoding=encoding,
        documents=documents,
        diagnostics=diagnostics,
        summary=dict(
            files=len(documents),
            categories=dict(
                sorted(Counter(d["classification"]["kind"] for d in documents).items())
            ),
            properties=sum(1 for d in documents for _ in properties(d["nodes"])),
            relationships=dict(
                sorted(
                    Counter(
                        link["resolution"]["status"]
                        for d in documents
                        for link in d["relationships"]
                    ).items()
                )
            ),
            errors=sum(d["severity"] == "error" for d in diagnostics),
            warnings=sum(d["severity"] == "warning" for d in diagnostics),
        ),
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--game-root", type=Path, default=os.environ.get("PROCYON_GAME_ROOT")
    )
    parser.add_argument(
        "--encoding",
        default="utf-8",
        help="Strict decoding; no replacement or fallback",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="New JSON file outside the project and input root",
    )
    args = parser.parse_args(argv)
    if args.game_root is None:
        parser.error("Supply --game-root or PROCYON_GAME_ROOT")
    try:
        root = args.game_root.resolve(strict=True)
        output = args.output.resolve()
        workspace = Path(__file__).resolve().parents[1]
        if output.is_relative_to(root) or output.is_relative_to(workspace):
            raise ValueError("Output must be outside both the input root and project")
        catalog = extract(root, encoding=args.encoding)
        payload = (
            json.dumps(catalog, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
        )
        # Exclusive create protects existing data, including any symlink target.
        with output.open("x", encoding="utf-8") as stream:
            stream.write(payload)
    except (OSError, ValueError, LookupError) as error:
        print(f"Extraction failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(catalog["summary"], sort_keys=True))
    return 1 if catalog["summary"]["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
