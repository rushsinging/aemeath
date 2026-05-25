#!/usr/bin/env bash
# Build aemeath CLI and install to ~/.local/bin/aemeath
set -euo pipefail

cd "$(dirname "$0")"

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="aemeath"

# Keep incremental build artifacts out of each checkout/worktree by default.
# Use the repository's common git dir so all linked worktrees share one target.
COMMON_GIT_DIR="$(git rev-parse --git-common-dir 2>/dev/null || true)"
if [[ -n "$COMMON_GIT_DIR" ]]; then
    DEFAULT_TARGET_DIR="$COMMON_GIT_DIR/aemeath-target"
else
    DEFAULT_TARGET_DIR="$HOME/.cache/aemeath-target"
fi
TARGET_DIR="${CARGO_TARGET_DIR:-$DEFAULT_TARGET_DIR}"
export CARGO_TARGET_DIR="$TARGET_DIR"

echo ">>> cargo build --release --package cli --target-dir $TARGET_DIR"
cargo build --release --package cli --target-dir "$TARGET_DIR"

mkdir -p "$INSTALL_DIR"
cp "$TARGET_DIR/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

# macOS Gatekeeper kills freshly-copied ad-hoc signed binaries on some
# versions due to `com.apple.provenance` xattr + signature mismatch after
# the move. Strip attrs and re-sign ad-hoc so the binary is runnable.
if [[ "$(uname)" == "Darwin" ]]; then
    xattr -cr "$INSTALL_DIR/$BIN_NAME" 2>/dev/null || true
    codesign --force --sign - "$INSTALL_DIR/$BIN_NAME" 2>/dev/null || true
fi

echo ">>> installed: $INSTALL_DIR/$BIN_NAME ($(du -h "$INSTALL_DIR/$BIN_NAME" | cut -f1))"
