"""Package a player download: the release game binary, content/ and the importer frozen by PyInstaller. No game content.
usage: python -m tools.package_release NAME   (after `cargo build --release` in game/; needs requirements-release.txt)
Writes dist/NAME/ (the folder players unpack) and, outside Windows, dist/NAME.tar.gz (keeps the executable bits).
Used by .github/workflows/release.yml; runs the same locally."""

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

content/README.md lists the licences of the files in content/ (fonts, hand-written data).
Not affiliated with or endorsed by Disney or the original developers.
"""


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
