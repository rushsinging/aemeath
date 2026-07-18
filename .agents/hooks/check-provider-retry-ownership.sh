#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
PROVIDER="$ROOT/agent/features/provider/src/adapters"
FAIL=0

# Only the pull-stream production prefix is in scope; legacy callback methods are retired by #907.
scan_prefix() {
  local file="$1"
  local marker="$2"
  awk -v marker="$marker" 'index($0, marker) { exit } { print FNR ":" $0 }' "$file"
}

check_pattern() {
  local message="$1"
  local pattern="$2"
  local fail=0
  local file marker output
  while [ "$#" -gt 2 ]; do
    file="$3"
    marker="$4"
    shift 2
    output="$(scan_prefix "$file" "$marker" | grep -E "$pattern" || true)"
    if [ -n "$output" ]; then
      printf '%s:%s\n' "$file" "$output" >&2
      fail=1
    fi
  done
  if [ "$fail" -ne 0 ]; then
    printf '[provider-retry-ownership] %s\n' "$message" >&2
    FAIL=1
  fi
}

check_pattern \
  "Provider pull-stream production code must not own retry loops or backoff sleeps." \
  'for attempt in 0\.\.self\.max_retries|tokio::time::sleep\(delay\)' \
  "$PROVIDER/anthropic.rs" 'async fn legacy_stream_message' \
  "$PROVIDER/ollama.rs" 'async fn legacy_stream_message' \
  "$PROVIDER/openai_compatible/request_body.rs" 'async fn legacy_stream_message'

check_pattern \
  "Provider pull-stream production code must not fall back to non-stream requests." \
  'FallbackPlanned|return send_message_non_stream|\.send_message_non_stream\(' \
  "$PROVIDER/anthropic.rs" 'async fn legacy_stream_message' \
  "$PROVIDER/ollama.rs" 'async fn legacy_stream_message' \
  "$PROVIDER/openai_compatible/request_body.rs" 'async fn legacy_stream_message'

if [ "$FAIL" -ne 0 ]; then
  exit 2
fi

echo "Provider retry/fallback ownership guard passed."
