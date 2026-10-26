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
# Force UTF-8 on stdio. On Windows the default code page mangles every
# non-ASCII character this tool prints, which corrupts logs and makes
# mojibake indistinguishable from real output.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


ROOT = Path(__file__).resolve().parent.parent

DIGEST_RE = re.compile(r"@sha256:[0-9a-f]{64}")
FROM_RE = re.compile(r"^\s*FROM\s+(.+?)\s*$", re.IGNORECASE)
ARG_RE = re.compile(r"^\s*ARG\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(\S+)\s*$")

BUILD_TIME_CAP_S = 600
IMAGE_SIZE_CAP_MB = 2048

# The scored output filenames. None of them may be baked into the image: the
# rule says the image may not ship cached outputs, and a pre-computed answer
# would also defeat the SHA gate on the holdout.
OUTPUT_FILES = ("summary.txt", "results.txt", "aggregates.txt", "distances.txt")


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
    for p in sorted(ROOT.glob("track-*/Dockerfile")):
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
    allow: set[str] = set()
    base_file = ROOT / "tooling" / "base-image.txt"
    if base_file.is_file():
        allow = {
            ln.strip()
            for ln in base_file.read_text(encoding="utf-8", errors="replace").splitlines()
            if ln.strip() and not ln.lstrip().startswith("#")
        }
    if not allow:
        res.bad("tooling/base-image.txt is empty — no allowlisted base images recorded")

    # The runtime base: the final image must be the clean runtime stage (the
    # last FROM in each Dockerfile), not the toolchain build stage. The runtime
    # image is the non-toolchain entry in the allowlist.
    runtime_ref = ""
    for ref in sorted(allow):
        if not ref.startswith("rust:"):
            runtime_ref = ref
    if not runtime_ref:
        res.bad("tooling/base-image.txt does not pin a non-toolchain runtime base")

    for path in files:
        rel = path.relative_to(ROOT)
        text = path.read_text(encoding="utf-8", errors="replace")
        args: dict[str, str] = {}
        unpinned: list[str] = []
        non_allowlisted: list[str] = []
        last_from = ""
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
            parts = [t for t in raw.split() if not t.startswith("--")]
            ref = resolve_base(path, parts[0], args) if parts else ""
            if ref:
                last_from = ref
            if not ref:
                unpinned.append(f"{raw}  (unresolvable ARG)")
            elif not DIGEST_RE.search(ref):
                unpinned.append(ref)
            elif allow and ref not in allow:
                non_allowlisted.append(ref)
            else:
                seen_refs.add(ref)
        for ref in unpinned:
            res.bad(
                f"{rel}: FROM {ref} is not digest-pinned "
                "(need image@sha256:<64 hex>)"
            )
        for ref in non_allowlisted:
            res.bad(
                f"{rel}: FROM {ref} is not in tooling/base-image.txt — the "
                "allowlist records every image this submission builds from"
            )
        if not unpinned and not non_allowlisted:
            res.ok(f"{rel}: every FROM is digest-pinned and allowlisted")
        if last_from and runtime_ref and last_from != runtime_ref:
            res.bad(
                f"{rel}: the runtime stage is {last_from}, expected {runtime_ref} — "
                "the final image must be the clean runtime base, not a toolchain image"
            )
        elif last_from:
            res.ok(f"{rel}: runtime stage is the clean {runtime_ref.split('@')[0]} base")

    # And every base used must be recorded in the single source of truth.
    if seen_refs and allow and not seen_refs.issubset(allow):
        res.bad(
            f"tooling/base-image.txt does not cover {', '.join(sorted(seen_refs - allow))}"
        )


def check_no_cached_outputs(res: Result) -> None:
    """No scored output file may exist in the build context."""
    present = []
    for name in OUTPUT_FILES:
        top = ROOT / name
        if top.is_file():
            present.append(name)
        for p in ROOT.glob(f"track-*/{name}"):
            present.append(str(p.relative_to(ROOT)))
        for p in ROOT.glob(f"track-*/src/{name}"):
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