"""Export one ship's physics + combat data (guns, bullets, damage sections, hit boxes) to JSON for the game.

usage: python -m tools.export_combat <game_root> Ship_Navy_Cutter <out.json>   (scene name read from the WOT)
Raw WOT literals are kept; interpretation is the game's job. Hit geometry is MDB model 0 per layer (mesh frame): an AABB plus the flat triangle list.
"""

import difflib
import json
import sys
from pathlib import Path

from tools.export_mesh_detail import collision_lods
from tools.export_ship import export as export_physics
from tools.gamefs import ci_file, ci_files
from tools.mdb import MdbError
from tools.mdb import parse as parse_mdb
from tools.mdb_to_glb import convert, hardpoints
from tools.models import model_path
from tools.msb import parse as parse_msb
from tools.msb import parse_scene
from tools.source_text import read_source_lines
from tools.wot import parse_lines, properties

GUN_KEYS = (
    "MuzzleSpeed",
    "TimeBetweenBursts",
    "BulletsPerBurst",
    "DuringBurstReloadTime",
    "MaximumRange_Range",
    "LongRange_Range",
    "EffectiveRange_Range",
    "BulletAffectedByGravity",
    "HorizontalRotationSpeed",
    "VerticalMinAngle",
    "VerticalMaxAngle",
    "VerticalRotationSpeed",
    "MuzzleFlashEffect",
    "MaximumRange_AccuracyDeviation",
    "LongRange_AccuracyDeviation",
    "EffectiveRange_AccuracyDeviation",
    "BulletTypeName_Primary",
    "Is PointDefense Gun",
    "Is TorpedoLauncher Gun",
)
GUN_SOUND_KEYS = (
    "SoundName",
    "SoundVolume",
    "SoundMaxDistance",
    "LobAngle",
    "Is Lobbing Gun",
    "Can Be Fired While Cloaked",
    "Victory Point Cost",
    "Exclusion Race",
    "Mount Type",
    "EffectiveRange_RicochetFactor",
    "LongRange_RicochetFactor",
    "MaximumRange_RicochetFactor",
)
# Bullet WOT surface names (ImpactSounds<surface>) -> the material keys used for damage.
SURFACES = {
    "Hull Wood": "Wood",
    "Hull ReInforced Wood": "Reinforced",
    "Hull Iron": "Iron",
    "Sail Cloth": "Cloth",
    "Wall Stone": "Stone",
    "Dragon Scale": "Dragon",
}


def debris_record(root, scene, outdir):
    """A section's `debrisMeshScene`: the piece that breaks off when the section is destroyed. Its mesh scene holds one
    `...Damage_LayerN` node in the hull's model space; that node's position is the piece's pivot. Writes
    models/debris/<scene>.glb and returns {model, pivot}, or None when the scene or its mesh is missing."""
    if not scene:
        return None
    try:
        nodes = parse_msb(
            ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
        )
    except FileNotFoundError:
        return None
    layer = next((n for n in nodes if "_layer" in n.name.lower()), None)
    if layer is None:
        return None
    try:
        mdb = ci_file(Path(root) / "BD_Meshes", f"{layer.name}.mdb")
    except FileNotFoundError:
        return None
    glb = f"{scene}.glb"
    if not model_path(outdir, glb).exists():
        model_path(outdir, glb, mkdir=True).write_bytes(
            convert(root, scene, files=[mdb])
        )
    return dict(model=glb, pivot=list(layer.pos))


def destruct_record(text):
    """A section's `destructFx` ("{ Final_Ship_Chunk 2.4 2.5 ( -1.8 10.2 5.6 ) }": effect or none, start and end seconds, and where
    the burst appears) as {fx, start, end, point}; None when absent or malformed. The point is in the section's own frame; the
    file lists its numbers in the order x, z, y, so `point` is reordered to game axes (x, y, z)."""
    w = (
        (text or "")
        .replace("{", " ")
        .replace("}", " ")
        .replace("(", " ")
        .replace(")", " ")
        .split()
    )
    if len(w) != 6:
        return None
    try:
        start, end, x, z, y = (float(v) for v in w[1:])
    except ValueError:
        return None
    return dict(
        fx=None if w[0].lower() == "none" else w[0],
        start=start,
        end=end,
        point=[x, y, z],
    )


def load(root, name):
    nodes, problems = parse_lines(
        read_source_lines(
            Path(root), Path("WorldObjects") / f"{name}.wot", encoding="utf-8"
        )
    )
    if problems:
        raise ValueError(problems[:3])
    return nodes


def flat(nodes):
    return {p["name"]: p["value"] for p in properties(nodes)}


def associations(nodes, attribute, mesh=None):
    props = {n["name"]: n["value"] for n in nodes if n["kind"] == "property"}
    mesh = props.get("MeshName", mesh)
    if props.get("AttributeName") == attribute:
        yield mesh, props["DescriptorName"], props["AssociationName"]
    for n in nodes:
        if n["kind"] == "block":
            yield from associations(n["children"], attribute, mesh)


def impact_sounds(nodes):
    """Material -> non-empty sound names from the bullet's ImpactSounds<surface> blocks (one is picked at random per hit)."""
    out = {}
    for n in nodes:
        if n["kind"] != "block":
            continue
        surface = n["name"].removeprefix("ImpactSounds")
        if surface in SURFACES:
            out[SURFACES[surface]] = [
                p["value"]
                for p in n["children"]
                if p["kind"] == "property" and p["value"]
            ]
        else:
            out |= impact_sounds(n["children"])
    return out


# Bullets the original spawns itself (hard-coded, not named by any gun): the Nova Mortar's burst; and bullets of non-ship shooters
# (mission 9's dark-matter blast, the dragons' breath, a mod's storm lightning) that every export carries when the install has them.
EXTRA_BULLETS = {"Bullet_NovaMortar": ("Bullet_NovaMortar_Secondary",)}
ALWAYS_BULLETS = (
    "Bullet_DarkMatterExplosion",
    "Bullet_DragonBreath",
    "Bullet_Lightning",
)


def bullet_record(root, b, outdir):
    """One bullet's damage, sounds and effects; lob/mine/grav behaviour fields where its custom-info factory has them."""
    bn = load(root, b)
    bp = flat(bn)
    body = None
    factory = bp.get("FactoryType", "")
    mine = "MineAI" in [
        p["value"] for p in properties(bn) if p["name"] == "FactoryType"
    ]
    if factory == "TorpedoCustomInfoFactory" or mine:
        # The bullet scene name differs from its MDB layer name; use its authored MSB root mesh.
        scene_nodes = parse_msb(
            ci_file(
                Path(root) / "BD_MeshScenes", bp["Mesh Scene Name"] + ".msb"
            ).read_bytes()
        )
        mesh = next(n.name for n in scene_nodes if n.parent < 0)
        if mine:  # the mine's scene root is an empty node; its mesh is the scene's first mesh element
            mesh = parse_scene(
                ci_file(
                    Path(root) / "BD_MeshScenes", bp["Mesh Scene Name"] + ".msb"
                ).read_bytes()
            )["meshes"][0]["name"]
        body = mesh + ".glb"
        model_path(outdir, body, mkdir=True).write_bytes(
            convert(
                root, mesh, files=[ci_file(Path(root) / "BD_Meshes", mesh + ".mdb")]
            )
        )
    # Nova mortar shells author no Lifetime (they burst on landing); 30 s matches the other lob shells.
    rec = dict(
        lifetime=bp.get("Lifetime", 30.0),
        hitpoints=bp["Damage HitPoints"],
        hit_fx=bp.get("HitEffect"),
        bullet_fx=bp.get("BulletEffect"),
        impact_snd=impact_sounds(bn),
        impact_vol=bp.get("Impact Sound Volume", 1.0),
        impact_max=bp.get("Impact Sound Max Distance", 600.0),
        damage={
            m: [
                bp[f"Damage Lower Bound: {m}"],
                bp[f"Damage Upper Bound: {m}"],
                bp[f"Chance of Critical Damage: {m}"],
                bp[f"Chance of Fire: {m}"],
                bp[f"Initial Fire Strength: {m}"],
            ]
            for m in ("Wood", "Reinforced", "Iron", "Cloth", "Stone", "Dragon")
        },
    )
    rec["kind"] = factory.removesuffix("CustomInfoFactory")
    if bp.get("Decal Damage Size"):
        rec["decal"] = bp["Decal Damage Size"]
    if "MaxRadius" in bp or "Radius" in bp:
        rec["blast"] = dict(
            radius=bp.get("MaxRadius", bp.get("Radius")),
            duration=bp.get("Duration", 0.0),
        )
    if factory == "EnergyNetCustomInfoFactory":
        rec["net"] = dict(
            duration=bp["Duration"], dissipated=bp["PercentageOfEnergyDissipated"]
        )
    if mine:
        rec["mine"] = dict(
            sight=bp.get("Default SightRange", 50.0),
            speed=bp.get("Maximum speed", 20.0),
        )
    if bp.get("Travel Sound"):
        rec["travel_snd"] = [
            bp["Travel Sound"],
            bp.get("Travel Sound Volume", 1.0),
            bp.get("Travel Sound Max Distance", 50.0),
        ]
    ricochet = {
        SURFACES[k.removeprefix("RicochetSound: ")]: v
        for k, v in bp.items()
        if k.startswith("RicochetSound: ") and v
    }
    if ricochet:
        rec["ricochet_snd"] = ricochet
    if body:
        rec["glb"] = body
    if body and not mine:
        rec["physics"] = {
            k: bp[k]
            for k in (
                "Mass",
                "MaxThrust",
                "MaxSpeed",
                "RotationalFriction",
                "MaxAngularAcceleration",
            )
        }
    return rec


def kv(text):
    out = {}
    for part in text.split(","):
        if "=" in part:
            k, v = part.split("=", 1)
            out[k] = v
    return out


def scene_name(nodes):
    return flat(nodes)["Mesh Scene Name"]


def gun_visual(root, gun, outdir):
    """Base + barrel models (own GLBs in outdir), barrel pivot and muzzle points, from the gun's MSB scene.
    Base = the root node, barrel = its GunVerticalPivotPlacement child (optional), muzzles = GunMuzzlePlacement nodes
    (positions in the base's frame, like the barrel mesh: the game draws both at minus the pivot inside the pitching barrel).
    Node rotations other than yaw are ignored.
    """
    scene = scene_name(load(root, gun))
    nodes = {
        n.id: n
        for n in parse_msb(
            ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
        )
    }
    base = next(n for n in nodes.values() if n.parent < 0)
    barrel = next(
        (n for n in nodes.values() if n.kind == "GunVerticalPivotPlacement"), None
    )
    muzzles = [n for n in nodes.values() if n.kind == "GunMuzzlePlacement"]
    out = {}
    for role, n in (("base", base), ("barrel", barrel)):
        if n is None:
            continue
        glb = f"gun_{n.name}.glb"
        try:
            mdb = ci_file(Path(root) / "BD_Meshes", f"{n.name}.mdb")
        except FileNotFoundError:
            continue  # below-deck guns (2Deck) have a scene node but no mesh: invisible, still fire
        if not model_path(outdir, glb).exists():
            model_path(outdir, glb, mkdir=True).write_bytes(
                convert(root, n.name, files=[mdb])
            )
        out[role] = glb
    out["pivot"] = list(barrel.pos) if barrel else [0.0, 0.0, 0.0]
    out["muzzles"] = [list(m.pos) for m in muzzles] or [[0.0, 1.0, 0.0]]
    return out


def export(root, ship, outdir):
    nodes = load(root, ship)
    scene = scene_name(nodes)
    hp = {n.label: (n, pos) for n, pos in hardpoints(root, scene)}
    msb = {
        n.id: n
        for n in parse_msb(
            ci_file(Path(root) / "BD_MeshScenes", f"{scene}.msb").read_bytes()
        )
    }
    guns, gun_defs, bullets, skipped = [], {}, {}, []
    for _, label, assoc in associations(nodes, "GunPlacement"):
        gun, arc, bank = assoc.split(",")[:3]
        try:
            if gun not in gun_defs:
                g = flat(load(root, gun))
                if g["BulletTypeName_Primary"] == "NoBulletType":
                    raise KeyError("no bullet")
                # Cached only once complete: a gun whose model fails (mods) must not leave a definition without visual.
                gun_defs[gun] = (
                    {k: g[k] for k in GUN_KEYS}
                    | {k: g.get(k) for k in GUN_SOUND_KEYS}
                    | {"visual": gun_visual(root, gun, outdir)}
                )
            b = gun_defs[gun]["BulletTypeName_Primary"]
            for name in (b, *EXTRA_BULLETS.get(b, ())):
                if name not in bullets:
                    bullets[name] = bullet_record(root, name, outdir)
            if label not in hp:
                # Retail typos between the WOT label and the MSB hardpoint (StarHammer 'Nova_Mortar' vs 'Nova_Motar').
                close = difflib.get_close_matches(label, list(hp), n=1, cutoff=0.85)
                label = close[0] if close else label
            node, pos = hp[label]
            # Base gun nodes already use MDB coordinates; their distinct tower origins must not be added again.
            if ship.startswith(("Base_", "Island_")):
                pos = node.pos
        except (KeyError, FileNotFoundError, ValueError) as e:
            skipped.append(f"{label}:{gun}:{e}")
            continue
        parent = msb.get(node.parent)
        guns.append(
            dict(
                label=label,
                gun=gun,
                arc_deg=float(arc),
                bank=bank,
                pos=list(pos),
                rot=list(node.rot),
                yaw=node.rot[0],
                parent_layer=(
                    parent.name if parent and parent.kind == "DamageSection" else None
                ),
            )
        )
    for name in ALWAYS_BULLETS:
        # Bullet_Lightning exists only in modded installs (vanilla storm lightning is Terrain_Nebula.wot data).
        if ci_files(Path(root) / "WorldObjects", name + ".wot"):
            bullets.setdefault(name, bullet_record(root, name, outdir))

    def placements(kind):
        out = []
        for _, label, assoc in associations(nodes, kind):
            if label in hp:
                node, pos = hp[label]
                out.append(
                    dict(label=label, fx=assoc, pos=list(pos), rot=list(node.rot))
                )
        return out

    sections = []
    for mesh, label, assoc in associations(nodes, "DamageSection"):
        a = kv(assoc)
        try:
            models = parse_mdb(
                ci_file(Path(root) / "BD_Meshes", f"{mesh}.mdb").read_bytes()
            ).models
        except MdbError as e:
            # Mods ship truncated placeholder meshes for some sections: the ship keeps its other sections.
            print(f"warning: {ship} section {label}: {mesh}.mdb unreadable ({e})")
            continue
        # The triangles are the detail level retail collides with; the box also covers the full-detail model (it feeds aim points,
        # ring and radius, which were measured on the visible hull).
        m = models[min(collision_lods(root)(mesh), len(models) - 1)]
        xs = list(zip(*[v.pos for v in models[0].vertices + m.vertices], strict=True))
        sections.append(
            dict(
                layer=mesh,
                label=label,
                material=a["material"],
                hp=float(a["hitpoints"]),
                vital_ship=a["vitalToShip"] == "1",
                vital_speed=a["vitalToMaxVelocity"] == "1",
                vital_turn=a["vitalToManeuverability"] == "1",
                vital_mission=a.get("vitalToMission") == "1",
                core_pct=float(a.get("vitalSectionCoreDamagePercent", 0)),
                # Neighbouring sections (layer names) a destroyed section passes damage on to.
                adjacent=[x for x in a.get("adjacent", "").split("+") if x],
                # System tags the section carries (crew speech when it is destroyed on the player's ship).
                criticals=[x for x in a.get("criticals", "").split("+") if x],
                debris_fx=a.get("debrisFx"),
                destruct_fx=(
                    a.get("destructFx", "").replace("{", "").split() or [None]
                )[0],
                destruct=destruct_record(a.get("destructFx")),
                debris=debris_record(root, a.get("debrisMeshScene"), outdir),
                aabb=[[min(c) for c in xs], [max(c) for c in xs]],
                tris=[
                    round(c, 3)
                    for a, b, c3, _ in m.faces
                    for i in (a, b, c3)
                    for c in m.vertices[i].pos
                ],
            )
        )
    p = flat(nodes)
    return dict(
        entity=ship,
        scene=scene,
        race=p.get("Ship Race", ""),
        **lifeboat_fields(p, sections),
        physics=base_safe_physics(root, ship),
        core_hp=float(p["Core Damage Section Max HitPoints"]),
        sections=sections,
        guns=guns,
        gun_defs=gun_defs,
        bullets=bullets,
        skipped_guns=skipped,
        engines=placements("EnginePortPlacement"),
        wakes=placements("WakePlacement"),
        flags=placements("FlagAttachmentPoint"),
        explosion_fx=p.get("Explosion Effect Name"),
        sight=float(p.get("Default SightRange", 400.0)),
        repair_fx=p.get("Repair Effect Name"),
        cloakable=p.get("Is Cloakable", False) is True,
        # Fleet-screen facts: point cost, crew posts ('Max number of <role>s'), display name string and ship-bar icon.
        points=int(p.get("Victory Point Cost", 0)),
        # Retail's own flag for ships the single-player fleet screens offer.
        skirmish=p.get("Available in Multiplayer?") is True,
        crew_posts={
            k[len("Max number of ") :]: int(v)
            for k, v in p.items()
            if k.startswith("Max number of ") and isinstance(v, int)
        },
        display_name=p.get("Displayable Shipname String ID"),
        icon=p.get("ShipBar Icon Texture"),
        engine_snd=[
            p.get(f"EngineSoundName {k}") for k in ("Half", "Full", "Emergency")
        ],
        # Selection ring radius as a fraction of the ship's radius (0.45-1.1 across the retail ships).
        ring=p.get("Selected Indicator Percentage Of Radius"),
    )


def base_safe_physics(root, ship):
    """Ship physics; bases author none (they never move), so they get static placeholders (Mass 1, no thrust/speed)."""
    try:
        return export_physics(root, ship)["properties"]
    except ValueError:
        if not ship.startswith(("Base_", "Island_")):
            raise
        zero = {"value": 0.0}
        return dict.fromkeys(
            ("MaxThrust", "MaxSpeed", "RotationalFriction", "MaxAngularAcceleration"),
            zero,
        ) | {"Mass": {"value": 1.0}, "CenterOfMass": {"value": [0.0, 0.0, 0.0]}}


def lifeboat_fields(p, sections):
    """Lifeboat metadata (docs/re/lifeboats.md): the explicit class flag, how many boats a destroyed ship leaves, the collision response
    type and the bounding sphere the pickup test uses. Without an authored sphere it is derived (PROVISIONAL): the authored box
    extents if any, else the union of the hit sections' boxes; a sphere around that box.
    """

    def vec(v):
        return list(v) if isinstance(v, (list, tuple)) else None

    sphere = dict(
        center=vec(p.get("LocalPosition")) or [0.0, 0.0, 0.0],
        radius=p.get("Radius"),
        authored=bool(p.get("User defined sphere size")),
    )
    if not sphere["authored"] or sphere["radius"] is None:
        lo, hi = vec(p.get("MinExtents")), vec(p.get("MaxExtents"))
        if not (lo and hi):
            lo, hi = (
                [min(s["aabb"][0][i] for s in sections) for i in range(3)],
                [max(s["aabb"][1][i] for s in sections) for i in range(3)],
            )
        sphere = dict(
            center=[(a + b) / 2 for a, b in zip(lo, hi, strict=True)],
            radius=sum((b - a) ** 2 for a, b in zip(lo, hi, strict=True)) ** 0.5 / 2,
            authored=False,
        )
    return dict(
        lifeboats=int(p.get("Number of lifeboats", 0)),
        is_lifeboat=bool(p.get("Is Lifeboat", False)),
        response=p.get("ResponseType", "Ship"),
        sphere=sphere,
    )


def fx_names(j):
    """Every effect name a ship export refers to."""
    names = {j.get("explosion_fx")}
    names |= {e["fx"] for e in j["engines"] + j["wakes"]}
    names |= {g.get("MuzzleFlashEffect") for g in j["gun_defs"].values()}
    for b in j["bullets"].values():
        names |= {b.get("hit_fx"), b.get("bullet_fx")}
    for s in j["sections"]:
        names |= {s.get("debris_fx"), s.get("destruct_fx")}
    return {n for n in names if n and n not in ("none", "NoEffect")}


if __name__ == "__main__":
    root, ship, out = sys.argv[1:4]
    j = export(root, ship, Path(out).parent)
    Path(out).write_text(json.dumps(j))
    print(
        "wrote",
        out,
        f"{len(j['guns'])} guns, {len(j['sections'])} sections, skipped {j['skipped_guns']}",
    )
