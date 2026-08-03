#!/usr/bin/env bash
set -euo pipefail

readonly REQUIRED_LLVM_COV_VERSION="0.8.7"
readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$ROOT"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "error: cargo-llvm-cov $REQUIRED_LLVM_COV_VERSION is required" >&2
    echo "install: cargo install cargo-llvm-cov --version $REQUIRED_LLVM_COV_VERSION --locked" >&2
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

report_json="$(mktemp "${TMPDIR:-/tmp}/aemeath-coverage.XXXXXX.json")"
trap 'rm -f "$report_json"' EXIT

cargo llvm-cov \
    --workspace \
    --exclude xtask \
    --quiet \
    --json \
    --summary-only \
    --output-path "$report_json"

cargo run --quiet -p xtask -- coverage-summary "$report_json" "$ROOT"
