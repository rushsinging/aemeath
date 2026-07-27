#!/usr/bin/env bash
# guard-registry:policy.runtime.capability-assembly
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
violations = []

FACTORY = root / "agent/features/runtime/src/application/run/context_factory.rs"
ENGINE = root / "agent/features/runtime/src/application/loop_engine/engine.rs"
STUCK = root / "agent/features/runtime/src/application/loop_engine/stuck_guard.rs"
RUN_DOMAIN = root / "agent/features/runtime/src/domain/agent_run/domain.rs"
INTERACTION = root / "agent/features/runtime/src/application/interaction/port.rs"
EMPTY_HOOK = root / "agent/features/runtime/src/application/hook/empty.rs"
# ── helpers ──
def strip_comments(text: str) -> str:
    """Remove //  and ///  and //!  line comments (but keep lines that contain code before the //)."""
    lines = []
    for line in text.split('\n'):
        stripped = line.strip()
        # Skip pure comment lines
        if stripped.startswith('//') or stripped.startswith('#!'):
            continue
        # Remove trailing comments from code lines
        # Only remove // style, keep URIs and string content
        idx = line.find('//')
        if idx >= 0:
            # Don't strip if // appears inside a string literal (crude check)
            before = line[:idx]
            if before.count('"') % 2 == 0:  # even number of quotes = not in string
                line = before
        lines.append(line)
    return '\n'.join(lines)

def production_text(p: Path) -> str:
    """Return production half (before #[cfg(test)]) with comments stripped."""
    if not p.is_file():
        return ""
    text = p.read_text()
    prod, _, _ = text.partition("#[cfg(test)]")
    return strip_comments(prod)

import glob as _glob

# ── 1. RuntimeContext::new( only in factory (production-only) ──
for candidate in _glob.glob("agent/features/runtime/src/application/**/*.rs", recursive=True):
    p = Path(candidate)
    if "_test" in p.stem or p.stem == "tests":
        continue
    if p.resolve() == FACTORY.resolve():
        continue
    prod = production_text(p)
    if re.search(r'RuntimeContext::new\s*\(', prod):
        violations.append(f"1. RuntimeContext::new( outside factory: {p}")

# ── 2. Factory's assemble() must construct the gating token ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if not re.search(r'RuntimeContextAssemblyToken::new\s*\(\)', prod):
        violations.append("2. Factory must construct RuntimeContextAssemblyToken")

# ── 3. Retired symbols absent from production code ──
RETIRED = {
    r'\bRuntimeContextParts\b': "RuntimeContextParts struct",
    r'\bassemble_main_runtime_context\b': "assemble_main_runtime_context function",
    r'\bModelStep::StopHookBlocked\b': "ModelStep::StopHookBlocked variant",
    r'\bInteractionBridge::disabled\b': "InteractionBridge::disabled() method",
}
for candidate in _glob.glob("agent/**/*.rs", recursive=True):
    p = Path(candidate)
    if "_test" in p.stem or p.stem == "tests":
        continue
    if "/tests/" in str(p):
        continue
    prod = production_text(p)
    for pat, name in RETIRED.items():
        if re.search(pat, prod):
            violations.append(f"3. Retired '{name}' in production: {p}")

# ── 4. RunKind::Main/Sub not in factory, engine, or launcher ──
for check_file in [FACTORY, ENGINE, root / "agent/features/runtime/src/application/run/launcher.rs"]:
    if not check_file.is_file():
        continue
    prod = production_text(check_file)
    if re.search(r'RunKind::(Main|Sub)', prod):
        violations.append(f"4. RunKind::Main/Sub in control-flow file: {check_file}")

# ── 5. record_stop_hook_block() called from shared engine, NOT from StuckGuard ──
if ENGINE.is_file():
    prod = production_text(ENGINE)
    if not re.search(r'record_stop_hook_block\s*\(', prod):
        violations.append("5. Shared engine must call record_stop_hook_block()")

if STUCK.is_file():
    prod = production_text(STUCK)
    if re.search(r'record_stop_hook_block\s*\(', prod):
        violations.append("5. StuckGuard must NOT call record_stop_hook_block()")

# ── 6. Run owns stop_hook_block_count and RetryExhausted ──
if RUN_DOMAIN.is_file():
    prod = production_text(RUN_DOMAIN)
    if not re.search(r'stop_hook_block_count', prod):
        violations.append("6. Run must own stop_hook_block_count field")
    if not re.search(r'fn record_stop_hook_block', prod):
        violations.append("6. Run must define record_stop_hook_block()")
    if not re.search(r'RetryExhausted', prod):
        violations.append("6. Run must define StopHookBlockResult::RetryExhausted")

# ── 7. Interaction modes wired: Client, ParentMediated, Unavailable ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    for mode in ["InteractionBindingMode::Client", "InteractionBindingMode::ParentMediated", "InteractionBindingMode::Unavailable"]:
        if mode not in prod:
            violations.append(f"7. Factory must wire {mode}")

# ── 8. Static reasoning contract ──
# Workflow graph/reasoning ports were removed. Runtime keeps the model's
# static reasoning level in RunContextBindings and factory assembly must
# explicitly select the retained Fixed mode.
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if "ReasoningBindingMode::Fixed" not in prod:
        violations.append("8. Factory must wire retained static ReasoningBindingMode::Fixed")
# ── 9. Sub Hook mode uses a boundary-filtering capability adapter ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if "HookBindingMode::BoundaryOnly" not in prod:
        violations.append("9. Factory must handle the Sub Hook binding mode")
    if "BoundaryHookPort" not in prod:
        violations.append("9. Factory must bind Sub Runs to BoundaryHookPort")
    if "EmptyHookPort" in prod:
        violations.append("9. Factory must not collapse BoundaryOnly into EmptyHookPort")
if EMPTY_HOOK.is_file():
    prod = production_text(EMPTY_HOOK)
    if "struct BoundaryHookPort" not in prod:
        violations.append("9. BoundaryHookPort implementation is missing")
    if not all(point in prod for point in ["SessionStart", "SessionEnd", "SubRunStart", "SubRunStop"]):
        violations.append("9. BoundaryHookPort must allow only Run/SubRun lifecycle boundaries")
    if "HookOutcome::proceed()" not in prod:
        violations.append("9. BoundaryHookPort must return proceed for filtered invocations")
else:
    violations.append("9. Hook capability adapter implementation is missing")

# ── 10. Workflow graph retired ──
# Reasoning graph ownership is deferred for redesign; no workflow crate or
# inherited reasoning constructor is required by the current architecture.
# ── 11. UnavailableInteractionPort exists ──
if INTERACTION.is_file():
    prod = production_text(INTERACTION)
    if "UnavailableInteractionPort" not in prod:
        violations.append("11. UnavailableInteractionPort must exist")

# ── 12. Factory's private create() returns typed errors ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if "fn create" not in prod:
        violations.append("12. Factory must have private create() method")
    if re.search(r'pub\s+fn\s+create', prod):
        violations.append("12. Factory create() must not be public")
    if "RuntimeContextAssemblyError" not in prod:
        violations.append("12. Factory must return typed RuntimeContextAssemblyError")

# ── 13. P6.2 pure-value preparation entry ──
PREPARER = root / "agent/features/runtime/src/application/run/preparer.rs"
MAIN_CALLER = root / "agent/features/runtime/src/application/loop_engine/chat/loop_runner.rs"
SUB_CALLER = root / "agent/features/runtime/src/application/run/derived/setup.rs"
if PREPARER.is_file():
    prod = production_text(PREPARER)
    signature = prod.partition("pub fn prepare(")[2].partition(") -> Result<PreparedRun")[0]
    if "request: RunPreparationRequest" not in signature:
        violations.append("13. RunPreparer::prepare must accept RunPreparationRequest")
    for retired in ["RunCapabilityBindings", "RunContextBindings", "RuntimeContext"]:
        if retired in signature:
            violations.append(f"13. RunPreparer::prepare signature must not expose {retired}")
for caller in [MAIN_CALLER, SUB_CALLER]:
    if not caller.is_file():
        continue
    prod = production_text(caller)
    for retired in ["RunCapabilityBindings", "RunContextBindings", "RuntimeContextParts", "SubRunCapabilitySource"]:
        if retired in prod:
            violations.append(f"13. Production Run caller assembles retired {retired}: {caller}")
    if re.search(r'RuntimeContext::new\s*\(|\.create\s*\(', prod):
        violations.append(f"13. Production Run caller bypasses RunPreparer: {caller}")

# ── 14. P6.3 one model invocation orchestration ──
MODEL_COORDINATOR = root / "agent/features/runtime/src/application/model/invocation.rs"
MODEL_ADAPTERS = [
    root / "agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs",
    root / "agent/features/runtime/src/application/run/derived/loop_run.rs",
]
if MODEL_COORDINATOR.is_file():
    prod = production_text(MODEL_COORDINATOR)
    if "pub(crate) async fn orchestrate_model_invocation" not in prod:
        violations.append("14. Model coordinator must expose the shared orchestration")
    if len(re.findall(r'async\s+fn\s+invoke_model_impl\s*\(', prod)) != 1:
        violations.append("14. Model coordinator must own exactly one invoke_model_impl")
for adapter in MODEL_ADAPTERS:
    if not adapter.is_file():
        continue
    prod = production_text(adapter)
    for retired in ["async fn invoke_model_impl", "ModelInvocationCoordinator::new()", "provider.invoke("]:
        if retired in prod:
            violations.append(f"14. Role adapter retains model orchestration '{retired}': {adapter}")

# ── 15. P6.4 one tool-round orchestration ──
TOOL_COORDINATOR = root / "agent/features/runtime/src/application/tool/coordination.rs"
TOOL_ADAPTERS = [
    root / "agent/features/runtime/src/application/loop_engine/chat/main_run_port.rs",
    root / "agent/features/runtime/src/application/run/derived/loop_run.rs",
]
if TOOL_COORDINATOR.is_file():
    prod = production_text(TOOL_COORDINATOR)
    if "pub(crate) struct ToolRoundContext" not in prod:
        violations.append("15. Tool coordinator must receive an explicit ToolRoundContext")
    if "pub(crate) trait ToolRoundObserver" not in prod:
        violations.append("15. Tool coordinator must expose a narrow ToolRoundObserver")
    if "pub struct ToolRoundOutcome" not in prod or "pub enum ToolRoundContinuation" not in prod:
        violations.append("15. Tool coordinator must return an explicit continuation outcome")
    for retired in ["ToolRoundProjection", "mark_tool_results_pending"]:
        if retired in prod:
            violations.append(f"15. Tool coordinator retains vague boundary '{retired}'")
    if "pub(crate) async fn orchestrate_tool_round" not in prod:
        violations.append("15. Tool coordinator must expose the shared round orchestration")
    if len(re.findall(r'async\s+fn\s+execute_tools_impl\s*<', prod)) != 1:
        violations.append("15. Tool coordinator must own exactly one execute_tools_impl")
for adapter in TOOL_ADAPTERS:
    if not adapter.is_file():
        continue
    prod = production_text(adapter)
    for retired in ["ToolRoundProjection", "mark_tool_results_pending", "async fn execute_tools_impl", "prepare_tool_round(", "execute_tool_round(", "agent.execute_prepared_tools("]:
        if retired in prod:
            violations.append(f"15. Role adapter retains tool orchestration '{retired}': {adapter}")

# ── Report ──
if violations:
    print(json.dumps({"decision": "block", "reason": "Runtime Capability Assembly guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Runtime Capability Assembly guard OK.")
PY
