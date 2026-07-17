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
  "Provider driver parsing and protocol selection must remain inside the Provider-owned ACL." \
  grep -RInE 'ProviderDriverKind::parse|OpenAIProviderConfig|ProtocolFamily|DriverSpec' \
  agent/features/runtime/src agent/composition/src apps/cli/src --include='*.rs'

report \
  "Provider implementation config must not escape through the crate-root façade." \
  grep -nE 'pub use .*OpenAIProviderConfig' agent/features/provider/src/lib.rs

if ! grep -q 'DriverSpec::parse(&options.driver' agent/features/provider/src/adapters/client.rs; then
  echo 'agent/features/provider/src/adapters/client.rs: LlmClient::from_config must parse DriverSpec inside Provider' >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 2
fi

echo "Provider driver ACL guard OK."
