#!/usr/bin/env bash
# Build aemeath CLI and install to ~/.local/bin/aemeath
set -euo pipefail

cd "$(dirname "$0")"

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] PWD=$PWD"

# 构建并安装当前工作区版本。main 与 linked worktree 均允许执行，
# 避免构建脚本因工作区形态而静默跳过。

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="aemeath"

# Use the target dir resolved from cargo metadata. Resolve it from Cargo itself
# so the install path can never drift from the generated worktree config.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps \
    | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])')"

# Limit parallel rustc jobs to avoid hook-time SIGTERM on memory pressure.
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

# #1935：默认启用 composition/systemone-daemon，使安装的 aemeath 可直接
# `systemone serve`（链入 llama.cpp/GGUF 引擎，体积约 +8M）。会话侧仍走
# DaemonScoringClient + UDS，不在会话进程内加载模型。
# 需要瘦身安装时：`AEMEATH_CLI_FEATURES= ./build_cli.sh`（空串关闭默认 feature）。
DEFAULT_CLI_FEATURES="composition/systemone-daemon"
CLI_FEATURES="${AEMEATH_CLI_FEATURES-$DEFAULT_CLI_FEATURES}"
FEATURE_ARGS=()
if [[ -n "$CLI_FEATURES" ]]; then
    FEATURE_ARGS=(--features "$CLI_FEATURES")
fi
echo ">>> cargo build --release --package cli ${FEATURE_ARGS[*]:-} --jobs $CARGO_BUILD_JOBS (target-dir: $TARGET_DIR)"
  
# build.rs 不再从 git tag 读版本号；显式 AEMEATH_VERSION 未设置时，
# 本地构建使用 0.0.0-<short commit> 标识对应源码 revision。
# 如果想为本地构建指定版本号，可手动 `AEMEATH_VERSION=0.1.0 ./build_cli.sh`。
if [[ -z "${AEMEATH_VERSION:-}" ]]; then
    commit="$(git rev-parse --short=8 HEAD 2>/dev/null || true)"
    export AEMEATH_VERSION="0.0.0-${commit:-unknown}"
    echo ">>> AEMEATH_VERSION 未设置 → 二进制版本号 = ${AEMEATH_VERSION}（dev build）"
else
    echo ">>> AEMEATH_VERSION=$AEMEATH_VERSION → 二进制版本号同上"
fi
  
cargo build --release --package cli "${FEATURE_ARGS[@]}" --jobs "$CARGO_BUILD_JOBS"

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
