"""Where the converted game assets are for tests, found like the game does (game/src/lib.rs `base_assets`):
PROCYON_ASSETS, else assets/ in the checkout, else the per-user folder. Tests that read them skip until the game is imported."""

import os
import unittest
from pathlib import Path

from tools.import_assets import user_dir


def _find():
    if os.environ.get("PROCYON_ASSETS"):
        return Path(os.environ["PROCYON_ASSETS"])
    checkout = Path(__file__).resolve().parents[1] / "assets"
    if (checkout / "ships").is_dir():
        return checkout
    data = user_dir()
    return data / "assets" if data else checkout


ASSETS = _find()
needs_assets = unittest.skipUnless(
    (ASSETS / "ships").is_dir(), "needs imported game assets (tools/build_assets.sh)"
)
