#!/bin/bash
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
cp "$TMP/repo/agent/features/runtime/src/application/run/context_factory.rs" "$BASELINE/context_factory.rs"
cp "$TMP/repo/agent/features/runtime/src/application/run/creation.rs" "$BASELINE/creation.rs"
cp "$TMP/repo/agent/features/runtime/src/application/run/launcher.rs" "$BASELINE/launcher.rs"
cp "$TMP/repo/agent/features/runtime/src/application/loop_engine/engine.rs" "$BASELINE/engine.rs"
cp "$TMP/repo/agent/features/runtime/src/application/loop_engine/tests.rs" "$BASELINE/loop_engine_tests.rs"
cp "$TMP/repo/agent/features/runtime/src/application/loop_engine/chat/loop_runner.rs" "$BASELINE/loop_runner.rs"
cp "$TMP/repo/agent/features/runtime/src/application/run/derived/setup.rs" "$BASELINE/derived_setup.rs"
cp "$TMP/repo/agent/features/runtime/src/application/run/derived/loop_run.rs" "$BASELINE/derived_loop_run.rs"

run_guard() {
  AEMEATH_PROJECT_DIR="$TMP/repo" /bin/bash "$GUARD"
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

# --fast-only：Stop 场景只做主检查；15 个变异回归（每个重跑 guard ~1.3s）
# 只在 full 门禁执行——fast 全量 wall 从 22s 降到 ~8s。
if [ "${1:-}" = "--fast-only" ]; then
  echo "Runtime Capability Assembly fast probe passed."
  exit 0
fi

CONTEXT_FACTORY="$TMP/repo/agent/features/runtime/src/application/run/context_factory.rs"
cat >>"$CONTEXT_FACTORY" <<'RUST'

#[cfg(test)]
impl RuntimeContextFactory {
    pub(crate) fn alternate_context_creator(&self) -> RuntimeContext {
        RuntimeContext::new(
            unreachable!(),
            unreachable!(),
            unreachable!(),
            RuntimeContextAssemblyToken::new(),
        )
    }
}
RUST
expect_failure test-context-creator "RuntimeContext creation must not have a test-only alternate entry"

cp "$BASELINE/context_factory.rs" "$CONTEXT_FACTORY"
LAUNCHER="$TMP/repo/agent/features/runtime/src/application/run/launcher.rs"
printf '%s\n' 'fn unapproved_prepare_caller(factory: &RuntimeContextFactory, request: &RunCreationRequest, bindings: &RunCreationBindings) { let _ = factory.prepare(request, bindings); }' >>"$LAUNCHER"
expect_failure unapproved-prepare-caller "RuntimeContextFactory::prepare has an unapproved caller"

cp "$BASELINE/launcher.rs" "$LAUNCHER"
printf '%s\n' 'fn unapproved_run_instance_creator() { let _ = RunInstance::new(unreachable!(), unreachable!(), unreachable!(), unreachable!()); }' >>"$LAUNCHER"
expect_failure unapproved-run-instance-creator "RunInstance::new has an unapproved caller"

cp "$BASELINE/launcher.rs" "$LAUNCHER"
MAIN_CALLER="$TMP/repo/agent/features/runtime/src/application/loop_engine/chat/loop_runner.rs"
python3 - "$MAIN_CALLER" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace("run_factory.create(request)", "bypass_main_run_factory(request)", 1)
path.write_text(source)
PY
expect_failure main-factory-bypass "Main Run must use RunFactory::create and RunLauncher::launch"

cp "$BASELINE/loop_runner.rs" "$MAIN_CALLER"
python3 - "$MAIN_CALLER" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace(
    "crate::application::run::launcher::launch(",
    "bypass_main_run_launcher(",
    1,
)
path.write_text(source)
PY
expect_failure main-launcher-bypass "Main Run must use RunFactory::create and RunLauncher::launch"

cp "$BASELINE/loop_runner.rs" "$MAIN_CALLER"
DERIVED_SETUP="$TMP/repo/agent/features/runtime/src/application/run/derived/setup.rs"
python3 - "$DERIVED_SETUP" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace(
    "run_factory.create(creation_request)",
    "bypass_derived_run_factory(creation_request)",
    1,
)
path.write_text(source)
PY
expect_failure derived-factory-bypass "Derived Run must use RunFactory::create"

cp "$BASELINE/derived_setup.rs" "$DERIVED_SETUP"
DERIVED_LAUNCHER="$TMP/repo/agent/features/runtime/src/application/run/derived/loop_run.rs"
python3 - "$DERIVED_LAUNCHER" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace(
    "crate::application::run::launcher::launch(instance",
    "bypass_derived_run_launcher(instance",
    1,
)
path.write_text(source)
PY
expect_failure derived-launcher-bypass "Derived Run must pass RunInstance to RunLauncher::launch"

cp "$BASELINE/derived_loop_run.rs" "$DERIVED_LAUNCHER"

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
python3 - "$CREATION" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
source = path.read_text()
source = source.replace(
    "pub struct RunCreationRequest {",
    "pub struct RunCreationRequest {\n    live_port: std::sync::Arc<dyn crate::ports::ProviderPort>,",
    1,
)
path.write_text(source)
PY
expect_failure live-run-creation-request "RunCreationRequest must contain pure values only"

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
