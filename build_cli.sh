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

# #1935：systemone embedded 评分引擎改为可选编译——所有平台默认不启用
# composition/systemone-daemon（llama.cpp/GGUF 引擎体积约 +8M，且 native 路径
# 仅 macOS arm64 可用，其余平台运行期 fail-closed）。需要 `systemone serve`
# 时显式开启：`AEMEATH_CLI_FEATURES="composition/systemone-daemon" ./build_cli.sh`。
DEFAULT_CLI_FEATURES=""
CLI_FEATURES="${AEMEATH_CLI_FEATURES-$DEFAULT_CLI_FEATURES}"
# features 用单字符串承载而非数组：macOS 自带的 bash 3.2 在 `set -u` 下展开空数组
# （`"${ARGS[@]}"`）会报 unbound variable，默认无 features 时构建必然失败。
FEATURE_ARGS=""
if [[ -n "$CLI_FEATURES" ]]; then
    FEATURE_ARGS="--features $CLI_FEATURES"
fi
echo ">>> cargo build --release --package cli ${FEATURE_ARGS} --jobs $CARGO_BUILD_JOBS (target-dir: $TARGET_DIR)"
  
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
  
# shellcheck disable=SC2086  # FEATURE_ARGS 为空或 `--features a,b`，需要有意分词
cargo build --release --package cli ${FEATURE_ARGS} --jobs "$CARGO_BUILD_JOBS"

mkdir -p "$INSTALL_DIR"
# 运行中的 aemeath 进程持有旧 inode，直接 cp 覆盖目标会报 ETXTBSY（文本文件忙）
# 并以非零退出结束脚本。先写同目录临时文件再原子 rename：正在运行的旧二进制
# 不受影响，新启动的进程使用新版本。
INSTALL_TMP="$INSTALL_DIR/.$BIN_NAME.tmp"
cp "$TARGET_DIR/release/$BIN_NAME" "$INSTALL_TMP"
chmod 755 "$INSTALL_TMP"
mv -f "$INSTALL_TMP" "$INSTALL_DIR/$BIN_NAME"

# macOS Gatekeeper kills freshly-copied ad-hoc signed binaries on some
# versions due to `com.apple.provenance` xattr + signature mismatch after
# the move. Strip attrs and re-sign ad-hoc so the binary is runnable.
if [[ "$(uname)" == "Darwin" ]]; then
    xattr -cr "$INSTALL_DIR/$BIN_NAME" 2>/dev/null || true
    codesign --force --sign - "$INSTALL_DIR/$BIN_NAME" 2>/dev/null || true
fi

echo ">>> installed: $INSTALL_DIR/$BIN_NAME ($(du -h "$INSTALL_DIR/$BIN_NAME" | cut -f1))"
