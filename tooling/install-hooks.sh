#!/bin/sh
# Install the campaign's git hooks.
#
#   sh tooling/install-hooks.sh
#
# Git does not version .git/hooks, so the hook lives in tooling/hooks/ and is
# installed by symlink (or copy, on filesystems without symlink support).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOKS_DIR="$ROOT/.git/hooks"

if [ ! -d "$ROOT/.git" ]; then
    echo "[hooks] not a git repository yet — run 'git init' first" >&2
    exit 1
fi

mkdir -p "$HOOKS_DIR"

for hook in pre-commit; do
    src="$ROOT/tooling/hooks/$hook"
    dst="$HOOKS_DIR/$hook"
    if [ ! -f "$src" ]; then
        echo "[hooks] missing $src" >&2
        exit 1
    fi
    # Prefer a copy: symlinks break when the repo is cloned on Windows or
    # mounted into a container.
    cp "$src" "$dst"
    chmod +x "$dst"
    echo "[hooks] installed $hook"
done

cat <<'EOF'

The pre-commit hook now blocks a commit that:
  * is not rustfmt-clean (the S axis is measured on formatted source)
  * has clippy warnings
  * fails to build offline
  * breaks the SHA / determinism gate for a track whose source changed

Bypass it only when you know why:  git commit --no-verify
EOF
