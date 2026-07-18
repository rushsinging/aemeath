#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

fail=0
LOGGING="$ROOT/packages/global/logging/src"
RUNTIME="$ROOT/agent/features/runtime/src"
APP="$ROOT/agent/composition/src/app.rs"

logging_env="$(grep -RInE '(std::env::|use[[:space:]]+std::env|env::(var|var_os)[[:space:]]*\()' "$LOGGING" --include='*.rs' --exclude='*test*.rs' || true)"
if [ -n "$logging_env" ]; then
  printf '%s\n' "$logging_env" >&2
  echo "Logging settings injection guard FAILED: Logging production code must not read env; use Composition-mapped LoggingSettings." >&2
  fail=1
fi

runtime_wiring="$(grep -RInE '(UnifiedLogger|LoggingSettings|init_logging[[:space:]]*\(|AEMEATH_LOG_STDERR|use[[:space:]]+logging[^;]*[[:space:]]+as[[:space:]])' "$RUNTIME" --include='*.rs' --exclude='*test*.rs' | grep -vE ':[0-9]+:[[:space:]]*//' || true)"
if [ -n "$runtime_wiring" ]; then
  printf '%s\n' "$runtime_wiring" >&2
  echo "Logging settings injection guard FAILED: Runtime must not assemble or initialize Logging." >&2
  fail=1
fi

composition_runtime_call="$(grep -RIn 'runtime::from_args_with_workspace' "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" --include='*.rs' --exclude='*test*.rs' || true)"
if [ "$(printf '%s\n' "$composition_runtime_call" | grep -c . || true)" -ne 1 ] || [[ "$composition_runtime_call" != *"agent/composition/src/runtime.rs"* ]]; then
  printf '%s\n' "$composition_runtime_call" >&2
  echo "Runtime workspace bootstrap must have exactly one production consumer in Composition runtime.rs." >&2
  fail=1
fi

all_init_imports="$(grep -RIlE 'use[[:space:]]+logging::[^;]*\bUnifiedLogger\b' "$ROOT/agent" "$ROOT/apps" "$ROOT/packages" --include='*.rs' --exclude='*test*.rs' | sed "s#^$ROOT/##" || true)"
if [ "$all_init_imports" != "agent/composition/src/app.rs" ]; then
  printf '%s\n' "$all_init_imports" >&2
  echo "Logging settings injection guard FAILED: UnifiedLogger may only be imported by Composition app.rs." >&2
  fail=1
fi

init_count="$(grep -Ec 'UnifiedLogger[[:space:]]*::[[:space:]]*init[[:space:]]*\(' "$APP" || true)"
if [ "$init_count" -ne 1 ]; then
  echo "Logging settings injection guard FAILED: app.rs must contain exactly one UnifiedLogger::init expression." >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Logging settings injection guard passed."
