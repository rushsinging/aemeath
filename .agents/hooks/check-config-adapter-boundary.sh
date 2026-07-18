#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
APPLICATION="$ROOT/agent/features/config/src/application.rs"
ADAPTERS="$ROOT/agent/features/config/src/adapters.rs"

violations=""
if [ -f "$APPLICATION" ]; then
  set +e
  violations=$(python3 "$ROOT/.agents/hooks/check-config-adapter-boundary.py" "$APPLICATION")
  scan_status=$?
  set -e
  if [ "$scan_status" -ne 0 ] && [ "$scan_status" -ne 2 ]; then
    echo '{"decision":"block","reason":"Config adapter boundary scanner failed to execute."}'
    exit 2
  fi
fi
stubs=""
if [ -f "$ADAPTERS" ]; then
  stubs=$(grep -nE 'TODO:.*(adapter|FileAdapter|CliArgsAdapter|Claude)|Placeholder|pub fn read\([^)]*\).*ConfigPatch::default' "$ADAPTERS" || true)
fi

if [ -n "$violations$stubs" ]; then
  echo '{"decision":"block","reason":"Config application must only orchestrate adapters; direct fs/JSON parsing or adapter stubs are forbidden."}'
  [ -z "$violations" ] || echo "$violations"
  [ -z "$stubs" ] || echo "$stubs"
  exit 2
fi

echo "Config adapter boundary guard OK."
