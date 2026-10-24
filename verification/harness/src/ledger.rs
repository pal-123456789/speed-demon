//! The measurement ledger — an append-only JSONL record of every run.
//!
//! Why this exists: the judges award 8% of every track for the optimisation
//! log, and they ask for "measurements, not adjectives". A hand-written
//! table drifts from reality. Here every measurement the harness takes is
//! appended to a ledger, and the PERF.md tables are GENERATED from the
//! ledger. A number in PERF.md is therefore always a number that a machine
//! produced.
//!
//! Format: one JSON object per line, keys in a fixed order so the file
//! diffs cleanly and stays greppable.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// One measured run.
#[derive(Debug, Clone)]
pub struct RunRecord {
    pub track: char,
    /// The git commit that produced the binary, so a number is always
    /// traceable to a source state.
    pub commit: String,
    /// Unix seconds.
    pub timestamp: i64,
    /// Run index within the batch, 1-based.
    pub run: u32,
    /// Wall time in seconds, measured around the container run.
    pub wall_s: f64,
    /// Peak memory in MiB, from the cgroup kernel counter.
    pub peak_mib: Option<f64>,
    /// Where the peak came from ("cgroup:memory.peak" or "rss-sampled").
    pub peak_source: String,
    /// SHA-256 of the output file.
    pub output_sha256: String,
    /// The expected SHA-256, if one is pinned.
    pub expected_sha256: Option<String>,
    /// Bytes written to the output file.
    pub output_bytes: u64,
    /// Container exit code.
    pub exit_code: i32,
    /// A short label for the experiment this run represents ("baseline",
    /// "ladder-03-radix", ...). Lets the ladder table be built from data.
    pub label: String,
    /// Threads used, if the track parameterises them.
    pub threads: u32,
    /// True when the run measured the pre-kickoff *scaffold* rather than a real
    /// implementation — i.e. the output file still carries the
    /// `#SCAFFOLD_NOT_IMPLEMENTED` sentinel.
    ///
    /// This is recorded at write time, where the evidence is authoritative,
    /// rather than inferred later by the doc generator: a scaffold reads one
    /// file and writes ~54 bytes, so its wall time is pure process start-up and
    /// publishing it would claim a thousandfold speed-up over the reference on
    /// the strength of a binary that does nothing.
    pub scaffold: bool,
}

impl RunRecord {
    /// Whether this run passed the correctness gate, given the pinned hash.
    ///
    /// With no hash pinned the gate is UNVERIFIABLE, not passed — which is
    /// why `passed` is only consulted when `expected_sha256` is `Some`.
    #[allow(dead_code)]
    pub fn passed(&self) -> bool {
        match &self.expected_sha256 {
            Some(exp) => exp.eq_ignore_ascii_case(&self.output_sha256),
            None => false,
        }
    }

    fn to_json(&self) -> String {
        // Every string field must be escaped. The previous version interpolated
        // `label` and `peak_source` raw, so a label containing a backslash (a
        // Windows path, which is exactly what a `--ledger` test produces) or a
        // quote produced INVALID JSONL. Nothing failed at write time: the
        // corruption was only visible later, as a `serde`-style parse error in
        // whatever read the ledger back — and `gen_docs.py` silently skipped
        // unparseable lines, so measured rows quietly disappeared from PERF.md.
        let opt_str = |v: &Option<String>| match v {
            Some(s) => json_str(s),
            None => "null".to_string(),
        };
        let opt_num = |v: &Option<f64>| match v {
            Some(x) => format!("{x:.4}"),
            None => "null".to_string(),
        };
        format!(
            "{{\"track\":{},\"commit\":{},\"timestamp\":{},\"run\":{},\
             \"wall_s\":{:.4},\"peak_mib\":{},\"peak_source\":{},\
             \"output_sha256\":{},\"expected_sha256\":{},\"output_bytes\":{},\
             \"exit_code\":{},\"label\":{},\"threads\":{},\"scaffold\":{}}}",
            json_str(&self.track.to_string()),
            json_str(&self.commit),
            self.timestamp,
            self.run,
            self.wall_s,
            opt_num(&self.peak_mib),
            json_str(&self.peak_source),
            json_str(&self.output_sha256),
            opt_str(&self.expected_sha256),
            self.output_bytes,
            self.exit_code,
            json_str(&self.label),
            self.threads,
            self.scaffold,
        )
    }
}

/// Encode `s` as a JSON string literal, including the surrounding quotes.
///
/// Escapes what RFC 8259 requires: `"`, `\`, and the C0 control characters.
/// Non-ASCII is passed through as UTF-8, which is valid JSON and keeps the
/// ledger readable (paths and labels are the common case).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Append a record to the ledger, creating it if needed.
pub fn append(path: &Path, rec: &RunRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", rec.to_json())
}

/// Read every record from the ledger.
pub fn read_all(path: &Path) -> std::io::Result<Vec<RunRecord>> {
    let raw = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(r) = parse_line(line) {
            out.push(r);
        }
    }
    Ok(out)
}

/// A deliberately small, dependency-free JSON reader for the fixed shape
/// this tool writes. It extracts fields by key rather than building a full
/// JSON value, because the shape is known and a real parser is overkill.
fn parse_line(line: &str) -> Option<RunRecord> {
    let field = |key: &str| -> Option<String> {
        let pat = format!("\"{key}\":");
        let i = line.find(&pat)? + pat.len();
        let rest = &line[i..];
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"')?;
            Some(stripped[..end].to_string())
        } else {
            let end = rest.find([',', '}'])?;
            Some(rest[..end].trim().to_string())
        }
    };
    let field_f64 = |key: &str| -> Option<f64> { field(key)?.parse().ok() };
    let field_u32 = |key: &str| -> Option<u32> { field(key)?.parse().ok() };
    let field_u64 = |key: &str| -> Option<u64> { field(key)?.parse().ok() };
    let field_i32 = |key: &str| -> Option<i32> { field(key)?.parse().ok() };
    let field_i64 = |key: &str| -> Option<i64> { field(key)?.parse().ok() };

    Some(RunRecord {
        track: field("track")?.chars().next()?,
        commit: field("commit")?,
        timestamp: field_i64("timestamp")?,
        run: field_u32("run")?,
        wall_s: field_f64("wall_s")?,
        peak_mib: field_f64("peak_mib"),
        peak_source: field("peak_source").unwrap_or_default(),
        output_sha256: field("output_sha256")?,
        expected_sha256: field("expected_sha256").filter(|s| s != "null" && !s.is_empty()),
        output_bytes: field_u64("output_bytes")?,
        exit_code: field_i32("exit_code")?,
        label: field("label")?,
        threads: field_u32("threads")?,
        // Absent in records written before this field existed. Those were all
        // scaffold runs (the only thing that could run pre-kickoff), but we do
        // not assume it: the doc generator has its own size heuristic.
        scaffold: field("scaffold").map(|v| v == "true").unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunRecord {
        RunRecord {
            track: 'A',
            commit: "abc1234".into(),
            timestamp: 1_700_000_000,
            run: 1,
            wall_s: 1.31,
            peak_mib: Some(210.0),
            peak_source: "cgroup:memory.peak".into(),
            output_sha256: "a".repeat(64),
            expected_sha256: Some("a".repeat(64)),
            output_bytes: 4096,
            exit_code: 0,
            label: "baseline".into(),
            threads: 12,
            scaffold: false,
        }
    }

    #[test]
    fn roundtrips_through_jsonl() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("sd-ledger-test-{}.jsonl", std::process::id()));
        let rec = sample();
        append(&path, &rec).unwrap();
        let all = read_all(&path).unwrap();
        assert_eq!(all.len(), 1);
        let got = &all[0];
        assert_eq!(got.track, 'A');
        assert_eq!(got.wall_s, 1.31);
        assert_eq!(got.peak_mib, Some(210.0));
        assert_eq!(got.output_sha256, "a".repeat(64));
        assert_eq!(got.expected_sha256, Some("a".repeat(64)));
        assert_eq!(got.label, "baseline");
        assert_eq!(got.threads, 12);
        assert!(got.passed());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_expected_hash_still_parses() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("sd-ledger-test2-{}.jsonl", std::process::id()));
        let mut rec = sample();
        rec.expected_sha256 = None;
        append(&path, &rec).unwrap();
        let all = read_all(&path).unwrap();
        assert_eq!(all[0].expected_sha256, None);
        let _ = std::fs::remove_file(&path);
    }
}
