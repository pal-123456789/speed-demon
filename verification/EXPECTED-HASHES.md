# Which expected hash applies to which data

Two hash tables exist for every track, and using the wrong one produces a
**false FAIL on a correct answer**. That is the most expensive kind of error in
this repository, because it teaches whoever is on shift to ignore the gate.

## The two scales

| | data directory | expected-output hash | how to verify |
|---|---|---|---|
| **sample** | `verification/data/track-X/` (committed, ~35 MB total) | the pin, `track-X/verification/expected/<output>.sha256` | `sd-harness verify --data verification/data/track-X` |
| **full** | the board's 790 MB corpus, e.g. `D:\sd-data\track-X` | published in `verification/contract.toml` | `python check.py <t> --data <dir> --expected <full hash>` |

## The rule

**The pin belongs to the sample corpus, and to nothing else.**

`verification/data` is what a fresh clone has, so the committed pin must match
it — that is what `tooling/ci.sh` checks on every run and what makes the gate
meaningful to someone who has just cloned the repository.

The full-scale hashes cannot live in the pin file, because then CI would compare
sample-scale output against a full-scale expectation and fail 0/5 on correct
code. This was tried, and it did exactly that.

## The trap

`sd-harness verify` reads the pin and has **no `--expected` flag**. So running it
against the full corpus reports:

```
run 1-5: sha f86973c21022          <- the published expected hash
SHA-256 gate : FAIL (0/5 runs matched the expected hash)
```

That output is a lie about the code. The binary is correct and the comparison is
wrong. If you see a full-scale `sd-harness verify` fail with a hash that looks
familiar, check the scale before touching the implementation.

Full-scale verification goes through the organizers' own checker, which takes the
expectation explicitly:

```
python check.py b --data /mnt/d/sd-data/track-b --expected <hash from contract.toml>
```

That also builds and runs the real container, so it tests the assembly rather
than the binary alone.

## The proper fix, not yet made

`sd-harness` should derive the expected hash from the data it is pointed at —
hash the input, look up the scale, return that scale's output hash — the way
`tooling/official-data.py --expected-for <dir>` already does. Until then this file
is the mitigation, and the pin is sample-only by design.
