#!/usr/bin/env bash
# 检查所有生产代码的 log target 值必须以 aemeath: 开头（或引用常量）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"

violations=$(rg 'target:\s*"([^"]*)"' \
  --type rust \
  "$ROOT" \
  -g '!packages/global/logging/src/**' \
  -g '!**/tests/**' \
  -g '!**/*test*.rs' \
  -g '!target/**' \
  | grep -v 'aemeath:' || true) # guard-registry:scope.logging.owned-targets

if [ -n "$violations" ]; then
  echo "✗ log target must start with 'aemeath:' (or use LOG_TARGET constant):" >&2
  echo "$violations" >&2
  exit 1
fi

echo "✓ log target prefix check passed"
