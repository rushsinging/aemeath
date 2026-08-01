#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cp -R "$ROOT/." "$TMP/repo"
GUARD="$TMP/repo/.agents/hooks/check-runtime-capability-assembly.sh"
BASELINE="$TMP/baseline"
mkdir -p "$BASELINE"
cp "$TMP/repo/agent/features/runtime/src/application.rs" "$BASELINE/application.rs"
cp "$TMP/repo/agent/features/runtime/src/lib.rs" "$BASELINE/lib.rs"
cp "$TMP/repo/agent/features/runtime/src/application/run/creation.rs" "$BASELINE/creation.rs"
cp "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs" "$BASELINE/engine.rs"
cp "$TMP/repo/agent/features/runtime/src/application/loop_engine/tests.rs" "$BASELINE/loop_engine_tests.rs"

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP/repo" bash "$GUARD"
}

expect_failure() {
  local label="$1"
  local expected="$2"
  local output
  local status=0
  output="$(run_guard 2>&1)" || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" <<<"$output"; then
    echo "[runtime-capability-assembly] $label did not fail with expected diagnostic" >&2
    echo "$output" >&2
    exit 1
  fi
}

run_guard >/dev/null

APPLICATION="$TMP/repo/agent/features/runtime/src/application.rs"
python3 - "$APPLICATION" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source.replace("pub(crate) mod run;", "pub mod run;"))
PY
expect_failure public-run-module "Runtime application module 'run' must be crate-private"

cp "$BASELINE/application.rs" "$TMP/repo/agent/features/runtime/src/application.rs"
LIB="$TMP/repo/agent/features/runtime/src/lib.rs"
python3 - "$LIB" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
path.write_text(source.replace(
    "#[cfg(test)]",
    "pub use application::run::creation::RunInstance;\n\n#[cfg(test)]",
    1,
))
PY
expect_failure unapproved-root-export "Runtime crate root exposes unapproved façade symbol: RunInstance"

cp "$BASELINE/lib.rs" "$TMP/repo/agent/features/runtime/src/lib.rs"
CREATION="$TMP/repo/agent/features/runtime/src/application/run/creation.rs"
python3 - "$CREATION" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace(
    "pub struct SessionSnapshot {",
    "pub struct SessionSnapshot {\n    live_port: std::sync::Arc<dyn crate::ports::ProviderPort>,",
    1,
)
path.write_text(source)
PY
expect_failure live-snapshot "SessionSnapshot must contain pure values only"

cp "$BASELINE/creation.rs" "$TMP/repo/agent/features/runtime/src/application/run/creation.rs"
printf '%s\n' 'pub trait RenamedFatPort: InputPort + ModelInvocationPort + ToolOrchestrationPort {}' >>"$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs"
expect_failure fat-supertrait "Runtime must not define a trait that aggregates multiple Loop capability categories"

cp "$BASELINE/engine.rs" "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs"
printf '%s\n' 'pub type RenamedFatAlias = dyn InputPort + ModelInvocationPort + ToolOrchestrationPort;' >>"$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs"
expect_failure fat-trait-alias "Runtime must not define a trait-object alias that aggregates multiple Loop capability categories"

cp "$BASELINE/engine.rs" "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs"
TESTS="$TMP/repo/agent/features/runtime/src/application/loop_engine/tests.rs"
printf '%s\n' 'struct RenamedFatFake;' 'impl InputPort for RenamedFatFake {}' 'impl ModelInvocationPort for RenamedFatFake {}' 'impl ToolOrchestrationPort for RenamedFatFake {}' >>"$TESTS"
expect_failure fat-test-double "Runtime type RenamedFatFake implements multiple Loop capability categories"

cp "$BASELINE/loop_engine_tests.rs" "$TMP/repo/agent/features/runtime/src/application/loop_engine/tests.rs"
run_guard >/dev/null

echo "Runtime Capability Assembly terminal boundary probes passed."
