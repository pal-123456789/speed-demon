# How this submission is verified

Every claim of correctness in this repository is backed by a command that can be
run, and every gate that guards it has been made to **fail on purpose** at least
once. That second part is not decoration: three of our gates were green for weeks
because they *could not fail*, and this file records them, because a verification
story that hides its own near-misses is not a verification story.

---

## 1. The layers

| Layer | What it proves | Command |
|---|---|---|
| **Published hash, full scale** | the binary reproduces the organizers' answer on the 790 MB corpus | `sh tooling/verify-all-published.sh` |
| **SHA-256 gate, sample scale** | the same on the committed 25 MB corpus a fresh clone has | `sh tooling/ci.sh` (track gates) |
| **Determinism** | five runs, byte-identical output | inside either of the above |
| **Tiny oracle corpus** | exact match against the reference on the hostile cases | `verification/cases/track-X/` |
| **Mutation evidence** | the correctness apparatus *detects* injected bugs | `python tooling/mutate.py` |
| **End-to-end sandbox** | real assembly: one file out, read-only input, stray-file detection | `sh tooling/e2e-sandbox.sh` |
| **Cross-file consistency** | 221 assertions that a shared fact has one source | `python tooling/check-consistency.py` |
| **Claim audit** | no unsourced performance number in any judge-facing file | `python tooling/check_claims.py --strict` |

Run everything with `sh tooling/ci.sh`. It builds nothing the tracks do not build
themselves, and it is the same command a fresh clone can run.

### Two scales, and why the pin is sample-only

There are two hash tables per track and using the wrong one produces a **false
FAIL on correct code**. The committed pin matches `verification/data`, which is
the sample corpus — that is what CI verifies, and it is the only data a fresh
clone has. Full-scale expectations are the published ones and are passed
explicitly, because a single pin cannot serve both scales.

This is not hypothetical. A full-scale `sd-harness verify` prints:

```
run 1-5: sha f86973c21022          <- the published expected hash
SHA-256 gate : FAIL (0/5 runs matched the expected hash)
```

Track B was correct and the gate called it wrong. That false alarm cost real
time and very nearly cost Track A's trailing-newline bug fix. See
[`verification/EXPECTED-HASHES.md`](../verification/EXPECTED-HASHES.md).

---

## 2. Mutation evidence — does the apparatus actually detect bugs?

"The tests pass" is a claim about the tests. `tooling/mutate.py` corrupts the tiny
corpora in ways corresponding to the specific bugs each track spec warns about,
re-runs the **oracle** on the corrupted data, and requires the stored expected
answer to stop matching.

**13 mutations: 11 must be caught, 2 must NOT be.**

```
  ok  [a] status-5xx        caught: error count ignores 5xx
  ok  [a] endpoint-byte     caught: group key compared with normalisation
  ok  [a] hour-boundary     caught: hour derived by truncation
  ok  [a] duplicate-line    caught: duplicate lines counted once
  ok  [a] method-control    CONTROL: method is ignored by the spec
  ok  [c] amount-digit      caught: cents parsed as float
  ok  [c] trailing-space    caught: trailing space trimmed from a name
  ok  [c] unknown-region    caught: unknown region_id silently grouped
  ok  [c] id-sum            caught: id_sum from the wrong column
  ok  [c] bom-control       CONTROL: a mid-file BOM is skipped either way
  ok  [d] duplicate-query   caught: duplicate query lines collapsed
  ok  [d] weight-zeroed     caught: weights read as 0 or not read
  ok  [b] doc-id-shift      caught: document ids assumed to start at 1

mutate: 13/13 behaved correctly (11/11 bugs caught, 2 controls held)
```

The two controls are the point. A harness that reported 13 of 13 without them
would be measuring "any change at all", not detection. Anchors are unique byte
strings and a **missing anchor is an error, not a silent no-op**, so a corpus edit
cannot quietly turn a mutation into a pass.

---

## 3. The bug ledger — what verification actually caught

Ordered by how expensive they would have been.

| # | Bug | Why it would have cost us | Caught by |
|---|---|---|---|
| 1 | **Track C produced 5 different answers in 5 runs** — `mem::take` on the endpoint intern table, so the first group to touch an endpoint emptied it and every later group wrote an empty endpoint; which group won depended on `HashMap` iteration order, seeded per process | a wrong answer that changes every run is the worst failure mode available | determinism gate, in one line |
| 2 | **Track A dropped a file's last line** when it had no trailing newline (85-byte repro) | the 238 MB corpus always ends with a newline, so **the primary gate can never surface this** — and final scoring uses a hidden holdout seed | differential test: 29 curated + 64 fuzz files, 7/24 mismatched before |
| 3 | **Track D parsed a phantom `0 0 0` edge** when the first parse window landed on the header's line feed | invisible on the 92 MB corpus (first boundary is ~7.6 MB in); **also invisible to the SHA gate** | synthetic differential graphs |
| 4 | **Track D used `u32` distance labels in an early design** | weights run to `2^32-1` and the spec's worked example answers `8589934594`; u32 is correct only if `w_max <= 2468` | reading the frozen spec's own example |
| 5 | **Track A's hour came from an mtime bucket** in early notes | the spec caps file mtimes, so every record would have the same hour | cross-check against the spec |
| 6 | **`check.py`'s frozen expectation disagreed with the corpus** | every harness run failed | harness SHA gate |
| 7 | Track C committed **sample-scale pins** against full-scale data | false FAIL on correct code — see section 1 | running CI, not reading it |
| 8 | **`gen-fixtures.py` destroyed the official sample corpus** when run with `--all` (2,177,777 B → 226 B) | all 12 published hashes would have been unverifiable | integrity gate + refusal added |

### Three gates that were green because they could not fail

Recorded here because this is the failure mode that hides longest.

1. **`e2e-sandbox.sh`** — its `seed()` had uppercase `case` labels (`A)`) matched
   against a lowercase `$tid`, so **no arm ever ran and it wrote zero input
   files**. Every binary then correctly refused to run, and the gate blamed the
   binaries. It passed for weeks only because the pre-kickoff scaffolds wrote
   their sentinel output regardless of input; the real path was never exercised.
2. **Its fixtures** were written for those same scaffolds and did not match the
   frozen formats — Track A had no METHOD field, C had no `note`/`region_id` and
   no `regions.csv`, D had no weight column. A gate fed malformed input tests the
   error path and nothing else.
3. **`check-output-names.py`** read a scaffold constant, `DEFAULT_OUTPUT`, that
   real implementations do not have, so it compared an empty string against
   `check.py` and reported "the scored filenames disagree — a guaranteed zero"
   for three tracks.

Each was fixed **and made to fail on purpose** before being called fixed: renaming
`summary.txt` to `summery.txt` produces exactly one correctly-named failure, and
Track D's answer on the gate's 5-node graph (`8, 5, 0`) is hand-checkable and
matches the six bytes the gate reports.

---

## 4. How to measure, so the measurement is not the bug

Two rules this project learned the hard way, both now enforced by tooling.

**Compare against the thing you are replacing, not against its ancestor.**
A Track D rewrite was reported as "1.62× faster" — measured against the *original*
single-threaded binary, while the board was already running something faster. It
shipped, and Track D regressed 1.579 → 1.874 s on the board, about 1.9 points on a
track that counts.

**When the effect is the size of the noise, interleave or do not measure.**
Track D's run-to-run spread on the development box reaches 20%, the same size as
the effects being chased. Measuring A in one block and B in another measures the
load, not the code. `tooling/ab-test.sh` alternates them:

```
median OLD_07878c4 = 4.385 s
median NEW_d1794b2 = 4.553 s     ratio 0.96x   -> NEW is 4% SLOWER
```

which agreed with the board and turned a revert from instinct into evidence.

**The board is the only authoritative timer.** Local measurements run through
`/mnt/d` (drvfs) and have disagreed with the board by an order of magnitude in
magnitude — a change that measured 3% locally was 13.7% on the board, and one that
measured +46% locally was +2.3% on the board. Local numbers are for direction and
for the **hash**; the hash is exact either way, which is why every change is
accepted or rejected on it.

---

## 5. What is verified right now

At commit `afee31c`, five runs each, every run byte-identical and matching the
organizers' published full-scale hash:

| Track | T (local) | Output | Published hash |
|---|---|---|---|
| A | 0.28 s | `summary.txt` | `495ca09682b9673e…` |
| B | 5.00 s | `results.txt` | `f86973c210228141…` |
| C | 2.00 s | `aggregates.txt` | `d8088b1cd42ba133…` |
| D | 2.85 s | `distances.txt` | `a23dbdf0a3e49cc8…` |

Board scores at the best-scoring commit per track: A 59.487 · B 34.347 ·
C 25.015 · D 41.820.

`sh tooling/ci.sh` reports `CI: ALL GATES GREEN`.

**Zero wrong answers across every push this project has ever made.** The only
FAILED runs in our history are the pre-kickoff scaffolds. That record is the asset
we protect above any single track's points, because final scoring runs a hidden
holdout seed where a wrong answer is worth zero.
