# The organizers' public repo — vendored byte-identical for verification

Source: https://github.com/hackathon-raptors/speed-demon-2026 (published at the
spec freeze). Copied verbatim: `specs/`, `generators/`, `reference/`, `harness/`,
`tests/`, `CONVENTIONS.md`.

**Why this lives here and what it costs.** Rule 16 makes byte-identical copies
of the organizers' published generators and references free, and everything
under `reference/` sits OUTSIDE every `track-*/` folder — so it never enters a
build context, never enters an image, and never counts in any track's S or D.
It is our verification material, not submission code.

**What it unlocks.**
1. `python tooling/official-tests.py` — the organizers' OWN test suite. The fast
   subset runs the spec worked examples and hand-written edge cases for all four
   tracks (milliseconds); `--full` runs all 58 tests (~2 minutes) including
   generator determinism, scale/format checks and the Bellman-Ford cross-check.
   We ran it: **58/58 pass**, which proves our vendored copies are intact and
   that our reading of every contract matches theirs.
2. `harness/static_axes.py` + `tests/fixtures/static_axes/rust/` define exactly
   how a Rust track folder is measured: prose and data are not code, `.sccignore`
   is honoured, registry-vendored packages are excluded from S but counted in D,
   and `[dependencies]` (including path and git deps) counts in D while
   dev-/build-dependencies and workspace tables do not.
3. `harness/dockerfile_check.py` is the pre-build guardrail we must satisfy;
   `harness/run_submission.py` is the run procedure; `board/axes.py` is the
   scoring path.

**Do not edit anything under here.** It is a copy of record; if the organizers
publish an update, replace the directory wholesale.