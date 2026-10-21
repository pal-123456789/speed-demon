//! sd-harness — the Speed Demon scoring-box replica.
//!
//! This is the tool that runs ON the scoring box (HEL1, Debian 13, Docker,
//! cgroup v2) and reproduces the organizer's measurement protocol exactly:
//!
//!   * the verbatim `docker run` flags, including `--cpuset-cpus=0-11`,
//!     `--memory=8g --memory-swap=8g` (swap disabled), `--network=none`,
//!     `--read-only` and the 1 GiB tmpfs
//!   * peak memory from the kernel's cgroup `memory.peak` — never from RSS
//!     sampling, and never guessed
//!   * 5 runs, highest and lowest discarded, mean of the middle three
//!   * the four gates: SHA-256 correctness, determinism across runs, the
//!     10x reference timeout, and the sandbox (exactly one output file)
//!
//! Every measurement is appended to a JSONL ledger so the optimisation log
//! in PERF.md can be GENERATED from measurements instead of written from
//! memory.
//!
//! Usage:
//!   sd-harness bench   --track A [--runs 5] [--data DIR] [--out DIR] [--label L]
//!   sd-harness verify  --track A
//!   sd-harness ledger  [--track A] [--json]
//!   sd-harness plan    [--track A]
//!   sd-harness prewarm --file PATH
//!   sd-harness contract

mod cgroup;
mod contract;
mod ledger;
mod lockdown;
mod sha256;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use contract::Track;

const DEFAULT_LEDGER: &str = "verification/ledger.jsonl";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }
    let cmd = args[1].clone();
    let opts = parse_opts(&args[2..]);

    let result = match cmd.as_str() {
        "bench" => cmd_bench(&opts),
        "verify" => cmd_verify(&opts),
        "ledger" => cmd_ledger(&opts),
        "plan" => cmd_plan(&opts),
        "prewarm" => cmd_prewarm(&opts),
        "hash" => cmd_hash(&opts),
        "contract" => cmd_contract(&opts),
        "lockdown" => cmd_lockdown(&opts),
        "--help" | "-h" | "help" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = result {
        eprintln!("\nERROR: {e}");
        std::process::exit(1);
    }
}

fn usage() {
    eprintln!(
        "sd-harness — Speed Demon scoring-box replica

  sd-harness bench   --track A|B|C|D|E [--runs N] [--data DIR] [--out DIR]
                     [--image IMG] [--label NAME] [--no-docker] [--ledger PATH]
  sd-harness verify  --track A|B|C|D|E [...]
  sd-harness ledger  [--track A] [--json]
  sd-harness plan    [--track A]
  sd-harness prewarm --file PATH
  sd-harness contract"
    );
}

// ── argument parsing ───────────────────────────────────────────────────────

struct Opts {
    map: BTreeMap<String, String>,
    flags: Vec<String>,
}

impl Opts {
    fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }
    fn get_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }
    fn has(&self, key: &str) -> bool {
        self.flags.iter().any(|f| f == key)
    }
    fn req_track(&self) -> Result<char, String> {
        let t = self
            .get("track")
            .ok_or_else(|| "--track is required (A|B|C|D|E)".to_string())?;
        let c = t
            .chars()
            .next()
            .ok_or_else(|| format!("bad --track value: {t}"))?
            .to_ascii_uppercase();
        if !contract::tracks().contains_key(&c) {
            return Err(format!("unknown track {c}"));
        }
        Ok(c)
    }
}

fn parse_opts(args: &[String]) -> Opts {
    let mut map = BTreeMap::new();
    let mut flags = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if let Some((k, v)) = name.split_once('=') {
                map.insert(k.to_string(), v.to_string());
                i += 1;
            } else if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                map.insert(name.to_string(), args[i + 1].clone());
                i += 2;
            } else {
                flags.push(name.to_string());
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    Opts { map, flags }
}

// ── shared helpers ─────────────────────────────────────────────────────────

fn repo_root() -> PathBuf {
    // The harness is invoked from the repository root.
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The pinned expected output hash, if the sample has shipped.
fn expected_hash(track: &Track, root: &Path) -> Option<String> {
    let dir = root
        .join("tracks")
        .join(track.dir)
        .join("verification")
        .join("expected");
    for name in [
        format!("{}.sha256", track.output),
        "sample.sha256".to_string(),
    ] {
        let p = dir.join(&name);
        if let Ok(raw) = std::fs::read_to_string(&p) {
            let raw = raw.trim();
            if let Some(hex) = raw.split_whitespace().next() {
                if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(hex.to_ascii_lowercase());
                }
            }
        }
    }
    None
}

/// One container run, measured.
struct RunOutcome {
    wall_s: f64,
    peak_mib: Option<f64>,
    peak_source: String,
    output_sha256: String,
    output_bytes: u64,
    exit_code: i32,
    extra_files: Vec<String>,
}

/// The verbatim scoring command. `--rm` is deliberately NOT used: the peak
/// must be read from the cgroup after the container exits, which means the
/// container has to still exist. We remove it explicitly afterwards.
fn run_container(
    track: &Track,
    image: &str,
    data_dir: &Path,
    out_dir: &Path,
) -> Result<RunOutcome, String> {
    let stamp = now_unix();
    let name = format!("sd-harness-{}-{}", track.id, stamp);
    let cidfile = std::env::temp_dir().join(format!("{}.cid", name));

    // Clear the output directory: the sandbox gate requires exactly one
    // file in /out, and a stale file from a previous run would fake it.
    let _ = std::fs::create_dir_all(out_dir);
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }

    let data_arg = format!("{}:/data:ro", data_dir.display());
    let out_arg = format!("{}:/out:rw", out_dir.display());
    let cid_arg = format!("{}", cidfile.display());
    let track_env = format!("TRACK={}", track.id);

    let args = vec![
        "run",
        "--name",
        &name,
        "--cidfile",
        &cid_arg,
        "--cpuset-cpus=0-11",
        "--cpuset-mems=0",
        "--memory=8g",
        "--memory-swap=8g",
        "--pids-limit=512",
        "--network=none",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,size=1g,exec",
        "--ulimit",
        "nofile=8192:8192",
        "-v",
        &data_arg,
        "-v",
        &out_arg,
        "-e",
        &track_env,
        image,
    ];

    let start = Instant::now();
    let out = Command::new("docker")
        .args(&args)
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;
    let wall = start.elapsed().as_secs_f64();
    let exit_code = out.status.code().unwrap_or(-1);

    // Read the peak from the cgroup while the container still exists.
    let mut peak_mib = None;
    let mut peak_source = String::new();
    if let Ok(cid) = std::fs::read_to_string(&cidfile) {
        let cid = cid.trim();
        for path in [
            format!(
                "/sys/fs/cgroup/system.slice/docker-{}.scope/memory.peak",
                cid
            ),
            format!("/sys/fs/cgroup/docker/{}/memory.peak", cid),
        ] {
            let p = Path::new(&path);
            if p.is_file() {
                if let Ok(raw) = std::fs::read_to_string(p) {
                    if let Ok(bytes) = raw.trim().parse::<u64>() {
                        peak_mib = Some(bytes as f64 / (1024.0 * 1024.0));
                        peak_source = format!("cgroup:{path}");
                        break;
                    }
                }
            }
        }
    }
    if peak_mib.is_none() {
        if let Some(p) = cgroup::find_container_peak() {
            peak_mib = Some(p.bytes as f64 / (1024.0 * 1024.0));
            peak_source = format!("cgroup:{}", p.source);
        }
    }
    if peak_mib.is_none() {
        // Last resort: the session cgroup. Still the kernel counter, never a
        // sampled estimate.
        if let Ok(v) = cgroup::peak_mib() {
            peak_mib = Some(v);
            peak_source = "cgroup:session".to_string();
        }
    }

    // Clean up: remove the container and the cidfile.
    let _ = Command::new("docker").args(["rm", "-f", &name]).output();
    let _ = std::fs::remove_file(&cidfile);

    if exit_code != 0 {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "container exited with code {exit_code}\n--- stderr ---\n{stderr}",
        ));
    }

    // Inspect the output directory: the sandbox gate is "exactly one file".
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for e in entries.flatten() {
            if e.path().is_file() {
                files.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    files.sort();
    let extra_files = if files.len() == 1 {
        Vec::new()
    } else {
        files.clone()
    };

    let out_path = out_dir.join(track.output);
    let (output_sha256, output_bytes) = match sha256::sha256_file(&out_path) {
        Ok(h) => {
            let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            (h, bytes)
        }
        Err(e) => return Err(format!("cannot hash {}: {e}", out_path.display())),
    };

    Ok(RunOutcome {
        wall_s: wall,
        peak_mib,
        peak_source,
        output_sha256,
        output_bytes,
        exit_code,
        extra_files,
    })
}

/// Run the binary directly, without a container. For local iteration on a
/// machine without Docker. The sandbox gate is reported as SKIPPED because
/// it cannot be evaluated outside the container.
fn run_direct(
    track: &Track,
    bin: &Path,
    data_dir: &Path,
    out_dir: &Path,
) -> Result<RunOutcome, String> {
    let _ = std::fs::create_dir_all(out_dir);
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    // Outside a container there is no cgroup to read, so the peak is
    // sampled from /proc while the child runs. It is a LOWER BOUND and is
    // labelled as such — a sampled number must never be presented as the
    // kernel's counter.
    let start = Instant::now();
    let child = Command::new(bin)
        .env("SD_DATA_DIR", data_dir)
        .env("SD_OUT_DIR", out_dir)
        .env("TRACK", track.id.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {}: {e}", bin.display()))?;

    let pid = child.id();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = {
        let stop = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || cgroup::sample_peak_rss_mib(pid, &stop))
    };
    let child_out = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for {}: {e}", bin.display()))?;
    let wall = start.elapsed().as_secs_f64();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let sampled_peak = sampler.join().ok().flatten();
    let exit_code = child_out.status.code().unwrap_or(-1);
    let child = child_out;
    if exit_code != 0 {
        return Err(format!(
            "binary exited with code {exit_code}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&child.stderr)
        ));
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for e in entries.flatten() {
            if e.path().is_file() {
                files.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    files.sort();
    let extra_files = if files.len() == 1 {
        Vec::new()
    } else {
        files.clone()
    };
    let out_path = out_dir.join(track.output);
    let (output_sha256, output_bytes) = match sha256::sha256_file(&out_path) {
        Ok(h) => {
            let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            (h, bytes)
        }
        Err(e) => return Err(format!("cannot hash {}: {e}", out_path.display())),
    };
    Ok(RunOutcome {
        wall_s: wall,
        peak_mib: sampled_peak,
        peak_source: "rss-sampled (LOWER BOUND — not the cgroup counter)".to_string(),
        output_sha256,
        output_bytes,
        exit_code,
        extra_files,
    })
}

// ── commands ───────────────────────────────────────────────────────────────

fn cmd_bench(opts: &Opts) -> Result<(), String> {
    let id = opts.req_track()?;
    let all = contract::tracks();
    let track = all.get(&id).unwrap();
    let root = repo_root();
    let runs: u32 = opts.get_or("runs", "5").parse().unwrap_or(5);
    let data_dir = PathBuf::from(opts.get_or(
        "data",
        &format!("{}/verification/data/track-{}", root.display(), id),
    ));
    let out_dir = PathBuf::from(opts.get_or(
        "out",
        &format!("{}/verification/out/track-{}", root.display(), id),
    ));
    let image = opts.get_or("image", "speeddemon/solo:bench");
    let label = opts.get_or("label", "bench");
    let ledger_path = PathBuf::from(opts.get_or("ledger", DEFAULT_LEDGER));
    let no_docker = opts.has("no-docker");
    let threads: u32 = opts.get_or("threads", "12").parse().unwrap_or(12);

    println!("=== BENCH track {id} ({}) ===", track.dir);
    println!("  data    : {}", data_dir.display());
    println!("  out     : {}", out_dir.display());
    println!(
        "  runs    : {runs} (discard high+low, mean of middle {})",
        runs.saturating_sub(2)
    );
    println!("  label   : {label}");
    if !no_docker {
        println!("  image   : {image}");
    }

    let expected = expected_hash(track, &root);
    match &expected {
        Some(h) => println!("  expected: {h}"),
        None => println!("  expected: (none pinned yet — the SHA gate is UNVERIFIABLE)"),
    }

    let mut walls = Vec::new();
    let mut hashes = Vec::new();
    let mut peaks = Vec::new();
    let mut all_ok = true;

    for i in 1..=runs {
        let outcome = if no_docker {
            let bin = PathBuf::from(opts.get_or(
                "bin",
                &format!(
                    "{}/target/release/speeddemon-{}",
                    root.display(),
                    id.to_ascii_lowercase()
                ),
            ));
            run_direct(track, &bin, &data_dir, &out_dir)?
        } else {
            run_container(track, &image, &data_dir, &out_dir)?
        };

        let rec = ledger::RunRecord {
            track: id,
            commit: git_commit(),
            timestamp: now_unix(),
            run: i,
            wall_s: outcome.wall_s,
            peak_mib: outcome.peak_mib,
            peak_source: outcome.peak_source.clone(),
            output_sha256: outcome.output_sha256.clone(),
            expected_sha256: expected.clone(),
            output_bytes: outcome.output_bytes,
            exit_code: outcome.exit_code,
            label: label.clone(),
            threads,
        };
        let _ = ledger::append(&ledger_path, &rec);

        let used = if i == 1 || i == runs {
            " (discarded)"
        } else {
            " <- used"
        };
        println!(
            "  run {i}: {:8.3}s  peak {:>8}  sha {}{}",
            outcome.wall_s,
            outcome
                .peak_mib
                .map(|p| format!("{p:.1} MiB"))
                .unwrap_or_else(|| "n/a".to_string()),
            &outcome.output_sha256[..12],
            used
        );

        if !outcome.extra_files.is_empty() {
            eprintln!(
                "  SANDBOX FAIL: expected exactly 1 file in the output dir, found {:?}",
                outcome.extra_files
            );
            all_ok = false;
        }

        walls.push(outcome.wall_s);
        hashes.push(outcome.output_sha256);
        if let Some(p) = outcome.peak_mib {
            peaks.push(p);
        }
    }

    // Trimmed mean: discard the highest and the lowest.
    let mut sorted = walls.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let used: Vec<f64> = if sorted.len() >= 3 {
        sorted[1..sorted.len() - 1].to_vec()
    } else {
        sorted.clone()
    };
    let t = used.iter().sum::<f64>() / used.len() as f64;
    let spread = (sorted[sorted.len() - 1] - sorted[0]) / t * 100.0;

    println!("\n  T (trimmed mean) : {t:.4} s");
    println!(
        "  fastest / slowest: {:.3} s / {:.3} s   spread {spread:.1}%",
        sorted[0],
        sorted[sorted.len() - 1]
    );
    println!(
        "  ratio vs reference: {:.3}x   (reference {:.2} s on HEL1)",
        t / track.ref_time_s,
        track.ref_time_s
    );
    if spread > 5.0 {
        eprintln!("  WARNING: spread > 5% — the organizer re-runs batches like this");
    }
    if let Some(max_peak) = peaks
        .iter()
        .cloned()
        .fold(None::<f64>, |a, b| Some(a.map_or(b, |x| x.max(b))))
    {
        println!("  peak memory      : {max_peak:.1} MiB (max of the runs)");
        if let Some(ref_mb) = track.ref_mb() {
            println!("  M ratio          : {:.2}x", ref_mb / max_peak);
        } else {
            println!(
                "  M ratio          : DEFERRED (the reference peak memory is not published yet)"
            );
        }
    } else {
        println!("  peak memory      : n/a (run inside a container to read cgroup memory.peak)");
    }

    // Compare against the previous measurement of the same label, so a
    // regression is visible in the terminal and not only in the ledger.
    if let Some(prev) = lockdown::last_measurement(&root, id) {
        if prev.label == label && prev.wall_s > 0.0 {
            let delta = (t - prev.wall_s) / prev.wall_s * 100.0;
            let arrow = if delta < 0.0 { "faster" } else { "SLOWER" };
            println!(
                "  vs. previous '{}': {:.4}s -> {:.4}s  ({:+.1}% {})",
                label, prev.wall_s, t, delta, arrow
            );
        }
    }

    // G1 correctness + determinism
    let uniq: BTreeMap<&String, usize> = hashes.iter().fold(BTreeMap::new(), |mut m, h| {
        *m.entry(h).or_insert(0) += 1;
        m
    });
    if uniq.len() == 1 {
        println!(
            "  determinism      : byte-identical across {} runs",
            hashes.len()
        );
    } else {
        eprintln!(
            "  CORRECTNESS FAIL: {} distinct output hashes across runs",
            uniq.len()
        );
        all_ok = false;
    }
    match &expected {
        Some(exp) => {
            let matched = recs_passed(&hashes, exp);
            if matched == hashes.len() {
                println!(
                    "  SHA-256 gate     : PASS ({matched}/{} runs matched)",
                    hashes.len()
                );
            } else {
                eprintln!(
                    "  SHA-256 gate     : FAIL ({matched}/{} runs matched the expected hash)",
                    hashes.len()
                );
                all_ok = false;
            }
        }
        None => println!("  SHA-256 gate     : UNVERIFIABLE (no expected hash pinned)"),
    }

    // G3 timeout
    if sorted[sorted.len() - 1] < track.timeout_s {
        println!(
            "  timeout gate     : PASS (worst {:.2}s < {:.0}s)",
            sorted[sorted.len() - 1],
            track.timeout_s
        );
    } else {
        eprintln!("  TIMEOUT FAIL: worst run exceeds {:.0}s", track.timeout_s);
        all_ok = false;
    }

    println!("\n  ledger: {}", ledger_path.display());
    if all_ok {
        println!("  RESULT: PASS");
        Ok(())
    } else {
        println!("  RESULT: FAIL");
        Err("one or more gates failed".to_string())
    }
}

fn cmd_verify(opts: &Opts) -> Result<(), String> {
    // Verification is a single-run bench plus the gate report; the full
    // 5-run protocol lives in `bench`.
    let mut o = Opts {
        map: opts.map.clone(),
        flags: opts.flags.clone(),
    };
    o.map.insert("runs".into(), "1".into());
    cmd_bench(&o)
}

fn cmd_ledger(opts: &Opts) -> Result<(), String> {
    let root = repo_root();
    let path = PathBuf::from(opts.get_or("ledger", DEFAULT_LEDGER));
    let _ = root;
    let recs =
        ledger::read_all(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let filter = opts.get("track").map(|t| t.to_ascii_uppercase());

    if opts.has("json") {
        for r in &recs {
            println!(
                "{{\"track\":\"{}\",\"label\":\"{}\",\"wall_s\":{:.4},\"peak_mib\":{},\"sha\":\"{}\",\"commit\":\"{}\"}}",
                r.track,
                r.label,
                r.wall_s,
                r.peak_mib.map(|p| format!("{p:.2}")).unwrap_or_else(|| "null".into()),
                r.output_sha256,
                r.commit
            );
        }
        return Ok(());
    }

    // Group by (track, label) and report the trimmed mean.
    let mut groups: BTreeMap<(char, String), Vec<f64>> = BTreeMap::new();
    for r in &recs {
        if let Some(f) = &filter {
            if r.track != f.chars().next().unwrap_or('?') {
                continue;
            }
        }
        groups
            .entry((r.track, r.label.clone()))
            .or_default()
            .push(r.wall_s);
    }
    if groups.is_empty() {
        println!("(ledger is empty: {})", path.display());
        return Ok(());
    }
    println!(
        "{:<6} {:<24} {:>10} {:>10}",
        "TRACK", "LABEL", "T (s)", "RUNS"
    );
    for ((track, label), mut walls) in groups {
        walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let used: Vec<f64> = if walls.len() >= 3 {
            walls[1..walls.len() - 1].to_vec()
        } else {
            walls.clone()
        };
        let t = used.iter().sum::<f64>() / used.len() as f64;
        println!("{track:<6} {label:<24} {t:>10.4} {:>10}", walls.len());
    }
    Ok(())
}

fn cmd_plan(opts: &Opts) -> Result<(), String> {
    let all = contract::tracks();
    let selected: Vec<&Track> = match opts.get("track") {
        Some(t) => {
            let c = t.chars().next().unwrap().to_ascii_uppercase();
            vec![all.get(&c).unwrap()]
        }
        None => all.values().collect(),
    };
    println!("K BUDGET  (K = (Tref/T)^0.40 x (Mref/M)^0.20 x (Sref/S)^0.15 x (Dref/D)^0.15 x (Xref/X)^0.10)");
    println!("Every ratio is capped at 50x. speed = 100*ln(K)/ln(25).\n");
    for track in selected {
        println!("--- Track {} ({}) ---", track.id, track.dir);
        println!(
            "  reference: {:.2}s  timeout {:.0}s  M axis: {}",
            track.ref_time_s,
            track.timeout_s,
            if track.m_is_compressed_size {
                "compressed size"
            } else {
                "peak memory"
            }
        );
        let rows: &[(f64, f64, f64, f64)] = &[
            // (T ratio, M ratio, S ratio, X ratio)
            (20.0, 10.0, 0.60, 1.00),
            (30.0, 15.0, 0.62, 1.05),
            (45.0, 22.0, 0.65, 1.10),
        ];
        for (t, m, s, x) in rows {
            let (k, c) = contract::coefficient(*t, *m, *s, 1.0, *x);
            println!(
                "  T {t:>5.1}x  M {m:>5.1}x  S {s:>5.2}x  X {x:>5.2}x  ->  \
                 K {k:>6.3}   speed {:>5.1}   (T {:.3} M {:.3} S {:.3} X {:.3})",
                contract::speed_score(k),
                c[0],
                c[1],
                c[2],
                c[4]
            );
        }
        println!();
    }
    Ok(())
}

fn cmd_prewarm(opts: &Opts) -> Result<(), String> {
    let file = opts
        .get("file")
        .ok_or_else(|| "--file is required".to_string())?;
    let p = Path::new(file);
    if !p.is_file() {
        return Err(format!("not a file: {file}"));
    }
    // The organizer uses `vmtouch -t` to pull the dataset into the page
    // cache before each batch, so the measurement is of our code and not of
    // the NVMe. Reading the file achieves the same thing.
    let start = Instant::now();
    let mut f = std::fs::File::open(p).map_err(|e| format!("{e}"))?;
    use std::io::Read;
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    println!(
        "pre-warmed {total} bytes from {file} in {:.3}s (page cache is now warm)",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

/// How many of the run hashes matched the expected value.
fn recs_passed(hashes: &[String], expected: &str) -> usize {
    hashes
        .iter()
        .filter(|h| h.eq_ignore_ascii_case(expected))
        .count()
}

/// Hash a file, so an expected hash can be pinned into
/// `tracks/<track>/verification/expected/<output>.sha256`.
///
/// Small files are hashed in one shot; large ones are streamed, so a 312 MB
/// input never has to fit in memory.
fn cmd_hash(opts: &Opts) -> Result<(), String> {
    let file = opts
        .get("file")
        .ok_or_else(|| "--file is required".to_string())?;
    let p = Path::new(file);
    if !p.is_file() {
        return Err(format!("not a file: {file}"));
    }
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let digest = if size <= 64 * 1024 * 1024 {
        let data = std::fs::read(p).map_err(|e| format!("{e}"))?;
        sha256::hex(&sha256::sha256(&data))
    } else {
        sha256::sha256_file(p).map_err(|e| format!("{e}"))?
    };
    println!("{digest}  {file}");
    Ok(())
}

/// The pre-freeze lockdown, in Rust. Same checklist as the PowerShell
/// version, but it runs on the scoring box, where there is no PowerShell.
fn cmd_lockdown(opts: &Opts) -> Result<(), String> {
    let all = contract::tracks();
    let selected: Vec<char> = match opts.get("track") {
        Some(t) => vec![t.chars().next().unwrap().to_ascii_uppercase()],
        None => all.keys().copied().collect(),
    };
    let root = repo_root();
    let ld = lockdown::run(&root, &all, &selected);

    println!("{}", "=".repeat(72));
    println!("PRE-FREEZE LOCKDOWN");
    println!("{}", "=".repeat(72));
    println!("  This is the last gate before the code freeze.");
    println!("  A FAIL here means: do NOT freeze.\n");

    for c in &ld.checks {
        let colour = c.status.tag();
        if c.detail.is_empty() {
            println!("  [{colour}] {}", c.name);
        } else {
            println!("  [{colour}] {}  {}", c.name, c.detail);
        }
    }

    let failures = ld.failures();
    println!("\n{}", "=".repeat(72));
    if failures == 0 {
        println!("LOCKDOWN: ALL CHECKS PASSED — safe to freeze.");
        Ok(())
    } else {
        println!("LOCKDOWN: {failures} CHECK(S) FAILED — do NOT freeze.");
        Err("lockdown failed".to_string())
    }
}

fn cmd_contract(_opts: &Opts) -> Result<(), String> {
    let all = contract::tracks();
    println!("FROZEN HEL1 CONTRACT");
    println!(
        "weights: T {:.2}  M {:.2}  S {:.2}  D {:.2}  X {:.2}  (sum {:.2})",
        contract::W_T,
        contract::W_M,
        contract::W_S,
        contract::W_D,
        contract::W_X,
        contract::W_T + contract::W_M + contract::W_S + contract::W_D + contract::W_X
    );
    println!("axis cap: {}x", contract::AXIS_CAP);
    println!();
    println!(
        "{:<3} {:<10} {:>9} {:>9} {:>9} {:>9} {:>9} {:<18}",
        "ID", "DIR", "REF(s)", "TIMEOUT", "REF(MB)", "REF(B)", "M=size", "INPUT"
    );
    for t in all.values() {
        println!(
            "{:<3} {:<10} {:>9.2} {:>9.0} {:>9} {:>9} {:>9} {:<18}",
            t.id,
            t.dir,
            t.ref_time_s,
            t.timeout_s,
            t.ref_mem_mb
                .map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| "DEFERRED".into()),
            t.ref_src_bytes
                .map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| "DEFERRED".into()),
            if t.m_is_compressed_size { "yes" } else { "no" },
            t.input
        );
    }
    Ok(())
}
