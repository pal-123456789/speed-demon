.PHONY: help verify bench freeze-check fmt clippy kcalc test axes fixtures docs \
        guard harness ci pgo prewarm plan hash clean

# The campaign is a Rust workspace, but the tooling is Python and the
# verification is split between PowerShell (Windows dev) and a Rust harness
# (the scoring box). The `harness` targets are the ones that matter on
# HEL1, where there is no PowerShell and no Python.

help:
	@echo "Speed Demon — campaign targets"
	@echo ""
	@echo "  BUILD"
	@echo "    make build          — offline release build of everything"
	@echo "    make test           — unit tests (core + harness)"
	@echo "    make fmt            — rustfmt all crates"
	@echo "    make clippy         — clippy, warnings denied"
	@echo ""
	@echo "  GATES (the four organizers' gates)"
	@echo "    make verify         — SHA + determinism + timeout + sandbox, all tracks"
	@echo "    make bench TRACK=A  — 5-run trimmed-mean bench for one track"
	@echo "    make freeze-check   — the full pre-freeze lockdown"
	@echo "    make ci             — every gate in one command (the one to run)"
	@echo ""
	@echo "  THE SCORING BOX (Linux, no PowerShell needed)"
	@echo "    make harness        — build the native harness"
	@echo "    make plan           — the K budget for every track"
	@echo "    make prewarm F=…    — pull a dataset into the page cache"
	@echo "    make hash F=…       — SHA-256 of a file, for pinning an expected hash"
	@echo ""
	@echo "  TOOLING"
	@echo "    make guard          — rule 05 pre-kickoff compliance check"
	@echo "    make axes           — the S and D axes"
	@echo "    make fixtures       — regenerate the anti-overfit fixture corpus"
	@echo "    make docs           — regenerate the per-track PERF.md and README.md"
	@echo "    make kcalc          — the K-coefficient calculator"
	@echo "    make pgo TRACK=A    — offline profile-guided build"

build:
	cargo build --release --offline

test:
	cargo test --offline

verify:
	pwsh -File verification/verify.ps1 -All

bench:
	pwsh -File verification/bench.ps1 -Track $(TRACK) -Runs 5

freeze-check:
	pwsh -File verification/pre-freeze.ps1 -All

# Every gate, in the cheapest-fails-first order. This is the command to run
# before every commit during the 72 hours.
ci:
	sh tooling/ci.sh

harness:
	cargo build --release --offline -p sd-harness

plan:
	cargo run --release --offline -p sd-harness -- plan

prewarm:
	cargo run --release --offline -p sd-harness -- prewarm --file $(F)

hash:
	cargo run --release --offline -p sd-harness -- hash --file $(F)

guard:
	python3 tooling/rule05_guard.py

axes:
	python3 tooling/measure_axes.py --all

fixtures:
	python3 tooling/gen-fixtures.py --all

docs:
	python3 tooling/gen_docs.py

kcalc:
	python3 tooling/k_calc.py

pgo:
	sh tooling/pgo-build.sh --track $(TRACK)

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --offline -- -D warnings

clean:
	cargo clean
