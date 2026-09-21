#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-cross-bc-construction-registry.sh"

if [ ! -x "$GUARD" ]; then
  echo '[cross-bc-construction-registry] guard is missing' >&2
  exit 3
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/.agents/hooks" \
  "$TMP/agent/features/storage/src/adapters" \
  "$TMP/agent/features/memory/src/adapters" \
  "$TMP/agent/features/runtime/src/application" \
  "$TMP/agent/composition/src" \
  "$TMP/apps/cli/src"

REGISTRY='{"construction_symbols":[
{"id":"construction.storage.FileSystemDatasetAdapter","symbol":"FileSystemDatasetAdapter","owner_crate":"storage","kind":"adapter","allowed_paths":["agent/composition/src"],"guard":"check-cross-bc-construction-registry.sh","reason":"probe","tracking_issue":1067},
{"id":"construction.project.wire_production_workspace","symbol":"wire_production_workspace","owner_crate":"project","kind":"wire","allowed_paths":["agent/composition/src"],"guard":"check-cross-bc-construction-registry.sh","reason":"probe","tracking_issue":1067}
]}'
printf '%s' "$REGISTRY" >"$TMP/.agents/architecture-guard-registry.json"

# feature crate 的 adapters 模块：定义受保护 adapter 与未登记 adapter（fail-closed 探针）
cat >"$TMP/agent/features/storage/src/adapters.rs" <<'RS'
pub mod adapters {
    pub struct FileSystemDatasetAdapter;
    pub struct TapeArchiveAdapter;
}
RS

mkdir -p "$TMP/agent/features/storage/src"
cat >"$TMP/agent/features/storage/src/lib.rs" <<'RS'
pub mod adapters;
RS

# project crate 定义 wire 函数
mkdir -p "$TMP/agent/features/project/src/adapters"
cat >"$TMP/agent/features/project/src/adapters/wiring.rs" <<'RS'
pub fn wire_production_workspace(root: std::path::PathBuf) -> usize { root.iter().count() }
RS
cat >"$TMP/agent/features/project/src/adapters.rs" <<'RS'
pub mod wiring;
RS
cat >"$TMP/agent/features/project/src/lib.rs" <<'RS'
pub mod adapters;
RS

cat >"$TMP/agent/composition/src/runtime.rs" <<'RS'
fn assemble() { let _ = storage::adapters::adapters::FileSystemDatasetAdapter; }
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[cross-bc-construction-registry] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

# 1. 基线 clean pass
run_guard >/dev/null

# 2. fail-closed：未登记 adapter 在 runtime 生产段构造 → exit 2
cat >"$TMP/agent/features/runtime/src/application/bootstrap.rs" <<'RS'
fn bad() { let _probe = memory::adapters::TapeArchiveAdapter::new(); }
RS
expect_failure unregistered-adapter "unregistered cross-BC adapter construction 'TapeArchiveAdapter'"
rm "$TMP/agent/features/runtime/src/application/bootstrap.rs"

# 3. 已登记符号越界：runtime 构造 FileSystemDatasetAdapter → exit 2
cat >"$TMP/agent/features/runtime/src/application/bootstrap.rs" <<'RS'
fn bad() { let _probe = storage::adapters::adapters::FileSystemDatasetAdapter::new(); }
RS
expect_failure out-of-allowlist "outside allowed paths"
rm "$TMP/agent/features/runtime/src/application/bootstrap.rs"

# 4. cfg(test) 段内的构造不拦截（测试支持合法）
cat >"$TMP/agent/features/runtime/src/application/bootstrap.rs" <<'RS'
#[cfg(any(test, feature = "dev"))]
pub mod test_support {
    pub fn probe() { let _ = storage::adapters::adapters::FileSystemDatasetAdapter::new(); }
}
RS
run_guard >/dev/null
rm "$TMP/agent/features/runtime/src/application/bootstrap.rs"

# 5. wire 越界：runtime 调 project::wire_production_workspace → exit 2
cat >"$TMP/agent/features/runtime/src/application/bootstrap.rs" <<'RS'
fn bad() { let _ = project::wire_production_workspace(std::path::PathBuf::from("/tmp")); }
RS
expect_failure wire-out-of-allowlist "wire call 'project…::wire_production_workspace' outside allowed paths"
rm "$TMP/agent/features/runtime/src/application/bootstrap.rs"

# 6. registry 缺 construction_symbols 段 → exit 2
printf '%s' '{"entries":[]}' >"$TMP/.agents/architecture-guard-registry.json"
expect_failure missing-section "construction_symbols section is missing"
printf '%s' "$REGISTRY" >"$TMP/.agents/architecture-guard-registry.json"

# 7. 恢复后 clean pass
run_guard >/dev/null

echo 'Cross-BC construction registry guard sanity checks passed.'
