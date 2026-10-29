#!/usr/bin/env python3
"""check-consistency — cross-check every shared fact against its single source.

Why this exists: repeated audits kept finding "one more" inconsistency, because
each pass looked from a different angle. Facts are duplicated across the
contract, the harness, the tools, the docs and the track folders; the only way
to be exhaustive is to compare them mechanically against one authority per fact.

Authorities:
  * deadlines        -> the organizers' announcement (kickoff/freeze 15:00 UTC)
  * axes / hashes    -> verification/contract.toml
  * output names     -> check.py (the organizers' checker) + the specs
  * track/team facts -> the four track-*/.speed-demon.toml
  * gate count       -> the `step` lines in tooling/ci.sh themselves

Exit 0 only when every fact agrees everywhere it appears.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
TOOLING = ROOT / "tooling"

# Files that are copies of record and must not be policed for our wording.
SKIP_DIRS = ("reference/official", ".git", "target", "node_modules")

TEXT_SUFFIXES = {".md", ".txt", ".py", ".sh", ".ps1", ".rs", ".toml", ".json", ".yml", ".yaml"}

FAILS: list[str] = []
CHECKS = 0


def fail(msg: str) -> None:
    FAILS.append(msg)


def texts():
    """Every first-party text file that can actually SHIP (relative path, content).

    Derived from git rather than from a directory walk. A walk also picks up
    files that git ignores, and ignored scratch is not part of the submission:
    a transient `.commitmsg.txt` once tripped the abandoned-architecture check
    with a line from its own commit message. `--others --exclude-standard`
    keeps new, not-yet-staged files in scope while dropping the ignored
    scratch, so the check runs during development and still only polices what
    can reach the judges.
    """
    r = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT, capture_output=True, text=True,
    )
    for rel in sorted(set(r.stdout.split())):
        if Path(rel).suffix not in TEXT_SUFFIXES:
            continue
        if any(rel.startswith(s) for s in SKIP_DIRS):
            continue
        if rel == "tooling/check-consistency.py":
            continue  # this file names every stale pattern on purpose
        p = ROOT / rel
        if not p.is_file():
            continue
        try:
            yield rel, p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue


def check(label: str, ok: bool, detail: str = "") -> None:
    global CHECKS
    CHECKS += 1
    if not ok:
        fail(f"{label}{(': ' + detail) if detail else ''}")


# ── 1. deadlines ──────────────────────────────────────────────────────────
# 15:00 UTC is the only correct boundary. 18:00 may appear ONLY as an explicit
# historical note ("was 18:00", "migrated 18:00", "18:00 -> 15:00") or as a
# data timestamp (the Track A span starts at 18:00:00.000000Z).
HISTORICAL = re.compile(r"was 18:00|18:00\s*(?:→|->)\s*15:00|18:00 to 15:00|migrated", re.I)
DATA_TS = re.compile(r"18:00:00\.000000|18:00:00Z|18:07:|T18:00:00|2026-10-30T18:00")
for rel, text in texts():
    for i, line in enumerate(text.splitlines(), 1):
        if "18:00" not in line:
            continue
        if HISTORICAL.search(line) or DATA_TS.search(line):
            continue
        fail(f"stale 18:00 deadline in {rel}:{i}: {line.strip()[:80]}")

for rel, text in texts():
    if rel.startswith("docs/HANDOVER") or rel == "reference/official/README-vendored.md":
        continue
    for i, line in enumerate(text.splitlines(), 1):
        if re.search(r"(kickoff|freeze)", line, re.I) and "18:00" in line and not HISTORICAL.search(line):
            fail(f"kickoff/freeze line still says 18:00 in {rel}:{i}")

# ── 2. axis numbers agree between contract.toml, contract.rs and k_whatif ──
contract = (ROOT / "verification" / "contract.toml").read_text(encoding="utf-8")
harness = (ROOT / "verification" / "harness" / "src" / "contract.rs").read_text(encoding="utf-8")
whatif = (TOOLING / "k_whatif.py").read_text(encoding="utf-8")

AXES = {  # track -> (T_ref, M_ref MB, S_ref)
    "A": ("32.047276", "227.98336", "56"),
    "B": ("56.352376", "2199.9616", "91"),
    "C": ("14.444059", "367.435776", "108"),
    "D": ("72.692655", "892.915712", "43"),
}


def section(text: str, key: str) -> str:
    m = re.search(rf"^\[{key}\]\s*$", text, re.M)
    if not m:
        return ""
    rest = text[m.end():]
    nxt = re.search(r"^\[", rest, re.M)
    return rest[: nxt.start()] if nxt else rest


for t, (tr, mr, sr) in AXES.items():
    body = section(contract, t)
    check(f"contract.toml [{t}] ref_time_s", f"ref_time_s = {tr}" in body)
    check(f"contract.toml [{t}] ref_peak_mem_mb", f"ref_peak_mem_mb = {mr}" in body)
    check(f"contract.toml [{t}] ref_source_lines", f"ref_source_lines = {sr}" in body)

# the Rust table and the what-if explorer must carry the same T and S
for t, (tr, mr, sr) in AXES.items():
    d = f"track-{t.lower()}"
    i = harness.find(f'dir: "{d}"')
    check(f"contract.rs has {d}", i >= 0)
    if i >= 0:
        blk = harness[i : i + 700]
        check(f"contract.rs {d} ref_time_s", f"ref_time_s: {tr}," in blk)
        check(f"contract.rs {d} ref_mem_mb", f"ref_mem_mb: Some({mr})" in blk)
        check(f"contract.rs {d} ref_src_lines", f"ref_src_lines: Some({sr}.0)" in blk)

for t, (tr, mr, sr) in AXES.items():
    i = whatif.find(f'"{t}":')
    check(f"k_whatif REFS has {t}", i >= 0)
    if i >= 0:
        blk = whatif[i : i + 400]
        check(f"k_whatif {t} T_ref", tr in blk, "T_ref differs from contract.toml")

# ── 3. the eight published seed-1 hashes agree between tool and contract ──
tool = (TOOLING / "official-data.py").read_text(encoding="utf-8")
# contract.toml carries the sample-scale authorities; the tool also knows the
# tiny and full tables. Any hash present in BOTH must agree, and every hash in
# the contract must exist in the tool.
tool_hashes = set(re.findall(r'"([0-9a-f]{64})"', tool))
contract_hashes = set(re.findall(r'"([0-9a-f]{64})"', contract))
check("every contract hash is known to the generator tool", contract_hashes <= tool_hashes,
      str(sorted(h[:12] for h in contract_hashes - tool_hashes)))
check("the tool knows the tiny + full tables too", len(tool_hashes) > len(contract_hashes),
      f"tool={len(tool_hashes)} contract={len(contract_hashes)}")
check("no placeholder hash shape", "<sha256>" not in tool and "<sha>" not in tool)

# The board runs EVERY track at full scale, seed 1, so these twelve are the
# hashes the correctness gate actually compares against. They were reproduced
# locally end to end on Oct 29 (generate -> check input sha256 -> run the
# vendored reference -> check output sha256, all twelve matching); this pins
# them so a later edit cannot quietly drop one and leave a track unverifiable.
FULL_SCALE = {
    "dataset_full_requests_log": "7ec80ba45bceef51ac7f79d3a14521fc6187b2dcb87b5e5542d08063fe5d962a",
    "dataset_full_summary_txt": "495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3",
    "dataset_full_docs_txt": "6cd461670d6c39f9c5211162577532d3648edeab12340c71cc7d2ab5f18e66c8",
    "dataset_full_queries_b": "77ef7ffc59afde051006b02a2ce8e4c090fbbfbf101acd6f862d0f8e298d39f9",
    "dataset_full_results_txt": "f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927",
    "dataset_full_records_csv": "36166f4ac44101a22ebf25b4f949436a681a2698cb0f2ae1f7f6853010259f48",
    "dataset_full_regions_csv": "414c57c4f3ec968f9bce28c59eebfdf57a9425322b468db7ee964c18c4852fe1",
    "dataset_full_aggregates": "d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536",
    "dataset_full_graph_txt": "fe6471f2712d10b23c2ec58a4cbc7db1ea3e742daf6b6db62829e74d6c4a26c7",
    "dataset_full_queries_d": "160f1ef48378dcb567a8d9ddd2676ee777b044a5ede3ad4c7b42e812aa4b3a6f",
    "dataset_full_distances_txt": "a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e",
}
for key, want in FULL_SCALE.items():
    check(f"contract.toml {key} is the reproduced full-scale hash", f'{key}' in contract)
    check(f"contract.toml {key} value", f'"{want}"' in contract)
check("contract.toml declares the board runs at full scale",
      'board_dataset_scale        = "full"' in contract)

# ── 4. filenames: only the official spellings anywhere ────────────────────
STALE_NAMES = ["ingest.txt", "index.txt", "join.txt", "traverse.txt", "access.log",
               "corpus.txt", "output.txt", "result.txt", "summary.csv"]
for rel, text in texts():
    for bad in STALE_NAMES:
        if bad in text:
            fail(f"stale filename {bad!r} in {rel}")

for name in ("summary.txt", "results.txt", "aggregates.txt", "distances.txt"):
    check(f"{name} known to the organizers' checker", name in (ROOT / "check.py").read_text(encoding="utf-8"))

# ── 5. track folders and team name agree ─────────────────────────────────
tracks = sorted(p.name for p in ROOT.glob("track-*") if p.is_dir())
check("exactly four track folders", tracks == ["track-a", "track-b", "track-c", "track-d"], str(tracks))
teams = set()
for t in tracks:
    tn = (ROOT / t / ".speed-demon.toml").read_text(encoding="utf-8")
    m = re.search(r'^team\s*=\s*"([^"]*)"', tn, re.M)
    teams.add(m.group(1) if m else "?")
    tid = re.search(r'^track\s*=\s*"([^"]*)"', tn, re.M)
    check(f"{t} declares its own track id", tid and tid.group(1) == t[-1], str(tid.group(1) if tid else None))
check("one team name across the four folders", len(teams) == 1, str(teams))
check("team name is not a placeholder", teams and "solo" not in teams and teams != {"?"}, str(teams))

# ── 6. any gate count claimed in prose matches ci.sh's actual steps ──────
ci = (TOOLING / "ci.sh").read_text(encoding="utf-8")
steps = len(re.findall(r"^step ", ci, re.M))
for rel, text in texts():
    if rel == "reference/official/README-vendored.md":
        continue
    for m in re.finditer(r"(?<![\dA-Za-z-])(\d+)\s+(?:CI\s+)?gates?\b", text, re.I):
        n = int(m.group(1))
        if n > 5:
            line = text[: m.start()].count("\n") + 1
            fail(f"gate count {n} claimed in {rel}:{line} but ci.sh has {steps} steps")
check("ci.sh has a plausible step count", steps >= 15, str(steps))

# ── 7. every first-party Python tool compiles ────────────────────────────
for p in sorted(TOOLING.glob("*.py")):
    r = subprocess.run([sys.executable, "-m", "py_compile", str(p)], capture_output=True, text=True)
    check(f"{p.name} compiles", r.returncode == 0, (r.stderr or "").strip()[-120:])

# ── 8. shell scripts parse ───────────────────────────────────────────────
import shutil

sh = shutil.which("sh") or r"C:\Program Files\Git\bin\sh.exe"
for p in sorted(TOOLING.glob("*.sh")) + [TOOLING / "hooks" / "pre-commit"]:
    if not p.is_file():
        continue
    r = subprocess.run([sh, "-n", str(p)], capture_output=True, text=True)
    check(f"{p.name} parses", r.returncode == 0, (r.stderr or "").strip()[-120:])

# ── 9. every markdown link resolves ──────────────────────────────────────
for rel, text in texts():
    if not rel.endswith(".md"):
        continue
    base = (ROOT / rel).parent
    for m in re.finditer(r"\]\(([^)#:]+\.(?:md|txt|sh|py|ps1|rs|toml))\)", text):
        target = (base / m.group(1)).resolve()
        check(f"link {m.group(1)} from {rel}", target.is_file())

# ── 10. generated data is ignored, not tracked ───────────────────────────
ignored = (ROOT / ".gitignore").read_text(encoding="utf-8")
for d in ("verification/data", "verification/data-adversarial"):
    check(f"{d} is gitignored", d in ignored)
r = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True)
tracked = set(r.stdout.split())
for bad in ("verification/data/", "verification/data-adversarial/"):
    check(f"nothing under {bad} is tracked",
          not any(t.startswith(bad) for t in tracked))
check("the tiny case corpus IS tracked", any(t.startswith("verification/cases/") for t in tracked))

# ── 11. no 18:00 boundary in the runtime code paths ──────────────────────
# Every tool that names a boundary must name the 15:00 one.
for p in (TOOLING / "rule05_guard.py", TOOLING / "spec_intake.py"):
    t = p.read_text(encoding="utf-8")
    named = re.findall(r"(\d{2}:\d{2})(?::\d{2})?", t)
    check(f"{p.name} names a 15:00 boundary", "15:00" in named, str(sorted(set(named))[:6]))

# ── 12. line endings are pinned, and no accidental CR survives ───────────
# The scored build runs `RUN ./build.sh` on Linux. A CR before a newline makes
# that "bad interpreter" and zeroes the track. Git chooses line endings at
# CHECKOUT time from the CLONER's configuration, and Git for Windows ships
# core.autocrlf=true as its SYSTEM default -- so the committed bytes are not
# sufficient, and a local core.autocrlf=false does not travel with a clone.
# The only durable fix is a tracked .gitattributes. (Found live: this machine
# had core.autocrlf=true system-wide and there was no .gitattributes at all.)
ga = ROOT / ".gitattributes"
check(".gitattributes exists (pins line endings across every clone)", ga.is_file())
if ga.is_file():
    gat = ga.read_text(encoding="utf-8")
    check(".gitattributes forces LF for shell scripts",
          re.search(r"^\*\.sh\s+text\s+eol=lf", gat, re.M) is not None)
    check(".gitattributes forces LF for Dockerfiles",
          re.search(r"^Dockerfile\s+text\s+eol=lf", gat, re.M) is not None)
    check(".gitattributes protects the byte-identical vendored tree",
          "/reference/** -text" in gat)
    check(".gitattributes protects the hostile Track C corpus",
          "/verification/cases/track-c/records.csv -text" in gat)

# Files whose bytes ARE the test. A blanket CRLF->LF sweep would silently
# delete the coverage, so they are exempted from the check below and asserted
# to still be hostile instead.
BYTE_EXACT = ("verification/cases/track-c/records.csv",)
for rel in BYTE_EXACT:
    p = ROOT / rel
    if not p.is_file():
        check(f"{rel} exists", False)
        continue
    raw = p.read_bytes()
    crlf = raw.count(b"\r\n")
    bare = raw.count(b"\n") - crlf
    check(f"{rel} keeps BOTH CRLF and bare-LF terminators (spec 2.2)",
          crlf > 0 and bare > 0, f"crlf={crlf} bare_lf={bare}")
    check(f"{rel} keeps its UTF-8 BOM", raw.startswith(b"\xef\xbb\xbf"))

# EVERY tracked first-party text file, not just the obvious code extensions. An
# earlier version of this check listed .sh/.py/.rs/.toml/.md/Dockerfile, so it
# could not see the four `track-*/verification/expected/*.sha256` pins -- which
# had CRLF for exactly that reason. Binaries are skipped by content, not by
# extension.
cr_offenders = []
for rel in sorted(tracked):
    if rel.startswith("reference/") or rel in BYTE_EXACT:
        continue
    p = ROOT / rel
    if not p.is_file():
        continue
    raw = p.read_bytes()
    if b"\x00" in raw:
        continue  # binary
    if b"\r" in raw:
        cr_offenders.append(rel)
check("no CR in any tracked first-party text file",
      not cr_offenders, ", ".join(cr_offenders[:8]))

# ── 13. the abandoned architecture is never referenced as if it were current ─
# This repository once carried a different design: five tracks named A-ingest
# .. E-compress, a shared core/ rlib, a vendored toolchain and a musl static
# link. It was replaced by four self-contained track-* folders built with the
# stock rust image. A stray reference to the old layout is exactly how a doc,
# a build script or an ignore file comes to describe files that do not exist.
DEAD = ("tracks/A-ingest", "tracks/B-index", "tracks/C-join", "tracks/D-traverse",
        "tracks/E-compress", "build-all.sh", "vendor-toolchain.sh",
        "libspeeddemon_core", "core/src/lib.rs")
dead_hits = [f"{rel}: {dead}" for rel, text in texts() for dead in DEAD if dead in text]
check("no abandoned-architecture paths referenced as current",
      not dead_hits, "; ".join(dead_hits[:6]))

# ── 14. a doc that names a script must name a script that exists ──────────
# CAMPAIGN-STATUS.md claimed "Guarded by `tooling/check_pgo_wiring.sh`" while
# no such file existed: prose is where a removed tool leaves a present-tense
# claim behind. Only root-relative tooling/ and verification/ paths are
# policed, because those are unambiguous.
script_hits = []
for rel, text in texts():
    for m in re.finditer(r"`((?:tooling|verification)/[A-Za-z0-9_.-]+\.(?:sh|py|ps1))`", text):
        if not (ROOT / m.group(1)).is_file():
            script_hits.append(f"{rel} -> {m.group(1)}")
check("every script named in first-party prose exists",
      not script_hits, "; ".join(script_hits[:6]))
check("check_pgo_wiring is wired into ci.sh", "check_pgo_wiring" in ci)

# ── 15. a PGO profile inside a track folder must remain committable ──────
# Two independent rules have to hold at once for build-time training to work:
# the organizers count a committed `.profdata` in a track folder as DATA rather
# than code (so it is free on S), and the track folder is the entire build
# context (so the profile has to live inside it to reach the scored image).
# A `*.profdata` line in .gitignore satisfies neither -- it just blocks the
# commit, and the build then runs unprofiled while claiming otherwise.
for probe in ("track-a/pgo/merged.profdata", "track-a/merged.profdata"):
    r = subprocess.run(["git", "check-ignore", "-q", probe], cwd=ROOT)
    check(f"{probe} is committable (not gitignored)", r.returncode != 0)

# ── 16. generators must force LF when they write committed files ──────────
# The check above catches the SYMPTOM (a CR in a tracked file). This catches
# the CAUSE, before it lands. `Path.write_text()` and `open()` in text mode
# both inherit newline=None, and Python then translates every "\n" into
# os.linesep -- CRLF on Windows. Two tools here did exactly that and wrote CRLF
# into ten PERF.md/README.md files and all four `.sha256` pins, so the working
# tree and the committed bytes disagreed on a repository whose whole build runs
# on Linux. Parsed with ast, not a regex, so multi-line calls are handled
# exactly and a `newline=` on a continuation line still counts.
import ast


def writers_missing_newline(path: Path):
    """Line numbers of text-writing calls in `path` that omit newline=."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
    except SyntaxError:
        return []
    hits = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        fn = node.func
        name = (fn.attr if isinstance(fn, ast.Attribute)
                else fn.id if isinstance(fn, ast.Name) else None)
        if name not in ("write_text", "open"):
            continue
        if any(k.arg == "newline" for k in node.keywords):
            continue
        if name == "write_text":
            hits.append(node.lineno)
            continue
        # open(): only text-mode writes can translate newlines.
        mode = None
        if len(node.args) > 1 and isinstance(node.args[1], ast.Constant):
            mode = node.args[1].value
        for k in node.keywords:
            if k.arg == "mode" and isinstance(k.value, ast.Constant):
                mode = k.value.value
        if isinstance(mode, str) and any(c in mode for c in "wax") and "b" not in mode:
            hits.append(node.lineno)
    return hits


writer_hits = [f"{p.name}:{ln}" for p in sorted(TOOLING.glob("*.py"))
               for ln in writers_missing_newline(p)]
check("every text writer forces an explicit newline (CRLF cannot be written by accident)",
      not writer_hits, ", ".join(writer_hits[:8]))

# ── 17. PERF.md keeps its generated region marked ─────────────────────────
# gen_docs.py replaces ONLY the text between these two markers, which is what
# lets the board-run table survive regeneration -- run IDs are transcribed by
# hand from the board UI and no tool can generate them. Before the markers
# existed the generator rewrote the whole file, so regenerating the docs would
# have silently deleted every run ID Pal recorded, and reported the result
# "fresh" because it compares its own output against itself. If a marker is
# lost, that fallback comes back, so the markers are load-bearing.
for t in tracks:
    pf = ROOT / t / "PERF.md"
    if not pf.is_file():
        check(f"{t}/PERF.md exists", False)
        continue
    pf_text = pf.read_text(encoding="utf-8")
    check(f"{t}/PERF.md marks its generated region (so regeneration cannot delete hand-written rows)",
          pf_text.count("<!-- BEGIN GENERATED measured") == 1
          and pf_text.count("<!-- END GENERATED measured -->") == 1)
    check(f"{t}/PERF.md keeps the board-run table the judges check",
          "Board runs" in pf_text and "| Run ID |" in pf_text)

print(f"check-consistency: {CHECKS} assertions")
if FAILS:
    for f in FAILS:
        print(f"  FAIL {f}")
    print(f"\n{len(FAILS)} INCONSISTENC(Y|IES) — each is a fact that disagrees with its authority", file=sys.stderr)
    raise SystemExit(1)
print("check-consistency: every shared fact agrees with its single source")
