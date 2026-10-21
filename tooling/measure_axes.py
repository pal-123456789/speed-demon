#!/usr/bin/env python3
"""
Speed Demon — S-axis and D-axis measurement.

S  source size, weight 0.15
   "Bytes of source after running your language's canonical formatter,
    stripping comments, and normalising identifiers to fixed-width tokens."
   The identifier step is what makes the axis fair: renaming every variable
   to a single letter changes nothing, so readable code is free. This script
   approximates the organizer's normalisation so you can see where you sit
   BEFORE the freeze instead of after.

D  dependency weight, weight 0.15
   D = 1 + (non-stdlib runtime dependencies) + (vendored source bytes / 50000)
   The reference has none, so D_ref = 1 and every dependency is a straight
   cost. A zero-dependency submission starts this axis at 1.0 against
   everyone else's 2.0 or 3.0 — worth roughly 10-17% on K before any other
   work.

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
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

TRACK_DIRS = {
    "A": "A-ingest",
    "B": "B-index",
    "C": "C-join",
    "D": "D-traverse",
    "E": "E-compress",
}

# The published reference source size for Track A (the event page states
# 3,180 B). Other tracks' reference sizes are not published; they come from
# the Oct 28 frozen spec.
REF_SOURCE_BYTES = {"A": 3180}

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


def measure_source(track_dir: Path) -> dict:
    """Approximate the S axis for one track directory."""
    files = sorted((track_dir / "src").rglob("*.rs")) if (track_dir / "src").exists() else []
    raw_bytes = 0
    stripped_bytes = 0
    normalised_bytes = 0
    per_file = []
    for f in files:
        raw = f.read_bytes()
        text = raw.decode("utf-8", errors="replace")
        stripped = strip_comments(text)
        normalised = normalise_identifiers(stripped)
        # The organiser runs a canonical formatter first; rustfmt output is
        # close enough for a trend indicator, and the ratio (not the absolute)
        # is what we are tracking.
        raw_bytes += len(raw)
        stripped_bytes += len(stripped.encode("utf-8"))
        normalised_bytes += len(normalised.encode("utf-8"))
        per_file.append({
            "file": str(f.relative_to(ROOT)),
            "raw": len(raw),
            "no_comments": len(stripped.encode("utf-8")),
            "normalised": len(normalised.encode("utf-8")),
        })
    return {
        "files": per_file,
        "raw_bytes": raw_bytes,
        "stripped_bytes": stripped_bytes,
        "normalised_bytes": normalised_bytes,
    }


# ── D axis ────────────────────────────────────────────────────────────────

def measure_dependencies(track_dir: Path) -> dict:
    """D = 1 + non-stdlib runtime deps + vendored source bytes / 50000."""
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
                if name and name != "speeddemon-core":
                    deps.append(name)

    # vendored third-party source (NOT our own crate's source)
    for vname in ("vendor", "third_party", "external"):
        vdir = track_dir / vname
        if vdir.exists():
            vendor_dirs.append(str(vdir))
            for f in vdir.rglob("*"):
                if f.is_file():
                    vendored_bytes += f.stat().st_size

    d = 1.0 + len(deps) + (vendored_bytes / 50000.0)
    return {
        "declared_dependencies": len(deps),
        "dependencies": deps,
        "vendored_bytes": vendored_bytes,
        "vendor_dirs": vendor_dirs,
        "D": d,
    }


# ── report ────────────────────────────────────────────────────────────────

def report(track: str, as_json: bool) -> dict:
    tdir = ROOT / "tracks" / TRACK_DIRS[track]
    s = measure_source(tdir)
    d = measure_dependencies(tdir)
    ref = REF_SOURCE_BYTES.get(track)
    s_ratio = (ref / s["normalised_bytes"]) if (ref and s["normalised_bytes"]) else None

    result = {
        "track": track,
        "S": s,
        "S_ratio_vs_reference": s_ratio,
        "D": d,
    }
    if not as_json:
        print(f"\n=== TRACK {track} ({TRACK_DIRS[track]}) ===")
        print(f"  S  source size")
        print(f"     raw bytes        : {s['raw_bytes']}")
        print(f"     comments stripped: {s['stripped_bytes']}")
        print(f"     identifiers norm.: {s['normalised_bytes']}   <- the S axis")
        if s_ratio is not None:
            print(f"     vs reference {ref} B: ratio {s_ratio:.4f}x  "
                  f"contributes {s_ratio**0.15:.4f} to K")
        else:
            print("     vs reference      : (reference size not published — "
                  "pin it from the Oct 28 spec)")
        for f in s["files"]:
            print(f"       {f['file']:<40} raw={f['raw']:<7} norm={f['normalised']}")
        print(f"  D  dependency weight")
        print(f"     runtime deps     : {d['declared_dependencies']} {d['dependencies']}")
        print(f"     vendored bytes   : {d['vendored_bytes']}")
        print(f"     D                : {d['D']:.4f}  "
              f"(D_ref = 1.00, contributes {d['D']**-0.15:.4f} to K)")
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
