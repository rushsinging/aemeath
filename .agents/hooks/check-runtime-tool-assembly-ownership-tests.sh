#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GUARD="$SCRIPT_DIR/check-runtime-tool-assembly-ownership.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/agent/features/runtime/src/application/client" "$TMP/agent/features/runtime/src/application" "$TMP/agent/composition/src"
cat >"$TMP/agent/features/runtime/src/application/client/from_args.rs" <<'RS'
struct RuntimeContextFactory;
pub struct RuntimeBootstrapDependencies {
    runtime_context_factory: RuntimeContextFactory,
    tool_catalog: (), skill_catalog: (), skill_materializer: (),
    tool_result_materializer: (), active_run: (),
}
RS
cat >"$TMP/agent/features/runtime/src/application/runtime_context_factory.rs" <<'RS'
struct RuntimeServices { tool_execution: (), tool_context_binding: () }
pub struct RuntimeContextFactory { services: RuntimeServices }
impl RuntimeContextFactory {
    pub fn new(tool_execution: (), tool_context_binding: ()) -> Self {
        Self { services: RuntimeServices { tool_execution, tool_context_binding } }
    }
}
RS
cat >"$TMP/agent/composition/src/runtime.rs" <<'RS'
fn wire_runtime_tool_assembly() {
    wire_builtin_catalog_execution();
    wire_skills();
    AtomicBlobToolResultStore::new();
    ActiveRunRegistry::default();
    RuntimeContextFactory::new(tool_assembly.execution.clone(), tool_assembly.binding.clone());
}
RS

run_guard() { AEMEATH_PROJECT_DIR="$TMP" "$GUARD"; }
expect_failure() {
  local label="$1" expected="$2"
  local output status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[runtime-tool-assembly] $label did not fail with exit 2 diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null
printf '%s\n' 'fn bad() { tools::composition::wire_builtin_catalog_execution(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-tools-factory 'Runtime bootstrap must consume injected Tool ports'
sed -i.bak '$d' "$TMP/agent/features/runtime/src/application/client/from_args.rs"
rm -f "$TMP/agent/features/runtime/src/application/client/from_args.rs.bak"

printf '%s\n' 'fn bad() { FileSystemBlobAdapter::new(); }' >>"$TMP/agent/features/runtime/src/application/client/from_args.rs"
expect_failure runtime-storage-factory 'Runtime bootstrap must not construct Tool Result filesystem backing'
sed -i.bak '$d' "$TMP/agent/features/runtime/src/application/client/from_args.rs"
rm -f "$TMP/agent/features/runtime/src/application/client/from_args.rs.bak"

python3 - "$TMP/agent/features/runtime/src/application/client/from_args.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source.replace(
    "    runtime_context_factory: RuntimeContextFactory,\n",
    "    runtime_context_factory: RuntimeContextFactory,\n    tool_execution: (),\n",
))
PY
expect_failure duplicate-tool-execution 'Runtime bootstrap must not duplicate factory-owned tool_execution'
python3 - "$TMP/agent/features/runtime/src/application/client/from_args.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source.replace("    tool_execution: (),\n", ""))
PY

python3 - "$TMP/agent/features/runtime/src/application/runtime_context_factory.rs" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source.replace("tool_execution: ()", "missing_execution: ()"))
PY
expect_failure missing-factory-execution 'RuntimeContextFactory constructor must receive tool_execution'

printf '%s\n' 'Runtime Tool assembly ownership guard sanity checks passed.'
