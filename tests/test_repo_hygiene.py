"""Repository rules that keep OpenProcyon shareable (AGENTS.md "Hard rules"), enforced for every contributor and tool:
no original game files, no executable addresses or decompiler output, no reverse-engineering tooling in tracked files."""

import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Original-game formats and anything converted from them. content/ ships only our own or freely licensed files.
GAME_SUFFIXES = {
    ".wot",
    ".twt",
    ".mdb",
    ".msb",
    ".eft",
    ".hdt",
    ".tst",
    ".fft",
    ".tga",
    ".dds",
    ".bik",
    ".exe",
    ".dll",
    ".ogg",
    ".glb",
    ".wav",
}
TEXT_SUFFIXES = {
    ".rs",
    ".py",
    ".md",
    ".sh",
    ".toml",
    ".json",
    ".txt",
    ".yml",
    ".yaml",
    "Makefile",
}

ADDRESS = re.compile(
    r"\b0x0{0,2}(?!7fffff)[4-7][0-9a-fA-F]{5}(?![0-9a-fA-F])|\b(?:FUN|DAT|LAB|PTR)_[0-9a-fA-F]{6,8}\b|\bTP::\w+"
)
DECOMPILER = re.compile(
    r"\b(?:param_\d+|[iuf]Var\d+|local_[0-9a-f]{2,}|undefined[248]?)\b"
)
TOOLING = re.compile(
    r"ghidra|\bIDA Pro\b|steamstub|unpacked (?:community )?exe|community exe|decompil|disassembl|tp_win32"
    # Describe the original's behaviour, not its executable: no "the exe" wording, no internal routine/functor class names.
    r"|\bexe\b(?!\")|(?-i:\b[A-Z]\w*(?:Routine|Functor)\b)|exe_strings",
    re.IGNORECASE,
)
# Files that state the rules may name what they forbid.
RULE_FILES = {"AGENTS.md", "README.md"}


def tracked():
    try:
        out = subprocess.run(
            ["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, check=True
        ).stdout
        return [ROOT / p for p in out.decode().split("\0") if p]
    except (OSError, subprocess.CalledProcessError):
        return [
            p
            for p in ROOT.rglob("*")
            if p.is_file()
            and not {".git", "target", "assets", "local", ".venv"} & set(p.parts)
        ]


class RepoHygiene(unittest.TestCase):
    def setUp(self):
        self.files = [p for p in tracked() if p.is_file()]

    def test_no_game_files_are_tracked(self):
        bad = [
            str(p.relative_to(ROOT))
            for p in self.files
            if p.suffix.lower() in GAME_SUFFIXES
            or p.relative_to(ROOT).parts[0] == "assets"
        ]
        self.assertEqual(
            bad, [], "original or converted game files must never be committed"
        )

    def test_no_executable_addresses_decompiler_output_or_re_tooling(self):
        problems = []
        for p in self.files:
            rel = p.relative_to(ROOT).as_posix()
            if rel == "tests/test_repo_hygiene.py":
                continue
            if p.suffix not in TEXT_SUFFIXES and p.name not in TEXT_SUFFIXES:
                continue
            for n, line in enumerate(p.read_text(errors="replace").splitlines(), 1):
                if (
                    ADDRESS.search(line)
                    or DECOMPILER.search(line)
                    or (rel not in RULE_FILES and TOOLING.search(line))
                ):
                    problems.append(f"{rel}:{n}: {line.strip()[:120]}")
        self.assertEqual(
            problems,
            [],
            "describe behaviour in your own words; no addresses, decompiler output or RE tooling (AGENTS.md)",
        )

    def test_patterns(self):
        for hit in (
            "exe 0x5e4fd0",
            "`FUN_005621a0`",
            "iVar3 = param_1;",
            "DAT_0074d8e0",
            "**0x0055ecf0**",
            "`TP::ILineOfSightOccluder`",
        ):
            self.assertTrue(ADDRESS.search(hit) or DECOMPILER.search(hit), hit)
        for ok in ("0x46546C67", "rng & 0x7fffff", "command id 0x10", "0x3C6EF35F"):
            self.assertFalse(ADDRESS.search(ok) or DECOMPILER.search(ok), ok)


if __name__ == "__main__":
    unittest.main()
