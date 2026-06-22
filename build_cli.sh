#!/usr/bin/env bash
# Build aemeath CLI and install to ~/.local/bin/aemeath
set -euo pipefail

cd "$(dirname "$0")"

# Set per-branch CARGO_TARGET_DIR to keep worktree builds isolated in ~/.cache.
if [ -f ".cargo/set-target.sh" ]; then
    source ".cargo/set-target.sh"
fi

echo "[hook-env] AEMEATH_PROJECT_DIR=${AEMEATH_PROJECT_DIR:-<unset>}"
echo "[hook-env] CLAUDE_PROJECT_DIR=${CLAUDE_PROJECT_DIR:-<unset>}"
echo "[hook-env] PWD=$PWD"

# 仅在主仓库（main 工作区）构建并安装：worktree 中跳过 build/install，
# 避免把未合并分支的二进制装成全局工具污染日常使用的 ~/.local/bin/aemeath。
# 用 git 原生检测 linked worktree（absolute-git-dir ≠ git-common-dir）。
abs_git_dir="$(git rev-parse --absolute-git-dir 2>/dev/null || true)"
abs_common_dir="$(cd "$(git rev-parse --git-common-dir 2>/dev/null)" 2>/dev/null && pwd || true)"
if [[ -n "$abs_git_dir" && -n "$abs_common_dir" && "$abs_git_dir" != "$abs_common_dir" ]]; then
    echo ">>> 在 git worktree 中，跳过 build/install（仅在 main 主工作区构建）"
    exit 0
fi

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BIN_NAME="aemeath"

# Use the target dir resolved from cargo metadata (set by .cargo/set-target.sh
# or by CARGO_TARGET_DIR from the caller). Resolve it from cargo itself so the
# install path can never drift from the actual build output.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps \
    | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])')"

# Limit parallel rustc jobs to avoid hook-time SIGTERM on memory pressure.
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

echo ">>> cargo build --release --package cli --jobs $CARGO_BUILD_JOBS (target-dir: $TARGET_DIR)"

# build.rs 不再从 git tag 读版本号；显式 AEMEATH_VERSION 未设置时
# 二进制内嵌的是 Cargo.toml 占位符 0.0.0（dev build 标识）。
# 如果想为本地构建打某个版本号，可手动 `AEMEATH_VERSION=0.1.0 ./build_cli.sh`。
if [[ -z "${AEMEATH_VERSION:-}" ]]; then
    echo ">>> AEMEATH_VERSION 未设置 → 二进制版本号 = 0.0.0（dev build）"
else
    echo ">>> AEMEATH_VERSION=$AEMEATH_VERSION → 二进制版本号同上"
fi

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
