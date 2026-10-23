"""Package a player download: the release game binary, content/ and the importer frozen by PyInstaller. No game content.
usage: python -m tools.package_release NAME   (after `cargo build --release` in game/; needs requirements-release.txt)
Writes dist/NAME/ (the folder players unpack) and, outside Windows, dist/NAME.tar.gz (keeps the executable bits).
Used by .github/workflows/release.yml; runs the same locally."""

import importlib.metadata
import json
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SUFFIX = ".exe" if sys.platform == "win32" else ""
# Files the frozen importer reads from the checkout (tools resolve them next to their own module, see export_voices/trim_hud_rects).
IMPORTER_DATA = ["content/voice/events.json", "content/data/hud/bar.json"]

PLAY = """OpenProcyon: an unofficial, open-source engine for Treasure Planet: Battle at Procyon (2002).
This download contains no game content: you need your own copy of the original game.

1. Start {game} (double-click it). The first time, it finds your install (Steam or disc install, or type, paste or drop
   the game folder) and converts it once (read-only, a minute or two) into {assets}, then the game starts.
2. Or convert from a terminal in this folder:
     {importer} --game "<path to Treasure Planet Battle at Procyon>"
   pass --out <dir> to put the assets elsewhere and set PROCYON_ASSETS to that folder.
   Then run {game} (add --mission 1 to go straight into Mission 1).

OpenProcyon is free software under the GNU GPL v3.0 or later (LICENSE); source code:
https://github.com/DeraymaekerThibault/OpenProcyon
content/README.md lists the licences of the files in content/ (fonts, hand-written data);
THIRD-PARTY-NOTICES.txt those of the libraries inside the game and the importer.
Not affiliated with or endorsed by Disney or the original developers.
"""


LICENCE_NAMES = ("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")
# What the frozen importer bundles besides the Python runtime (PyInstaller's bootloader is GPL with a linking exception).
BUNDLED_PYTHON = ["numpy", "pillow", "pyinstaller"]


def licence_files(folder):
    return sorted(
        p
        for p in folder.iterdir()
        if p.is_file() and p.name.upper().startswith(LICENCE_NAMES)
    )


def third_party_notices():
    """THIRD-PARTY-NOTICES.txt: the licence texts of every Rust crate the game links on this platform and of the Python
    runtime and packages inside the importer. Identical texts are written once, under the list of packages that use them."""
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version=1", "--locked"],
            cwd=REPO / "game",
            capture_output=True,
            check=True,
            text=True,
        ).stdout
    )
    # cargo tree resolves the dependencies for the host platform, i.e. the platform being packaged.
    tree = subprocess.run(
        [
            "cargo",
            "tree",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--locked",
        ],
        cwd=REPO / "game",
        capture_output=True,
        check=True,
        text=True,
    ).stdout
    linked = {tuple(line.split()[:2]) for line in tree.splitlines() if line.strip()}
    packages = []  # (label, licence expression, licence files)
    for p in meta["packages"]:
        if (p["name"], f"v{p['version']}") not in linked or p["name"] == "openprocyon":
            continue
        authors = ", ".join(p["authors"]) or p.get("repository") or "see the crate"
        files = licence_files(Path(p["manifest_path"]).parent)
        packages.append(
            (f"{p['name']} {p['version']} ({authors})", p["license"] or "", files)
        )
    runtime = licence_files(Path(sys.base_prefix)) + sorted(
        Path(sys.base_prefix).glob("lib*/python3*/LICENSE*")
    )
    packages.append((f"Python {sys.version.split()[0]}", "PSF-2.0", runtime))
    for name in BUNDLED_PYTHON:
        d = importlib.metadata.distribution(name)
        # Only the dist-info's own licence files: numpy and Pillow list the libraries their wheels bundle in them.
        files = [
            Path(d.locate_file(f))
            for f in d.files or []
            if ".dist-info" in f.parts[0] and f.name.upper().startswith(LICENCE_NAMES)
        ]
        packages.append(
            (
                f"{d.metadata['Name']} {d.version}",
                d.metadata.get("License-Expression") or d.metadata.get("License") or "",
                files,
            )
        )
    texts = {}  # text with whitespace collapsed -> (text, labels of the packages that ship it)
    lines = [
        "OpenProcyon links or bundles the following third-party software. Their licences follow, each text once, after the",
        "packages that ship it.",
        "",
    ]
    for label, expr, files in sorted(packages, key=lambda p: p[0].lower()):
        lines.append(f"{label}: {expr}")
        for f in files:
            text = f.read_text(encoding="utf-8", errors="replace").strip()
            texts.setdefault(" ".join(text.split()), (text, []))[1].append(label)
        if not files:
            # ponytail: crates without a licence file get only their SPDX expression; add the standard texts if a licence asks for more.
            lines.append(
                "  (no licence file in the package; licence by its SPDX expression above)"
            )
    for text, labels in texts.values():
        lines += ["", "=" * 100, "Used by: " + "; ".join(labels), "-" * 100, text]
    return "\n".join(lines) + "\n"


def freeze_importer(work):
    """openprocyon-import: tools.import_assets with every tools module it runs in-process, plus the content/ files they read."""
    data = [f"--add-data={REPO / f}:{Path(f).parent.as_posix()}" for f in IMPORTER_DATA]
    subprocess.run(
        [
            sys.executable,
            "-m",
            "PyInstaller",
            "--noconfirm",
            "--onefile",
            "--console",
            "--name=openprocyon-import",
            f"--paths={REPO}",
            # The steps are started by module name (runpy), so PyInstaller cannot find them by following imports.
            "--collect-submodules=tools",
            f"--distpath={work / 'dist'}",
            f"--workpath={work / 'build'}",
            f"--specpath={work}",
            *data,
            str(REPO / "tools/import_assets.py"),
        ],
        check=True,
        cwd=REPO,
    )
    return work / "dist" / f"openprocyon-import{SUFFIX}"


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    name = argv[0]
    game = REPO / "game/target/release" / f"openprocyon{SUFFIX}"
    if not game.is_file():
        sys.exit(f"build the game first (cargo build --release in game/): no {game}")
    dist = REPO / "dist"
    out = dist / name
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    importer = freeze_importer(dist / "pyinstaller")
    shutil.copy2(game, out)
    shutil.copy2(importer, out)
    shutil.copy2(REPO / "LICENSE", out)
    (out / "THIRD-PARTY-NOTICES.txt").write_text(
        third_party_notices(), encoding="utf-8"
    )
    # content/ ships with the code; its README records each file's licence and travels with it.
    shutil.copytree(REPO / "content", out / "content")
    windows = sys.platform == "win32"
    (out / "PLAY.txt").write_text(
        PLAY.format(
            importer=r".\openprocyon-import" if windows else "./openprocyon-import",
            game="openprocyon" if windows else "./openprocyon",
            assets=r"%APPDATA%\openprocyon\assets"
            if windows
            else "~/.local/share/openprocyon/assets",
        ),
        newline="\r\n" if windows else "\n",
    )
    if not windows:
        with tarfile.open(dist / f"{name}.tar.gz", "w:gz") as tar:
            tar.add(out, arcname=name)
    print(f"packaged {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
