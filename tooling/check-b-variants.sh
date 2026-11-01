#!/bin/sh
# Representation-independence test for Track B.
#
# The build chooses, from a sample, which terms are held as bitmaps and which as posting lists.
# Both representations must answer IDENTICALLY, so the answer must not change when the
# classification is forced to either extreme. This builds three variants of the track source --
# as written, every sampled term a bitmap, and no bitmaps at all -- and requires all three to
# produce the same results.txt on every data directory given.
#
# WHY THIS MATTERS MORE THAN A HASH CHECK: the published-corpus hash proves the shipped
# classification is right on seed 1. It says nothing about the MIXED chains -- a bitmap AND-ed
# with a list, a list promoted to a bitmap by an OR, a bitmap demoted back by an AND -- which is
# where a representation bug hides. Forcing the threshold to both extremes exercises every
# transition on real data, and fuzz-b-queries.py builds query sets that oversample them.
#
# usage: check-b-variants.sh TRUSTED_BIN TRACK_B_MAIN_RS DATA_DIR [DATA_DIR ...]
#   TRUSTED_BIN    a binary already known good (e.g. the committed build)
#   DATA_DIR       directories holding docs.txt + queries.txt
#
# A local verification tool. It is NOT solution code and must stay outside track-*/.
set -u

OLD=$1
SRC=$2
shift 2

if [ ! -x "$OLD" ]; then echo "not executable: $OLD"; exit 2; fi
if [ ! -f "$SRC" ]; then echo "no such source: $SRC"; exit 2; fi
if [ $# -eq 0 ]; then echo "usage: $0 TRUSTED_BIN TRACK_B_MAIN_RS DATA_DIR [DATA_DIR ...]"; exit 2; fi

# The classification decision, exactly as the source writes it. If the source changes shape,
# FAIL LOUDLY rather than silently test the wrong thing -- a test that quietly stops testing is
# worse than no test.
LINE='if d != 0 \&\& d as u64 \* 1024 >= ndocs as u64 {'
grep -q 'd as u64 \* 1024 >= ndocs as u64' "$SRC" || {
  echo "classify() threshold not found in $SRC -- this test no longer applies, do not ignore it"
  exit 2
}

FLAGS="-C opt-level=3 -C codegen-units=1 -C panic=abort -C target-cpu=x86-64-v3"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cp "$SRC" "$tmp/as_written.rs"
sed "s/$LINE/if d != 0 {/" "$SRC" > "$tmp/all_bitmaps.rs"
sed "s/$LINE/if false {/" "$SRC" > "$tmp/no_bitmaps.rs"

for v in as_written all_bitmaps no_bitmaps; do
  rustc --edition 2021 $FLAGS -o "$tmp/$v" "$tmp/$v.rs" 2>"$tmp/$v.err" || {
    echo "BUILD FAIL: $v"; head -20 "$tmp/$v.err"; exit 1
  }
done

fail=0
for d in "$@"; do
  mkdir -p "$tmp/o"; rm -f "$tmp/o/results.txt"
  SD_DATA_DIR="$d" SD_OUT_DIR="$tmp/o" "$OLD" || { echo "trusted binary FAILED on $d"; fail=1; continue; }
  want=$(sha256sum "$tmp/o/results.txt" | cut -c1-16)
  for v in as_written all_bitmaps no_bitmaps; do
    rm -f "$tmp/o/results.txt"
    SD_DATA_DIR="$d" SD_OUT_DIR="$tmp/o" "$tmp/$v" || { echo "FAIL (exit) $v on $d"; fail=1; continue; }
    got=$(sha256sum "$tmp/o/results.txt" | cut -c1-16)
    if [ "$got" = "$want" ]; then
      echo "ok    $v  $d  $got"
    else
      echo "DIFF  $v  $d  trusted=$want got=$got"
      fail=1
    fi
  done
done

if [ $fail = 0 ]; then echo "ALL MATCH -- the answer does not depend on the representation"; else echo "MISMATCH -- do not push"; fi
exit $fail
