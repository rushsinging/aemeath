#!/usr/bin/env bash
# Build aemeath CLI and install to ~/.local/bin/aemeath
set -euo pipefail

cd "$(dirname "$0")"

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] AEMEATH_IN_WORKTREE=${AEMEATH_IN_WORKTREE:-<unset>}"
echo "[hook-env] PWD=$PWD"

# 仅在主仓库（main 工作区）构建并安装：aemeath 在 git worktree 中执行 hook 时
# 注入 AEMEATH_IN_WORKTREE=1，此时跳过 build/install，避免把未合并分支的二进制
# 装成全局工具污染日常使用的 ~/.local/bin/aemeath。
if [[ "${AEMEATH_IN_WORKTREE:-0}" == "1" ]]; then
    echo ">>> 在 git worktree 中，跳过 build/install（仅在 main 主工作区构建）"
    exit 0
fi

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="aemeath"

# Use the target dir from .cargo/config.toml (build.target-dir), still honoring
# CARGO_TARGET_DIR when the caller sets one. Resolve it from cargo itself so the
# install path can never drift from the actual build output.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps \
    | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])')"

# Limit parallel rustc jobs to avoid hook-time SIGTERM on memory pressure.
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

echo ">>> cargo build --release --package cli --jobs $CARGO_BUILD_JOBS (target-dir: $TARGET_DIR)"
cargo build --release --package cli --jobs "$CARGO_BUILD_JOBS"

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
