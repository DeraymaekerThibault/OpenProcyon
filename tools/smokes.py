"""Helpers for tools/smokes.sh: the change -> smoke map and the verdict of one smoke run (no game is launched here).
usage: python3 -m tools.smokes plan [--base REF]   print the smokes for the files changed against REF (default origin/main)
       python3 -m tools.smokes verdict KIND EXIT LOG   print why a run failed (exit status 1) or nothing (0)
The names are the first column of the table in tools/smokes.sh; `battles` stands for tools/battle_check.py."""

import re
import subprocess
import sys
from pathlib import Path

from tools import gamelog

REPO = Path(__file__).resolve().parents[1]
# A self-judging script logs one of these when its assertions all held (smoke/lifeboats.rs, smoke/fleet.rs).
PASS = re.compile(r": PASS| smoke passed| ECS smoke passed| dispatch passed")
RELEVANT = re.compile(
    r"panick|error|fail|assert|shot\.sh|PASS|passed|imported", re.IGNORECASE
)

# (path regex, smokes) in docs/development.md's module ownership order; every matching row adds its smokes.
MAP = [
    (
        r"^game/src/(combat/|collision/|ship|scenario)",
        ["battles", "damage", "target", "course"],
    ),
    (r"^game/src/victory", ["victory", "defeat", "battles"]),
    (
        r"^game/src/(rules/|campaign|lifeboats|briefing)",
        ["briefing-skip", "lifeboat-m1", "storm-m3", "hunt-m3"],
    ),
    (r"^game/src/(docking|boarding)", ["dock", "battles"]),
    (r"^game/src/(classic/|panels|menu|options_menu)", ["pause", "fleet", "mods"]),
    (r"^game/src/(fx/|render/|nebula|cloak)", ["fxtest"]),
    # Wiring: only a real run finds system conflicts, so everything runs.
    (r"^game/src/(lib\.rs|loading|smoke/)|^game/(Cargo|build)", ["all"]),
]


def plan(files):
    """(smokes, unmapped) for changed paths: smoke names in table order are the caller's job; `all` means every smoke and the battle check."""
    names, unmapped = [], []
    for f in files:
        hits = [s for rx, s in MAP if re.search(rx, f)]
        if hits:
            names += [n for h in hits for n in h]
        elif f.startswith("game/"):
            unmapped.append(f)
    return list(dict.fromkeys(names)), unmapped


def changed_files(base="origin/main"):
    """Paths changed against the merge base with `base`, committed or not, plus untracked files."""

    def git(*a):
        return subprocess.run(
            ["git", *a], cwd=REPO, capture_output=True, text=True, check=True
        ).stdout.split("\n")

    mb = git("merge-base", "HEAD", base)[0]
    return sorted(
        {
            f
            for f in git("diff", "--name-only", mb)
            + git("ls-files", "-o", "--exclude-standard")
            if f
        }
    )


def verdict(kind, code, log):
    """Why a run failed, or None. `asserts` scripts must log their PASS line (they may quit before their picture, so the exit status
    does not matter then); `visual` ones only need a clean exit (shot.sh already checks the picture is not black)."""
    if kind == "asserts":
        if "panicked at" in log:
            return "panicked"
        return (
            None
            if PASS.search(log)
            else "the PASS line never appeared (the script did not reach its assertions)"
        )
    return None if code == 0 else f"exit status {code}"


def tail(log, n=10):
    """The last n lines worth reading: those that look like a verdict or an error, else simply the last ones."""
    lines = [ln for ln in re.sub(r"\x1b\[[0-9;]*m", "", log).splitlines() if ln.strip()]
    return ([ln for ln in lines if RELEVANT.search(ln)] or lines)[-n:]


def main(argv):
    if argv[:1] == ["plan"]:
        base = argv[argv.index("--base") + 1] if "--base" in argv else "origin/main"
        names, unmapped = plan(changed_files(base))
        for f in unmapped:
            print(f"smokes: no mapping for {f}", file=sys.stderr)
        print(" ".join(names))
    elif argv[:1] == ["verdict"] and len(argv) == 4:
        log = gamelog.read(argv[3], r"panicked at|" + PASS.pattern)
        why = verdict(argv[1], int(argv[2]), log)
        if why:
            print(why)
            print("\n".join("    " + ln for ln in tail(log)))
            sys.exit(1)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
