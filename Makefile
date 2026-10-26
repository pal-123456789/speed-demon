.PHONY: help verify bench freeze-check fmt clippy kcalc test axes fixtures docs \
        guard harness ci plan prewarm hash build spec clean

# The tracks are self-contained rustc-only folders (track-A..track-D); the
# verification harness is a standalone Rust package at verification/harness;
# the tooling is Python. The `harness` targets are the ones that matter on
# fsn1, where there is no PowerShell and no Python.
#
# `python3` is hardcoded nowhere: some hosts ship only `python` (Windows,
# slim containers), so make picks whichever interpreter exists.

PY := $(shell command -v python3 2>/dev/null || command -v python 2>/dev/null)
HMANIFEST := verification/harness/Cargo.toml

help:
	@echo "Speed Demon — campaign targets"
	@echo ""
	@echo "  BUILD"
	@echo "    make build          — offline release build of the harness"
	@echo "    make test           — harness unit tests"
	@echo "    make fmt            — rustfmt the harness (best effort)"
	@echo "    make clippy         — clippy, warnings denied (best effort)"
	@echo ""
	@echo "  GATES (the three organizers' gates)"
	@echo "    make verify         — correctness + timeout + sandbox, all tracks"
	@echo "    make bench TRACK=A  — 5-run trimmed-mean bench for one track"
	@echo "    make freeze-check   — the full pre-freeze lockdown"
	@echo "    make ci             — every gate in one command (the one to run)"
	@echo ""
	@echo "  THE SCORING BOX (Linux, no PowerShell needed)"
	@echo "    make harness        — build the native harness"
	@echo "    make plan           — the K budget for every track"
	@echo "    make prewarm F=...    — pull a dataset into the page cache"
	@echo "    make hash F=...       — SHA-256 of a file, for pinning an expected hash"
	@echo ""
	@echo "  TOOLING"
	@echo "    make guard          — rule 05 pre-kickoff compliance check"
	@echo "    make spec           — the Oct 28 SPEC.md intake checklist"
	@echo "    make axes           — the S and D axes"
	@echo "    make fixtures       — regenerate the anti-overfit fixture corpus"
	@echo "    make docs           — regenerate the per-track PERF.md and README.md"
	@echo "    make kcalc          — the K-coefficient calculator"

build:
	cargo build --release --offline --manifest-path $(HMANIFEST)

test:
	cargo test --offline --manifest-path $(HMANIFEST)

verify:
	@PS=$$(command -v pwsh || command -v powershell); \
	if [ -n "$$PS" ]; then \
	  "$$PS" -NoProfile -File verification/verify.ps1 -All; \
	else \
	  cargo run --release --offline --manifest-path $(HMANIFEST) -- verify --track "$(or $(TRACK),A)"; \
	fi

bench:
	@PS=$$(command -v pwsh || command -v powershell); \
	if [ -n "$$PS" ]; then \
	  "$$PS" -NoProfile -File verification/bench.ps1 -Track $(TRACK) -Runs 5; \
	else \
	  cargo run --release --offline --manifest-path $(HMANIFEST) -- bench --track $(TRACK) --runs 5; \
	fi

freeze-check:
	@PS=$$(command -v pwsh || command -v powershell); \
	if [ -n "$$PS" ]; then \
	  "$$PS" -NoProfile -File verification/pre-freeze.ps1 -All; \
	else \
	  cargo run --release --offline --manifest-path $(HMANIFEST) -- lockdown; \
	fi

# Every gate, in the cheapest-fails-first order. This is the command to run
# before every commit during the 72 hours.
ci:
	sh tooling/ci.sh

harness:
	cargo build --release --offline --manifest-path $(HMANIFEST)

plan:
	cargo run --release --offline --manifest-path $(HMANIFEST) -- plan

prewarm:
	cargo run --release --offline --manifest-path $(HMANIFEST) -- prewarm --file $(F)

hash:
	cargo run --release --offline --manifest-path $(HMANIFEST) -- hash --file $(F)

guard:
	$(PY) tooling/rule05_guard.py

spec:
	$(PY) tooling/spec_intake.py --status

axes:
	$(PY) tooling/measure_axes.py --all

fixtures:
	$(PY) tooling/gen-fixtures.py --all

docs:
	$(PY) tooling/gen_docs.py

kcalc:
	$(PY) tooling/k_calc.py

whatif:
	$(PY) tooling/k_whatif.py --selfcheck
	$(PY) tooling/k_whatif.py --track A --t-abs 1.07 --m-abs 15 --lines 150

fmt:
	-cargo fmt --manifest-path $(HMANIFEST) --all

clippy:
	-cargo clippy --manifest-path $(HMANIFEST) --all-targets --offline -- -D warnings

clean:
	cargo clean --manifest-path $(HMANIFEST)