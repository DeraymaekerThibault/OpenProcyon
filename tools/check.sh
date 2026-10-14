#!/bin/sh
# The validation gate: format, lint, repository hygiene and tests. usage: tools/check.sh
# Rust tests load the converted game assets; without an import (tools/build_assets.sh) they are compiled but not run.
set -e
cd "$(dirname "$0")/.."
imported=$(python3 -c 'import sys; sys.path.insert(0, "tests"); from imported import ASSETS; print(int((ASSETS / "ships").is_dir()))')
cd game
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
if [ "$imported" = 1 ]; then
  cargo test --release
else
  cargo test --release --no-run
  echo "NOTE: Rust tests compiled but not run: no imported game assets (run tools/build_assets.sh --game <install>)." >&2
fi
cd ..
python3 -m unittest discover -s tests
