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

use crate::contract::{self, Track};
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
/// Count the DIRECT dependencies a track's Cargo.toml declares.
///
/// The D-axis gate used to read `declared_dependencies` out of the track's
/// hand-maintained `.speed-demon.toml` and believe it. That is the same
/// defect this whole audit is about: a self-declared number treated as
/// evidence. This counts what the manifest actually says, so the two can be
/// compared.
///
/// Rules:
/// * only the `[dependencies]` table counts — dev-dependencies and
///   build-dependencies are not runtime dependencies;
/// * a `path` dependency is this repository's own code. It is source under
///   the S axis, not a third-party dependency under D, so it does not count.
pub fn count_direct_dependencies(cargo_toml: &Path) -> Option<usize> {
    let text = fs::read_to_string(cargo_toml).ok()?;
    let mut in_deps = false;
    let mut count = 0usize;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('#') {
            continue;
        }
        if s.starts_with('[') {
            in_deps = s == "[dependencies]";
            continue;
        }
        if !in_deps || !s.contains('=') {
            continue;
        }
        let (name, value) = s.split_once('=')?;
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // A path dependency is our own code.
        if value.contains("path") {
            continue;
        }
        count += 1;
    }
    Some(count)
}

fn toml_value(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                // Strip an inline comment OUTSIDE quotes, then unquote.
                // `trim_matches('"')` on the raw value left the comment glued
                // to the string, so a legal TOML file with
                //   output_file = "ingest.txt"   # the scored artefact
                // produced the "value" `ingest.txt"       # the scored artefact`
                // and three spurious FAILs. `contract.rs` already parsed this
                // correctly; the two readers now agree on the dialect.
                let raw = v.trim();
                let value = if let Some(rest) = raw.strip_prefix('"') {
                    match rest.find('"') {
                        Some(end) => rest[..end].to_string(),
                        None => raw.to_string(),
                    }
                } else {
                    raw.split('#').next().unwrap_or(raw).trim().to_string()
                };
                return Some(value);
            }
        }
    }
    None
}

/// The pinned expected hash for a track, if the file holds a real one.
///
/// Mirrors `main::expected_hash`: 64 hex characters, optionally followed by
/// the conventional `  <filename>` column. A file that exists but does not
/// contain a hash is not a pin.
fn read_pinned_hash(dir: &Path, output: &str) -> Option<String> {
    let base = dir.join("verification").join("expected");
    for name in [format!("{output}.sha256"), "sample.sha256".to_string()] {
        let Ok(raw) = fs::read_to_string(base.join(&name)) else {
            continue;
        };
        let raw = raw.trim().trim_start_matches('\u{feff}');
        if let Some(hex) = raw.split_whitespace().next() {
            if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(hex.to_ascii_lowercase());
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
        // An unknown track letter must be a reported failure, not a panic:
        // `lockdown --track Z` used to abort with "no entry found for key"
        // (exit 101) and print no verdict at all.
        let Some(track) = tracks.get(id) else {
            ld.fail(
                &format!("track {id} exists"),
                "unknown track letter (expected A|B|C|D|E)",
            );
            continue;
        };
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

        // The D-axis honesty, from the track's own declaration — and now
        // cross-checked against the manifest it claims to describe.
        let manifest = dir.join(".speed-demon.toml");
        let declared = toml_value(&manifest, "declared_dependencies")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(-1);
        let actual = count_direct_dependencies(&dir.join("Cargo.toml"));
        match actual {
            Some(n) if n as i64 == declared => {
                let factor = contract::d_ratio(n as u32);
                ld.pass(
                    "D axis: declared_dependencies == 0",
                    &format!("declared={declared} (matches Cargo.toml), D factor {factor:.4}"),
                );
            }
            Some(n) => {
                // The declared number and the manifest disagree: one of them
                // is wrong, and we cannot tell which, so the check fails.
                ld.fail(
                    "D axis: declared_dependencies == 0",
                    &format!(
                        "declared={declared} but Cargo.toml declares {n} direct \
                         dependency(ies); D factor would be {:.4}",
                        contract::d_ratio(n as u32)
                    ),
                );
            }
            None => {
                ld.fail(
                    "D axis: declared_dependencies == 0",
                    "cannot read the track's Cargo.toml, so the declaration cannot be checked",
                );
            }
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
        match find_sentinel(&dir.join("src")) {
            Ok(None) => ld.pass("no scaffold sentinel in source", ""),
            Ok(Some(found)) => ld.fail(
                "no scaffold sentinel in source",
                &format!("{found} — a stub must never reach the freeze"),
            ),
            Err(why) => ld.fail(
                "no scaffold sentinel in source",
                &format!("{why} — the check did not run"),
            ),
        }

        // The correctness gate can only be checked once the sample hash is
        // pinned. `is_file()` was not enough: a placeholder file containing
        // "TODO-fill-this-in" certified a gate that `bench` proves is not
        // checkable, so the lockdown said PASS while the gate was unverifiable.
        // This now parses the pin exactly as `main::expected_hash` does, and a
        // missing or unreadable pin FAILS rather than skipping: the lockdown's
        // job is to refuse to certify, and "the correctness gate is not
        // checkable" is not a state a frozen submission may be in.
        match read_pinned_hash(&dir, track.output) {
            Some(h) => ld.pass("sample SHA-256 pinned", &format!("{}...", &h[..12])),
            None => ld.fail(
                "sample SHA-256 green",
                &format!(
                    "no valid hash in tracks/{}/verification/expected/{}.sha256 \
                     (need 64 hex chars) — the correctness gate cannot be verified",
                    track.dir, track.output
                ),
            ),
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

    // ── git state ───────────────────────────────────────────────────────
    // The submission must be a public repository with an OSI licence at the
    // freeze. When git is unavailable these three gates are recorded as
    // FAILURES rather than as a single skip: the old code skipped once and
    // never mentioned the dirty-tree or missing-remote checks at all, so on a
    // host without git a dirty tree with no public remote reported
    // "ALL CHECKS PASSED — safe to freeze". Not being able to check a gate is
    // not evidence that the gate passes.
    if git_output(root, &["rev-parse", "--git-dir"]).is_some() {
        match git_output(root, &["rev-parse", "HEAD"]) {
            Some(head) if !head.is_empty() => {
                let short: String = head.chars().take(12).collect();
                ld.pass(
                    "git repository with a recorded commit",
                    &format!("HEAD={short}"),
                )
            }
            _ => ld.fail("git repository with a recorded commit", "no commit"),
        }

        // The measurement ledger is an append-only log: running the gates
        // legitimately appends to it, so it is not an "uncommitted change".
        // Everything else is.
        let dirty = git_output(root, &["status", "--porcelain"]).unwrap_or_default();
        let real: Vec<&str> = dirty
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter(|l| !l.trim_end().ends_with("verification/ledger.jsonl"))
            .collect();
        if real.is_empty() {
            ld.pass("working tree clean", "");
        } else {
            ld.fail(
                "working tree clean",
                &format!("{} uncommitted change(s)", real.len()),
            );
        }

        // A remote must exist AND be usable as a public push target. Counting
        // `git remote -v` lines accepted any single remote whatever its URL
        // (a `file://` path passed) and rejected a push-only remote that
        // yields one line.
        match git_output(root, &["remote", "-v"]) {
            Some(remotes) if !remotes.trim().is_empty() => {
                let names: Vec<&str> = remotes
                    .lines()
                    .filter_map(|l| l.split_whitespace().next())
                    .collect();
                let urls: Vec<&str> = remotes
                    .lines()
                    .filter_map(|l| l.split_whitespace().nth(1))
                    .collect();
                let public = urls.iter().find(|u| {
                    u.starts_with("https://") || u.starts_with("git@") || u.starts_with("ssh://")
                });
                match public {
                    Some(u) => ld.pass(
                        "public remote configured",
                        &format!("{} -> {u}", names.first().copied().unwrap_or("?")),
                    ),
                    None => ld.fail(
                        "public remote configured",
                        &format!("remotes={:?} — none is a public https/ssh URL", urls),
                    ),
                }
            }
            _ => ld.fail("public remote configured", "no remote configured"),
        }
    } else {
        for name in [
            "git repository with a recorded commit",
            "working tree clean",
            "public remote configured",
        ] {
            ld.fail(name, "git is not available here — this gate did not run");
        }
    }

    ld
}

/// Run a git command in `root` and return its trimmed stdout, if it succeeds.
fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl Lockdown {
    fn pass_track_header(&mut self, _track: &Track) {}
}

/// Find the scaffold sentinel in a source tree, if present.
///
/// Returns `Some(reason)` when the check cannot be answered — a missing or
/// empty `src/` is a FAILURE, not a pass. The previous version returned `None`
/// for both "no sentinel" and "nothing to scan", so a track directory with an
/// empty `src/` was certified sentinel-free.
pub fn find_sentinel(src: &Path) -> Result<Option<String>, String> {
    if !src.is_dir() {
        return Err("no src/ directory to scan".to_string());
    }
    let mut scanned = 0usize;
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                scanned += 1;
                let bytes = match fs::read(&p) {
                    Ok(b) => b,
                    Err(e) => return Err(format!("cannot read {}: {e}", p.display())),
                };
                if bytes
                    .windows(b"SCAFFOLD_NOT_IMPLEMENTED".len())
                    .any(|w| w == b"SCAFFOLD_NOT_IMPLEMENTED")
                {
                    return Ok(Some(
                        p.strip_prefix(src.parent().unwrap_or(src))
                            .unwrap_or(&p)
                            .display()
                            .to_string(),
                    ));
                }
            }
        }
    }
    if scanned == 0 {
        return Err("src/ contains no files to scan".to_string());
    }
    Ok(None)
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
        assert!(matches!(find_sentinel(&dir), Ok(Some(_))));
        fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
        assert!(matches!(find_sentinel(&dir), Ok(None)));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_or_missing_src_is_not_a_pass() {
        // The check must be able to say "I could not look", which is not the
        // same answer as "I looked and found nothing".
        let dir = std::env::temp_dir().join(format!("sd-sentinel-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(find_sentinel(&dir).is_err(), "missing src/ must not pass");
        fs::create_dir_all(&dir).unwrap();
        assert!(find_sentinel(&dir).is_err(), "empty src/ must not pass");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_placeholder_hash_is_not_a_pin() {
        let root = std::env::temp_dir().join(format!("sd-pin-{}", std::process::id()));
        let dir = root.join("A-ingest");
        let expected = dir.join("verification").join("expected");
        fs::create_dir_all(&expected).unwrap();
        let pin = expected.join("ingest.txt.sha256");
        fs::write(&pin, "TODO-fill-this-in\n").unwrap();
        assert_eq!(read_pinned_hash(&dir, "ingest.txt"), None);
        fs::write(
            &pin,
            "  f34f78e97f1a00000000000000000000000000000000000000000000000000ab  ingest.txt\n",
        )
        .unwrap();
        assert!(read_pinned_hash(&dir, "ingest.txt").is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn inline_comments_do_not_leak_into_a_toml_value() {
        let dir = std::env::temp_dir().join(format!("sd-toml2-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let p = dir.join("x.toml");
        fs::write(
            &p,
            "output_file = \"ingest.txt\"   # the scored artefact\ndeclared_dependencies = 0  # none\n",
        )
        .unwrap();
        assert_eq!(
            toml_value(&p, "output_file").as_deref(),
            Some("ingest.txt"),
            "an inline comment must not become part of the value"
        );
        assert_eq!(
            toml_value(&p, "declared_dependencies").as_deref(),
            Some("0")
        );
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
