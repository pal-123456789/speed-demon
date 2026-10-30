#!/bin/sh
# Interleaved A/B of two builds of the same track.
#
# Interleaving is the whole point. Board spread for these tracks reaches 16-20%,
# which is the same size as the effects being chased, so measuring A in one block
# and B in another measures the load rather than the code. This alternates them
# and reports medians.
#
#   usage: sh tooling/ab-track.sh <track> <bin-a> <bin-b> <label-a> <label-b> <expected-sha256> <output-name> [rounds]
set -u

track="$1"; A="$2"; B="$3"; LA="$4"; LB="$5"; EXPECT="$6"; OUTNAME="$7"
rounds="${8:-4}"
DATA="/d/sd-data/track-$track"
rm -f /tmp/sd-ab-a.txt /tmp/sd-ab-b.txt

run_one() {
    rm -rf /tmp/sd-ab; mkdir -p /tmp/sd-ab
    s=$(date +%s.%N)
    SD_DATA_DIR="$DATA" SD_OUT_DIR=/tmp/sd-ab "$1" >/dev/null 2>&1 || true
    e=$(date +%s.%N)
    got=$(sha256sum "/tmp/sd-ab/$OUTNAME" 2>/dev/null | cut -d' ' -f1)
    if [ "$got" = "$EXPECT" ]; then v=ok; else v=WRONG; fi
    awk -v a="$s" -v b="$e" -v v="$v" 'BEGIN{printf "%.3f %s\n", b-a, v}'
}

i=1
while [ "$i" -le "$rounds" ]; do
    run_one "$A" >> /tmp/sd-ab-a.txt
    run_one "$B" >> /tmp/sd-ab-b.txt
    i=$((i + 1))
done

echo "  $LA:"; sort -n /tmp/sd-ab-a.txt | awk '{printf "    %ss  %s\n", $1, $2}'
echo "  $LB:"; sort -n /tmp/sd-ab-b.txt | awk '{printf "    %ss  %s\n", $1, $2}'
echo
awk -v la="$LA" -v lb="$LB" '
  NR==FNR { a[FNR]=$1; na=FNR; next } { b[FNR]=$1; nb=FNR }
  END {
    asort(a); asort(b)
    ma = (na%2) ? a[(na+1)/2] : (a[na/2]+a[na/2+1])/2
    mb = (nb%2) ? b[(nb+1)/2] : (b[nb/2]+b[nb/2+1])/2
    printf "  median %s = %.4fs   median %s = %.4fs   ->  %s is %.2fx %s\n",
           la, ma, lb, mb, (mb<ma?lb:la), (ma<mb?mb/ma:ma/mb), (mb<ma?"faster":"slower")
  }' /tmp/sd-ab-a.txt /tmp/sd-ab-b.txt
rm -f /tmp/sd-ab-a.txt /tmp/sd-ab-b.txt
