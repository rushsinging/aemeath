#!/usr/bin/env bash
set -euo pipefail

readonly REQUIRED_LLVM_COV_VERSION="0.8.7"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$ROOT"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "error: cargo-llvm-cov $REQUIRED_LLVM_COV_VERSION is required" >&2
    exit 1
fi

installed_version="$(cargo llvm-cov --version | awk '{print $2}')"
if [[ "$installed_version" != "$REQUIRED_LLVM_COV_VERSION" ]]; then
    echo "error: cargo-llvm-cov $REQUIRED_LLVM_COV_VERSION is required (found $installed_version)" >&2
    exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/coverage}"
export CARGO_LLVM_COV_TARGET_DIR="${CARGO_LLVM_COV_TARGET_DIR:-$CARGO_TARGET_DIR/llvm-cov-target}"
export CARGO_LLVM_COV_BUILD_DIR="${CARGO_LLVM_COV_BUILD_DIR:-$CARGO_TARGET_DIR/llvm-cov-build}"
# llvm-cov 默认并行度 = host CPU 数（16），并行编译 + 并行跑全部 crate 测试
# 导致 runner 内存/资源峰值，在测试运行中段取消 job（SIGTERM +
# "The operation was canceled"，实测 CI 必现、本地不现）。`-j 2` 是 llvm-cov
# 自身的并行参数（CARGO_BUILD_JOBS 不影响其测试并行），将编译与测试并行度
# 限制为 2；配合 RUST_TEST_THREADS=1 降低单进程内线程峰值。
export RUST_TEST_THREADS=1

report_json="$(mktemp "${TMPDIR:-/tmp}/aemeath-coverage.XXXXXX.json")"
trap 'rm -f "$report_json"' EXIT

cargo llvm-cov \
    -j 2 \
    --workspace \
    --exclude xtask \
    --quiet \
    --json \
    --summary-only \
    --output-path "$report_json"

cargo run --quiet -p xtask -- coverage-summary "$report_json" "$ROOT"
