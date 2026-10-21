//! cgroup v2 memory.peak reader — the M axis.
//!
//! The organizer reads peak memory from the kernel's high-water mark,
//! not from RSS sampling: `memory.peak` catches transient spikes that any
//! sampler would miss. This module reads the same value for our local runs
//! and reports it honestly when it is not available (non-Linux hosts, or a
//! cgroup v1 system) rather than substituting a guess.

use std::fs;
use std::path::{Path, PathBuf};

/// Where the peak lives for the *whole session's* cgroup on a typical
/// systemd + cgroup v2 host. `find_container_peak` looks for per-container
/// scopes first and falls back to this.
const SESSION_PEAK_CANDIDATES: &[&str] = &[
    // Docker via systemd: one scope per container.
    "/sys/fs/cgroup/system.slice/docker.scope/memory.peak",
    // Non-systemd docker (cgroupfs driver).
    "/sys/fs/cgroup/docker/memory.peak",
    // Plain session cgroup (bare-metal run without containers).
    "/sys/fs/cgroup/memory.peak",
];

/// Find a `memory.peak` file whose cgroup was written to recently.
///
/// Strategy: scan one level under `/sys/fs/cgroup/system.slice` for
/// `docker-<id>.scope` directories (the systemd driver's layout) and return
/// the first with a readable `memory.peak`. This is inherently racy with
/// concurrent containers, but scoring runs are serialized, so the most
/// recently modified scope is the one that just finished.
pub fn find_container_peak() -> Option<PeakReading> {
    let base = Path::new("/sys/fs/cgroup/system.slice");
    if let Ok(entries) = fs::read_dir(base) {
        let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("docker-") && name.ends_with(".scope") {
                let peak = e.path().join("memory.peak");
                if peak.is_file() {
                    let mtime = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    candidates.push((peak, mtime));
                }
            }
        }
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((path, _)) = candidates.first() {
            if let Some(p) = read_peak(path) {
                return Some(p);
            }
        }
    }
    for c in SESSION_PEAK_CANDIDATES {
        let p = Path::new(c);
        if p.is_file() {
            if let Some(reading) = read_peak(p) {
                return Some(reading);
            }
        }
    }
    None
}

/// A peak reading with its provenance, so the report never blurs "measured"
/// into "assumed".
#[derive(Debug, Clone)]
pub struct PeakReading {
    pub bytes: u64,
    pub source: String,
}

fn read_peak(path: &Path) -> Option<PeakReading> {
    // memory.peak contains a single integer byte count. (On very old
    // kernels the file may not exist; on cgroup v1 the equivalent is
    // memory.usage_in_bytes / memory.max_usage_in_bytes, which we do not
    // trust and report as unavailable instead.)
    let raw = fs::read_to_string(path).ok()?;
    let bytes = raw.trim().parse::<u64>().ok()?;
    Some(PeakReading {
        bytes,
        source: path.display().to_string(),
    })
}

/// Peak memory in MiB, or `None` with the reason it could not be read.
pub fn peak_mib() -> Result<f64, String> {
    match find_container_peak() {
        Some(p) => Ok(p.bytes as f64 / (1024.0 * 1024.0)),
        None => Err("no cgroup v2 memory.peak found (are we on Linux with cgroup v2?)".to_string()),
    }
}

/// A userspace sampler that watches RSS at high frequency as a *fallback*
/// for machines without cgroup v2. It under-reports by design — it is a
/// lower bound, never a substitute for the kernel counter, and the report
/// labels it as such.
pub fn sample_peak_rss_mib(child_pid: u32, stop: &std::sync::atomic::AtomicBool) -> Option<f64> {
    let statm = format!("/proc/{child_pid}/statm");
    let mut peak = 0.0f64;
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        if let Ok(s) = fs::read_to_string(&statm) {
            // statm: size resident shared text lib data dt (in pages)
            if let Some(resident_pages) = s.split_whitespace().nth(1) {
                if let Ok(pages) = resident_pages.parse::<u64>() {
                    // 4 KiB pages on x86-64 Linux.
                    let mib = (pages * 4096) as f64 / (1024.0 * 1024.0);
                    if mib > peak {
                        peak = mib;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if peak > 0.0 {
        Some(peak)
    } else {
        None
    }
}
