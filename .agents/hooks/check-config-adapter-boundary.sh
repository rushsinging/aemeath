#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
APPLICATION="$ROOT/agent/features/config/src/application.rs"
ADAPTERS="$ROOT/agent/features/config/src/adapters.rs"

violations=""
if [ -f "$APPLICATION" ]; then
  # Production application code must only orchestrate adapters. A terminal
  # inline #[cfg(test)] module may use filesystem/serde fixtures.
  violations=$(python3 - "$APPLICATION" <<'PY'
from pathlib import Path
import re
import sys
text = Path(sys.argv[1]).read_text()
marker = re.search(r'(?m)^\s*#\[cfg\(test\)\]\s*\n\s*mod\s+tests\s*\{', text)
production = text[:marker.start()] if marker else text
pattern = re.compile(r'tokio::fs|std::fs|read_to_string|serde_json::(?:from_|to_)')
for number, line in enumerate(production.splitlines(), 1):
    if pattern.search(line):
        print(f"{number}:{line}")
PY
  )
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
