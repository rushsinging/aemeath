#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

LOGGING_SRC="$ROOT/packages/global/logging/src"
STATIC_ALLOW='^(adapters/context\.rs:(SCOPED_CONTEXT|LEGACY_EXECUTION_CONTEXT|BOOT_TS|APP_VERSION|PID|TEST_LOCK)|adapters/file_sink\.rs:(UNKNOWN_TARGET_REPORTS|LOGGER))$'

static_declarations="$(python3 - "$LOGGING_SRC" <<'PY'
import pathlib
import re
import sys
root = pathlib.Path(sys.argv[1])
pattern = re.compile(r'(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?static(?:\s+mut)?\s+([A-Z][A-Z0-9_]*)\s*:')
for path in root.rglob('*.rs'):
    if 'test' in path.name:
        continue
    for name in pattern.findall(path.read_text()):
        print(f'{path.relative_to(root)}:{name}')
PY
)"

static_violations="$(printf '%s\n' "$static_declarations" | grep -vE "$STATIC_ALLOW" || true)"
macro_violations="$(grep -RInE '(lazy_static!|thread_local!)[[:space:]]*\{' "$LOGGING_SRC" --include='*.rs' --exclude='*test*.rs' || true)"

if [ -n "$static_violations" ] || [ -n "$macro_violations" ]; then
  [ -z "$static_violations" ] || printf '%s\n' "$static_violations" >&2
  [ -z "$macro_violations" ] || printf '%s\n' "$macro_violations" >&2
  echo "Logging scope context guard FAILED: unregistered process-global or thread-local state is forbidden; use LogContext task-local scope or register proven process metadata." >&2
  exit 2
fi

echo "Logging scope context guard passed."
