#!/bin/bash
# guard-registry:policy.repository.guard-selftest-crate-api
# 守卫自测（#1074 L3'）：以 fixture 迷你仓库端到端核验 check-crate-api-boundary.sh
# 的 L0/L1/L2 判定（lib.rs 内部层 pub mod 禁令、跨 crate 层段穿透拒绝、façade 符号
# 白名单），防止守卫重构后规则静默失效。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-crate-api-boundary.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.agents/hooks"  # 通过部分守卫的 AEMEATH_PROJECT_DIR 有效性检查

# ---------- fixture 迷你仓库 ----------
# context：窄 façade（四层私有 + 根符号），登记在守卫 ROOT_ACCESS_ALLOW.context 的最小子集。
mkdir -p "$TMP/agent/features/context/src/domain" "$TMP/agent/features/runtime/src/consumer" \
  "$TMP/agent/features/tools/src" "$TMP/agent/features/storage/src"
# tools/storage 的 crate-root 公开面受守卫精确核对（expected == found），
# fixture 直接复制真实 lib.rs 文本（守卫做文本解析，不编译，无需模块文件）。
cp "$SCRIPT_DIR/../../agent/features/tools/src/lib.rs" "$TMP/agent/features/tools/src/lib.rs"
cp "$SCRIPT_DIR/../../agent/features/storage/src/lib.rs" "$TMP/agent/features/storage/src/lib.rs"
printf 'mod domain;\n' >"$TMP/agent/features/context/src/lib.rs"
printf 'pub struct CompactOutcome;\npub struct SessionId;\n' >"$TMP/agent/features/context/src/domain/mod.rs"
printf 'use context::{CompactOutcome, SessionId};\n' >"$TMP/agent/features/runtime/src/consumer/legit.rs"

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP" "$GUARD" 2>&1
}

failures=0
expect_block() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard)" || status=$?
  if [ "$status" -eq 0 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[crate-api-selftest] $label 未按预期阻断 (status=$status)" >&2
    grep -F "$expected" <<<"$output" >/dev/null || echo "  期望消息未出现: $expected" >&2
    failures=$((failures + 1))
  else
    echo "[crate-api-selftest] $label: 拦截生效"
  fi
}

# ---------- 场景 ----------
# 1. L0：context lib.rs 内部层 pub mod 复活
cp "$TMP/agent/features/context/src/lib.rs" "$TMP/context-lib.bak"
printf 'pub mod domain;\n' >"$TMP/agent/features/context/src/lib.rs"
expect_block "L0 pub mod domain" "must stay private"
cp "$TMP/context-lib.bak" "$TMP/agent/features/context/src/lib.rs"

# 2. L1：跨 crate 层段穿透
printf 'use context::domain::CompactOutcome as _P;\n' >"$TMP/agent/features/runtime/src/consumer/probe.rs"
expect_block "L1 context::domain:: 穿透" "forbidden"
rm "$TMP/agent/features/runtime/src/consumer/probe.rs"

# 3. L2：未登记 façade 符号
printf 'use context::NotRegisteredSymbol as _Q;\n' >"$TMP/agent/features/runtime/src/consumer/probe.rs"
expect_block "L2 未登记符号" "forbidden"
rm "$TMP/agent/features/runtime/src/consumer/probe.rs"

# 4. L2 合法 + clean：已登记符号经 crate 根消费
if run_guard >/dev/null 2>&1; then
  echo "[crate-api-selftest] 登记符号消费 + clean 基线: pass"
else
  echo "[crate-api-selftest] 登记符号消费被误拦:" >&2
  run_guard | head -5 >&2
  failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
  echo "[crate-api-selftest] FAILED: $failures 个场景不符预期" >&2
  exit 2
fi
echo "Crate API boundary self-test passed."
