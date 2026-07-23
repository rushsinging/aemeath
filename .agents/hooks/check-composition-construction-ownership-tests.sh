#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-composition-construction-ownership.sh"

if [ ! -x "$GUARD" ]; then
  echo '[composition-construction] aggregate guard is missing' >&2
  exit 3
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/.agents/hooks" \
  "$TMP/agent/features/runtime/src/application/client" \
  "$TMP/agent/features/config/src" \
  "$TMP/agent/features/context/src/adapters"

for guard in \
  check-session-management-ownership.sh \
  check-config-store-ownership.sh \
  check-runtime-tool-assembly-ownership.sh \
  check-runtime-hook-assembly-ownership.sh; do
  cat >"$TMP/.agents/hooks/$guard" <<'SH'
#!/usr/bin/env bash
exit 0
SH
  chmod +x "$TMP/.agents/hooks/$guard"
done

cat >"$TMP/.agents/hooks/check-architecture-guards.sh" <<'SH'
run_guard fast "$HOOKS_DIR/check-session-management-ownership.sh"
run_guard fast "$HOOKS_DIR/check-config-store-ownership.sh"
run_guard fast "$HOOKS_DIR/check-runtime-tool-assembly-ownership.sh"
run_guard fast "$HOOKS_DIR/check-runtime-hook-assembly-ownership.sh"
SH
cat >"$TMP/.agents/architecture-guard-registry.json" <<'JSON'
{"entries":[
{"id":"policy.session-management.composition-ownership","guard":"check-session-management-ownership.sh","classification":"target_capability_policy","status":"active"},
{"id":"policy.config.override-store.composition-ownership","guard":"check-config-store-ownership.sh","classification":"target_capability_policy","status":"active"},
{"id":"policy.runtime.tool-assembly.composition-ownership","guard":"check-runtime-tool-assembly-ownership.sh","classification":"target_capability_policy","status":"active"},
{"id":"policy.runtime.hook-assembly.composition-ownership","guard":"check-runtime-hook-assembly-ownership.sh","classification":"target_capability_policy","status":"active"}
]}
JSON
cat >"$TMP/agent/features/runtime/src/application/client/from_args.rs" <<'RS'
fn injected_resources() {}
RS
cat >"$TMP/agent/features/config/src/application.rs" <<'RS'
fn injected_store() {}
RS
cat >"$TMP/agent/features/context/src/adapters/atomic_blob_session_management.rs" <<'RS'
fn injected_blob() {}
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[composition-construction] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null
sed -i.bak '/check-runtime-tool-assembly-ownership/d' "$TMP/.agents/hooks/check-architecture-guards.sh"
expect_failure missing-wiring 'missing fast registration for check-runtime-tool-assembly-ownership.sh'
mv "$TMP/.agents/hooks/check-architecture-guards.sh.bak" "$TMP/.agents/hooks/check-architecture-guards.sh"

python3 - "$TMP/.agents/architecture-guard-registry.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path))
data['entries'] = [entry for entry in data['entries'] if entry['id'] != 'policy.config.override-store.composition-ownership']
json.dump(data, open(path, 'w'))
PY
expect_failure missing-registry 'missing active target policy for check-config-store-ownership.sh'
python3 - "$TMP/.agents/architecture-guard-registry.json" <<'PY'
import json, sys
path = sys.argv[1]
data = json.load(open(path))
data['entries'].append({"id":"policy.config.override-store.composition-ownership","guard":"check-config-store-ownership.sh","classification":"target_capability_policy","status":"active"})
json.dump(data, open(path, 'w'))
PY

printf '%s\n' 'fn bad() { FileSystemBlobAdapter::new(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-storage 'Runtime must not construct FileSystemBlobAdapter'
sed -i.bak '$d' "$TMP/agent/features/runtime/src/application/client/from_args.rs"
mv "$TMP/agent/features/runtime/src/application/client/from_args.rs.bak" "$TMP/agent/features/runtime/src/application/client/from_args.rs"

printf '%s\n' 'fn bad() { storage::api::file_system_blob(); }' >>"$TMP/agent/features/config/src/application.rs"
expect_failure config-storage 'Config must not construct file_system_blob'
sed -i.bak '$d' "$TMP/agent/features/config/src/application.rs"
mv "$TMP/agent/features/config/src/application.rs.bak" "$TMP/agent/features/config/src/application.rs"

printf '%s\n' 'fn bad() { storage::api::file_system_blob(); }' >>"$TMP/agent/features/context/src/adapters/atomic_blob_session_management.rs"
expect_failure context-storage 'Context must not construct file_system_blob'

echo 'Composition construction ownership guard sanity checks passed.'
