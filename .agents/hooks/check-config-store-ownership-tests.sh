#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-config-store-ownership.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/agent/features/config/src" "$TMP/agent/composition/src"
cat >"$TMP/agent/features/config/src/application.rs" <<'RS'
pub fn wire_project_config_with_cli(native_store: NativeConfigStore) {}
pub fn wire_project_config(native_store: NativeConfigStore) {}
pub fn for_project(native_store: NativeConfigStore) {}
RS
cat >"$TMP/agent/composition/src/app.rs" <<'RS'
fn wire_config_override_store() {
    NativeConfigStore::new(file_system_blob());
}
fn bootstrap() {
    wire_project_config_with_cli(wire_config_override_store()?);
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
    echo "[config-store-ownership] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
  "$@"
}

run_guard >/dev/null
printf '%s\n' 'fn bad() { storage::api::file_system_blob("root"); }' >>"$TMP/agent/features/config/src/application.rs"
expect_failure config-construction 'Config application must consume injected NativeConfigStore' sed -i.bak '$d' "$TMP/agent/features/config/src/application.rs"
rm -f "$TMP/agent/features/config/src/application.rs.bak"

printf '%s\n' 'fn wire_config_override_store() { NativeConfigStore::new(file_system_blob()); }' >>"$TMP/agent/composition/src/app.rs"
expect_failure duplicate-composition-factory 'Composition must define exactly one config override store factory' sed -i.bak '$d' "$TMP/agent/composition/src/app.rs"
rm -f "$TMP/agent/composition/src/app.rs.bak"

run_guard >/dev/null
echo 'Config override store ownership guard sanity checks passed.'
