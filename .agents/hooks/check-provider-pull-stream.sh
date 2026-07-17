#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

fail=0
report() {
  local message="$1"
  shift
  local output
  output="$($@ 2>/dev/null || true)"
  if [ -n "$output" ]; then
    printf '%s\n' "$output" >&2
    printf '[architecture] %s\n' "$message" >&2
    fail=1
  fi
}

report \
  "retired Provider/Runtime callback contracts must not return to production code." \
  grep -RInE '\b(CallbackHandler|StreamHandler|RuntimeStreamHandler)\b|stream_message_raw|\.stream_message\(' \
  agent/features/provider/src agent/features/runtime/src agent/features/context/src --include='*.rs'

report \
  "Runtime and Context production paths must consume InvocationStream instead of legacy decoder sinks." \
  bash -c "grep -RInE '\\b(LegacyStreamSink|legacy_stream_message)\\b' agent/features/runtime/src agent/features/context/src --include='*.rs' --exclude='*_tests.rs' | grep -vE '/tests?\\.rs:'"

if ! grep -q 'async fn invocation_stream' agent/features/provider/src/ports.rs; then
  echo 'agent/features/provider/src/ports.rs: LlmProvider must expose invocation_stream' >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Provider pull-stream callback retirement guard OK."
