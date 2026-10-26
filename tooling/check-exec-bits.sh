#!/bin/sh
# Check that a clone has the executable bits the build depends on.
# A non-executable hook is silently ignored on Linux, and a non-executable
# build.sh fails with a permission error only when someone finally runs it.
#
# The source of truth is the git INDEX MODE (100755), not the working-tree
# -x test: on Windows dev hosts the working tree has no executable attribute
# (so -x reports NO-X even when the index is right), while the index mode is
# exactly what a fresh clone on the scoring box will carry. A Linux developer
# cloning with core.fileMode=true gets the same bits the index records.
set -u
cd "${1:-.}" || exit 1
missing=0
for f in tooling/ci.sh tooling/install-hooks.sh tooling/pin-base-image.sh \
         tooling/check-exec-bits.sh tooling/hooks/pre-commit \
         track-A/build.sh track-B/build.sh track-C/build.sh track-D/build.sh
do
    mode=$(git ls-files -s "$f" 2>/dev/null | awk '{print $1}')
    if [ "$mode" = "100755" ]; then
        echo "  +x   $f"
    else
        echo "  NO-X $f (index mode: ${mode:-untracked})"
        missing=$((missing + 1))
    fi
done
echo ""
if [ "$missing" = "0" ]; then
    echo "all executable bits present (index mode 100755)"
    exit 0
fi
echo "$missing file(s) missing the executable bit — git update-index --chmod=+x <file>"
exit 1