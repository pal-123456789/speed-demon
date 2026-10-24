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
pub fn json_str(s: &str) -> String {
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
///
/// Unreadable rows are an ERROR, not a silent omission. The previous version
/// skipped them (`if let Some(r) = parse_line(line)`), so a row written by any
/// other tool — or a half-written final line after an interrupted append —
/// vanished without a diagnostic, and every number generated from the ledger
/// was quietly computed over a subset of the evidence. The doc generator
/// (`tooling/gen_docs.py`) already refuses to work from a partially readable
/// ledger for exactly this reason; the harness is the last place that should
/// be lax about it.
///
/// The one tolerated exception is a truncated FINAL line, which is what an
/// interrupted append actually produces: that is reported on stderr and
/// dropped, because failing the whole read would make one lost run destroy
/// access to every earlier measurement.
pub fn read_all(path: &Path) -> std::io::Result<Vec<RunRecord>> {
    let raw = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = raw.lines().collect();
    let last_content = lines
        .iter()
        .rposition(|l| !l.trim().is_empty() && !l.trim().starts_with('#'));
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line) {
            Ok(r) => out.push(r),
            Err(e) if Some(i) == last_content => {
                eprintln!(
                    "  WARN: ignoring a truncated final ledger row (line {}): {e}",
                    i + 1
                );
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("ledger line {} is unreadable: {e}", i + 1),
                ));
            }
        }
    }
    Ok(out)
}

/// A key-lookup scanner for the flat object this tool writes.
///
/// This is not a general JSON parser, but it does have to agree with the JSON
/// that other writers produce. The previous version required a value to start
/// with `"` IMMEDIATELY after the colon, which is true only of this crate's own
/// output: `json.dumps`, `jq -c` and `serde_json` all emit `"track": "A"`, so
/// every foreign row was misparsed as a number and kept its quotes (`track`
/// became the character `"`), and string values were truncated at the first
/// escaped quote. Both are fixed here: whitespace after the colon is skipped
/// and escapes are honoured.
fn parse_line(line: &str) -> Result<RunRecord, String> {
    let field = |key: &str| -> Option<String> { field_raw(line, key) };
    let need = |key: &str| -> Result<String, String> {
        field(key).ok_or_else(|| format!("missing field {key:?}"))
    };
    let num = |key: &str| -> Result<String, String> { need(key) };

    let parse_f64 = |key: &str| -> Result<f64, String> {
        let raw = num(key)?;
        raw.parse::<f64>()
            .map_err(|e| format!("{key}: {raw:?} is not a number: {e}"))
    };
    let parse_u32 = |key: &str| -> Result<u32, String> {
        let raw = num(key)?;
        raw.parse::<u32>()
            .map_err(|e| format!("{key}: {raw:?} is not an integer: {e}"))
    };
    let parse_u64 = |key: &str| -> Result<u64, String> {
        let raw = num(key)?;
        raw.parse::<u64>()
            .map_err(|e| format!("{key}: {raw:?} is not an integer: {e}"))
    };
    let parse_i32 = |key: &str| -> Result<i32, String> {
        let raw = num(key)?;
        raw.parse::<i32>()
            .map_err(|e| format!("{key}: {raw:?} is not an integer: {e}"))
    };
    let parse_i64 = |key: &str| -> Result<i64, String> {
        let raw = num(key)?;
        raw.parse::<i64>()
            .map_err(|e| format!("{key}: {raw:?} is not an integer: {e}"))
    };

    let track_raw = need("track")?;
    let track = track_raw
        .chars()
        .next()
        .ok_or_else(|| "track is empty".to_string())?;

    Ok(RunRecord {
        track,
        commit: need("commit")?,
        timestamp: parse_i64("timestamp")?,
        run: parse_u32("run")?,
        wall_s: parse_f64("wall_s")?,
        peak_mib: field("peak_mib").and_then(|s| s.parse::<f64>().ok()),
        peak_source: field("peak_source").unwrap_or_default(),
        output_sha256: need("output_sha256")?,
        expected_sha256: field("expected_sha256").filter(|s| s != "null" && !s.is_empty()),
        output_bytes: parse_u64("output_bytes")?,
        exit_code: parse_i32("exit_code")?,
        label: need("label")?,
        threads: parse_u32("threads")?,
        // Absent in records written before this field existed. Those were all
        // scaffold runs (the only thing that could run pre-kickoff), but we do
        // not assume it: the doc generator has its own size heuristic.
        scaffold: field("scaffold").map(|v| v == "true").unwrap_or(false),
    })
}

/// Find `"key"` followed by `:` and return its raw value, decoded.
fn field_raw(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(rel) = line[from..].find(&needle) {
        let after_key = from + rel + needle.len();
        let rest = line[after_key..].trim_start();
        // Not this occurrence: it is the value of another field, or a key with
        // a different name that happens to contain this one.
        let Some(after_colon) = rest.strip_prefix(':') else {
            from = after_key;
            continue;
        };
        let value = after_colon.trim_start();
        if let Some(body) = value.strip_prefix('"') {
            return Some(decode_string(body));
        }
        let end = value.find([',', '}']).unwrap_or(value.len());
        return Some(value[..end].trim().to_string());
    }
    None
}

/// Decode a JSON string body (the part after the opening quote) up to its
/// closing quote. Unknown escapes are passed through rather than dropped, so a
/// malformed row is reported by the field's own parser instead of silently
/// losing characters.
fn decode_string(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(ch) => out.push(ch),
                        None => out.push_str(&hex),
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            c => out.push(c),
        }
    }
    out
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
