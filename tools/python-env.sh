# Sourced (POSIX sh) by tools/check.sh, shot.sh and smokes.sh, which set `root` to the checkout first: the Python they run.
# Sets `py` and puts the checkout's virtualenv first on PATH (bin/ on Linux and macOS, Scripts/ on Windows, where python3 may be
# the Microsoft Store stub). The Makefile's PY is the same rule.
for _pe_bin in .venv/bin .venv/Scripts; do [ -d "$root/$_pe_bin" ] && PATH="$root/$_pe_bin:$PATH"; done
# python3 when it runs (not the Windows Store stub, not absent: the Windows venv only has `python`), else python.
py=python3; "$py" -c '' >/dev/null 2>&1 || py=python
