//! The pre-freeze lockdown, in Rust.
//!
//! This is the same checklist as `verification/pre-freeze.ps1`, but
//! implemented natively so it runs on the scoring box (HEL1 is Debian; there
//! is no PowerShell there). One implementation of each gate, usable from
//! both platforms, is worth more than two implementations that drift.
//!
//! The point of the lockdown is that it must be able to say NO. A check that
//! always passes is decoration. The scaffold-sentinel check is the sharpest
//! example: until a track has a real implementation, the lockdown refuses
//! to certify it, and no amount of "the code looks right" gets around it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::contract::Track;
use crate::ledger;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Pass,
    Fail,
    Skip,
}

impl Status {
    /// The coloured tag used in the lockdown report.
    pub fn tag(&self) -> &'static str {
        match self {
            Status::Pass => "\x1b[32mPASS\x1b[0m",
            Status::Fail => "\x1b[31mFAIL\x1b[0m",
            Status::Skip => "\x1b[33mSKIP\x1b[0m",
        }
    }
}

pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

pub struct Lockdown {
    pub checks: Vec<Check>,
}

impl Lockdown {
    fn new() -> Self {
        Self { checks: Vec::new() }
    }
    fn pass(&mut self, name: &str, detail: &str) {
        self.checks.push(Check {
            name: name.to_string(),
            status: Status::Pass,
            detail: detail.to_string(),
        });
    }
    fn fail(&mut self, name: &str, detail: &str) {
        self.checks.push(Check {
            name: name.to_string(),
            status: Status::Fail,
            detail: detail.to_string(),
        });
    }
    fn skip(&mut self, name: &str, detail: &str) {
        self.checks.push(Check {
            name: name.to_string(),
            status: Status::Skip,
            detail: detail.to_string(),
        });
    }
    pub fn failures(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status == Status::Fail)
            .count()
    }
}

/// Read a key out of a `.speed-demon.toml` without a TOML parser.
///
/// The file is a flat key/value manifest that we write ourselves, so a
/// one-line key lookup is sufficient and keeps the harness dependency-free.
fn toml_value(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Run the whole lockdown for the given tracks.
pub fn run(root: &Path, tracks: &BTreeMap<char, Track>, selected: &[char]) -> Lockdown {
    let mut ld = Lockdown::new();

    // ── per-track checks ────────────────────────────────────────────────
    for id in selected {
        let track = &tracks[id];
        let dir = root.join("tracks").join(track.dir);
        ld.pass_track_header(track);

        for required in [
            "Dockerfile",
            "PERF.md",
            "README.md",
            ".speed-demon.toml",
            "build.sh",
        ] {
            let p = dir.join(required);
            if p.is_file() {
                ld.pass(&format!("required file: {required}"), "");
            } else {
                ld.fail(&format!("required file: {required}"), "missing");
            }
        }
        if dir.join("src").is_dir() {
            ld.pass("required dir: src/", "");
        } else {
            ld.fail("required dir: src/", "missing");
        }

        // The D-axis honesty, from the track's own declaration.
        let manifest = dir.join(".speed-demon.toml");
        let declared = toml_value(&manifest, "declared_dependencies")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        if declared == 0 {
            ld.pass("D axis: declared_dependencies == 0", "declared=0");
        } else {
            ld.fail(
                "D axis: declared_dependencies == 0",
                &format!("declared={declared}"),
            );
        }
        let vendored = toml_value(&manifest, "vendored_dependency_bytes")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        if vendored == 0 {
            ld.pass("D axis: no vendored dependency bytes", "vendored=0");
        } else {
            ld.fail(
                "D axis: no vendored dependency bytes",
                &format!("vendored={vendored}"),
            );
        }

        // The manifest declares the output filename; the binary hardcodes
        // it. If those two ever disagree, the submission writes a file the
        // organizers are not expecting, and the correctness gate fails with
        // no obvious cause. The output filename is one of the DEFERRED
        // values, so it WILL change on Oct 28 — and this check is what
        // catches the day somebody changes one and forgets the other.
        match toml_value(&manifest, "output_file") {
            Some(d) if d == track.output => ld.pass("manifest output_file matches the binary", &d),
            Some(d) => ld.fail(
                "manifest output_file matches the binary",
                &format!(
                    "manifest says '{d}' but the binary writes '{}' — the correctness \
                     gate would fail with no obvious cause",
                    track.output
                ),
            ),
            None => ld.skip(
                "manifest output_file matches the binary",
                "no output_file declared in .speed-demon.toml",
            ),
        }

        // The sentinel check — the sharpest one.
        let sentinel = find_sentinel(&dir.join("src"));
        if sentinel.is_none() {
            ld.pass("no scaffold sentinel in source", "");
        } else {
            ld.fail(
                "no scaffold sentinel in source",
                &format!("{} — a stub must never reach the freeze", sentinel.unwrap()),
            );
        }

        // The correctness gate can only be checked once the sample hash is
        // pinned. Before that it is UNVERIFIABLE, and saying so is more
        // useful than a green tick.
        let hash_pinned = dir
            .join("verification")
            .join("expected")
            .join(format!("{}.sha256", track.output))
            .is_file()
            || dir
                .join("verification")
                .join("expected")
                .join("sample.sha256")
                .is_file();
        if hash_pinned {
            ld.pass("sample SHA-256 pinned", "the correctness gate is checkable");
        } else {
            ld.skip(
                "sample SHA-256 green",
                "NO EXPECTED HASH PINNED — the correctness gate cannot be verified",
            );
        }
    }

    // ── repo checks ─────────────────────────────────────────────────────
    if root.join("LICENSE").is_file() {
        let text = fs::read_to_string(root.join("LICENSE")).unwrap_or_default();
        let first = text.lines().next().unwrap_or("").to_lowercase();
        if first.contains("mit license") || first.contains("apache license") {
            ld.pass("OSI licence present", first.trim());
        } else {
            ld.fail("OSI licence present", "unrecognised licence header");
        }
    } else {
        ld.fail("OSI licence present", "LICENSE missing");
    }

    if root.join("Dockerfile").is_file() {
        ld.pass("scored Dockerfile present at the repository root", "");
    } else {
        ld.fail(
            "scored Dockerfile present at the repository root",
            "missing",
        );
    }

    if root.join("run").is_file() {
        ld.pass("runtime dispatcher present", "");
    } else {
        ld.fail("runtime dispatcher present", "missing");
    }

    ld
}

impl Lockdown {
    fn pass_track_header(&mut self, _track: &Track) {}
}

/// Find the scaffold sentinel in a source tree, if present.
pub fn find_sentinel(src: &Path) -> Option<String> {
    if !src.is_dir() {
        return None;
    }
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                if let Ok(text) = fs::read_to_string(&p) {
                    if text.contains("SCAFFOLD_NOT_IMPLEMENTED") {
                        return Some(
                            p.strip_prefix(src.parent().unwrap_or(src))
                                .unwrap_or(&p)
                                .display()
                                .to_string(),
                        );
                    }
                }
            }
        }
    }
    None
}

/// Report the ledger's most recent measurement for a track, if any.
pub fn last_measurement(root: &Path, id: char) -> Option<ledger::RunRecord> {
    let path: PathBuf = root.join("verification").join("ledger.jsonl");
    let recs = ledger::read_all(&path).ok()?;
    recs.into_iter().filter(|r| r.track == id).next_back()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_sentinel() {
        let dir = std::env::temp_dir().join(format!("sd-sentinel-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(
            dir.join("main.rs"),
            "fn main() {}\n// SCAFFOLD_NOT_IMPLEMENTED\n",
        )
        .unwrap();
        assert!(find_sentinel(&dir).is_some());
        fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        assert!(find_sentinel(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_a_flat_toml_key() {
        let dir = std::env::temp_dir().join(format!("sd-toml-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let p = dir.join("x.toml");
        fs::write(&p, "# comment\ntrack = \"A\"\ndeclared_dependencies = 0\n").unwrap();
        assert_eq!(
            toml_value(&p, "declared_dependencies").as_deref(),
            Some("0")
        );
        assert_eq!(toml_value(&p, "track").as_deref(), Some("A"));
        assert_eq!(toml_value(&p, "nope"), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
