#!/usr/bin/env bash
# Guard: ConfigAppService 只能由 Config crate 内部与 Composition 构造；Runtime/TUI/CLI 禁止 new。
# 同时禁止 TUI/CLI 持有 Config-owned reader/query/writer/participant/watch 类型。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

violations=$(grep -rn "ConfigAppService::new" \
  "$ROOT/agent/features/runtime/src/" "$ROOT/apps/cli/src/" \
  --include="*.rs" 2>/dev/null | \
# guard-registry:scope.config.reader-tests-and-reflection
  grep -v "_test" | \
# guard-registry:scope.config.reader-tests-and-reflection
  grep -v "tests" | \
  grep -v "trait_reflection.rs" || true) # guard-registry:scope.config.reader-tests-and-reflection

contract_leaks=$(grep -RInE '\b(ConfigReader|ConfigQuery|ConfigWriter|ProjectConfigParticipant|ConfigSubscription|watch::Receiver<ConfigSnapshot>)\b' \
  "$ROOT/apps/cli/src" 2>/dev/null || true)

if [ -n "$violations$contract_leaks" ]; then
  echo "Config reader injection guard FAILED" >&2
  [ -z "$violations" ] || echo "$violations" >&2
  [ -z "$contract_leaks" ] || echo "$contract_leaks" >&2
  exit 2
fi

echo "Config reader injection guard OK."
