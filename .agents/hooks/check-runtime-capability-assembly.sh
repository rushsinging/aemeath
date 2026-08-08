#!/bin/bash
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
RUN_FACTORY = root / "agent/features/runtime/src/application/run/factory.rs"
RUN_CREATION = root / "agent/features/runtime/src/application/run/creation.rs"
RUN_LAUNCHER = root / "agent/features/runtime/src/application/run/launcher.rs"
RUN_MODULE = root / "agent/features/runtime/src/application/run.rs"
APPLICATION_MODULE = root / "agent/features/runtime/src/application.rs"
RUNTIME_LIB = root / "agent/features/runtime/src/lib.rs"
ENGINE = root / "agent/features/runtime/src/application/loop_engine/engine.rs"
ENGINE_RESPONSIBILITIES = root / "agent/features/runtime/src/application/loop_engine/engine"
RUN_LOOP = root / "agent/features/runtime/src/application/loop_engine/run_loop.rs"
STUCK = root / "agent/features/runtime/src/application/loop_engine/stuck_guard.rs"
RUN_DOMAIN = root / "agent/features/runtime/src/domain/agent_run/domain.rs"
RUN_DOMAIN_EVENT = root / "agent/features/runtime/src/domain/agent_run/event.rs"
RUN_DOMAIN_STATE = root / "agent/features/runtime/src/domain/agent_run/state.rs"
RUNTIME_STREAM_EVENT = root / "agent/features/runtime/src/application/loop_engine/chat/events.rs"
ACTIVE_RUN_REGISTRY = root / "agent/features/runtime/src/application/run/active_registry.rs"
SDK_RUN = root / "packages/sdk/src/run.rs"
SDK_CHAT_EVENT = root / "packages/sdk/src/chat_event.rs"
SDK_WIRE = root / "packages/sdk/src/wire.rs"
INTERACTION = root / "agent/features/runtime/src/application/interaction/port.rs"
EMPTY_HOOK = root / "agent/features/runtime/src/application/hook/empty.rs"
RUNTIME_SOURCE = root / "agent/features/runtime/src"
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
        comment_index = line.find('//')
        if comment_index >= 0:
            # Don't strip if // appears inside a string literal (crude check)
            before = line[:comment_index]
            if before.count('"') % 2 == 0:  # even number of quotes = not in string
                line = before
        lines.append(line)
    return '\n'.join(lines)

def strip_string_literals(source: str) -> str:
    """Mask Rust string and character literals before call-site scans."""
    literal_pattern = re.compile(r'r(?P<hashes>#+)?".*?"(?P=hashes)|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'', re.S)
    return literal_pattern.sub('""', source)


def remove_cfg_test_items(text: str) -> str:
    """Remove each item annotated with ``#[cfg(test)]`` without truncating later code."""
    marker = "#[cfg(test)]"
    while True:
        item_start = text.find(marker)
        if item_start < 0:
            return text

        item_end = cfg_test_item_end(text, item_start)
        if item_end is None:
            return text[:item_start]
        text = text[:item_start] + text[item_end:]


def cfg_test_item_end(text: str, item_start: int) -> int | None:
    marker = "#[cfg(test)]"
    cursor = item_start + len(marker)
    while True:
        while cursor < len(text) and text[cursor].isspace():
            cursor += 1
        if cursor >= len(text) or not text.startswith("#[", cursor):
            break
        attribute_end = text.find("]", cursor + 2)
        if attribute_end < 0:
            return None
        cursor = attribute_end + 1

    semicolon = text.find(";", cursor)
    opening_brace = text.find("{", cursor)
    if semicolon >= 0 and (opening_brace < 0 or semicolon < opening_brace):
        return semicolon + 1
    if opening_brace < 0:
        return None

    depth = 1
    item_end = opening_brace + 1
    while item_end < len(text) and depth > 0:
        if text[item_end] == "{":
            depth += 1
        elif text[item_end] == "}":
            depth -= 1
        item_end += 1
    return item_end if depth == 0 else None


def cfg_test_items(text: str) -> list[str]:
    """Return complete items annotated with ``#[cfg(test)]``."""
    items = []
    marker = "#[cfg(test)]"
    search_from = 0
    while True:
        item_start = text.find(marker, search_from)
        if item_start < 0:
            return items
        item_end = cfg_test_item_end(text, item_start)
        if item_end is None:
            return items
        items.append(text[item_start:item_end])
        search_from = item_end


def production_text(path: Path) -> str:
    """Return non-test source with comments stripped.

    Runtime tests live both in separate test files and in item-level
    ``#[cfg(test)]`` modules. Removing each test item preserves production items
    declared after test-only modules while keeping structural checks test-blind.
    """
    if not path.is_file():
        return ""
    return strip_comments(remove_cfg_test_items(path.read_text()))


def rust_source_paths() -> list[Path]:
    return sorted(RUNTIME_SOURCE.rglob("*.rs"))


def is_test_source(path: Path) -> bool:
    return path.stem == "tests" or "_test" in path.stem or "tests" in path.parts


def struct_body(source: str, struct_name: str) -> str:
    declaration = re.search(
        rf'\b(?:pub(?:\([^)]*\))?\s+)?struct\s+{re.escape(struct_name)}\s*\{{',
        source,
    )
    if declaration is None:
        return ""
    body_start = declaration.end()
    depth = 1
    cursor = body_start
    while cursor < len(source) and depth > 0:
        if source[cursor] == "{":
            depth += 1
        elif source[cursor] == "}":
            depth -= 1
        cursor += 1
    return source[body_start : cursor - 1] if depth == 0 else ""


def root_exported_names(source: str) -> set[str]:
    names = set()
    for export_match in re.finditer(r'(?ms)^pub\s+use\s+([^;]+);', source):
        expression = export_match.group(1).strip()
        if "*" in expression:
            names.add("*")
            continue
        if "{" in expression:
            members = expression.split("{", 1)[1].rsplit("}", 1)[0]
            for member in members.split(","):
                member = member.strip()
                if not member or member == "self":
                    continue
                names.add(member.split(" as ")[-1].strip())
        else:
            names.add(expression.split("::")[-1].split(" as ")[-1].strip())
    return names


import glob as _glob

# ── 1. RuntimeContext creation has one production algorithm and no test alternate ──
for source_path in rust_source_paths():
    source = source_path.read_text()
    source_without_literals = strip_string_literals(source)
    production_source = production_text(source_path)
    if (
        source_path.resolve() != FACTORY.resolve()
        and re.search(
            r'RuntimeContext::new\s*\(',
            strip_string_literals(production_source),
        )
    ):
        violations.append(f"1. RuntimeContext::new( outside factory: {source_path}")
    if source_path.resolve() == FACTORY.resolve():
        for test_item in cfg_test_items(source_without_literals):
            if "alternate_context_creator" in test_item and (
                re.search(r'RuntimeContext::new\s*\(', test_item)
                or re.search(r'->\s*(?:Result\s*<\s*)?RuntimeContext\b', test_item)
            ):
                violations.append(
                    "1. RuntimeContext creation must not have a test-only alternate entry"
                )
                break

# ── 2. RuntimeContextAssemblyToken creation stays in the production factory algorithm ──
for source_path in rust_source_paths():
    source = source_path.read_text()
    source_without_literals = strip_string_literals(source)
    production_source = strip_string_literals(production_text(source_path))
    test_source = "\n".join(cfg_test_items(source_without_literals))
    if (
        "RuntimeContextAssemblyToken::new_for_test" in source_without_literals
        or (
            source_path.resolve() != FACTORY.resolve()
            and re.search(r'RuntimeContextAssemblyToken::new\s*\(\)', production_source)
        )
        or (
            source_path.resolve() != FACTORY.resolve()
            and re.search(r'RuntimeContextAssemblyToken::new\s*\(\)', test_source)
        )
    ):
        violations.append(
            f"2. RuntimeContextAssemblyToken::new has an unapproved caller: {source_path}"
        )

if FACTORY.is_file():
    prod = production_text(FACTORY)
    if not re.search(r'RuntimeContextAssemblyToken::new\s*\(\)', prod):
        violations.append("2. Factory must construct RuntimeContextAssemblyToken")

# ── 2a. RuntimeContextFactory::prepare has one approved caller ──
for source_path in rust_source_paths():
    source = strip_string_literals(strip_comments(source_path.read_text()))
    if source_path.resolve() == FACTORY.resolve():
        source = re.sub(r'fn\s+prepare\s*\(', 'fn approved_prepare_definition(', source, count=1)
    prepare_calls = re.findall(r'(?:\.|::)prepare\s*\(', source)
    if prepare_calls and source_path.resolve() != RUN_FACTORY.resolve():
        violations.append(
            f"2a. RuntimeContextFactory::prepare has an unapproved caller: {source_path}"
        )

# ── 2b. RunInstance::new has one approved caller ──
for source_path in rust_source_paths():
    source = strip_string_literals(strip_comments(source_path.read_text()))
    if re.search(r'RunInstance::new\s*\(', source) and source_path.resolve() != RUN_FACTORY.resolve():
        violations.append(f"2b. RunInstance::new has an unapproved caller: {source_path}")

# ── 3. Retired symbols absent from production code ──
RETIRED = {
    r'\bRuntimeContextParts\b': "RuntimeContextParts struct",
    r'\bRuntimeResources\b': "RuntimeResources container",
    r'\bChatRuntimeContext\b': "ChatRuntimeContext wrapper",
    r'\bChatLoopContext\b': "ChatLoopContext compatibility parameter bag",
    r'\bRunLoopPort\b': "fat RunLoopPort",
    r'\bMainRunPort\b': "MainRunPort role adapter",
    r'\bSubAgentRun\b': "SubAgentRun role adapter",
    r'\bassemble_main_runtime_context\b': "assemble_main_runtime_context function",
    r'\bModelStep::StopHookBlocked\b': "ModelStep::StopHookBlocked variant",
    r'\bInteractionBridge::disabled\b': "InteractionBridge::disabled() method",
}
for candidate in _glob.glob("agent/**/*.rs", recursive=True):
    candidate_path = Path(candidate)
    if "_test" in candidate_path.stem or candidate_path.stem == "tests":
        continue
    if "/tests/" in str(candidate_path):
        continue
    production_source = production_text(candidate_path)
    for retired_pattern, retired_name in RETIRED.items():
        if re.search(retired_pattern, production_source):
            violations.append(f"3. Retired '{retired_name}' in production: {candidate_path}")

# ── 3a. Runtime application must not construct concrete adapters ──
for source_path in rust_source_paths():
    if "/application/" not in source_path.as_posix() or is_test_source(source_path):
        continue
    production_source = production_text(source_path)
    if re.search(r'\bcrate\s*::\s*adapters\s*::', production_source):
        violations.append(
            f"3a. Runtime application constructs or imports a concrete adapter: {source_path}"
        )

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
    if ENGINE_RESPONSIBILITIES.is_dir():
        prod += "\n" + "\n".join(
            production_text(path) for path in sorted(ENGINE_RESPONSIBILITIES.glob("*.rs"))
        )
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

# ── 8. Reasoning capability has one production selector ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if "fn select_reasoning_port(" not in prod:
        violations.append("8. Factory must own the single production reasoning selector")
    if "ReasoningSelection" not in prod:
        violations.append("8. Factory reasoning selector must return a narrow typed selection")
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
    if "point.metadata().class == HookClass::Boundary" not in prod:
        violations.append("9. BoundaryHookPort must derive filtering from HookPointMetadata.class")
    if re.search(r'matches!\s*\(\s*point\s*,\s*HookPoint::', prod):
        violations.append("9. BoundaryHookPort must not duplicate a HookPoint variant allow-list")
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

# ── 12. RuntimeContext production creation stays factory-private ──
if FACTORY.is_file():
    prod = production_text(FACTORY)
    if "pub(crate) fn prepare" not in prod:
        violations.append("12. Factory must expose one crate-private preparation entry")
    if re.search(r'pub\s+fn\s+(?:prepare|assemble|create)\s*\(', prod):
        violations.append("12. Factory production preparation must not be public")
    if "RunCreationError" not in prod:
        violations.append("12. Factory preparation must return typed RunCreationError")

# ── 13. P6.9.9 pure-value Run creation entry and aggregate launch ──
MAIN_CALLERS = [
    root / "agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs",
    root / "agent/features/runtime/src/application/loop_engine/chat/session_driver/run_preparation.rs",
]
DERIVED_CALLER = root / "agent/features/runtime/src/application/run/derived/setup.rs"
DERIVED_LAUNCHER = root / "agent/features/runtime/src/application/run/derived/loop_run.rs"
if RUN_FACTORY.is_file():
    prod = production_text(RUN_FACTORY)
    signature_match = re.search(
        r'pub\(crate\)\s+fn\s+create\s*\(\s*&self,\s*request:\s*RunCreationRequest,?\s*\)\s*->\s*Result<RunInstance,\s*RunCreationError>',
        prod,
        re.S,
    )
    if not signature_match:
        violations.append("13. RunFactory::create must accept only RunCreationRequest and return RunInstance")
    for retired in ["RunCapabilityBindings", "RunContextBindings", "RuntimeContextParts", "RunCreationBindings,"]:
        signature = signature_match.group(0) if signature_match else ""
        if retired in signature:
            violations.append(f"13. RunFactory::create signature must not expose {retired}")
else:
    violations.append("13. RunFactory implementation is missing")

if RUN_CREATION.is_file():
    prod = production_text(RUN_CREATION)
    if "pub struct RunInstance" not in prod:
        violations.append("13. RunInstance aggregate is missing")
    if re.search(r'pub\s+fn\s+into_parts\s*\(', prod):
        violations.append("13. RunInstance must not expose public aggregate unpacking")
else:
    violations.append("13. Run creation model is missing")

if RUN_LAUNCHER.is_file():
    prod = production_text(RUN_LAUNCHER)
    if not re.search(r'pub\s+async\s+fn\s+launch\s*\(\s*instance:\s*&mut\s+RunInstance', prod):
        violations.append("13. RunLauncher::launch must consume a complete mutable RunInstance")
    for retired in ["launch_prepared", "mut run: Run", "execution: &mut RunExecutionState"]:
        if retired in prod:
            violations.append(f"13. RunLauncher retains split or legacy launch shape: {retired}")

for caller in [*MAIN_CALLERS, DERIVED_CALLER]:
    if not caller.is_file():
        continue
    prod = production_text(caller)
    for retired in ["RunCapabilityBindings", "RunContextBindings", "RuntimeContextParts", "SubRunCapabilitySource", "RunPreparer", "PreparedRun", "RunPreparationRequest", "PreparedSubRun"]:
        if retired in prod:
            violations.append(f"13. Production Run caller retains retired shape {retired}: {caller}")
    if re.search(r'RuntimeContext::new\s*\(', prod):
        violations.append(f"13. Production Run caller constructs RuntimeContext directly: {caller}")
    if "run_instance.into_parts()" in prod:
        violations.append(f"13. Production Run caller unpacks RunInstance before launch: {caller}")

main_prod = "\n".join(production_text(caller) for caller in MAIN_CALLERS if caller.is_file())
if main_prod:
    if "run_factory.create(preparation.request)" not in main_prod or "run::launcher::launch(" not in main_prod:
        violations.append("13. Main Run must use RunFactory::create and RunLauncher::launch")
if DERIVED_CALLER.is_file():
    prod = production_text(DERIVED_CALLER)
    if not re.search(r'run_factory\.create\s*\(\s*creation_request\s*\)', prod):
        violations.append("13. Derived Run must use RunFactory::create")
if DERIVED_LAUNCHER.is_file():
    prod = production_text(DERIVED_LAUNCHER)
    if "run::launcher::launch(instance" not in prod:
        violations.append("13. Derived Run must pass RunInstance to RunLauncher::launch")

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
    if "pub(crate) struct ToolRoundCoordinator" not in prod:
        violations.append("15. Tool coordinator must expose the shared round owner")
    if "pub(crate) fn new(context: ToolRoundContext" not in prod:
        violations.append("15. ToolRoundCoordinator must own explicit context construction")
    if "pub(crate) async fn execute(" not in prod:
        violations.append("15. ToolRoundCoordinator must expose the shared round execution")
    if len(re.findall(r'async\s+fn\s+execute_tools_impl\s*<', prod)) != 1:
        violations.append("15. Tool coordinator must own exactly one execute_tools_impl")
for adapter in TOOL_ADAPTERS:
    if not adapter.is_file():
        continue
    prod = production_text(adapter)
    for retired in ["ToolRoundProjection", "mark_tool_results_pending", "async fn execute_tools_impl", "prepare_tool_round(", "execute_tool_round(", "agent.execute_prepared_tools("]:
        if retired in prod:
            violations.append(f"15. Role adapter retains tool orchestration '{retired}': {adapter}")

# ── 16. Runtime names describe one responsibility; broad Projection names are forbidden ──
# A mapper may transform values, but "Projection" must not hide a mixed lifecycle,
# state owner, module, type, trait, function, method, or local binding.
for candidate in _glob.glob("agent/features/runtime/src/**/*.rs", recursive=True):
    path = Path(candidate)
    if is_test_source(path):
        continue
    production = production_text(path)
    if re.search(r'\b[A-Za-z][A-Za-z0-9_]*Projection[A-Za-z0-9_]*\b', production):
        violations.append(f"16. Runtime production type/trait uses broad Projection naming: {path}")
    if re.search(r'\b(?:projection_[A-Za-z0-9_]+|[A-Za-z0-9_]+_projection)\b', production):
        violations.append(f"16. Runtime production identifier uses broad projection naming: {path}")

# ── 17. Runtime exposes only explicit façades ──
for module_name in ["adapters", "application", "domain", "ports"]:
    if not re.search(rf'^pub\(crate\)\s+mod\s+{module_name}\s*;', production_text(RUNTIME_LIB), re.M):
        violations.append(f"17. Runtime module '{module_name}' must be crate-private")
if APPLICATION_MODULE.is_file():
    application_source = production_text(APPLICATION_MODULE)
    for module_match in re.finditer(r'^pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;', application_source, re.M):
        violations.append(
            f"17. Runtime application module '{module_match.group(1)}' must be crate-private"
        )

runtime_lib_source = production_text(RUNTIME_LIB)
if re.search(r'(?m)^pub\s+use\s+[^;]*\*\s*;', runtime_lib_source):
    violations.append("17. Runtime crate root must not use wildcard exports")
approved_root_exports = {
    "ActiveRunRegistry",
    "AgentClient",
    "AgentClientImpl",
    "AgentRunnerAssembly",
    "AtomicBlobToolResultStore",
    "ChangeSet",
    "ChatEvent",
    "ChatRequest",
    "ChatStream",
    "CompleteReflectionResult",
    "CostInfo",
    "InitialProviderAssembly",
    "ModelRuntimeSettings",
    "ParentRunContextSource",
    "ProjectContext",
    "PromptAssembly",
    "PromptContext",
    "ProviderBinding",
    "ProviderBuildSpec",
    "ProviderFactory",
    "ProviderPort",
    "ProviderCompactGenerator",
    "ReflectionError",
    "ReflectionTaskAdapter",
    "ReflectionTaskCompletion",
    "ReflectionTaskCompletionStatus",
    "ReflectionTaskMetadata",
    "ReflectionTaskRequest",
    "ReflectionTaskSubmitOutcome",
    "ReflectionTaskTrigger",
    "ResumeError",
    "RuntimeLifecycleEvent",
    "RuntimeBootstrapDependencies",
    "RuntimeContextFactory",
    "RuntimeCoreDependencies",
    "RuntimeIngressAssembly",
    "RuntimeToolAssemblyDependencies",
    "SessionBootstrapAssembly",
    "SkillBootstrapAssembly",
    "TaskSummary",
    "ToolResultBlobError",
    "ToolResultBlobPort",
    "ToolResultBlobRef",
    "ToolResultMaterializationPolicy",
    "ToolResultMaterializer",
    "build_agent_runner",
    "build_static_prompt",
    "build_system_prompt_parts",
    "config_snapshot_to_sdk",
    "from_args_with_workspace",
    "map_lifecycle_event",
    "resolve_concurrency_limits",
    "resolve_model_runtime_settings",
    "resume_session_to_backing",
}
for exported_name in sorted(root_exported_names(runtime_lib_source) - approved_root_exports):
    violations.append(f"17. Runtime crate root exposes unapproved façade symbol: {exported_name}")

if RUN_MODULE.is_file():
    run_module_source = production_text(RUN_MODULE)
    for module_match in re.finditer(r'^pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;', run_module_source, re.M):
        violations.append(
            f"17. Runtime Run module '{module_match.group(1)}' must be crate-private"
        )
    if re.search(r'(?m)^pub(?:\(crate\))?\s+use\s+[^;]*\*\s*;', run_module_source):
        violations.append("17. Runtime Run façade must not use wildcard exports")

# ── 18. Facts, snapshots, and requests contain pure values only ──
creation_source = production_text(RUN_CREATION)
pure_value_types = ["SessionSnapshot", "ParentRunFacts", "RunCreationRequest"]
for type_name in pure_value_types:
    body = struct_body(creation_source, type_name)
    if not body:
        violations.append(f"18. Pure-value Runtime type is missing: {type_name}")
        continue
    forbidden_live_values = [
        r'\bArc\s*<',
        r'\bBox\s*<\s*dyn\b',
        r'\bdyn\s+[A-Za-z_]',
        r'\bMutex\s*<',
        r'\bRwLock\s*<',
        r'\bSender\s*<',
        r'\bReceiver\s*<',
        r'\bRuntimeContext\b',
        r'\bRuntimeWorkspaceAccess\b',
        r'\bRunCreationBindings\b',
        r'\bSessionRunBindings\b',
        r'\bParentRunBindings\b',
        r'\bTypeId\b',
        r'\bAny\b',
    ]
    if any(re.search(pattern, body) for pattern in forbidden_live_values):
        violations.append(f"18. {type_name} must contain pure values only")

# ── 19. Loop capabilities remain structurally narrow in production and tests ──
loop_capability_names = {
    "InputPort",
    "EventSinkPort",
    "RunControlPort",
    "RunLifecyclePort",
    "InteractionMailboxPort",
    "StepPersistencePort",
    "CompactionPort",
    "ModelInvocationPort",
    "ToolOrchestrationPort",
    "StuckHandlingPort",
    "PlanApprovalPort",
}
for source_path in rust_source_paths():
    source = strip_comments(source_path.read_text())
    source_without_literals = re.sub(r'"(?:\\.|[^"\\])*"', '""', source)
    for trait_match in re.finditer(
        r'\btrait\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^\{;]+)', source_without_literals
    ):
        inherited = set(re.findall(r'\b[A-Za-z_][A-Za-z0-9_]*Port\b', trait_match.group(2)))
        if len(inherited & loop_capability_names) > 1:
            violations.append(
                "19. Runtime must not define a trait that aggregates multiple Loop capability categories"
            )
    for alias_match in re.finditer(r'\btype\s+[A-Za-z_][A-Za-z0-9_]*\s*=\s*([^;]+);', source_without_literals):
        aliased = set(re.findall(r'\b[A-Za-z_][A-Za-z0-9_]*Port\b', alias_match.group(1)))
        if len(aliased & loop_capability_names) > 1:
            violations.append(
                "19. Runtime must not define a trait-object alias that aggregates multiple Loop capability categories"
            )
    implementations = {}
    for implementation_match in re.finditer(
        r'\bimpl(?:\s*<[^\{;]*?>)?\s+([A-Za-z_][A-Za-z0-9_]*Port)\s+for\s+([A-Za-z_][A-Za-z0-9_]*)',
        source_without_literals,
    ):
        port_name, type_name = implementation_match.groups()
        if port_name in loop_capability_names:
            implementations.setdefault(type_name, set()).add(port_name)
    for type_name, ports in implementations.items():
        if len(ports) > 1:
            violations.append(
                f"19. Runtime type {type_name} implements multiple Loop capability categories"
            )
if RUN_LOOP.is_file():
    run_loop_source = production_text(RUN_LOOP)
    if re.search(r'\bimpl(?:\s*<[^\{;]*?>)?\s+(?:Clone|[A-Za-z_][A-Za-z0-9_]*Port)\s+for\s+RunLoop\b', run_loop_source):
        violations.append("19. RunLoop must orchestrate narrow ports without implementing them or Clone")

# ── 20. Runtime has no dynamic capability locator ──
for source_path in rust_source_paths():
    if is_test_source(source_path):
        continue
    production = production_text(source_path)
    if re.search(r'\b(?:dyn\s+Any|TypeId|service_locator|capability_map|service_map)\b', production):
        violations.append(f"20. Runtime production code contains a dynamic capability locator: {source_path}")

# ── 21. Run lifecycle exposes Step cancellation and typed Run termination only ──
retired_run_lifecycle_symbols = {
    RUN_DOMAIN_STATE: [
        r'(?s)enum\s+RunStatus\s*\{[^}]*\bCancelling\b',
        r'(?s)enum\s+RunStatus\s*\{[^}]*\bCancelled\b',
        r'\bCancellationFinished\b',
    ],
    RUN_DOMAIN: [r'\bRunCancellationRequest\b', r'\brequest_cancellation\b', r'\bfinish_cancellation\b'],
    RUN_DOMAIN_EVENT: [r'\bCancellationRequested\b', r'\bCancelled\b'],
    RUNTIME_STREAM_EVENT: [r'\bRunCancelling\b', r'\bRunCancelled\b'],
    SDK_RUN: [r'\bCancelRunOutcome\b'],
    SDK_CHAT_EVENT: [r'\bRunCancelling\b', r'\bRunCancelled\b'],
    SDK_WIRE: [r'\bCancelRunOutcome\b'],
}
for source_path, retired_patterns in retired_run_lifecycle_symbols.items():
    production = production_text(source_path)
    for retired_pattern in retired_patterns:
        if re.search(retired_pattern, production):
            violations.append(
                f"21. Retired Run cancellation lifecycle symbol matches {retired_pattern}: {source_path}"
            )

registry_source = production_text(ACTIVE_RUN_REGISTRY)
for lifecycle_copy in [
    r'\bcancelling\s*:\s*',
    r'\bterminal\s*:\s*',
    r'\bclaim_cancellation\b',
    r'\bclaim_terminal\b',
]:
    if re.search(lifecycle_copy, registry_source):
        violations.append(
            f"21. ActiveRunRegistry retains a parallel Run lifecycle owner matching {lifecycle_copy}"
        )

# ── Report ──
if violations:
    print(json.dumps({"decision": "block", "reason": "Runtime Capability Assembly guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Runtime Capability Assembly guard OK.")
PY
