#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
PROVIDER="$ROOT/agent/features/provider/src"
FAIL=0

report_matches() {
  local message="$1"
  shift
  local output
  output="$("$@" 2>/dev/null || true)"
  if [ -n "$output" ]; then
    printf '%s\n' "$output" >&2
    printf '[provider-usage-capability] %s\n' "$message" >&2
    FAIL=1
  fi
}

# Pull-stream usage extraction must preserve missing fields. Legacy parsers may remain until #907.
report_matches \
  'RawUsage pull parsing must not coerce missing values to zero or truncate u64 with `as u32`.' \
  grep -nE 'unwrap_or\(0\)[[:space:]]+as[[:space:]]+u32|\.map\(\|v\|[[:space:]]*v[[:space:]]+as[[:space:]]+u32\)' \
  "$PROVIDER/adapters/stream.rs"
# OpenAI-compatible driver maxima must derive from reasoning_capability, not per-driver overrides.
max_override_count="$(grep -c 'fn max_reasoning_level' "$PROVIDER/adapters/openai_compatible/driver.rs" || true)"
if [ "$max_override_count" -ne 1 ]; then
  printf '[provider-usage-capability] expected exactly one default max_reasoning_level derived from capability, found %s\n' "$max_override_count" >&2
  FAIL=1
fi

clamp_count="$(grep -c 'fn clamp_effort' "$PROVIDER/adapters/openai_compatible/driver.rs" || true)"
if [ "$clamp_count" -ne 1 ]; then
  printf '[provider-usage-capability] expected exactly one capability-derived clamp_effort implementation, found %s\n' "$clamp_count" >&2
  FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
  exit 2
fi

echo 'Provider usage/capability guard passed.'
