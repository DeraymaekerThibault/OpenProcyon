"""Where the converted game assets are for tests, found like the game does (game/src/lib.rs `base_assets`):
PROCYON_ASSETS, else assets/ in the checkout, else the per-user folder. Tests that read them skip until the game is imported."""

import os
import unittest
from pathlib import Path


def _find():
    if os.environ.get("PROCYON_ASSETS"):
        return Path(os.environ["PROCYON_ASSETS"])
    checkout = Path(__file__).resolve().parents[1] / "assets"
    if (checkout / "ships").is_dir():
        return checkout
    data = os.environ.get("XDG_DATA_HOME") or (os.environ.get("HOME") and str(Path(os.environ["HOME"]) / ".local/share")) or os.environ.get("APPDATA")
    return Path(data) / "openprocyon/assets" if data else checkout


ASSETS = _find()
needs_assets = unittest.skipUnless((ASSETS / "ships").is_dir(), "needs imported game assets (tools/build_assets.sh)")
