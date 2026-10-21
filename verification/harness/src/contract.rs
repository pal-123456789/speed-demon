//! The frozen HEL1 contract, in one place.
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
    /// Reference wall time on HEL1, seconds.
    pub ref_time_s: f64,
    /// Reference peak memory on HEL1, MB (None where unpublished).
    pub ref_mem_mb: Option<f64>,
    /// Reference source size, bytes (None where unpublished).
    pub ref_src_bytes: Option<f64>,
    /// Hard timeout = 10 x reference time, seconds.
    pub timeout_s: f64,
    /// The M axis holds compressed size rather than peak memory.
    pub m_is_compressed_size: bool,
}

impl Track {
    /// The reference M value that feeds the M axis ratio.
    ///
    /// For four tracks the M axis is peak memory, so the reference value is
    /// the reference's own peak. For Track E the M axis is *compressed
    /// size*, and the ratio is (our ratio) / (the reference's 2.41x), so the
    /// reference's own M ratio against itself is exactly 1.0.
    pub fn ref_mb(&self) -> Option<f64> {
        if self.m_is_compressed_size {
            Some(1.0)
        } else {
            self.ref_mem_mb
        }
    }
}

pub fn tracks() -> BTreeMap<char, Track> {
    let mut m = BTreeMap::new();
    m.insert(
        'A',
        Track {
            id: 'A',
            dir: "A-ingest",
            input: "access.log",
            output: "ingest.txt",
            ref_time_s: 41.20,
            ref_mem_mb: Some(2940.0),
            ref_src_bytes: Some(3180.0),
            timeout_s: 412.0,
            m_is_compressed_size: false,
        },
    );
    m.insert(
        'B',
        Track {
            id: 'B',
            dir: "B-index",
            input: "corpus.txt",
            output: "index.txt",
            ref_time_s: 58.70,
            ref_mem_mb: None,
            ref_src_bytes: None,
            timeout_s: 587.0,
            m_is_compressed_size: false,
        },
    );
    m.insert(
        'C',
        Track {
            id: 'C',
            dir: "C-join",
            input: "records.csv",
            output: "join.txt",
            ref_time_s: 36.90,
            ref_mem_mb: None,
            ref_src_bytes: None,
            timeout_s: 369.0,
            m_is_compressed_size: false,
        },
    );
    m.insert(
        'D',
        Track {
            id: 'D',
            dir: "D-traverse",
            input: "graph.txt",
            output: "traverse.txt",
            ref_time_s: 72.40,
            ref_mem_mb: None,
            ref_src_bytes: None,
            timeout_s: 724.0,
            m_is_compressed_size: false,
        },
    );
    m.insert(
        'E',
        Track {
            id: 'E',
            dir: "E-compress",
            input: "corpus.bin",
            output: "corpus.sd",
            ref_time_s: 29.80,
            ref_mem_mb: None,
            ref_src_bytes: None,
            timeout_s: 298.0,
            m_is_compressed_size: true,
        },
    );
    m
}

/// The scoring weights. They sum to exactly 1.0.
pub const W_T: f64 = 0.40;
pub const W_M: f64 = 0.20;
pub const W_S: f64 = 0.15;
pub const W_D: f64 = 0.15;
pub const W_X: f64 = 0.10;

/// Every ratio is capped at 50x.
pub const AXIS_CAP: f64 = 50.0;

/// speed_score = 100 * ln(K) / ln(25), clamped to [0, 100].
pub fn speed_score(k: f64) -> f64 {
    if k <= 0.0 {
        return 0.0;
    }
    (100.0 * k.ln() / 25f64.ln()).clamp(0.0, 100.0)
}

/// Compute K from raw per-axis ratios. Returns (K, per-axis contributions).
pub fn coefficient(t: f64, m: f64, s: f64, d: f64, x: f64) -> (f64, [f64; 5]) {
    let ratios = [t, m, s, d, x];
    let weights = [W_T, W_M, W_S, W_D, W_X];
    let mut contrib = [0.0f64; 5];
    let mut k = 1.0f64;
    for i in 0..5 {
        let capped = ratios[i].clamp(0.0, AXIS_CAP);
        contrib[i] = capped.powf(weights[i]);
        k *= contrib[i];
    }
    (k, contrib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_the_published_nightshade_example() {
        // From the event page: Team Nightshade, Track A.
        let (k, c) = coefficient(31.4504, 14.0, 0.5889, 1.0, 1.0612);
        assert!((k - 6.2569).abs() < 0.001, "K was {k}");
        assert!((c[0] - 3.972).abs() < 0.001, "T contribution was {}", c[0]);
        assert!((c[1] - 1.695).abs() < 0.001, "M contribution was {}", c[1]);
        assert!((c[2] - 0.924).abs() < 0.001, "S contribution was {}", c[2]);
        assert!((c[3] - 1.000).abs() < 0.001);
        assert!((c[4] - 1.006).abs() < 0.001);
        assert!(
            (speed_score(k) - 56.97).abs() < 0.05,
            "speed was {}",
            speed_score(k)
        );
    }

    #[test]
    fn weights_sum_to_one() {
        let sum = W_T + W_M + W_S + W_D + W_X;
        assert!((sum - 1.0).abs() < 1e-12);
    }

    #[test]
    fn every_axis_is_capped_at_fifty() {
        let (k, _) = coefficient(1e9, 1e9, 1e9, 1e9, 1e9);
        // 50^1.0 == 50
        assert!((k - 50.0).abs() < 1e-6, "K was {k}");
    }

    #[test]
    fn speed_curve_anchors() {
        assert!((speed_score(25.0) - 100.0).abs() < 1e-9);
        assert!((speed_score(5.0) - 50.0).abs() < 1e-9);
        assert!((speed_score(1.0)).abs() < 1e-9);
        assert!(speed_score(0.5).abs() < 1e-9);
    }

    #[test]
    fn track_table_is_complete() {
        let t = tracks();
        assert_eq!(t.len(), 5);
        for id in ['A', 'B', 'C', 'D', 'E'] {
            let tr = &t[&id];
            assert!(
                (tr.timeout_s - tr.ref_time_s * 10.0).abs() < 0.1,
                "track {id}"
            );
        }
    }
}
