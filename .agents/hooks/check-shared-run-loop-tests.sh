#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -R "$ROOT/." "$TMP/repo"
probe="$TMP/repo/agent/features/runtime/src/application/session_bypass.rs"
printf '%s\n' 'fn bypass(_: context::session::ChatChain) {}' > "$probe"
set +e
output="$(AEMEATH_PROJECT_DIR="$TMP/repo" bash "$TMP/repo/.agents/hooks/check-shared-run-loop.sh" 2>&1)"
status=$?
set -e
if [ "$status" -ne 2 ] || [[ "$output" != *"Runtime 生产代码必须只经 Context"* ]]; then
  printf '%s\n' "$output" >&2
  echo "shared-loop Session boundary probe was not rejected" >&2
  exit 1
fi
printf '%s\n' 'const projection_start_index: usize = 0;' > "$probe"
set +e
output="$(AEMEATH_PROJECT_DIR="$TMP/repo" bash "$TMP/repo/.agents/hooks/check-shared-run-loop.sh" 2>&1)"
status=$?
set -e
if [ "$status" -ne 2 ]; then
  printf '%s\n' "$output" >&2
  echo "message ownership index probe was not rejected" >&2
  exit 1
fi
rm "$probe"
AEMEATH_PROJECT_DIR="$TMP/repo" bash "$TMP/repo/.agents/hooks/check-shared-run-loop.sh" >/dev/null
echo "Shared Run Loop boundary probe OK."
