#!/bin/bash
# guard-registry:policy.repository.guard-selftest-hexagonal
# 守卫自测（#1074 L3'）：以 fixture 迷你仓库端到端核验 check-hexagonal-layer-purity.sh
# 的判定（R8 方向、COLA 防复活、update 例外、config 三层锁定），防止守卫重构后规则
# 静默失效——仓库自身 clean 时坏规则不会被发现（宪法 7 条：每层保留测试）。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-hexagonal-layer-purity.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/.agents/hooks"  # 通过守卫的 AEMEATH_PROJECT_DIR 有效性检查

# ---------- fixture 迷你仓库（clean 基线） ----------
mkdir -p "$TMP/agent/features/runtime/src/domain" \
  "$TMP/agent/features/runtime/src/adapters" \
  "$TMP/agent/features/policy/src" \
  "$TMP/agent/features/config/src/adapters" \
  "$TMP/agent/features/update/src/gateway"

printf 'mod domain;\nmod adapters;\n' >"$TMP/agent/features/runtime/src/lib.rs"
printf 'pub struct SessionId;\n' >"$TMP/agent/features/runtime/src/domain/session.rs"
printf 'pub struct ProviderAdapter;\n' >"$TMP/agent/features/runtime/src/adapters/provider.rs"

printf 'mod domain;\nmod adapters;\n' >"$TMP/agent/features/policy/src/lib.rs"
printf 'pub enum PolicyDecision { Allow }\n' >"$TMP/agent/features/policy/src/domain.rs"
# policy adapters.rs 存在时守卫执行 AllowAll-only 内容检查；空文件即合法。
: >"$TMP/agent/features/policy/src/adapters.rs"

printf 'mod domain;\nmod ports;\nmod adapters;\n' >"$TMP/agent/features/config/src/lib.rs"
printf 'pub struct ConfigUpdate;\n' >"$TMP/agent/features/config/src/domain.rs"
printf 'pub trait ConfigReader {}\n' >"$TMP/agent/features/config/src/ports.rs"
printf 'mod app_service;\n' >"$TMP/agent/features/config/src/adapters.rs"
printf 'pub struct ConfigAppService;\n' >"$TMP/agent/features/config/src/adapters/app_service.rs"

printf 'mod api;\nmod contract;\nmod gateway;\n' >"$TMP/agent/features/update/src/lib.rs"
printf 'pub struct UpdateApi;\n' >"$TMP/agent/features/update/src/api.rs"
printf 'pub struct UpdateContract;\n' >"$TMP/agent/features/update/src/contract.rs"
printf 'pub struct UpdateGateway;\n' >"$TMP/agent/features/update/src/gateway.rs"
printf 'pub fn check_version() {}\n' >"$TMP/agent/features/update/src/gateway/version.rs"

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP" "$GUARD" 2>&1
}

failures=0
expect_block() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard)" || status=$?
  if [ "$status" -eq 0 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[hexagonal-selftest] $label 未按预期阻断 (status=$status)" >&2
    grep -F "$expected" <<<"$output" >/dev/null || echo "  期望消息未出现: $expected" >&2
    failures=$((failures + 1))
  else
    echo "[hexagonal-selftest] $label: 拦截生效"
  fi
}

# ---------- 场景 ----------
# 1. R8：runtime domain 依赖 adapters
printf 'use crate::adapters::ProviderAdapter as _Probe;\n' >>"$TMP/agent/features/runtime/src/domain/session.rs"
expect_block "R8 runtime domain->adapters" "must not depend on crate::adapters"
sed -i.bak '$d' "$TMP/agent/features/runtime/src/domain/session.rs" && rm -f "$TMP/agent/features/runtime/src/domain/session.rs.bak"

# 2. R8：config ports 依赖 adapters
printf 'use crate::adapters::app_service as _Probe;\n' >>"$TMP/agent/features/config/src/ports.rs"
expect_block "R8 config ports->adapters" "must not depend on crate::adapters"
sed -i.bak '$d' "$TMP/agent/features/config/src/ports.rs" && rm -f "$TMP/agent/features/config/src/ports.rs.bak"

# 3. COLA 复活：runtime business/ 目录
mkdir -p "$TMP/agent/features/runtime/src/business"
expect_block "COLA 复活 runtime business/" "legacy COLA directory is forbidden"
rmdir "$TMP/agent/features/runtime/src/business"

# 4. update 越界：domain.rs 出现
printf '// probe\n' >"$TMP/agent/features/update/src/domain.rs"
expect_block "update 越界 domain.rs" "Update retains COLA layout beyond its registered exception"
rm "$TMP/agent/features/update/src/domain.rs"

# 5. config application 层复活
printf '// probe\n' >"$TMP/agent/features/config/src/application.rs"
expect_block "config application.rs 复活" "Config top-level source files must be"
rm "$TMP/agent/features/config/src/application.rs"

# 6. clean 基线全绿
if run_guard >/dev/null 2>&1; then
  echo "[hexagonal-selftest] clean 基线: pass"
else
  echo "[hexagonal-selftest] clean 基线被误拦:" >&2
  run_guard | head -5 >&2
  failures=$((failures + 1))
fi

if [ "$failures" -ne 0 ]; then
  echo "[hexagonal-selftest] FAILED: $failures 个场景不符预期" >&2
  exit 2
fi
echo "Hexagonal layer purity self-test passed."
