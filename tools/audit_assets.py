#!/usr/bin/env python3
"""List converted assets whose name is never referenced by game code or JSON data (hint of unimplemented features).
Heuristic: stem (case-insensitive) must appear in game/src or any assets/**/*.json outside the file itself.
Directory-scanned assets and prefix-built names give false positives; review by group."""

import collections
import pathlib

root = pathlib.Path(__file__).resolve().parents[1]
corpus = []
for p in list((root / "game/src").rglob("*.rs")) + list(
    (root / "assets").rglob("*.json")
):
    corpus.append(p.read_text(errors="ignore").lower())
corpus = "\n".join(corpus)
unused = collections.defaultdict(list)
for p in sorted((root / "assets").rglob("*")):
    if not p.is_file() or p.suffix == ".json":
        continue
    if p.stem.lower() not in corpus:
        unused[str(p.relative_to(root / "assets").parent)].append(p.name)
tot = 0
for d, fs in unused.items():
    tot += len(fs)
    print(f"{d}: {len(fs)}  e.g. {', '.join(fs[:6])}")
print("total unreferenced:", tot)
