"""Where the converted game assets are for tests: tools.import_assets.find_assets (PROCYON_ASSETS, this checkout's assets/,
the main checkout's when this is a linked worktree, then the per-user folder). Tests that read them skip until the game is imported."""

import unittest
from pathlib import Path

from tools.import_assets import find_assets

ASSETS = find_assets() or Path(__file__).resolve().parents[1] / "assets"
needs_assets = unittest.skipUnless(
    (ASSETS / "ships").is_dir(), "needs imported game assets (tools/build_assets.sh)"
)
