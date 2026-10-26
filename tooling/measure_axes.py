#!/usr/bin/env python3
"""
Speed Demon — S-axis and D-axis measurement.

CHANGED BEFORE THE OCT 28 FREEZE. The definitions below are the organizers'
revised ones, published in the hackathon channel and consolidated in
docs/RULE-CHANGES-oct28.md. Both axes are now penalties only: neither can score
above 1.0, so nothing here can earn points by shrinking or restructuring.

S  source size, weight 0.15
   "Source size is code lines counted by scc after your language's formatter,
    across each track's folder, minus registry packages first published before
    Oct 1, 2026 by people outside the team. It covers everything in the
    track's folder at the recorded commit, which is all the build can see."
   The factor is capped at 1.0: it can only cost, never pay.
   The old definitions — bytes after comment stripping, identifier
   normalisation, or a repo-wide count — are superseded and do not approximate
   the scored quantity.

D  dependency weight, weight 0.15
   D = 1 + (direct non-stdlib runtime dependencies in your manifest)
   "The vendored-bytes term is gone." This replaces
   `1 + deps + vendored_bytes/50000`, which counted vendored source twice, and
   the short-lived `0.90^deps` reading, which was never the rule. The
   reference declares none, so D_ref = 1 and this repo still sits at 1.0. One
   dependency gives D = 2, a factor of 2^-0.15 = 0.90 on K.

Usage
-----
    python tooling/measure_axes.py --track A
    python tooling/measure_axes.py --all
    python tooling/measure_axes.py --all --json
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


ROOT = Path(__file__).resolve().parent.parent

TRACK_DIRS = {
    "A": "track-A",
    "B": "track-B",
    "C": "track-C",
    "D": "track-D",
}

# The published reference source size for Track A: 56 CODE LINES (the S unit
# is code lines by scc). Other tracks' reference sizes are not published;
# they come from the Oct 28 frozen spec.
REF_SOURCE_LINES = {"A": 56}

# The D formula from the Oct 28 rule change: D = 1 + the number of direct
# non-stdlib runtime dependencies. Must match `d_value` in
# verification/harness/src/contract.rs.
D_BASE = 1.0

# ── S axis ────────────────────────────────────────────────────────────────

IDENT_RE = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")
# Rust keywords and built-in types are not "identifiers" we normalise away;
# they are fixed-width tokens already.
KEYWORDS = {
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "for",
    "while", "loop", "if", "else", "match", "return", "use", "mod", "const",
    "static", "type", "where", "as", "in", "ref", "move", "unsafe", "crate",
    "self", "super", "true", "false", "Some", "None", "Ok", "Err", "Result",
    "Option", "Vec", "String", "u8", "u16", "u32", "u64", "usize", "i8", "i16",
    "i32", "i64", "isize", "f32", "f64", "bool", "char", "str", "Self",
}


def strip_comments(src: str) -> str:
    """Remove // line comments and /* */ block comments, respecting string
    literals and char literals so a '//' inside a string survives."""
    out = []
    i = 0
    n = len(src)
    in_str = False
    in_char = False
    in_line = False
    in_block = False
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if in_line:
            if c == "\n":
                in_line = False
                out.append(c)
            i += 1
            continue
        if in_block:
            if c == "*" and nxt == "/":
                in_block = False
                i += 2
                continue
            i += 1
            continue
        if in_str:
            out.append(c)
            if c == "\\":
                if i + 1 < n:
                    out.append(src[i + 1])
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if in_char:
            out.append(c)
            if c == "\\":
                if i + 1 < n:
                    out.append(src[i + 1])
                i += 2
                continue
            if c == "'":
                in_char = False
            i += 1
            continue
        # normal state
        if c == "/" and nxt == "/":
            in_line = True
            i += 2
            continue
        if c == "/" and nxt == "*":
            in_block = True
            i += 2
            continue
        if c == '"':
            in_str = True
            out.append(c)
            i += 1
            continue
        if c == "'":
            # lifetime ('a) vs char literal — treat a quote followed by a
            # char and a closing quote as a char literal
            if i + 2 < n and src[i + 2] == "'":
                in_char = True
            else:
                out.append(c)
                i += 1
                continue
        out.append(c)
        i += 1
    return "".join(out)


def normalise_identifiers(src: str) -> str:
    """Replace every identifier with a fixed-width token of the same length
    class, so renaming cannot change the measurement. We keep the LENGTH of
    the original identifier, which is the only property the organiser's
    normaliser can reasonably preserve (a fixed-width token per identifier)."""
    def repl(m: re.Match) -> str:
        name = m.group(0)
        if name in KEYWORDS:
            return name
        return "x" * min(len(name), 16)
    return IDENT_RE.sub(repl, src)


def _scc_code_lines(path: Path) -> int | None:
    """Code lines for `path` as `scc` counts them, or None if scc is absent.

    The organizers count S with `scc`, so guessing with our own tokenizer is a
    trend indicator at best. When scc is installed we use it directly.
    """
    exe = shutil.which("scc")
    if not exe or not path.exists():
        return None
    try:
        out = subprocess.run(
            [exe, "--format", "json", str(path)],
            capture_output=True, text=True, timeout=120, check=False,
        )
        if out.returncode != 0 or not out.stdout.strip():
            return None
        data = json.loads(out.stdout)
        return int(sum(lang.get("Code", 0) for lang in data))
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def _fallback_code_lines(text: str) -> int:
    """A deterministic stand-in for scc's Code column: non-blank, non-comment.

    Used only when scc is unavailable, and labelled as such in the report so a
    number from this path is never mistaken for the scored quantity.
    """
    stripped = strip_comments(text)
    return sum(1 for line in stripped.splitlines() if line.strip())


def measure_source(track_dir: Path) -> dict:
    """Measure the S axis: code lines by scc over THIS TRACK'S FOLDER.

    After the Oct 28 change, S is counted with scc across the track's folder
    ("all the build can see"), minus registry packages first published before
    Oct 1, 2026 by people outside the team. We ship no registry packages, so
    the folder is the scope. The factor is capped at 1.0: being small earns
    nothing.
    """
    src_files = sorted(
        p
        for p in track_dir.rglob("*")
        if p.is_file()
        and "target" not in p.parts
        and "verification" not in p.parts
        and p.name not in ("run", "run.exe", "run.pdb")
        and not p.name.endswith((".pdb", ".exe"))
    )
    per_file = []
    fallback = 0
    for f in src_files:
        n = _fallback_code_lines(f.read_bytes().decode("utf-8", errors="replace"))
        fallback += n
        per_file.append({"file": str(f.relative_to(ROOT)), "code_lines": n})
    scc_lines = _scc_code_lines(track_dir)
    return {
        "files": per_file,
        "code_lines": scc_lines if scc_lines is not None else fallback,
        "counter": "scc" if scc_lines is not None else "fallback(non-blank,non-comment)",
        "scc_available": scc_lines is not None,
        "fallback_code_lines": fallback,
    }


# ── D axis ────────────────────────────────────────────────────────────────

def measure_dependencies(track_dir: Path) -> dict:
    """D = 1 + (direct non-stdlib runtime dependencies in the manifest).

    Changed before the Oct 28 freeze (docs/RULE-CHANGES-oct28.md): "D = 1 + the
    number of direct non-stdlib runtime dependencies declared in your manifest.
    The vendored-bytes term is gone."

    This replaces `1 + deps + vendored_bytes/50000`, which counted vendored
    source a second time, and the short-lived `0.90^deps` reading, which was
    never the rule the organizers published. Vendored bytes are still reported
    for information, but they are not in the formula.
    """
    cargo = track_dir / "Cargo.toml"
    deps = []
    vendored_bytes = 0
    vendor_dirs = []

    if cargo.exists():
        text = cargo.read_text(encoding="utf-8", errors="replace")
        in_deps = False
        for line in text.splitlines():
            s = line.strip()
            if s.startswith("["):
                in_deps = s in ("[dependencies]",)
                continue
            if in_deps and "=" in s and not s.startswith("#"):
                name = s.split("=")[0].strip()
                if name:
                    deps.append(name)

    # vendored third-party source (NOT our own crate's source) — reported only
    for vname in ("vendor", "third_party", "external"):
        vdir = track_dir / vname
        if vdir.exists():
            vendor_dirs.append(str(vdir))
            for f in vdir.rglob("*"):
                if f.is_file():
                    vendored_bytes += f.stat().st_size

    d = D_BASE + len(deps)
    return {
        "declared_dependencies": len(deps),
        "dependencies": deps,
        "vendored_bytes": vendored_bytes,
        "vendor_dirs": vendor_dirs,
        "D": d,
    }


# ── report ────────────────────────────────────────────────────────────────

def report(track: str, as_json: bool) -> dict:
    tdir = ROOT / TRACK_DIRS[track]
    s = measure_source(tdir)
    d = measure_dependencies(tdir)
    ref = REF_SOURCE_LINES.get(track)
    # S is code lines by scc over the track's own folder, so the ratio uses
    # that count against the reference (published: Track A = 56 lines).
    s_ratio = (ref / s["code_lines"]) if (ref and s["code_lines"]) else None
    # "Its factor is capped at 1.0" — the axis can only cost.
    s_factor = min(1.0, s_ratio ** 0.15) if s_ratio else None

    result = {
        "track": track,
        "S": s,
        "S_ratio_vs_reference": s_ratio,
        "S_factor_capped": s_factor,
        "D": d,
        "rules": "docs/RULE-CHANGES-oct28.md",
    }
    if not as_json:
        print(f"\n=== TRACK {track} ({TRACK_DIRS[track]}) ===")
        print(f"  S  source size — CODE LINES (scc) over {TRACK_DIRS[track]}/")
        print(f"     counter          : {s['counter']}")
        print(f"     folder code lines: {s['code_lines']}   <- the S axis")
        print(f"     S factor capped at 1.0: being smaller earns nothing")
        if s_ratio is not None:
            print(f"     vs reference ({ref} code lines): "
                  f"ratio {s_ratio:.4f}x  factor {s_factor:.4f}")
        else:
            print("     vs reference      : (reference code lines not published — "
                  "pin it from the Oct 28 spec)")
        for f in s["files"]:
            print(f"       {f['file']:<40} code_lines={f['code_lines']}")
        print(f"  D  dependency weight")
        print(f"     direct deps      : {d['declared_dependencies']} {d['dependencies']}")
        print(f"     vendored bytes   : {d['vendored_bytes']}  (reported only — "
              f"not in the formula since the Oct 28 change)")
        print(f"     D                : {d['D']:.4f}  "
              f"(1 + direct deps; contributes {(1.0 / d['D'])**0.15:.4f} to K)")
    return result


def main() -> int:
    p = argparse.ArgumentParser(description="Speed Demon S/D axis measurement")
    p.add_argument("--track", choices=list(TRACK_DIRS), help="one track")
    p.add_argument("--all", action="store_true", help="every track")
    p.add_argument("--json", action="store_true", help="JSON output")
    args = p.parse_args()

    if not args.track and not args.all:
        p.print_help()
        return 2

    targets = list(TRACK_DIRS) if args.all else [args.track]
    results = [report(t, args.json) for t in targets]

    if args.json:
        print(json.dumps(results, indent=2))
    elif args.all:
        print("\n--- D AXIS SUMMARY (D=1.00 is the zero-dependency baseline) ---")
        for r in results:
            print(f"  Track {r['track']}: D = {r['D']['D']:.4f}   "
                  f"deps={r['D']['declared_dependencies']}   "
                  f"vendored={r['D']['vendored_bytes']}B")
    return 0


if __name__ == "__main__":
    sys.exit(main())
