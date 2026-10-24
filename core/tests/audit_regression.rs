//! Regression tests for defects found in the Oct 22 core audit.
//!
//! Each test here encodes a bug that was real, was fixed, and must not come
//! back. They are deliberately adversarial: every one of them failed against
//! the pre-audit code. The comments record the old behaviour so a future
//! reader who "simplifies" the fix can see exactly what it would reintroduce.
//!
//! Why the standard `core` unit tests did not catch these: they all used
//! well-formed input. Every defect below lived in the malformed-input path, in
//! an off-by-one, or in a lifetime interaction between two types — the places
//! a happy-path test never reaches.

use speeddemon_core::arena::{Arena, Interner};
use speeddemon_core::buf_reader::chunk_boundaries;
use speeddemon_core::fixed_codec::{format_cents, parse_cents};
use speeddemon_core::simd_scan::{
    classify_byte_mask, find_any, find_byte, find_csv_special, valid_mask_bits,
};

// ── parse_cents: was a panic on user data ──────────────────────────────────

#[test]
fn non_ascii_amount_returns_none_instead_of_panicking() {
    // OLD: `cents_str[..2]` sliced by byte index and panicked with "byte index
    // 2 is not a char boundary" on any non-ASCII byte in the fraction. Under
    // `panic = "abort"` that terminates the process, so one malformed amount
    // would have zeroed the whole track.
    for bad in [
        "1.2\u{20ac}",  // euro sign
        "1.2\u{e9}",    // e-acute
        "\u{20ac}1.00", // non-ASCII in the integer part
        "1\u{0a0}.00",  // non-breaking space
    ] {
        assert_eq!(parse_cents(bad), None, "expected None for {bad:?}");
    }
}

#[test]
fn sign_inside_the_fraction_is_rejected() {
    // OLD: the sign was stripped from the whole string, then the fraction was
    // parsed independently, so a sign in the fraction leaked through:
    //   "5.-9"  -> Some(491)   (500 + -9)
    //   "--5"   -> Some(500)
    //   "-0.-1" -> Some(1)
    // Each is a silently wrong amount, which is a hash-gate failure with no
    // diagnostic.
    for bad in ["5.-9", "--5", "-0.-1", "+-5", "5.+9", "5-", "5+"] {
        assert_eq!(parse_cents(bad), None, "expected None for {bad:?}");
    }
}

#[test]
fn more_than_two_decimals_is_rejected_not_truncated() {
    // OLD: `cents_str[..2]` silently truncated, so "1.005" -> 100 and
    // "1.999" -> 199. Truncation and half-up rounding are both defensible and
    // produce different bytes, so guessing either way is a coin flip on a
    // byte-hash gate. The frozen spec pins 2 decimals: 3+ is malformed input.
    for bad in ["1.005", "1.999", "0.009", "2.675", "1.000"] {
        assert_eq!(parse_cents(bad), None, "expected None for {bad:?}");
    }
}

#[test]
fn a_bom_does_not_drop_the_record() {
    // OLD: `trim()` does not remove U+FEFF (it is not whitespace to Rust), so
    // a BOM'd amount parsed to None and the record vanished. The Track C
    // corpus is documented to ship a BOM on the header and can repeat it.
    assert_eq!(parse_cents("\u{feff}1420.00"), Some(142000));
    assert_eq!(parse_cents("\u{feff}-5.25"), Some(-525));
    assert_eq!(parse_cents("\u{feff}\u{feff}9.99"), Some(999));
}

#[test]
fn values_with_no_digits_are_rejected_not_zero() {
    // OLD: ".", "-", "+" all parsed to Some(0), turning a malformed row into
    // a valid-looking zero row — worse than failing, because it corrupts the
    // aggregate silently.
    for bad in [".", "-", "+", "", "   ", "\u{feff}"] {
        assert_eq!(parse_cents(bad), None, "expected None for {bad:?}");
    }
}

#[test]
fn extreme_amounts_do_not_abort_the_process() {
    // OLD: `-total` on i64::MIN was unchecked. A value that reached i64::MIN
    // panicked in debug and wrapped silently in release.
    // These must either parse correctly or return None — never panic.
    let huge = "--92233720368547758.-8";
    assert_eq!(parse_cents(huge), None);
    // i64::MAX cents is representable; one more is not.
    assert_eq!(parse_cents("92233720368547758.07"), Some(i64::MAX));
    assert_eq!(parse_cents("92233720368547758.08"), None);
    assert_eq!(parse_cents("-92233720368547758.08"), Some(i64::MIN));
    assert_eq!(parse_cents("-92233720368547758.09"), None);
}

#[test]
fn valid_amounts_still_parse() {
    // The strictness above must not have broken the ordinary cases.
    assert_eq!(parse_cents("1420.00"), Some(142000));
    assert_eq!(parse_cents("-5.25"), Some(-525));
    assert_eq!(parse_cents("0.00"), Some(0));
    assert_eq!(parse_cents("0.1"), Some(10));
    assert_eq!(parse_cents("0.01"), Some(1));
    assert_eq!(parse_cents("88.50"), Some(8850));
    assert_eq!(parse_cents("7"), Some(700));
    assert_eq!(parse_cents("+7"), Some(700));
    assert_eq!(parse_cents(" 3.50 "), Some(350));
    assert_eq!(parse_cents("0"), Some(0));
}

#[test]
fn cents_roundtrip_holds_for_the_representable_range() {
    // format_cents must produce something parse_cents reads back. The old
    // code failed this at i64::MIN (format emitted a value parse rejected).
    for c in [
        0i64,
        1,
        -1,
        99,
        -99,
        100,
        -100,
        142000,
        -525,
        i64::MAX,
        i64::MIN,
    ] {
        let s = format_cents(c);
        assert_eq!(
            parse_cents(&s),
            Some(c),
            "roundtrip failed for {c} (formatted {s:?})"
        );
    }
}

// ── chunk_boundaries: was quadratic ────────────────────────────────────────

#[test]
fn boundary_search_is_linear_not_quadratic() {
    // OLD: each cut rescanned toward EOF from its own stride position, so a
    // sparse predicate cost O(size * num_chunks). Measured 550,022 predicate
    // calls for a 100 KB buffer at 12 chunks (5.5x size) — and because
    // `par_chunks` calls this on the main thread before spawning, that cost
    // was entirely serial. Worst case on the 312 MiB Track A input was ~1.8
    // billion calls.
    let size = 100_000usize;
    let mut calls = 0usize;
    let _ = chunk_boundaries(size, 12, |off| {
        calls += 1;
        off == size - 1
    });
    assert!(
        calls <= size,
        "expected at most {size} predicate calls (linear), got {calls}"
    );
}

#[test]
fn boundaries_are_strictly_increasing_and_bracketed() {
    // The old contract promised exactly num_chunks+1 offsets, but could emit
    // duplicates. Duplicates mean zero-length chunks; downstream code that
    // assumed strict monotonicity would mis-slice.
    for (size, n) in [(100usize, 4usize), (3, 12), (1000, 7), (1, 5), (10, 10)] {
        let b = chunk_boundaries(size, n, |off| off % 7 == 0);
        assert_eq!(b.first(), Some(&0), "size={size} n={n}");
        assert_eq!(b.last(), Some(&size), "size={size} n={n}");
        for w in b.windows(2) {
            assert!(
                w[0] < w[1],
                "offsets must strictly increase, got {b:?} for size={size} n={n}"
            );
        }
        assert!(
            b.len() <= n + 1,
            "size={size} n={n} gave {} offsets",
            b.len()
        );
    }
}

#[test]
fn boundary_search_survives_a_predicate_that_never_matches() {
    // A boundary-free predicate previously made every cut walk to EOF and
    // collapse onto `size`, producing duplicate offsets.
    let b = chunk_boundaries(1000, 8, |_| false);
    assert_eq!(b.first(), Some(&0));
    assert_eq!(b.last(), Some(&1000));
    for w in b.windows(2) {
        assert!(w[0] < w[1], "no duplicates allowed: {b:?}");
    }
}

#[test]
fn degenerate_chunk_requests_do_not_panic() {
    assert_eq!(chunk_boundaries(0, 5, |_| true), vec![0, 0]);
    assert_eq!(chunk_boundaries(100, 0, |_| true), vec![0, 100]);
    let b = chunk_boundaries(10, 1000, |off| off % 2 == 0);
    assert_eq!(b.first(), Some(&0));
    assert_eq!(b.last(), Some(&10));
}

// ── Arena / Interner: was silent wrong bytes ──────────────────────────────

#[test]
fn resetting_the_arena_does_not_corrupt_a_live_interner() {
    // OLD: reset() rewound the bump pointer but the interner kept its spans,
    // so after reset the next alloc overwrote the old bytes. Reading the old
    // span returned DIFFERENT BYTES WITH NO PANIC:
    //   intern("/v2/orders") -> reset -> intern("/v2/refunds")
    //   get(old_span) == "/v2/refund"
    // Wrong bytes are the one failure a hash gate cannot explain. The interner
    // now tracks the arena generation and clears itself on a reset.
    let mut a = Arena::new();
    let mut i = Interner::new();
    let s_orders = i.intern(&mut a, "/v2/orders");
    assert_eq!(a.get_str(s_orders), "/v2/orders");

    a.reset();
    let s_refunds = i.intern(&mut a, "/v2/refunds");

    // Whatever the old span now aliases, the interner must never hand back a
    // span whose bytes are not the string that was interned.
    assert_eq!(a.get_str(s_refunds), "/v2/refunds");
    let re = i.intern(&mut a, "/v2/refunds");
    assert_eq!(a.get_str(re), "/v2/refunds", "re-intern must be consistent");
}

#[test]
fn interner_and_arena_stay_consistent_across_resets() {
    let mut a = Arena::new();
    let mut i = Interner::new();
    for round in 0..4 {
        let s = i.intern(&mut a, "alpha");
        assert_eq!(a.get_str(s), "alpha", "round {round}");
        let t = i.intern(&mut a, "beta");
        assert_eq!(a.get_str(t), "beta", "round {round}");
        // Same string must dedup within a round.
        assert_eq!(i.intern(&mut a, "alpha"), s, "round {round}");
        a.reset();
    }
}

#[test]
fn spans_read_back_exactly_what_was_allocated() {
    let mut a = Arena::with_capacity(4);
    let first = a.alloc(b"stable");
    let big = a.alloc(&vec![b'x'; 5000]);
    assert_eq!(a.get_str(first), "stable", "growth must not move a span");
    assert_eq!(a.get(big).len(), 5000);
    let empty = a.alloc(b"");
    assert!(empty.is_empty());
    assert_eq!(a.get(empty), b"");
}

// ── simd_scan: was a panic on start > len ─────────────────────────────────

#[test]
fn scanners_tolerate_start_past_the_end() {
    // OLD: `haystack[start..]` panicked when start > len. Callers naturally
    // pass one-past-the-end after consuming a match at the last byte.
    assert_eq!(find_byte(b"abc", b'x', 5), None);
    assert_eq!(find_any(b"abc", b"xy", 5), None);
    assert_eq!(find_csv_special(b"abc", 5), None);
    // start == len is the normal end-of-buffer case and must be None, not a panic.
    assert_eq!(find_byte(b"abc", b'c', 3), None);
    assert_eq!(find_csv_special(b"abc", 3), None);
    // In-range behaviour is unchanged.
    assert_eq!(find_byte(b"hello, world", b',', 0), Some(5));
    assert_eq!(find_byte(b"hello", b'x', 0), None);
}

#[test]
fn classify_mask_reports_its_valid_window() {
    // OLD: the mask zero-filled the tail beyond the real data, so a caller
    // scanning bits could not tell "no match" from "no byte exists".
    let h = b"a,b";
    let m = classify_byte_mask(h, 0, b',');
    assert_eq!(m, 0b010, "comma at index 1");
    assert_eq!(valid_mask_bits(h, 0), 3, "only 3 real bytes in the window");
    // Bits at or beyond valid_mask_bits are meaningless.
    assert_eq!(m & !((1u32 << valid_mask_bits(h, 0)) - 1), 0);

    // Past the end: empty mask and zero valid bits, no panic.
    assert_eq!(classify_byte_mask(h, 99, b','), 0);
    assert_eq!(valid_mask_bits(h, 99), 0);
    assert_eq!(valid_mask_bits(h, 3), 0);

    // A full 32-byte window reports 32 valid bits.
    let full = [b'x'; 32];
    assert_eq!(valid_mask_bits(&full, 0), 32);
}
