#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-runtime-hook-assembly-ownership.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/agent/features/runtime/src/application/client" "$TMP/agent/composition/src"
cat >"$TMP/agent/features/runtime/src/application/client/from_args.rs" <<'RS'
struct RuntimeBootstrapDependencies { hook_runner: () }
RS
cat >"$TMP/agent/composition/src/runtime.rs" <<'RS'
fn wire() {
    let hook_runner = hook::build_dispatcher(committed_snapshot().hooks());
}
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[runtime-hook-assembly] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null
printf '%s\n' 'fn bad() { build_hook_runner(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-hook-factory 'Runtime production must not define or invoke build_hook_runner'
sed -i.bak '$d' "$TMP/agent/features/runtime/src/application/client/from_args.rs"
rm -f "$TMP/agent/features/runtime/src/application/client/from_args.rs.bak"

printf '%s\n' 'fn bad() { Dispatcher::try_new(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-hook-dispatcher-direct 'Runtime production must not construct Hook dispatcher'
sed -i.bak '$d' "$TMP/agent/features/runtime/src/application/client/from_args.rs"
rm -f "$TMP/agent/features/runtime/src/application/client/from_args.rs.bak"

printf '%s\n' 'fn bad() { hook::build_dispatcher(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-hook-dispatcher 'Runtime production must not construct Hook dispatcher'

printf '%s\n' 'fn bad() { build_dispatcher(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-hook-bare-dispatcher 'Runtime production must not construct Hook dispatcher'

echo 'Runtime Hook assembly ownership guard sanity checks passed.'
