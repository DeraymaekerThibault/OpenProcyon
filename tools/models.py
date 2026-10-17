"""Where converted GLBs live: assets/models/<category>/<Name>.glb. The category is derived from the name (single source of truth)."""

from pathlib import Path


def category(name):
    n = name.removesuffix(".glb")
    if n.startswith(("gun_", "LaserTorpedo")):
        return "weapons"
    if n.startswith("skybox_"):
        return "sky"
    if n.startswith("Base"):
        return "bases"
    if n.startswith("SELECTION_INDICATOR"):
        return "ui"
    if "Broken" in n:
        return "debris"  # pieces that break off destroyed ship sections (WOT debrisMeshScene)
    if n.endswith(("Ship", "ShipWrecked")):
        return "ships"
    return "scenery"  # islands, asteroids, volcanoes, buoy, nebula, whale


def model_path(assets, name, mkdir=False):
    """Path of `name` (with or without .glb) under the assets root; mkdir creates the category folder for writing."""
    n = name.removesuffix(".glb")
    p = Path(assets) / "models" / category(n) / f"{n}.glb"
    if mkdir:
        p.parent.mkdir(parents=True, exist_ok=True)
    return p
