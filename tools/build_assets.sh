#!/usr/bin/env bash
# Import: convert the original install into the game's asset folder. Nothing from the install is modified.
# usage: tools/build_assets.sh [--game DIR] [--out DIR] [Ship_Name ...]
# The steps live in tools/import_assets.py (python -m tools.import_assets, also without bash); this wrapper keeps the old entry point
# and resolves a relative --out against the checkout, as before.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 -m tools.import_assets "$@"
