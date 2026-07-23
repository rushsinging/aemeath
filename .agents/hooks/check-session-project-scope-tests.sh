#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-session-project-scope.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/agent/features/context/src/ports" \
  "$TMP/agent/features/context/src/adapters" \
  "$TMP/agent/features/runtime/src/application/client" \
  "$TMP/agent/features/runtime/src/application/chat/looping"
cat >"$TMP/agent/features/context/src/ports/session_management.rs" <<'RS'
trait SessionManagementPort {
    fn load_for_project(&self) {}
    fn list_for_project(&self) {}
    fn export_for_project(&self) {}
    fn import_for_project(&self) {}
    fn update_metadata_for_project(&self) {}
    fn delete_for_project(&self) {}
}
RS
cat >"$TMP/agent/features/context/src/adapters/session_resume.rs" <<'RS'
fn resume() { load_for_project(); }
RS
cat >"$TMP/agent/features/runtime/src/application/client/trait_session.rs" <<'RS'
fn list() { list_for_project(); }
RS
cat >"$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs" <<'RS'
fn execute() {
    list_for_project();
    update_metadata_for_project();
    export_for_project();
    import_for_project();
    delete_for_project();
}
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[session-project-scope] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null
sed -i.bak 's/load_for_project/load_canonical/' "$TMP/agent/features/context/src/adapters/session_resume.rs"
expect_failure resume-load 'MainSessionWiring resume must use project-scoped session load'
mv "$TMP/agent/features/context/src/adapters/session_resume.rs.bak" "$TMP/agent/features/context/src/adapters/session_resume.rs"

sed -i.bak 's/export_for_project/export/' "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"
expect_failure runtime-export 'Runtime session export must use project-scoped export'
mv "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs.bak" "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"

sed -i.bak 's/update_metadata_for_project/update_metadata/' "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"
expect_failure runtime-rename 'Runtime session rename must use project-scoped metadata update'

sed -i.bak 's/import_for_project/import/' "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"
expect_failure runtime-import 'Runtime session import must use project-scoped import'
mv "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs.bak" "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"

sed -i.bak 's/delete_for_project/delete/' "$TMP/agent/features/runtime/src/application/chat/looping/idle_commands.rs"
expect_failure runtime-delete 'Runtime session delete must use project-scoped delete'

echo 'Session project scope guard sanity checks passed.'
