#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
# 守卫：如果 AEMEATH_PROJECT_DIR 不包含 .agents/hooks 说明不是项目根目录，
# 回退到 BASH_SOURCE 推导
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi
HOOKS_DIR="$ROOT/.agents/hooks"

"$HOOKS_DIR/check-cargo-dependency-graph.sh"
"$HOOKS_DIR/check-cli-thin-entry.sh"
"$HOOKS_DIR/check-share-no-upstream-deps.sh"
"$HOOKS_DIR/check-cola-layer-purity.sh"
"$HOOKS_DIR/check-forbidden-imports.sh"
"$HOOKS_DIR/check-rust-file-lines.sh"
"$HOOKS_DIR/check-tui-tea-purity.sh"
"$HOOKS_DIR/check-unsafe-text-ops.sh"

echo "All architecture guards passed."
