#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-session-management-ownership.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/agent/features/context/src/adapters" \
  "$TMP/agent/features/context/src/ports" \
  "$TMP/agent/features/runtime/src/application" \
  "$TMP/agent/composition/src"

cat >"$TMP/agent/features/context/src/adapters/atomic_blob_session_management.rs" <<'RS'
pub struct AtomicBlobSessionManagement;
RS
cat >"$TMP/agent/features/context/src/ports/session_management.rs" <<'RS'
pub trait SessionManagementPort {}
RS
cat >"$TMP/agent/features/runtime/src/application/client.rs" <<'RS'
fn consume() {}
RS
cat >"$TMP/agent/composition/src/runtime.rs" <<'RS'
fn wire() {
    let session_blob = file_system_blob();
    let _port = AtomicBlobSessionManagement::new(session_blob.clone());
    let _deps = MainSessionDependencies { session_management: session_management.clone() };
    RuntimeBootstrapDependencies::new(session_management);
}
RS

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP" "$GUARD"
}

expect_failure() {
  local label="$1" expected="$2"
  shift 2
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[session-management-ownership] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
  "$@"
}

run_guard >/dev/null
printf '%s\n' 'fn bad() { storage::api::file_system_blob("root"); }' >"$TMP/agent/features/context/src/adapters/bypass.rs"
expect_failure context-construction 'Context Session code must consume injected AtomicBlobPort' rm -f "$TMP/agent/features/context/src/adapters/bypass.rs"

printf '%s\n' 'fn bad() { context::list_session_entries(); }' >"$TMP/agent/features/runtime/src/application/bypass.rs"
expect_failure runtime-facade 'Runtime must consume injected SessionManagementPort' rm -f "$TMP/agent/features/runtime/src/application/bypass.rs"

run_guard >/dev/null
echo 'Session management ownership guard sanity checks passed.'
