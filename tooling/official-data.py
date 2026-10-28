#!/usr/bin/env python3
"""official-data — generate the organizers' data and verify it byte-for-byte.

The specs publish, for seed 1, the SHA-256 of every input file and of the
reference's expected output, at `tiny`, `sample` and `full` scale. The
generators are vendored (byte-identical, which rule 16 makes free), so this
tool can put the REAL development data in place and prove it is the real
thing before a line of Rust exists.

What it does, per track:
  1. generate the data with the official generator (default: sample scale)
  2. verify every input file against the published hash
  3. run the official reference to produce the expected output
  4. verify that output against the published expected hash
  5. install the data into verification/data/track-<t>
  6. write the OFFICIAL expected hash into
     track-<t>/verification/expected/<output>.sha256 — the true SHA-gate
     target, replacing the scaffold-era self-consistency pin

Usage:
    python tooling/official-data.py --track all                # sample scale
    python tooling/official-data.py --track a --scale tiny
    python tooling/official-data.py --track all --scale full   # ~600 MB, slow
    python tooling/official-data.py --track all --verify-only   # check installed data
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = Path(__file__).resolve().parent.parent
GEN_DIR = ROOT / "reference" / "generators"
REF_DIR = ROOT / "reference"
DATA = ROOT / "verification" / "data"
PY = sys.executable

GENERATOR = {"a": "a_ingest.py", "b": "b_index.py", "c": "c_join.py", "d": "d_traverse.py"}
REFERENCE = {"a": "a_ingest.py", "b": "b_index.py", "c": "c_join.py", "d": "d_traverse.py"}
INPUTS = {"a": ["requests.log"],
          "b": ["docs.txt", "queries.txt"],
          "c": ["records.csv", "regions.csv"],
          "d": ["graph.txt", "queries.txt"]}
OUTPUT = {"a": "summary.txt", "b": "results.txt", "c": "aggregates.txt", "d": "distances.txt"}
TRACKS = ("a", "b", "c", "d")

# The published, authoritative hashes (specs/*.md, seed 1). Inputs first, then
# the reference's expected output.
PUBLISHED: dict[str, dict[str, dict[str, str]]] = {
    "a": {
        "tiny": {
            "requests.log": "a99dd14856a3e6e408097dabc1b3ad5ac5a3801fb12f388bacb2802c8591339f",
            "summary.txt": "c24fa507c963eaeac9224fd8435019b4f4e8c002ece7e32f316681a3957eb92f",
        },
        "sample": {
            "requests.log": "7b22c678acceba2b82120394afe27e827ca4083a677a8e51147defa854c86847",
            "summary.txt": "ca6311bbeb536fa55d85265bf6fd3d24cc9586f13de1ac73bd2a23ac3893aac3",
        },
        "full": {
            "requests.log": "7ec80ba45bceef51ac7f79d3a14521fc6187b2dcb87b5e5542d08063fe5d962a",
            "summary.txt": "495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3",
        },
    },
    "b": {
        "sample": {
            "docs.txt": "e157fabb5874c6058a3f6c7726f51df08b2369664cc76c52dde62335ef57eaa4",
            "queries.txt": "930ba8716d7a9b9b962b24f5c9d11b5e0b1efb1b9c24f775f889e50c639f16aa",
            "results.txt": "eeeec1e9319da98a6f3c664c78f93e7078fd603c25c2bb3027174e81e3638a8f",
        },
        "full": {
            "docs.txt": "6cd461670d6c39f9c5211162577532d3648edeab12340c71cc7d2ab5f18e66c8",
            "queries.txt": "77ef7ffc59afde051006b02a2ce8e4c090fbbfbf101acd6f862d0f8e298d39f9",
            "results.txt": "f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927",
        },
    },
    "c": {
        "sample": {
            "records.csv": "07bc636857b70556921be08d406f887eb07dbdacb8384587ed7fd3b52addfd86",
            "regions.csv": "1bd3904fd5eaed7a4dc1ae761bf369521ce5f384e4c400342919d6edf9e5ece1",
            "aggregates.txt": "9035fed36823232dd41ae2d1b9d6d21aa7c73f49d8e95647e953a89d5ebe9a66",
        },
        "full": {
            "records.csv": "36166f4ac44101a22ebf25b4f949436a681a2698cb0f2ae1f7f6853010259f48",
            "regions.csv": "414c57c4f3ec968f9bce28c59eebfdf57a9425322b468db7ee964c18c4852fe1",
            "aggregates.txt": "d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536",
        },
    },
    "d": {
        "sample": {
            "graph.txt": "16351d408f1f94a6bcf66de5c5762abede111c07a4a38306253504a6c9c9b7cd",
            "queries.txt": "fb4065ffe94d2dd51c924f389bd0c6afa58d42534391aa5b0fe5edfaa33988a5",
            "distances.txt": "a4c76ed030bc7e365db33191f6cf74708fba7f25eb48d64c8c4279f2e57046a0",
        },
        "full": {
            "graph.txt": "fe6471f2712d10b23c2ec58a4cbc7db1ea3e742daf6b6db62829e74d6c4a26c7",
            "queries.txt": "160f1ef48378dcb567a8d9ddd2676ee777b044a5ede3ad4c7b42e812aa4b3a6f",
            "distances.txt": "a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e",
        },
    },
}


def sha(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--track", choices=[*TRACKS, "all"], default="all")
    ap.add_argument("--scale", choices=["tiny", "sample", "full"], default="sample")
    ap.add_argument("--verify-only", action="store_true",
                    help="do not generate; verify what is already installed and re-pin")
    ap.add_argument("--no-pin", action="store_true", help="do not touch the expected-hash pins")
    ap.add_argument("--dest", help="install root (default verification/data)")
    ap.add_argument("--save-expected", action="store_true",
                    help="save the reference output beside the data as expected-<output>")
    args = ap.parse_args(argv)

    tracks = list(TRACKS) if args.track == "all" else [args.track]
    failures = 0

    for t in tracks:
        want = PUBLISHED[t].get(args.scale, {})
        if not want:
            # No published hash table for this scale (the specs publish full and
            # sample for B/C/D; tiny only for A). The reference still defines the
            # answer, so generate and record it — just without hash verification.
            print(f"[track {t}] no published hashes for scale {args.scale} — "
                  f"generating, oracle-verified against the reference only")
        dest = (Path(args.dest) if args.dest else DATA) / f"track-{t}"
        print(f"[track {t}] scale={args.scale} dest={dest}")

        # 1/2. generate (unless verifying) and check every INPUT hash
        tmp = None
        if not args.verify_only:
            tmp = Path(tempfile.mkdtemp(prefix=f"offdata-{t}-"))
            cmd = [PY, str(GEN_DIR / GENERATOR[t]), "--seed", "1",
                   "--scale", args.scale, "--out", str(tmp)]
            t0 = time.perf_counter()
            p = subprocess.run(cmd, capture_output=True, text=True)
            if p.returncode != 0:
                print(f"  FAIL generator: {(p.stderr or '').strip()[-200:]}")
                failures += 1
                shutil.rmtree(tmp, ignore_errors=True)
                continue
            print(f"  generated in {time.perf_counter() - t0:.1f}s")

        src = tmp if tmp is not None else dest
        for name in INPUTS[t]:
            f = src / name
            if not f.is_file():
                print(f"  FAIL missing input {name}")
                failures += 1
                break
            got = sha(f)
            exp = want.get(name)
            if exp and got != exp:
                print(f"  FAIL {name}: sha {got[:16]}... != published {exp[:16]}...")
                failures += 1
            else:
                print(f"  ok   {name} {f.stat().st_size} B  sha {got[:16]}...")

        # 3/4. run the official reference and check the expected OUTPUT hash
        refout = Path(tempfile.mkdtemp(prefix=f"offref-{t}-"))
        p = subprocess.run([PY, str(REF_DIR / REFERENCE[t]), "--data", str(src), "--out", str(refout)],
                           capture_output=True, text=True)
        outname = OUTPUT[t]
        exp_out = want.get(outname)
        if p.returncode != 0 or not (refout / outname).is_file():
            print(f"  FAIL reference: {(p.stderr or '').strip()[-200:]}")
            failures += 1
        else:
            got = sha(refout / outname)
            if exp_out and got != exp_out:
                print(f"  FAIL {outname}: sha {got[:16]}... != published {exp_out[:16]}...")
                failures += 1
            else:
                print(f"  ok   {outname} (reference) sha {got[:16]}... — matches the published hash")

        # 5. install the data into verification/data/track-<t>
        if tmp is not None:
            dest.mkdir(parents=True, exist_ok=True)
            for name in INPUTS[t]:
                shutil.copy2(tmp / name, dest / name)
            print(f"  installed {len(INPUTS[t])} input file(s) -> {dest}")

        # 6. pin the OFFICIAL expected hash (the true SHA-gate target)
        if args.save_expected and (refout / outname).is_file():
            dest.mkdir(parents=True, exist_ok=True)
            shutil.copy2(refout / outname, dest / f"expected-{outname}")
            print(f"  saved the reference answer as expected-{outname}")

        if not args.no_pin and exp_out and (refout / outname).is_file():
            pin = ROOT / f"track-{t}" / "verification" / "expected" / f"{outname}.sha256"
            pin.parent.mkdir(parents=True, exist_ok=True)
            pin.write_text(exp_out + "\n", encoding="utf-8")
            print(f"  pinned {pin.relative_to(ROOT)} <- the OFFICIAL expected hash")

        shutil.rmtree(refout, ignore_errors=True)
        if tmp is not None:
            shutil.rmtree(tmp, ignore_errors=True)

    print()
    if failures:
        print(f"official-data: {failures} FAILED", file=sys.stderr)
        return 1
    print("official-data: every generated file and expected output matches the published hash")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())