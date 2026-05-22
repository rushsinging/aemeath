#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

"$ROOT/.agents/hooks/check-rust-file-lines.sh"
"$ROOT/scripts/check-tui-tea-purity.sh"
"$ROOT/.agents/hooks/check-unsafe-text-ops.sh"

echo "All architecture guards passed."
