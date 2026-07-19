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
  "provider invocation state must stay immutable; use InvocationScope instead of atomics or runtime setters." \
  grep -RInE 'Atomic(U32|U8|Bool)|set_(max_tokens|reasoning_level)|current_reasoning_level|reasoning_config\.lock\(' \
  agent/features/provider/src --include='*.rs' --exclude-dir=tests # guard-registry:scope.provider.invocation-tests

report \
  "runtime must not restore shared-client mutation or serialization locks." \
  grep -RInE 'shared_client_lock|set_(max_tokens|reasoning_level)|current_reasoning_level' \
  agent/features/runtime/src --include='*.rs'

if ! grep -q 'scope: &InvocationScope' agent/features/provider/src/ports.rs; then
  echo 'agent/features/provider/src/ports.rs: LlmProvider::invocation_stream must require &InvocationScope' >&2
  fail=1
fi

# #907: Provider 内部保留 LlmProvider trait 作为 driver 实现契约；
#       对外只经 provider::composition 暴露构造面、经 Published Language 暴露调用语义。
if ! grep -qE 'pub trait LlmProvider' agent/features/provider/src/ports.rs; then
  echo 'agent/features/provider/src/ports.rs: Provider must keep internal LlmProvider trait' >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Provider immutable invocation scope guard OK."
