#!/usr/bin/env bash
# Build aemeath CLI and install to ~/.local/bin/aemeath
set -euo pipefail

cd "$(dirname "$0")"

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] PWD=$PWD"

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="aemeath"

# Keep build artifacts isolated per checkout by default. A shared target dir can
# leave stale crate metadata across worktrees and make release hooks flaky.
TARGET_DIR="${CARGO_TARGET_DIR:-target/aemeath-cli}"
export CARGO_TARGET_DIR="$TARGET_DIR"

# Limit parallel rustc jobs to avoid hook-time SIGTERM on memory pressure.
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

echo ">>> cargo build --release --package cli --target-dir $TARGET_DIR --jobs $CARGO_BUILD_JOBS"
cargo build --release --package cli --target-dir "$TARGET_DIR" --jobs "$CARGO_BUILD_JOBS"

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
