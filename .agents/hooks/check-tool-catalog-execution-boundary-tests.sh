#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GUARD="$SCRIPT_DIR/check-tool-catalog-execution-boundary.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p \
  "$TMP/agent/features/runtime/src/application/chat/looping" \
  "$TMP/agent/features/tools/src/adapters" \
  "$TMP/agent/features/tools/src/domain"

cat >"$TMP/agent/features/runtime/src/application/chat/looping/ask_user.rs" <<'RS'
// Runtime owns this waiter; the Tools-only AskUser rule must not reject it.
async fn await_reply() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<String>();
}
RS
cat >"$TMP/agent/features/tools/src/adapters/execution.rs" <<'RS'
pub(crate) struct ExecutionAdapter;
RS
cat >"$TMP/agent/features/tools/src/adapters/ask_user.rs" <<'RS'
pub(crate) fn suspension() -> &'static str { "typed suspension" }
RS
cat >"$TMP/agent/features/tools/src/domain/suspension.rs" <<'RS'
pub enum ToolSuspension { UserInteraction(UserInteractionSpec) }
pub struct UserInteractionSpec { pub prompt: String }
RS
cat >"$TMP/agent/features/tools/src/domain/schema_validator.rs" <<'RS'
pub fn validate_tool_input() {}
RS
cat >"$TMP/agent/features/tools/src/lib.rs" <<'RS'
pub use domain::{ToolCatalogPort, ToolExecutionPort, ToolSuspension};
RS

run_guard() {
    AEMEATH_PROJECT_DIR="$TMP" "$GUARD" "$@"
}
expect_failure() {
    local label="$1" expected="$2"
    shift 2
    local output status=0
    output="$(run_guard 2>&1)" || status=$?
    if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
        echo "[tool-boundary-sanity] $label did not fail with expected exit 2 diagnostic" >&2
        echo "$output" >&2
        exit 1
    fi
    "$@"
}

run_guard >/dev/null
echo '[tool-boundary-sanity] positive fixture passed'

runtime_probe="$TMP/agent/features/runtime/src/application/bypass.rs"
printf '%s\n' 'fn bypass(registry: &ToolRegistry) { let _ = registry.get("Bash"); }' >"$runtime_probe"
expect_failure runtime-registry 'Runtime production code must not reference ToolRegistry' rm -f "$runtime_probe"

runtime_adapter_probe="$TMP/agent/features/runtime/src/application/private_adapter.rs"
printf '%s\n' 'use tools::adapters::execution::ExecutionAdapter;' >"$runtime_adapter_probe"
expect_failure runtime-private-adapter 'Runtime production code must not import Tools private adapters' rm -f "$runtime_adapter_probe"

runtime_backing_probe="$TMP/agent/features/runtime/src/application/private_backing.rs"
printf '%s\n' 'fn bypass(_: ToolBacking) {}' >"$runtime_backing_probe"
expect_failure runtime-private-backing 'Runtime production code must not reference Tools private backing or adapters' rm -f "$runtime_backing_probe"

cp "$TMP/agent/features/tools/src/adapters/execution.rs" "$TMP/execution.clean"
printf '%s\n' 'fn execute() { hook::before(); }' >>"$TMP/agent/features/tools/src/adapters/execution.rs"
expect_failure execution-hook 'Tools Execution adapter must not depend on policy/hook/sdk/tui/runtime' cp "$TMP/execution.clean" "$TMP/agent/features/tools/src/adapters/execution.rs"

cp "$TMP/agent/features/tools/src/domain/suspension.rs" "$TMP/suspension.clean"
printf '%s\n' 'pub struct BadSuspension { pub reply: Sender<String> }' >>"$TMP/agent/features/tools/src/domain/suspension.rs"
expect_failure suspension-sender 'Tool suspension PL must not contain channels, locks, or Arc' cp "$TMP/suspension.clean" "$TMP/agent/features/tools/src/domain/suspension.rs"

cp "$TMP/agent/features/tools/src/adapters/ask_user.rs" "$TMP/ask_user.clean"
printf '%s\n' 'const LEGACY: &str = "__ASK_USER__:";' >>"$TMP/agent/features/tools/src/adapters/ask_user.rs"
expect_failure ask-user-magic 'Tools AskUser must not use magic-string suspension protocols' cp "$TMP/ask_user.clean" "$TMP/agent/features/tools/src/adapters/ask_user.rs"

cp "$TMP/agent/features/tools/src/lib.rs" "$TMP/lib.clean"
printf '%s\n' 'pub use adapters::execution::ExecutionAdapter;' >>"$TMP/agent/features/tools/src/lib.rs"
expect_failure facade-adapter 'Tools crate-root must expose a composition factory, not concrete adapters' cp "$TMP/lib.clean" "$TMP/agent/features/tools/src/lib.rs"

schema_probe="$TMP/agent/features/runtime/src/application/schema_copy.rs"
printf '%s\n' 'fn validate_tool_input() { jsonschema::validate(); }' >"$schema_probe"
expect_failure schema-copy 'Schema validator implementation must exist only in Tools' rm -f "$schema_probe"

legacy_scope_probe="$TMP/agent/features/tools/src/adapters/legacy_scope.rs"
printf '%s\n' 'enum Legacy { LegacyNoAgent }' >"$legacy_scope_probe"
expect_failure legacy-scope 'Tools legacy Registry/Profile/SkillTool paths must stay retired' rm -f "$legacy_scope_probe"

run_guard >/dev/null
AEMEATH_PROJECT_DIR="$ROOT" "$GUARD" >/dev/null
echo 'Tool Catalog/Execution guard sanity checks passed.'
