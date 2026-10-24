#!/usr/bin/env python3
"""Check the image-contract rules announced before the Oct 28 freeze.

docs/RULE-CHANGES-oct28.md §3-4, quoting the organizers:

  "The final image must build from a published, digest-pinned toolchain image,
   so code can't hide in a custom base image."
  "Builds are capped at 10 minutes and images at 2 GB."
  "The image may not ship cached outputs."

None of the three was enforced anywhere in this repository. This script is the
gate. It is static — it needs no docker, no network and no registry — so it can
run on any host and in CI.

Exit status: 0 all rules satisfied, 1 a rule is violated, 2 the check itself
could not run.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

DIGEST_RE = re.compile(r"@sha256:[0-9a-f]{64}")
FROM_RE = re.compile(r"^\s*FROM\s+(.+?)\s*$", re.IGNORECASE)
ARG_RE = re.compile(r"^\s*ARG\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(\S+)\s*$")

BUILD_TIME_CAP_S = 600
IMAGE_SIZE_CAP_MB = 2048

# The scored output filenames. None of them may be baked into the image: the
# rule says the image may not ship cached outputs, and a pre-computed answer
# would also defeat the SHA gate on the holdout.
OUTPUT_FILES = ("ingest.txt", "index.txt", "join.txt", "traverse.txt", "corpus.sd")


class Result:
    def __init__(self) -> None:
        self.items: list[tuple[bool, str]] = []

    def ok(self, msg: str) -> None:
        self.items.append((True, msg))

    def bad(self, msg: str) -> None:
        self.items.append((False, msg))

    @property
    def failures(self) -> int:
        return sum(1 for good, _ in self.items if not good)

    def report(self) -> None:
        for good, msg in self.items:
            print(f"  {'ok  ' if good else 'FAIL'}  {msg}")


def dockerfiles() -> list[Path]:
    found = []
    top = ROOT / "Dockerfile"
    if top.is_file():
        found.append(top)
    for p in sorted(ROOT.glob("tracks/*/Dockerfile")):
        found.append(p)
    return found


def resolve_base(path: Path, value: str, args: dict[str, str]) -> str:
    """Substitute a ${VAR} / $VAR base reference using the file's own ARG defaults."""
    m = re.fullmatch(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?", value)
    if m:
        return args.get(m.group(1), "")
    return value


def check_base_pinned(res: Result) -> None:
    files = dockerfiles()
    if not files:
        res.bad("no Dockerfile found to inspect")
        return
    seen_refs: set[str] = set()
    for path in files:
        rel = path.relative_to(ROOT)
        text = path.read_text(encoding="utf-8", errors="replace")
        args: dict[str, str] = {}
        unpinned: list[str] = []
        for line in text.splitlines():
            if line.lstrip().startswith("#"):
                continue
            m = ARG_RE.match(line)
            if m:
                args[m.group(1)] = m.group(2)
                continue
            m = FROM_RE.match(line)
            if not m:
                continue
            raw = m.group(1)
            # Drop an `AS name` alias and any trailing --platform flag.
            parts = [t for t in raw.split() if not t.startswith("--")]
            ref = resolve_base(path, parts[0], args) if parts else ""
            if not ref:
                unpinned.append(f"{raw}  (unresolvable ARG)")
            elif not DIGEST_RE.search(ref):
                unpinned.append(ref)
            else:
                seen_refs.add(ref)
        if unpinned:
            for ref in unpinned:
                res.bad(
                    f"{rel}: FROM {ref} is not digest-pinned "
                    "(need image@sha256:<64 hex>)"
                )
        else:
            res.ok(f"{rel}: every FROM is digest-pinned")

    # The root image and the five per-track images must use the SAME base, or
    # "the image" the judges build is not the image anyone reviewed.
    if len(seen_refs) > 1:
        for ref in sorted(seen_refs):
            res.bad(f"the Dockerfiles disagree about the base image: {ref}")
    elif seen_refs:
        res.ok(f"every Dockerfile uses the same base image: {sorted(seen_refs)[0]}")

    # And that base must be the one recorded in the single source of truth.
    base_file = ROOT / "tooling" / "base-image.txt"
    if base_file.is_file():
        want = base_file.read_text(encoding="utf-8").strip()
        if want and seen_refs and want not in seen_refs:
            res.bad(
                f"tooling/base-image.txt says {want}, but the Dockerfiles use "
                f"{', '.join(sorted(seen_refs))}"
            )
    else:
        res.bad("tooling/base-image.txt is missing — the base image has no single source of truth")


def check_no_cached_outputs(res: Result) -> None:
    """No scored output file may exist in the build context."""
    present = []
    for name in OUTPUT_FILES:
        top = ROOT / name
        if top.is_file():
            present.append(name)
        for p in ROOT.glob(f"tracks/*/{name}"):
            present.append(str(p.relative_to(ROOT)))
        for p in ROOT.glob(f"tracks/*/src/{name}"):
            present.append(str(p.relative_to(ROOT)))
    if present:
        for p in present:
            res.bad(f"a scored output file is in the build context: {p}")
    else:
        res.ok("no scored output file is in the build context")

    # fixtures and generated data must stay out of the image
    ignore = ROOT / ".dockerignore"
    if not ignore.is_file():
        res.bad(".dockerignore is missing")
        return
    patterns = [
        ln.strip()
        for ln in ignore.read_text(encoding="utf-8", errors="replace").splitlines()
        if ln.strip() and not ln.strip().startswith("#")
    ]
    for required in ("verification/data/", "bin/"):
        if any(p.rstrip("/") == required.rstrip("/") for p in patterns):
            res.ok(f".dockerignore excludes {required}")
        else:
            res.bad(f".dockerignore does not exclude {required}")


def check_limits_recorded(res: Result) -> None:
    """The two hard ceilings must be recorded where the freeze reads them."""
    contract = ROOT / "verification" / "contract.toml"
    if not contract.is_file():
        res.bad("verification/contract.toml is missing")
        return
    text = contract.read_text(encoding="utf-8", errors="replace")
    for key, want in (
        ("build_time_cap_s", str(BUILD_TIME_CAP_S)),
        ("image_size_cap_mb", str(IMAGE_SIZE_CAP_MB)),
        ("base_image_must_be_digest_pinned", "true"),
    ):
        if re.search(rf"^\s*{key}\s*=\s*{want}\s*$", text, re.MULTILINE):
            res.ok(f"contract records {key} = {want}")
        else:
            res.bad(f"contract does not record {key} = {want}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args(argv)

    if not ROOT.is_dir():
        print(f"cannot locate the repository root from {ROOT}", file=sys.stderr)
        return 2

    res = Result()
    check_base_pinned(res)
    check_no_cached_outputs(res)
    check_limits_recorded(res)

    if not args.quiet:
        print("check_base_pinned: image-contract rules (docs/RULE-CHANGES-oct28.md)")
        res.report()
        print()
    if res.failures:
        print(f"check_base_pinned: {res.failures} FAILED", file=sys.stderr)
        return 1
    print("check_base_pinned: all rules satisfied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())