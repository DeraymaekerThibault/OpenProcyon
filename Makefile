.DEFAULT_GOAL := help
.PHONY: help run check fmt test-python shot smokes autoplay battles assets

# The checkout's virtualenv when there is one (bin/ on Linux and macOS, Scripts/ on Windows, where python3 may be the Store stub).
PY := $(or $(wildcard .venv/bin/python),$(if $(wildcard .venv/Scripts/python*),.venv/Scripts/python),python3)
# Windows: make may run recipes with cmd, so the shell scripts go through Git Bash (override with GIT_BASH=<path to bash>).
ifeq ($(OS),Windows_NT)
SH := "$(or $(GIT_BASH),$(or $(ProgramW6432),$(ProgramFiles))/Git/bin/bash)"
endif

help:
	$(info make run [ARGS="--mission 1"]     Run the game from the repository root)
	$(info make check                        Format check, strict Clippy, Rust + Python tests)
	$(info make fmt                          Format Rust and Python source)
	$(info make test-python                  Fast parser/exporter tests (no Rust build))
	$(info make shot OUT=local/ui.png [ARGS="--shot-frame 80 --late-frame 0"])
	$(info make smokes [ARGS=fleet]          Run the documented --script smokes (needs a display or Xvfb; ~20 s each))
	$(info make battles [ARGS=ambush]        Play short fixed-fleet skirmishes and check they behave like battles (~1-2 min))
	$(info make autoplay [ARGS=1]            Play campaign missions unattended and summarise what broke (long: ~mission length each))
	$(info make assets ARGS="--game <install dir>"  Import your copy of the game (once; again after exporter changes))

run:
	cargo run --release --manifest-path game/Cargo.toml -- $(ARGS)

check:
	$(SH) tools/check.sh

fmt:
	cargo fmt --manifest-path game/Cargo.toml
	$(PY) -m ruff format .
	$(PY) -m ruff check --fix .

test-python:
	$(PY) -m unittest discover -s tests

shot:
	$(if $(OUT),,$(error Use make shot OUT=local/ui.png [ARGS="..."]))
	$(SH) tools/shot.sh "$(OUT)" $(ARGS)

smokes:
	$(SH) tools/smokes.sh $(ARGS)

battles:
	$(PY) -m tools.battle_check $(ARGS)

autoplay:
	$(PY) -m tools.autoplay $(ARGS)

assets:
	$(PY) -m tools.import_assets $(ARGS)
