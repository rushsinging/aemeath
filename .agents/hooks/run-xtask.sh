#!/usr/bin/env bash
# Hook 入口：优先复用已构建的 xtask 二进制，避免 `cargo run -p xtask`
# 在 Stop/pre-commit 路径上因 workspace dirty / 并发编译触发超时。
#
# 新鲜度策略：只要 debug/xtask 可执行就直接用。NEVER 用源码 mtime
# 触发自动 rebuild——checkout/rebase 会 touch 依赖 crate，导致 hook
# 路径上 `cargo build -p xtask` 与其它 worktree 争锁并挂起。需要新
# 二进制时由开发者显式 `cargo build -p xtask`（或本脚本在缺失时构建一次）。
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-}"
if [ -z "$ROOT" ] || [ ! -d "$ROOT/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi
cd "$ROOT"

resolve_target_dir() {
  if command -v cargo >/dev/null 2>&1; then
    cargo metadata --format-version 1 --no-deps 2>/dev/null \
      | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])'
  else
    return 1
  fi
}

TARGET_DIR="$(resolve_target_dir || true)"
if [ -z "${TARGET_DIR:-}" ]; then
  # 门禁 fixture / 无 Cargo workspace：回退 cargo run，便于 PATH stub 拦截。
  exec cargo run --quiet -p xtask -- "$@"
fi

XTASK_BIN="$TARGET_DIR/debug/xtask"

if [ ! -x "$XTASK_BIN" ]; then
  cargo build --quiet -p xtask
fi

if [ ! -x "$XTASK_BIN" ]; then
  exec cargo run --quiet -p xtask -- "$@"
fi

exec "$XTASK_BIN" "$@"
