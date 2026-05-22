#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
HOOKS_DIR="$ROOT/.agents/hooks"

"$HOOKS_DIR/check-rust-file-lines.sh"
"$HOOKS_DIR/check-tui-tea-purity.sh"
"$HOOKS_DIR/check-unsafe-text-ops.sh"

echo "All architecture guards passed."
