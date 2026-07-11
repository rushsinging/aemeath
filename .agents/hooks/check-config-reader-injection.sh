#!/usr/bin/env bash
# Guard: runtime 消费方不得直接 ConfigAppService::new（应通过注入的 Arc<dyn ConfigReader>）
#
# 例外：
#   - from_args.rs / trait_model.rs（CLI 启动路径，暂未改造注入）
#   - 测试文件（_test / tests）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

violations=$(grep -rn "ConfigAppService::new" \
  "$ROOT/agent/features/runtime/src/" \
  --include="*.rs" 2>/dev/null | \
  grep -v "from_args.rs" | \
  grep -v "trait_model.rs" | \
  grep -v "_test" | \
  grep -v "tests" || true)

if [ -n "$violations" ]; then
  echo "❌ Config reader injection guard FAILED: runtime consumer directly new-ing ConfigAppService"
  echo "$violations"
  exit 1
fi

echo "Config reader injection guard OK."
