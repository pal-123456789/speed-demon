#!/bin/sh
# Interleaved A/B of two Track D builds. Interleaving is the point: this box's
# run-to-run spread for Track D reaches 20%, which is the same size as the effect
# being measured, so measuring A then B in blocks would just measure the load.
#
#   usage: sh tooling/ab-test.sh <binary-a> <binary-b> <label-a> <label-b>
set -u

A="$1"; B="$2"; LA="$3"; LB="$4"
DATA=/d/sd-data/track-d
EXPECT=a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e

run_one() {
    bin="$1"
    rm -rf /tmp/sd-ab; mkdir -p /tmp/sd-ab
    s=$(date +%s.%N)
    SD_DATA_DIR="$DATA" SD_OUT_DIR=/tmp/sd-ab "$bin" || echo "  (exit $?)"
    e=$(date +%s.%N)
    got=$(sha256sum /tmp/sd-ab/distances.txt 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$EXPECT" ]; then v=ok; else v=WRONG; fi
    awk -v a="$s" -v b="$e" -v n="$bin" -v v="$v" \
        'BEGIN{printf "%.3f %s %s\n", b-a, n, v}'
}

i=1
while [ "$i" -le 4 ]; do
    run_one "$A" >> /tmp/sd-ab-a.txt
    run_one "$B" >> /tmp/sd-ab-b.txt
    i=$((i + 1))
done

echo "  $LA:"
sort -n /tmp/sd-ab-a.txt | awk '{printf "    %ss  %s\n", $1, $3}'
echo "  $LB:"
sort -n /tmp/sd-ab-b.txt | awk '{printf "    %ss  %s\n", $1, $3}'
echo
awk -v la="$LA" -v lb="$LB" '
  NR==FNR { a[FNR]=$1; na=FNR; next }
  { b[FNR]=$1; nb=FNR }
  END {
    asort(a); asort(b)
    med_a = (na%2) ? a[(na+1)/2] : (a[na/2]+a[na/2+1])/2
    med_b = (nb%2) ? b[(nb+1)/2] : (b[nb/2]+b[nb/2+1])/2
    printf "  median %s = %.3fs   median %s = %.3fs   ratio = %.2fx\n",
           la, med_a, lb, med_b, med_a/med_b
  }' /tmp/sd-ab-a.txt /tmp/sd-ab-b.txt

rm -f /tmp/sd-ab-a.txt /tmp/sd-ab-b.txt
