# Generic infrastructure (copy-in material for H0)

> **Rule-05 stance (read this first).** The organizers allow *generic
> infrastructure* to be prepared before kickoff — rules 04/05 explicitly
> permit "reading, designing, and preparing the environment". What they do
> NOT allow is pre-written *solution* code: nothing that encodes a track's
> algorithm, output contract, or data layout.
>
> These four files are deliberately **track-agnostic primitives**: a bump
> arena, a newline splitter, a decimal parser, and a decimal formatter. They
> exist OUTSIDE any `track-*/` folder on purpose — so they are not scanned by
> `rule05_guard`, are not counted in any track's S factor, and can never be
> mistaken for solution code.
>
> **At H0, copy the files you need into `track-X/src/`** (or inline them).
> From that moment they count toward that track's S (code lines in the
> folder) — budget for it. Do NOT pre-place them in `track-*/src`.
>
> Everything is `#![no_std]`-compatible-free standard Rust, zero deps,
> zero unsafe, and each file compiles standalone:
>
>     rustc --edition 2021 --crate-type lib bump.rs lines.rs parse_u32.rs itoa.rs

## Files

| File | What it is | Where it pays |
|---|---|---|
| `bump.rs` | typed bump arena (`Bump<T>`: push/reset, no per-item alloc, no unsafe) | per-thread group tables, latency chunks, posting scratch — kills `Vec` doubling spikes (P7 §5) |
| `lines.rs` | `Lines` iterator over `&[u8]` — yields records split on `\n` and `\r\n`, zero-copy | A/C/D input streaming on top of `io.rs`'s `LineReader` |
| `parse_u32.rs` | fast checked decimal `u32` parse (returns value + bytes consumed; rejects overflow/non-digit) | status, latency_us, weights, node ids — the P2/P5 SWAR design starts from here |
| `itoa.rs` | `u32`/`u64` → decimal bytes into a reused buffer | one-buffer output building; never `format!` in hot paths (P7 §8) |

**What is deliberately NOT here** (write it in-window): percentile selection,
tokenizers, CSV quoting, graph layout, any track's aggregation shape — that is
solution code and belongs to the 72 hours.

## Copy-in recipe (H0, ~2 minutes)

```sh
for t in A B C D; do
  cp prep/bump.rs prep/lines.rs prep/parse_u32.rs prep/itoa.rs track-$t/src/
done
# then: sh tooling/bench-rung.sh A   (the measurement sandwich)
```
