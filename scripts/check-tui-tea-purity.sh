#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
exec "$ROOT/.agents/hooks/check-tui-tea-purity.sh" "$@"
