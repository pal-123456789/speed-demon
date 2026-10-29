//! The frozen fsn1 contract, in one place.
//!
//! Every number here is from the published event page. Nothing is invented.
//! Values the event page does not publish are `None` and are reported as
//! "DEFERRED" rather than guessed.

use std::collections::BTreeMap;

/// One track's immutable contract.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: char,
    pub dir: &'static str,
    /// The input filename inside /data. The organizers fix these in the
    /// track spec; the published previews give us some of them.
    pub input: &'static str,
    /// The single output filename written to /out.
    pub output: &'static str,
    /// Reference wall time on the scoring machine (fsn1), seconds.
    pub ref_time_s: f64,
    /// Reference peak memory, MB (None where unpublished).
    pub ref_mem_mb: Option<f64>,
    /// Reference source size, CODE LINES (None where unpublished). S is
    /// counted in lines by scc, not in bytes.
    pub ref_src_lines: Option<f64>,
    /// Hard timeout = 10 x reference time, seconds.
    pub timeout_s: f64,
}

impl Track {
    /// The reference M value that feeds the M axis ratio: the reference's own
    /// peak memory. Every remaining track uses peak memory for M.
    pub fn ref_mb(&self) -> Option<f64> {
        self.ref_mem_mb
    }
}

pub fn tracks() -> BTreeMap<char, Track> {
    let mut m = BTreeMap::new();
    m.insert(
        'A',
        Track {
            id: 'A',
            dir: "track-a",
            input: "requests.log",
            output: "summary.txt",
            // "Reference numbers are now measured on the scoring machine, five
            // runs each: Track A 32.05 s, B 56.35 s, C 14.44 s, D 72.69 s.
            // Timeouts are 10 times those."
            ref_time_s: 32.047276,
            ref_mem_mb: Some(227.98336),
            // The reference's source size is published: 56 lines.
            ref_src_lines: Some(56.0),
            timeout_s: 320.5,
        },
    );
    m.insert(
        'B',
        Track {
            id: 'B',
            dir: "track-b",
            input: "docs.txt",
            output: "results.txt",
            ref_time_s: 56.352376,
            ref_mem_mb: Some(2199.9616),
            ref_src_lines: Some(91.0),
            timeout_s: 563.5,
        },
    );
    m.insert(
        'C',
        Track {
            id: 'C',
            dir: "track-c",
            input: "records.csv",
            output: "aggregates.txt",
            ref_time_s: 14.444059,
            ref_mem_mb: Some(367.435776),
            ref_src_lines: Some(108.0),
            timeout_s: 144.4,
        },
    );
    m.insert(
        'D',
        Track {
            id: 'D',
            dir: "track-d",
            input: "graph.txt",
            output: "distances.txt",
            ref_time_s: 72.692655,
            ref_mem_mb: Some(892.915712),
            ref_src_lines: Some(43.0),
            timeout_s: 726.9,
        },
    );
    m
}

/// The scoring weights, as frozen before kickoff. They sum to exactly 1.0.
///
/// "New formula: K = (T_ref/T)^0.50 × (M_ref/M)^0.20 × (S_ref/S)^0.15 ×
/// (D_ref/D)^0.15. The scaling axis X is gone, and its weight moved to wall
/// time." — the organizers' list of changes before the Oct 28 freeze.
///
/// The X axis (scaling) is REMOVED. There is no fifth ratio, no fifth
/// contribution, and no fifth weight: the old 0.40/0.20/0.15/0.15/0.10 split
/// is dead.
pub const W_T: f64 = 0.50;
pub const W_M: f64 = 0.20;
pub const W_S: f64 = 0.15;
pub const W_D: f64 = 0.15;

/// The reference's D. The reference implementation declares no dependencies.
pub const D_REF: f64 = 1.0;

/// Memory is the only ratio with a cap: "Memory is capped at 50×."
///
/// Wall time has NO cap ("Wall time has no cap. It is never pathological, so
/// a faster program always earns a higher K"), and S and D are capped at 1.0
/// because they "can cost you but never pay".
pub const AXIS_CAP: f64 = 50.0;

/// A finite guard for the uncapped wall-time ratio. It is not a scoring cap:
/// 1e9× faster is not reachable, and without it a zero-time run would make K
/// infinite and the speed score NaN.
pub const T_RATIO_GUARD: f64 = 1.0e9;

/// The S axis can only cost: "Source size and dependencies are capped at 1.0:
/// they can cost you but never pay."
///
/// Being smaller than the reference earns nothing; being larger still costs.
/// Without this cap a hypothetical row with an S ratio above 1.0 would award
/// points the scorer does not give.
pub const S_FACTOR_CAP: f64 = 1.0;

/// The D axis can only cost. D = 1 + (direct non-stdlib runtime dependencies),
/// and D_ref = 1, so the ratio is 1/(1+n) and is always at most 1.0.
pub const D_FACTOR_CAP: f64 = 1.0;

/// D = 1 + the number of direct non-stdlib runtime dependencies declared in
/// the manifest. "The vendored-bytes term is gone."
pub fn d_value(n_deps: u32) -> f64 {
    D_REF + n_deps as f64
}

/// The D ratio implied by a manifest declaring `n` direct dependencies.
/// Zero dependencies gives 1.0; one gives 0.5, which contributes
/// 0.5^0.15 = 0.901 to K.
pub fn d_ratio(n_deps: u32) -> f64 {
    D_REF / d_value(n_deps)
}

/// speed_score = 50 * log10(K), clamped to [0, 100].
///
/// "Speed score = 50 × log10(K), clamped to 0 to 100: K = 10 is half marks and
/// K = 100 is full marks." This replaces the old 100·ln(K)/ln(25) curve, whose
/// anchor was K = 25.
pub fn speed_score(k: f64) -> f64 {
    if k <= 0.0 {
        return 0.0;
    }
    (50.0 * k.log10()).clamp(0.0, 100.0)
}

/// Compute K from raw per-axis ratios. Returns (K, per-axis contributions).
///
/// The ratios are (T_ref/T, M_ref/M, S_ref/S, D_ref/D) — "Every ratio below is
/// your number divided into the reference's."
pub fn coefficient(t: f64, m: f64, s: f64, d: f64) -> (f64, [f64; 4]) {
    // Wall time: no cap, only the finite guard.
    let t_ratio = t.clamp(0.0, T_RATIO_GUARD);
    // Memory: capped at 50x.
    let m_ratio = m.clamp(0.0, AXIS_CAP);
    // Source size and dependencies: capped at 1.0 — they can only cost.
    let s_ratio = s.clamp(0.0, S_FACTOR_CAP);
    let d_ratio = d.clamp(0.0, D_FACTOR_CAP);

    let contrib = [
        t_ratio.powf(W_T),
        m_ratio.powf(W_M),
        s_ratio.powf(W_S),
        d_ratio.powf(W_D),
    ];
    let k = contrib.iter().product();
    (k, contrib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_the_published_worked_example() {
        // The event page's worked example, recomputed under the new formula
        // and marked illustrative: Team Nightshade, Track A.
        //   T 25.0391x  M 5.70x  S 0.5138x  D 1.00x
        //   K = 6.41, speed score = 40.4
        let (k, c) = coefficient(25.0391, 5.7, 0.5138, 1.0);
        assert!((k - 6.41).abs() < 0.01, "K was {k}");
        assert!((c[0] - 5.004).abs() < 0.001, "T contribution was {}", c[0]);
        assert!((c[1] - 1.416).abs() < 0.001, "M contribution was {}", c[1]);
        assert!((c[2] - 0.905).abs() < 0.001, "S contribution was {}", c[2]);
        assert!((c[3] - 1.000).abs() < 0.001);
        assert!(
            (speed_score(k) - 40.4).abs() < 0.05,
            "speed was {}",
            speed_score(k)
        );
    }

    #[test]
    fn the_old_five_axis_nightshade_number_is_no_longer_reproducible() {
        // The pre-freeze formula (five axes, 0.40/0.20/0.15/0.15/0.10, speed
        // 100*ln(K)/ln(25)) produced K = 6.2569 and speed 56.97 for the old
        // example. Under the frozen formula that same input is a different
        // number, and a harness still producing 56.97 is running the OLD
        // rules. This test pins the new answer: K = 8.7810, speed 47.18.
        let (k, _) = coefficient(31.4504, 14.0, 0.5889, 1.0);
        assert!((k - 8.7810).abs() < 0.01, "K was {k}");
        assert!(
            (speed_score(k) - 47.18).abs() < 0.05,
            "speed was {}",
            speed_score(k)
        );

        // And the OLD number must not be what comes out, or this test has
        // proved nothing.
        assert!((k - 6.2569).abs() > 0.1, "the old K is still reproducible");
    }

    #[test]
    fn weights_sum_to_one() {
        let sum = W_T + W_M + W_S + W_D;
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn memory_is_capped_at_fifty_but_wall_time_is_not() {
        // "Memory is capped at 50×. Wall time is not."
        let (k, c) = coefficient(1e9, 1e9, 1e9, 1e9);
        // T saturates only at the finite guard; M at 50; S and D at 1.0.
        assert!(
            (c[1] - 50f64.powf(W_M)).abs() < 1e-9,
            "M contribution was {}",
            c[1]
        );
        assert!(
            (c[0] - 1e9f64.powf(W_T)).abs() < 1e-6,
            "T contribution was {}",
            c[0]
        );
        assert!((c[2] - 1.0).abs() < 1e-12);
        assert!((c[3] - 1.0).abs() < 1e-12);
        let expect = 1e9f64.powf(W_T) * 50f64.powf(W_M);
        assert!((k - expect).abs() < 1e-3, "K was {k}, expected {expect}");
    }

    #[test]
    fn a_faster_program_always_earns_more() {
        // "Wall time is never pathological, so a faster program always earns a
        // higher K." A cap on T would break this.
        let (k_slow, _) = coefficient(2.0, 1.0, 1.0, 1.0);
        let (k_fast, _) = coefficient(4.0, 1.0, 1.0, 1.0);
        assert!(k_fast > k_slow, "{k_fast} !> {k_slow}");
        let (k_huge, _) = coefficient(1000.0, 1.0, 1.0, 1.0);
        assert!(k_huge > k_fast);
    }

    #[test]
    fn source_size_can_only_cost() {
        // "Source size and dependencies are capped at 1.0: they can cost you
        // but never pay." A submission whose source is SMALLER than the
        // reference (ratio > 1) must earn nothing on S.
        let (k_small, c_small) = coefficient(1.0, 1.0, 4.0, 1.0);
        assert!((c_small[2] - 1.0).abs() < 1e-12, "S awarded {}", c_small[2]);
        assert!((k_small - 1.0).abs() < 1e-12, "K awarded {k_small}");

        // Being larger still costs, and the cap must not flatten that.
        let (_, c_big) = coefficient(1.0, 1.0, 0.25, 1.0);
        assert!(c_big[2] < 0.95, "S penalty vanished: {}", c_big[2]);
    }

    #[test]
    fn one_dependency_costs_ten_percent() {
        // "D = 1 + the number of direct non-stdlib runtime dependencies
        // declared in your manifest. ... One dependency gives 2^-0.15 = 0.90,
        // about 10% off K before any other work."
        assert!((d_value(0) - 1.0).abs() < 1e-12);
        assert!((d_value(1) - 2.0).abs() < 1e-12);
        assert!((d_value(3) - 4.0).abs() < 1e-12);
        assert!((d_ratio(0) - 1.0).abs() < 1e-12);
        assert!((d_ratio(1) - 0.5).abs() < 1e-12);

        // The published contribution for one dependency.
        let (_, c) = coefficient(1.0, 1.0, 1.0, d_ratio(1));
        assert!((c[3] - 0.90).abs() < 0.005, "D contribution was {}", c[3]);

        // The vendored-bytes term is gone: vendored source does NOT change D.
        // (D is a function of the manifest's dependency COUNT only.)
        assert!((d_value(0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn speed_curve_anchors() {
        // "Speed score = 50 × log10(K), clamped to 0 to 100: K = 10 is half
        // marks and K = 100 is full marks."
        assert!((speed_score(100.0) - 100.0).abs() < 1e-9);
        assert!((speed_score(10.0) - 50.0).abs() < 1e-9);
        assert!((speed_score(1.0)).abs() < 1e-9);
        assert!(speed_score(0.5).abs() < 1e-9);
        // Past K = 100 the score is clamped, but raw K still breaks ties.
        assert!((speed_score(1000.0) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn track_table_is_complete() {
        let t = tracks();
        // Four tracks. Track E was cut before the freeze.
        assert_eq!(t.len(), 4);
        for id in ['A', 'B', 'C', 'D'] {
            let tr = &t[&id];
            // Timeouts are 10 x reference time, rounded to the nearest integer.
            assert!(
                (tr.timeout_s - (tr.ref_time_s * 10.0).round()).abs() < 1.0,
                "track {id}: timeout {timeout_s} vs ref {ref_time_s}*10={expected}",
                timeout_s = tr.timeout_s,
                ref_time_s = tr.ref_time_s,
                expected = (tr.ref_time_s * 10.0).round()
            );
        }
        // Track E must be gone, not merely absent from the loop above.
        assert!(
            !t.contains_key(&'E'),
            "Track E was cut before the freeze but is still in the track table"
        );
    }

    // ── contract.toml is the single source of truth ──────────────────────
    //
    // The values below were DUPLICATED here and in verification/contract.toml,
    // and duplicated values drift: the contract file gets updated on Oct 28
    // and the Rust table silently keeps the old numbers, so the harness then
    // measures against a contract that no longer exists. This test parses the
    // TOML and asserts the Rust table still agrees, so the drift is caught at
    // `cargo test` time instead of at the scoring box.

    /// A deliberately tiny reader for the flat `key = value` sections the
    /// contract file uses. A full TOML parser would be a dependency; the
    /// file's shape is fixed and this repository writes it.
    fn read_contract(
    ) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
        let path = std::path::Path::new("../../verification/contract.toml");
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!(
                "cannot read {}: {e} — run cargo test from the harness crate dir",
                path.display()
            )
        });
        let mut sections = std::collections::BTreeMap::new();
        let mut current = String::new();
        sections.insert(current.clone(), std::collections::BTreeMap::new());
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current = line[1..line.len() - 1].to_string();
                sections.entry(current.clone()).or_default();
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let raw = v.trim();
                // Strip an inline comment, but only outside quotes:
                //   output_file = "corpus.sd"   # the scored artefact
                // The naive `split('#')` corrupts values that contain a '#'.
                let value = if let Some(rest) = raw.strip_prefix('"') {
                    match rest.find('"') {
                        Some(end) => rest[..end].to_string(),
                        None => raw.to_string(),
                    }
                } else {
                    let bare = raw.split('#').next().unwrap_or(raw).trim();
                    // TOML arrays (`key = [1, 2, 3]`) are read as the
                    // comma-separated body so the callers can `split(',')`
                    // them. Without this the first element keeps its leading
                    // `[` and the segment-geometry assertions below panic with
                    // `invalid digit found in string`.
                    bare.strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(bare)
                        .trim()
                        .to_string()
                };
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(k.trim().to_string(), value);
            }
        }
        sections
    }

    #[test]
    fn rust_table_agrees_with_contract_toml() {
        let doc = read_contract();
        let table = tracks();

        for id in ['A', 'B', 'C', 'D'] {
            let sec = doc
                .get(&id.to_string())
                .unwrap_or_else(|| panic!("contract.toml has no [{id}] section"));
            let tr = &table[&id];

            let ref_time: f64 = sec["ref_time_s"]
                .parse()
                .unwrap_or_else(|e| panic!("[{id}] ref_time_s: {e}"));
            assert!(
                (ref_time - tr.ref_time_s).abs() < 0.01,
                "[{id}] ref_time_s drifted: toml={} rust={}",
                ref_time,
                tr.ref_time_s
            );

            let timeout: f64 = sec["timeout_s"]
                .parse()
                .unwrap_or_else(|e| panic!("[{id}] timeout_s: {e}"));
            assert!(
                (timeout - tr.timeout_s).abs() < 0.5,
                "[{id}] timeout_s drifted: toml={} rust={}",
                timeout,
                tr.timeout_s
            );

            // The output filename: the contract calls it output_file, the
            // manifest too, and the lockdown compares the two. If they ever
            // disagree the correctness gate fails with no obvious cause.
            assert_eq!(
                sec.get("output_file").map(|s| s.as_str()),
                Some(tr.output),
                "[{id}] output_file drifted between contract.toml and the Rust table"
            );

            // The input filename is named per track in the contract
            // (input_file / corpus_file / records_file / graph_file).
            let input_keys = ["input_file", "corpus_file", "records_file", "graph_file"];
            let toml_input = input_keys.iter().find_map(|k| sec.get(*k));
            if let Some(toml_input) = toml_input {
                assert_eq!(
                    toml_input, tr.input,
                    "[{id}] input filename drifted between contract.toml and the Rust table"
                );
            }
        }

        // The global weights and the axis cap.
        let g = &doc["global"];
        let weight_pairs = [
            ("weight_t", W_T),
            ("weight_m", W_M),
            ("weight_s", W_S),
            ("weight_d", W_D),
        ];
        for (key, ours) in weight_pairs {
            let theirs: f64 = g[key]
                .parse()
                .unwrap_or_else(|e| panic!("[global] {key}: {e}"));
            assert!(
                (theirs - ours).abs() < 1e-9,
                "[global] {key} drifted: toml={theirs} rust={ours}"
            );
        }
        let cap: f64 = g["axis_cap"].parse().unwrap();
        assert!((cap - AXIS_CAP).abs() < 1e-9, "axis_cap drifted");

        // The X axis is GONE. A `weight_x` key left in the contract would mean
        // somebody is still scoring a fifth axis, so its absence is asserted.
        assert!(
            !g.contains_key("weight_x"),
            "[global] still has weight_x — the X axis was removed before the freeze"
        );
        assert!(
            !doc.contains_key("E"),
            "contract.toml still has an [E] section — Track E was cut before the freeze"
        );
        // The three gates.
        let gates = g["gates"].clone();
        assert!(
            gates.contains("correctness") && gates.contains("timeout") && gates.contains("sandbox"),
            "[global] gates must be exactly correctness, timeout, sandbox — got {gates}"
        );
        assert!(
            !gates.contains("speed"),
            "[global] gates still list a speed gate — the speed floor gate was removed"
        );
        // Track E (Compress) was removed before the freeze. The old segment-
        // geometry assertions for it have been deleted with the track.
    }
}
