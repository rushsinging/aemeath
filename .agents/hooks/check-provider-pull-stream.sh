#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

fail=0
report() {
  local message="$1"
  shift
  local output
  output="$("$@" 2>/dev/null || true)"
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

# #907: Provider 内部也必须零引用旧 sink / 回调契约；pull stream 是唯一出站语义。
report \
  "Provider internals must hold zero references to legacy decoder sinks or callback handlers." \
  grep -RInE '\b(LegacyStreamSink|legacy_stream_message|CallbackHandler|StreamHandler|RuntimeStreamHandler)\b|stream_message_raw|\.stream_message\(' \
    agent/features/provider/src --include='*.rs'

report \
  "Runtime and Context production paths and test doubles must consume InvocationStream instead of legacy decoder sinks." \
  grep -RInE '\b(LegacyStreamSink|legacy_stream_message)\b' \
    agent/features/runtime/src agent/features/context/src --include='*.rs'

if ! grep -qE '^[[:space:]]*async fn invocation_stream[[:space:]]*\(' agent/features/provider/src/ports.rs; then
  echo 'agent/features/provider/src/ports.rs: LlmProvider must expose invocation_stream' >&2
  fail=1
fi

report \
  "Provider-private InvocationSink must not escape into Runtime or Context." \
  grep -RInE '\bInvocationSink\b' \
    agent/features/runtime/src agent/features/context/src --include='*.rs'

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Provider pull-stream callback retirement guard OK."
