#!/usr/bin/env bash
# guard-registry:context.compact-continuation-checkpoint
set -euo pipefail

ROOT="${AEMEATH_GUARD_ROOT:-${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}}"
fail=0

report() {
  local message="$1"
  shift
  local output
  output="$(mktemp)"
  "$@" >"$output" || true
  if [ -s "$output" ]; then
    cat "$output" >&2
    echo "[compact-continuation] $message" >&2
    fail=1
  fi
  rm -f "$output"
}

check_root() {
  local root="$1"
  local context_adapter="$root/agent/features/context/src/adapters/compact_summary.rs"
  local runtime="$root/agent/features/runtime/src"
  [ -f "$context_adapter" ] && report \
    "previous checkpoint must not use authoritative head/tail slicing" \
    grep -nE 'slice_(head|tail)\([^,]*(previous_summary|previous_checkpoint)' "$context_adapter"
  [ -d "$runtime" ] && report \
    "Runtime must not own compact continuation summary state" \
    grep -RInE '\b(active_summary|continuation_(checkpoint|summary))[[:space:]]*:' "$runtime" --include='*.rs' --exclude='*_tests.rs' # guard-registry:scope.runtime.compact-continuation-tests
  [ -d "$runtime" ] && report \
    "Runtime must not assemble checkpoint section headings" \
    grep -RInE '## (Immutable Constraints|Resume Cursor|Continuation Status)' "$runtime" --include='*.rs' --exclude='*_tests.rs' # guard-registry:scope.runtime.compact-continuation-tests
  return "$fail"
}

self_test() {
  local fixture
  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' RETURN
  mkdir -p "$fixture/agent/features/context/src/adapters" "$fixture/agent/features/runtime/src"
  printf '%s\n' 'slice_tail(previous_summary, 10);' >"$fixture/agent/features/context/src/adapters/compact_summary.rs"
  if check_root "$fixture" >/dev/null 2>&1; then echo "slice probe was not rejected" >&2; return 1; fi
  fail=0
  printf '%s\n' 'normalize_to_budget(previous_summary);' >"$fixture/agent/features/context/src/adapters/compact_summary.rs"
  printf '%s\n' 'struct Owner { active_summary: String }' >"$fixture/agent/features/runtime/src/owner.rs"
  if check_root "$fixture" >/dev/null 2>&1; then echo "owner probe was not rejected" >&2; return 1; fi
  fail=0
  printf '%s\n' 'fn render() { let _ = "## Resume Cursor"; }' >"$fixture/agent/features/runtime/src/owner.rs"
  if check_root "$fixture" >/dev/null 2>&1; then echo "heading probe was not rejected" >&2; return 1; fi
  fail=0
  printf '%s\n' 'fn consume(window: ContextWindow) { drop(window); }' >"$fixture/agent/features/runtime/src/owner.rs"
  check_root "$fixture"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
else
  check_root "$ROOT"
fi
