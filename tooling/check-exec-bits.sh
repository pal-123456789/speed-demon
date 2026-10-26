#!/bin/sh
# Check that a clone has the executable bits the build depends on.
# A non-executable hook is silently ignored on Linux, and a non-executable
# build.sh fails with a permission error only when someone finally runs it.
set -u
root="${1:-.}"
cd "$root" || exit 1
missing=0
for f in run build-all.sh build-one.sh tooling/ci.sh tooling/install-hooks.sh \
         tooling/vendor-toolchain.sh tooling/pgo-build.sh tooling/test-dispatcher.sh \
         tooling/hooks/pre-commit tracks/A-ingest/build.sh tracks/B-index/build.sh \
         tracks/C-join/build.sh tracks/D-traverse/build.sh
do
    if [ -x "$f" ]; then
        echo "  +x   $f"
    else
        echo "  NO-X $f"
        missing=$((missing + 1))
    fi
done
echo ""
if [ "$missing" = "0" ]; then
    echo "all executable bits present"
    exit 0
fi
echo "$missing file(s) missing the executable bit"
exit 1
