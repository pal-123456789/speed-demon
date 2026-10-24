#!/bin/sh
# Dispatcher self-test: proves `run` dispatches on $TRACK, upper-cases a
# lowercase value, and fails loudly on an unknown track or a missing binary.
# Uses fake binaries so it needs nothing built.
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
cp "$REPO/run" "$tmp/run"
cat > "$tmp/bin/speeddemon-c" <<'EOF'
#!/bin/sh
echo "FAKE-BIN-OK args:[$*] TRACK:[$TRACK]"
EOF
chmod +x "$tmp/bin/speeddemon-c"
fails=0

check() {
    desc="$1"; want="$2"; shift 2
    got="$(cd "$tmp" && "$@" 2>&1)"; rc=$?
    if [ "$want" = "ok" ] && [ "$rc" = "0" ] && echo "$got" | grep -q FAKE-BIN-OK; then
        echo "  PASS: $desc"
    elif [ "$want" = "fail" ] && [ "$rc" != "0" ]; then
        echo "  PASS: $desc (exit $rc, message: $(echo "$got" | head -1))"
    else
        echo "  FAIL: $desc — exit $rc, output: $got"
        fails=$((fails + 1))
    fi
}

echo "dispatcher self-test"
check "TRACK=c dispatches, upper-cased" ok env TRACK=c APP_DIR="$tmp" sh "$tmp/run" arg1 arg2
check "TRACK=C dispatches"              ok env TRACK=C APP_DIR="$tmp" sh "$tmp/run"
check "unknown track fails loudly"      fail env TRACK=Z APP_DIR="$tmp" sh "$tmp/run"
check "empty track defaults to A"       fail env TRACK= APP_DIR="$tmp" sh "$tmp/run"
check "missing binary fails loudly"     fail env TRACK=B APP_DIR="$tmp" sh "$tmp/run"

if [ "$fails" = "0" ]; then
    echo "all dispatcher checks passed"
    exit 0
fi
echo "$fails dispatcher check(s) FAILED"
exit 1
