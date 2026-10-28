# Vendored references — the organizers' own implementations (the oracle)

**Provenance.** These five files are copied **byte-identical** from the
organizers' public starter template
(`github.com/hackathon-raptors/speed-demon-starter`, `track-*/`):

| File | Source | Role |
|---|---|---|
| `a_ingest.py` | `track-a/` | Track A reference (naive Python) |
| `b_index.py` | `track-b/` | Track B reference |
| `c_join.py` | `track-c/` | Track C reference |
| `d_traverse.py` | `track-d/` | Track D reference (and the 4th file in `run.py`) |
| `run.py` | `track-a/` | the reference's container entrypoint (TRACK dispatch) |

**Why this is allowed.** Rule: *"Byte-identical copies of our published
generators and references are free, so copying the generator in for
build-time training costs nothing."* The references are the organizers'
published artifacts, not our solution code; rule 05 forbids committing *our
own* solution code before kickoff, which these are not. They are used as a
**test oracle only**.

**Why they are OUTSIDE every track folder.** S counts everything in a track's
folder ("all the build can see"). `reference/` is not a build context, so it
never enters S, never enters an image, and is never scanned by
`tooling/rule05_guard.py` (which scans `track-*/src`).

**What they unlock.**

1. **`tooling/conformance.py`** — differential testing with the real oracle:
   ```
   python tooling/conformance.py --track all --oracle          # is our data in-contract?
   python tooling/conformance.py --track a --oracle --diff --build   # H0+ byte diff
   python tooling/conformance.py --track c --diff --minimize    # shrink a divergence
   ```
   The `--oracle` mode already caught **six real fixture defects** (a NUL in an
   integer column, a 4-column record, two more malformed rows, and a graph
   header whose `n` did not cover the published node ids).
2. **`tooling/profile-reference.py`** — measured phase attribution (see
   `docs/depth/92-REFERENCE-PROFILES.md`), which corrected four priority
   assumptions.
3. The **exact output bytes** — run any track's reference on any data dir to
   see what the SHA gate will demand, before writing a line of Rust.

**Do not edit these files.** If the organizers publish an update on Oct 28,
replace them wholesale (a byte-identical copy is still free; an edited one is
not).
