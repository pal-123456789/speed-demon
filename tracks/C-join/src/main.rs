//! Speed Demon — Track C (Join)
//!
//! SCAFFOLD: correctness-shaped I/O skeleton. The real implementation lands
//! inside the 72-hour window (H0). The sentinel line written to the output is
//! detected by `verification/pre-freeze.ps1`, so a stub can NEVER pass the
//! freeze checklist by accident.
//!
//! Contract (the four Dockerfile rules):
//!   1. read  from /data   (read-only mount)
//!   2. write exactly ONE file to /out
//!   3. exit 0
//!   4. build offline (see build.sh — rustc only, no cargo, no network)

use speeddemon_core::buf_reader::StreamingReader;
use speeddemon_core::det_writer::DetWriter;
use std::env;
use std::path::PathBuf;

const TRACK: &str = "C";
const DEFAULT_INPUT: &str = "records.csv";
const DEFAULT_OUTPUT: &str = "join.txt";

/// Sentinel that proves this binary is still the pre-kickoff scaffold.
/// `verification/pre-freeze.ps1` fails the build if it appears in any output.
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

/// Input path: `/data/<default>`, overridable by `SD_IN_<TRACK>`.
fn input_path() -> PathBuf {
    let key = format!("SD_IN_{TRACK}");
    let name = env::var(&key).unwrap_or_else(|_| DEFAULT_INPUT.to_string());
    data_dir().join(name)
}

/// Output path: `/out/<default>`, overridable by `SD_OUT_<TRACK>`.
fn output_path() -> PathBuf {
    let key = format!("SD_OUT_{TRACK}");
    let name = env::var(&key).unwrap_or_else(|_| DEFAULT_OUTPUT.to_string());
    out_dir().join(name)
}

fn main() {
    let input = input_path();
    let output = output_path();

    // Read the input with the streaming reader (never mmap: mmap pages are
    // charged to cgroup memory.peak and would inflate the M axis).
    let mut lines = 0u64;
    let mut bytes = 0u64;
    match StreamingReader::open(&input) {
        Ok(mut reader) => {
            let mut sink = |line: &[u8]| {
                lines += 1;
                bytes += line.len() as u64 + 1;
            };
            if let Err(e) = reader.for_each_line(&mut sink) {
                eprintln!("[track-{TRACK}] read error on {}: {e}", input.display());
            }
        }
        Err(e) => {
            // Never exit non-zero: a missing input is an environment problem,
            // and the harness treats a non-zero exit as a failed run.
            eprintln!("[track-{TRACK}] cannot open {}: {e}", input.display());
        }
    }

    // Write EXACTLY ONE file to /out.
    match DetWriter::create(&output) {
        Ok(mut w) => {
            let _ = w.write_line(SENTINEL);
            let _ = w.write_line(&format!("# track={TRACK} lines={lines} bytes={bytes}"));
            let _ = w.finish();
        }
        Err(e) => {
            eprintln!("[track-{TRACK}] cannot create {}: {e}", output.display());
        }
    }
}
