//! Speed Demon - Track D (Traverse) - SCAFFOLD.
//!
//! Pre-kickoff: a correctness-shaped I/O skeleton with NO track algorithm.
//! The real implementation lands inside the 72-hour window (H0). The
//! sentinel line below is detected by verification/pre-freeze.ps1, so this
//! stub can never pass the freeze checklist by accident.
//!
//! Docker contract: read /data (ro mount), write exactly one file to /out,
//! exit 0.

mod io;

use std::env;
use std::path::PathBuf;

const TRACK: &str = "D";
const DEFAULT_INPUT: &str = "graph.txt";
const DEFAULT_OUTPUT: &str = "traverse.txt";

/// Sentinel proving this binary is still the pre-kickoff scaffold.
const SENTINEL: &str = "#SCAFFOLD_NOT_IMPLEMENTED";

fn data_dir() -> PathBuf {
    env::var("SD_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data"))
}

fn out_dir() -> PathBuf {
    env::var("SD_OUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/out"))
}

/// Input path: /data/<default>, overridable by SD_IN_<TRACK>.
fn input_path() -> PathBuf {
    let name = env::var(format!("SD_IN_{TRACK}")).unwrap_or_else(|_| DEFAULT_INPUT.to_string());
    data_dir().join(name)
}

/// Output path: /out/<default>, overridable by SD_OUT_<TRACK>.
fn output_path() -> PathBuf {
    let name = env::var(format!("SD_OUT_{TRACK}")).unwrap_or_else(|_| DEFAULT_OUTPUT.to_string());
    out_dir().join(name)
}

fn main() {
    let input = input_path();
    let output = output_path();

    let mut lines = 0u64;
    let mut bytes = 0u64;
    match io::LineReader::open(&input) {
        Ok(mut reader) => {
            let mut sink = |l: &[u8]| {
                lines += 1;
                bytes += l.len() as u64 + 1;
            };
            if let Err(e) = reader.for_each_line(&mut sink) {
                eprintln!("[track-{TRACK}] read error on {}: {e}", input.display());
            }
        }
        Err(e) => eprintln!("[track-{TRACK}] cannot open {}: {e}", input.display()),
    }

    // Write EXACTLY ONE file to /out.
    match io::DetWriter::create(&output) {
        Ok(mut w) => {
            let _ = w.write_line(SENTINEL);
            let _ = w.write_line(&format!("# track={TRACK} lines={lines} bytes={bytes}"));
            let _ = w.finish();
        }
        Err(e) => eprintln!("[track-{TRACK}] cannot create {}: {e}", output.display()),
    }
}