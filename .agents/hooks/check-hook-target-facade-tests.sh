#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GUARD="$ROOT/.agents/hooks/check-hook-target-facade.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -R "$ROOT" "$TMP/repo"
REPO="$TMP/repo"

expect_block() {
  local label="$1"
  local output status
  set +e
  output="$(AEMEATH_PROJECT_DIR="$REPO" bash "$REPO/.agents/hooks/check-hook-target-facade.sh" 2>&1)"
  status=$?
  set -e
  [ "$status" -eq 2 ] || { echo "$label: expected exit 2, got $status: $output" >&2; exit 1; }
}

printf '\npub mod api {}\n' >> "$REPO/agent/features/hook/src/lib.rs"
expect_block api-module
cp "$ROOT/agent/features/hook/src/lib.rs" "$REPO/agent/features/hook/src/lib.rs"

printf '\npub use crate::adapters::legacy::HookRunner;\n' >> "$REPO/agent/features/hook/src/lib.rs"
expect_block legacy-reexport
cp "$ROOT/agent/features/hook/src/lib.rs" "$REPO/agent/features/hook/src/lib.rs"

printf '\nuse hook::api::HookRunner;\n' >> "$REPO/agent/features/runtime/src/application/resources.rs"
expect_block runtime-api-consumer
cp "$ROOT/agent/features/runtime/src/application/resources.rs" "$REPO/agent/features/runtime/src/application/resources.rs"

AEMEATH_PROJECT_DIR="$REPO" bash "$REPO/.agents/hooks/check-hook-target-facade.sh"
echo "Hook target facade guard regression tests passed"
