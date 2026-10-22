.DEFAULT_GOAL := help
.PHONY: help run check fmt test-python shot smokes assets

help:
	@printf '%s\n' \
	  'make run [ARGS="--mission 1"]     Run the game from the repository root' \
	  'make check                        Format check, strict Clippy, Rust + Python tests' \
	  'make fmt                          Format Rust and Python source' \
	  'make test-python                  Fast parser/exporter tests (no Rust build)' \
	  'make shot OUT=local/ui.png [ARGS="--shot-frame 80 --late-frame 0"]' \
	  'make smokes [ARGS=fleet]          Run the documented --script smokes (needs a display or Xvfb; ~20 s each)' \
	  'make assets ARGS="--game <install dir>"  Import your copy of the game (once; again after exporter changes)'

run:
	cargo run --release --manifest-path game/Cargo.toml -- $(ARGS)

check:
	tools/check.sh

fmt:
	cargo fmt --manifest-path game/Cargo.toml
	ruff format .
	ruff check --fix .

test-python:
	python3 -m unittest discover -s tests

shot:
	@test -n "$(OUT)" || { echo 'Use make shot OUT=local/ui.png [ARGS="..."]' >&2; exit 1; }
	tools/shot.sh "$(OUT)" $(ARGS)

smokes:
	tools/smokes.sh $(ARGS)

assets:
	python3 -m tools.import_assets $(ARGS)
