#!/bin/bash
# guard-registry:policy.runtime.activity-observation
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

root = Path.cwd()
runtime = root / "agent/features/runtime/src"
tui = root / "apps/cli/src/tui"
coordinator = runtime / "application/activity/coordinator.rs"
model = runtime / "application/activity/model.rs"
root_reducer = tui / "update/root_reducer.rs"
violations = []


def production_text(path: Path) -> str:
    source = path.read_text()
    source = re.sub(r"(?ms)#\[cfg\(test\)\].*?\n}\n", "", source)
    return "\n".join(
        line for line in source.splitlines()
        if not line.lstrip().startswith("//")
    )


def is_test(path: Path) -> bool:
    return "tests" in path.parts or "test" in path.stem


for path in runtime.rglob("*.rs"):
    if is_test(path) or path in {coordinator, model}:
        continue
    if "ActivityObservation {" in production_text(path):
        violations.append(
            f"Runtime ActivityObservation construction must stay in ActivityCoordinator: {path}"
        )

for path in tui.rglob("*.rs"):
    if is_test(path) or "scenario_tests" in path.parts or path == root_reducer:
        continue
    if "activity_observations_mut(" in production_text(path):
        violations.append(
            f"TUI ActivityObservation mutation must stay behind root reducer: {path}"
        )

live_status = production_text(tui / "view_assembler/live_status.rs")
for symbol in ("RunStatusView", "TuiRunStatus", "RunTransitioned"):
    if symbol in live_status:
        violations.append(f"LiveStatus must not depend on legacy Run status: {symbol}")

legacy_symbols = (
    "RunStatusObserved",
    "RunStateSnapshot",
    "run_state_snapshots",
    "active_main_run_snapshot",
    "TuiRunTiming",
    "SpinnerPhase",
    "chat_active",
    "running_tool_count",
)
for path in tui.rglob("*.rs"):
    if is_test(path) or path.name == "architecture_tests.rs":
        continue
    source = production_text(path)
    for symbol in legacy_symbols:
        if symbol in source:
            violations.append(f"legacy Activity display symbol {symbol}: {path}")

hook_parallel_symbols = (
    "RuntimeStreamEvent::HookEvent",
    "RuntimeStreamEvent::HookMessage",
    "RuntimeStreamEvent::StopHookBlocked",
    "ChatEvent::HookEvent",
    "ChatEvent::HookMessage",
    "ChatEvent::StopHookBlocked",
    "TuiRuntimeEvent::HookEvent",
    "TuiRuntimeEvent::HookMessage",
    "TuiRuntimeEvent::StopHookBlocked",
    "UiEvent::HookEvent",
    "UiEvent::HookMessage",
    "UiEvent::StopHookBlocked",
    "AppendHookNotice",
    "HookNotice",
)
for path in (runtime, root / "packages/sdk/src", tui):
    for source_path in path.rglob("*.rs"):
        if is_test(source_path) or "scenario_tests" in source_path.parts:
            continue
        source = production_text(source_path)
        for symbol in hook_parallel_symbols:
            if symbol in source:
                violations.append(
                    f"Hook display must use the unique Activity observation path, found {symbol}: {source_path}"
                )

allowed_direct_hook_dispatches = {
    runtime / "application/loop_engine/chat/hook_ui.rs",
    runtime / "application/hook/stop_coordination.rs",
    runtime / "application/hook/empty.rs",
    runtime / "application/prompt/build/prompt_build.rs",
    runtime / "application/prompt/instructions_hook.rs",
}
for source_path in runtime.rglob("*.rs"):
    if is_test(source_path) or source_path in allowed_direct_hook_dispatches:
        continue
    if ".dispatch_at(" in production_text(source_path):
        violations.append(
            f"Run Hook dispatch must publish an Activity lifecycle through the designated boundary: {source_path}"
        )

for path in (coordinator, tui / "effect/session/processing/logging.rs"):
    source = production_text(path)
    for field in (
        "run_id={}",
        "revision={}",
        "total_elapsed_ms={}",
        "active_elapsed_ms={}",
        "state_elapsed_ms={}",
    ):
        if field not in source:
            violations.append(f"Activity diagnostic missing {field}: {path}")
    for sensitive in ("raw_args", "stdout", "response={}"):
        if sensitive in source:
            violations.append(f"Activity diagnostic exposes {sensitive}: {path}")

if violations:
    print("[architecture] Runtime Activity observation guard failed:", file=sys.stderr)
    for violation in violations:
        print(f"  - {violation}", file=sys.stderr)
    sys.exit(2)

print("Runtime Activity observation guard passed.")
PY
