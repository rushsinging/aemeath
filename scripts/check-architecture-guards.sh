#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$ROOT/scripts/check-rust-file-lines.sh"
"$ROOT/scripts/check-tui-tea-purity.sh"
"$ROOT/scripts/check-unsafe-text-ops.sh"

echo "All architecture guards passed."
