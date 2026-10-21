.PHONY: verify bench freeze-check fmt clippy kcalc help

help:
	@echo "Targets:"
	@echo "  make verify        — offline build + SHA gate + determinism (all 5 tracks)"
	@echo "  make bench TRACK=A — 5-run trimmed-mean bench for one track"
	@echo "  make freeze-check  — full pre-freeze lockdown checklist"
	@echo "  make fmt           — rustfmt all crates"
	@echo "  make clippy        — clippy (offline)"
	@echo "  make kcalc         — run K-coefficient interactive calculator"

verify:
	pwsh -File verification/verify.ps1 -All

bench:
	pwsh -File verification/bench.ps1 -Track $(TRACK) -Runs 5

freeze-check:
	pwsh -File verification/pre-freeze.ps1 -All

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

kcalc:
	python tooling/k_calc.py
