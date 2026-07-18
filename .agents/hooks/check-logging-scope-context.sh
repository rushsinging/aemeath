#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

LOGGING_SRC="$ROOT/packages/global/logging/src"
RUNTIME_SRC="$ROOT/agent/features/runtime/src"
PROVIDER_SRC="$ROOT/agent/features/provider/src"
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

# guard-registry:scope.logging.registered-process-statics
static_violations="$(printf '%s\n' "$static_declarations" | grep -vE "$STATIC_ALLOW" || true)"
# guard-registry:scope.logging.scope-context-tests
macro_violations="$(grep -RInE '(lazy_static!|thread_local!)[[:space:]]*\{' "$LOGGING_SRC" --include='*.rs' --exclude='*test*.rs' || true)"

if [ -n "$static_violations" ] || [ -n "$macro_violations" ]; then
  [ -z "$static_violations" ] || printf '%s\n' "$static_violations" >&2
  [ -z "$macro_violations" ] || printf '%s\n' "$macro_violations" >&2
  echo "Logging scope context guard FAILED: unregistered process-global or thread-local state is forbidden; use LogContext task-local scope or register proven process metadata." >&2
  exit 2
fi

# guard-registry:scope.logging.scope-context-tests
legacy_consumers="$(grep -RInE '\b(set_session_id|set_current_(chat_id|turn|model|provider|request_id|role))[[:space:]]*\(' "$RUNTIME_SRC" "$PROVIDER_SRC" --include='*.rs' --exclude='*test*.rs' | grep -vE '/tests?/' || true)"
if [ -n "$legacy_consumers" ]; then
  printf '%s\n' "$legacy_consumers" >&2
  echo "Logging scope context guard FAILED: Runtime/Provider production code must use immutable LogContext scopes, not legacy process-global setters (qualified, imported, or wrapped)." >&2
  exit 2
fi

for file in \
  "$RUNTIME_SRC/application/client/trait_chat.rs" \
  "$RUNTIME_SRC/application/chat/looping/main_run_port.rs"; do
  if grep -nE 'tokio::spawn[[:space:]]*\(' "$file" >/dev/null; then
    grep -nE 'tokio::spawn[[:space:]]*\(' "$file" >&2 || true
    echo "Logging scope context guard FAILED: controlled Runtime production tasks must use logging::spawn_instrumented." >&2
    exit 2
  fi
done

for file in \
  "$RUNTIME_SRC/application/chat/looping/agent_calls.rs" \
  "$RUNTIME_SRC/application/chat/looping/non_agent.rs"; do
  production_prefix="$(awk '/^#\[cfg\(test\)\]/{exit} {print}' "$file")"
  if printf '%s\n' "$production_prefix" | grep -nE 'tokio::spawn[[:space:]]*\(' >/dev/null; then
    printf '%s\n' "$production_prefix" | grep -nE 'tokio::spawn[[:space:]]*\(' >&2 || true
    echo "Logging scope context guard FAILED: controlled Runtime production tasks must use logging::spawn_instrumented." >&2
    exit 2
  fi
done

if ! grep -q 'logging::capture()' "$PROVIDER_SRC/adapters/stream.rs" || \
   ! grep -q 'logging::instrument' "$PROVIDER_SRC/adapters/stream.rs"; then
  echo "Logging scope context guard FAILED: Provider blocking stream bridge must capture and instrument opaque LogContext." >&2
  exit 2
fi

echo "Logging scope context guard passed."
