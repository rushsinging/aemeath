#!/usr/bin/env bash
set -euo pipefail

# 功能：检查未迁移 feature 访问只经 `<feature>::api`，并锁定已迁移 feature 的
#       crate-root 窄 façade。
# 作用：禁止穿透对方 contract/gateway/core/business/utils 内部路径；Storage 与 Runtime
#       只允许经显式登记的 crate-root Published Language / production 入口访问。
# 例外：无路径级白名单。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
FEATURE_CRATES = {
    "runtime",
    "project",
    "policy",
    "context",
    "memory",
    "provider",
    "tools",
    "storage",
    "hook",
    "audit",
    "update",
    "workflow",
}
INTERNAL_SEGMENTS = {"contract", "gateway", "core", "business", "utils"}
API_FACADE_ALLOWED_SEGMENTS = {"contract", "gateway"}
ROOT_REEXPORT_ALLOW = {
    "project": {"ProjectContext"},
}
PROJECT_ROOT_ACCESS_ALLOW = {
    "GitOperationError",
    "GitProbeError",
    "PreparedWorkspaceRestore",
    "ProjectIdentity",
    "WorkspaceControl",
    "WorkspaceError",
    "WorkspaceFrame",
    "WorkspaceId",
    "WorkspaceInitError",
    "WorkspacePersist",
    "WorkspaceRead",
    "WorkspaceRestoreError",
    "WorkspaceViews",
    "WorkspaceWiring",
    "WorktreeKind",
    "wire_production_workspace",
}
PROJECT_ROOT_PUBLIC_ALLOW = PROJECT_ROOT_ACCESS_ALLOW | {"LOG_TARGET"}
# 已迁移 feature 的目标 façade 位于 crate 根；集合必须保持窄且由真实消费者证明。
# Tools crate-root façade (#993): tools 已迁到 domain/adapters 六边形物理层，
# 只经 crate 根发布 Published Language，禁止恢复 tools::api。
TOOLS_DOMAIN_FACADE = {
    "AgentProgressEvent",
    "AgentProgressKind",
    "AgentRunRequest",
    "AgentRunTerminal",
    "AgentRunner",
    "AgentToolCallProgress",
    "ImageData",
    "PolicyDecision",
    "ProfileExpansionError",
    "RegistryScopeName",
    "SessionReminder",
    "SessionReminders",
    "Tool",
    "ToolCapabilities",
    "ToolCapability",
    "ToolCatalogPort",
    "ToolCatalogSnapshot",
    "ToolExecutionContext",
    "ToolExecutionOutcome",
    "ToolExecutionPort",
    "ToolInvocation",
    "ToolListProvider",
    "ToolOutcome",
    "ToolProfile",
    "ToolProfileName",
    "ToolResources",
    "ToolResult",
    "TypedTool",
    "TypedToolAdapter",
    "TypedToolResult",
}
TOOLS_ADAPTER_FACADE = {
    "is_readonly_command",
    "register_all_tools",
    "register_all_tools_except_agent",
    "register_subagent_tools",
    "wire_tools",
    "DefaultToolCatalogGateway",
    "McpConnectionManager",
    "McpServerConfig",
    "McpToolDef",
    "McpTransportKind",
    "McpTool",
    "ToolCatalog",
    "ToolCatalogGateway",
    "ToolRegistry",
}
TOOLS_ROOT_ACCESS_ALLOW = {"LOG_TARGET", "types"} | TOOLS_DOMAIN_FACADE | TOOLS_ADAPTER_FACADE

ROOT_ACCESS_ALLOW = {
    "provider": {
        "CancellationSignal",
        "InvocationDelta",
        "InvocationEvent",
        "InvocationOptions",
        "InvocationRequest",
        "InvocationStream",
        "InvocationScope",
        "LlmClient",
        "LlmConfigOptions",
        "LlmError",
        "LlmProvider",
        "LlmProviderGateway",
        "LlmClientPool",
        "ModelCapability",
        "ModelId",
        "ModelToolSchema",
        "ProviderCompletion",
        "ProviderContentBlock",
        "ProviderDriverKind",
        "ProviderError",
        "ProviderErrorKind",
        "ProviderStopReason",
        "ProviderToolCall",
        "ProviderToolCallId",
        "RawUsageSnapshot",
        "ReasoningCapability",
        "ReasoningLevel",
        "ReasoningMappingKind",
        "StopReason",
        "StreamResponse",
        "SystemBlock",
        "Usage",
        "DEFAULT_TIMEOUT_SECS",
        "wire_provider",
    },
    "audit": {
        "AppendLogError",
        "AppendLogLine",
        "AppendLogNamespace",
        "AppendLogReader",
        "AppendLogStream",
        "FileUsageAppendStore",
        "Pagination",
        "TimeRange",
        "UsageAppendStorePort",
        "UsagePipelineMetricsSnapshot",
        "UsageSender",
        "UsageShutdownOutcome",
        "UsageWorkerConfig",
        "UsageWorkerHandle",
        "UsageCursor",
        "UsageDropReason",
        "UsageEmitOutcome",
        "UsageEnvelopeV1",
        "UsagePage",
        "UsageQuery",
        "UsageQueryError",
        "UsageQueryPort",
        "UsageQueryWarning",
        "UsageRecord",
        "UsageSummary",
        "CURRENT_USAGE_SCHEMA_VERSION",
        "file_usage_append_store",
        "start_usage_worker",
    },
    "runtime": {
        "AgentClientImpl",
        "RuntimeBootstrapDependencies",
        "RuntimeConfigDependencies",
        "UsageSink",
        "from_args_with_workspace",
    },
      "policy": set(),
    "workflow": set(),
    "project": PROJECT_ROOT_ACCESS_ALLOW,
    "tools": TOOLS_ROOT_ACCESS_ALLOW,
    # Context 的 Target façade 位于 crate 根；只允许访问这些稳定发布模块。
    "context": {"compact", "context_port", "domain", "guidance", "session", "skill", "compose_session_task_capture", "LegacyTaskCapture"},
    # Storage 的 #991 过渡 façade；最终随 #880/#983/#883/#884 收敛。
    "storage": {
        "Batch",
        "BatchStatus",
        "MemoryStore",
        "SafeOpenOptions",
        "SafePathSegment",
        "SafeStorageDir",
        "SafeStorageEntry",
        "SafeStorageFileType",
        "SafeStorageRoot",
        "Task",
        "TaskPriority",
        "TaskSnapshot",
        "TaskStatus",
        "TaskStore",
        "memory_base_dir",
        "project_file_name",
        "project_file_name_from_path",
    },
}

CONTEXT_FORBIDDEN_PATHS = {
    "agent/features/context/src/api.rs",
    "agent/features/context/src/gateway.rs",
    "agent/features/context/src/capabilities",
}
POLICY_FORBIDDEN_PATHS = {
    "agent/features/policy/src/api.rs",
    "agent/features/policy/src/api",
    "agent/features/policy/src/business.rs",
    "agent/features/policy/src/business",
    "agent/features/policy/src/contract.rs",
    "agent/features/policy/src/contract",
    "agent/features/policy/src/core.rs",
    "agent/features/policy/src/core",
    "agent/features/policy/src/gateway.rs",
    "agent/features/policy/src/gateway",
    "agent/features/policy/src/capabilities.rs",
    "agent/features/policy/src/capabilities",
}
PROJECT_FORBIDDEN_PATHS = {
    "agent/features/project/src/api.rs",
    "agent/features/project/src/business.rs",
    "agent/features/project/src/business",
    "agent/features/project/src/contract.rs",
    "agent/features/project/src/contract",
    "agent/features/project/src/core.rs",
    "agent/features/project/src/core",
    "agent/features/project/src/gateway.rs",
    "agent/features/project/src/gateway",
    "agent/features/project/src/capabilities.rs",
    "agent/features/project/src/capabilities",
}

path_pattern = re.compile(
    r"(?<![A-Za-z0-9_:])(?:::)?("
    + "|".join(sorted(map(re.escape, FEATURE_CRATES)))
    + r")::([A-Za-z_][A-Za-z0-9_]*)"
)
braced_pattern = re.compile(
    r"(?<![A-Za-z0-9_:])(?:::)?("
    + "|".join(sorted(map(re.escape, FEATURE_CRATES)))
    + r")::\s*\{([^}]*)"
)
tools_legacy_api_pattern = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?tools\s*::\s*api\b")
crate_reexport_pattern = re.compile(r"crate::([A-Za-z_][A-Za-z0-9_]*)")
project_wildcard_pattern = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?project\s*::\s*\*")
audit_wildcard_pattern = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?audit\s*::\s*\*")
project_braced_full_pattern = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?project\s*::\s*\{(.*?)\}", re.DOTALL)
project_path_full_pattern = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?project\s*::\s*([A-Za-z_][A-Za-z0-9_]*)", re.DOTALL)
project_pub_mod_pattern = re.compile(r"^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\b", re.MULTILINE)
project_pub_item_pattern = re.compile(
    r"^\s*pub\s+(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\"[^\"]+\"\s+)?"
    r"(fn|struct|enum|trait|type|static|union|macro)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\b",
    re.MULTILINE,
)
project_pub_const_pattern = re.compile(r"^\s*pub\s+const\s+([A-Za-z_][A-Za-z0-9_]*)\b", re.MULTILINE)
project_pub_use_pattern = re.compile(r"^\s*pub\s+use\s+([^;]+);", re.MULTILINE | re.DOTALL)


def crate_for(path: Path) -> str | None:
    parts = path.parts
    if len(parts) >= 3 and parts[0] == "agent":
        if parts[1] == "shared":
            return "share"
        if parts[1] == "features" and len(parts) >= 4:
            return parts[2]
        return parts[1]
    if len(parts) >= 2 and parts[0] == "packages":
        return parts[1]
    if len(parts) >= 2 and parts[0] == "apps":
        return parts[1]
    return None


def is_generated_or_target(path: Path) -> bool:
    rel = path.as_posix()
    return "/target/" in rel or rel.startswith("target/")


def top_level_items(inner: str) -> list[str]:
    items = []
    depth = 0
    start = 0
    for index, char in enumerate(inner):
        if char == "{":
            depth += 1
        elif char == "}":
            depth = max(0, depth - 1)
        elif char == "," and depth == 0:
            items.append(inner[start:index].strip())
            start = index + 1
    tail = inner[start:].strip()
    if tail:
        items.append(tail)
    return items


def check_cross_crate_line(
    current_crate: str | None, line: str, local_modules: set[str] | None = None
) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//") or stripped.startswith("*"):
        return []

    # 剥离字符串字面量内容，避免把日志 target 字符串（如 "tools::audit"）
    # 误判为跨 crate 模块访问。
    line = re.sub(r'"[^"]*"', '""', line)

    violations: list[str] = []
    if audit_wildcard_pattern.search(line) and current_crate not in {"audit", "share"}:
        violations.append(
            "cross-feature wildcard import from audit is forbidden; import registered crate-root symbols explicitly"
        )
    for match in path_pattern.finditer(line):
        prefix = line[: match.start()].rstrip()
        if "::" in prefix or "{" in prefix or (
            line[: match.start()].endswith(" ")
            and not (prefix.endswith("use") or prefix.endswith("pub use") or prefix.endswith("="))
        ):
            continue
        target, segment = match.groups()
        # 跳过文件内声明的本地模块（如 TUI 的 update 模块、sdk 的 update 模块）
        if local_modules and target in local_modules:
            continue
        if target == current_crate or current_crate == "share":
            continue
        if segment in ROOT_REEXPORT_ALLOW.get(target, set()) and stripped.startswith("pub use "):
            continue
        if segment in ROOT_ACCESS_ALLOW.get(target, set()):
            continue
        if target in {"audit", "policy", "provider", "project", "runtime", "tools"}:
            violations.append(
                f"cross-feature access to {target}::{segment} is forbidden; use the registered {target} crate-root facade"
            )
            continue
        if segment != "api":
            violations.append(
                f"cross-feature access to {target}::{segment} is forbidden; use {target}::api"
            )

    for match in braced_pattern.finditer(line):
        prefix = line[: match.start()].rstrip()
        if "::" in prefix or "{" in prefix or (
            line[: match.start()].endswith(" ")
            and not (prefix.endswith("use") or prefix.endswith("pub use") or prefix.endswith("="))
        ):
            continue
        target, inner = match.groups()
        if local_modules and target in local_modules:
            continue
        if target == current_crate or current_crate == "share":
            continue
        for item in top_level_items(inner):
            item_name = item.split("::", 1)[0].strip()
            item_name = item_name.split(" as ", 1)[0].strip()
            if not item_name:
                continue
            if item_name in ROOT_REEXPORT_ALLOW.get(target, set()) and stripped.startswith("pub use "):
                continue
            if item_name in ROOT_ACCESS_ALLOW.get(target, set()):
                continue
            if target in {"audit", "policy", "provider", "project", "runtime", "tools"}:
                violations.append(
                    f"cross-feature braced import from {target} exposes {item_name}; use the registered {target} crate-root facade"
                )
                continue
            if item_name != "api":
                violations.append(
                    f"cross-feature braced import from {target} exposes {item_name}; use {target}::api::..."
                )
    return violations


def check_api_line(line: str) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//"):
        return []
    violations: list[str] = []
    for segment in crate_reexport_pattern.findall(line):
        if segment not in API_FACADE_ALLOWED_SEGMENTS:
            violations.append(
                "feature api.rs may only re-export crate::contract or crate::gateway"
            )
    return violations


def project_import_violations(current_crate: str | None, text: str) -> list[str]:
    if current_crate in {"project", "share"}:
        return []
    stripped_text = re.sub(r'"(?:\\.|[^"\\])*"', '""', text)
    violations: list[str] = []
    if project_wildcard_pattern.search(stripped_text):
        violations.append("cross-feature wildcard import from project is forbidden; import registered crate-root symbols explicitly")
    text_without_braces = project_braced_full_pattern.sub("", stripped_text)
    for match in project_path_full_pattern.finditer(text_without_braces):
        symbol = match.group(1)
        prefix = text_without_braces[: match.start()].rstrip()
        is_pub_use = prefix.endswith("pub use")
        if symbol in ROOT_REEXPORT_ALLOW.get("project", set()) and is_pub_use:
            continue
        if symbol not in ROOT_ACCESS_ALLOW["project"]:
            violations.append(
                f"cross-feature access to project::{symbol} is forbidden; use the registered Project crate-root facade"
            )
    for match in project_braced_full_pattern.finditer(stripped_text):
        for item in top_level_items(match.group(1)):
            item_name = item.split("::", 1)[0].strip()
            item_name = item_name.split(" as ", 1)[0].strip()
            if item_name and item_name not in ROOT_ACCESS_ALLOW["project"]:
                violations.append(
                    f"cross-feature braced import from project exposes {item_name}; use the registered Project crate-root facade"
                )
    return violations


def project_public_facade_violations(text: str) -> list[str]:
    violations: list[str] = []
    for module in project_pub_mod_pattern.findall(text):
        violations.append(f"Project lib.rs must not expose public module `{module}`")
    for kind, symbol in project_pub_item_pattern.findall(text):
        violations.append(f"Project lib.rs must not define public {kind} `{symbol}`; use registered re-exports")
    public_symbols = set(project_pub_const_pattern.findall(text))
    for expression in project_pub_use_pattern.findall(text):
        if "*" in expression:
            violations.append("Project lib.rs must not use wildcard public re-exports")
            continue
        if "{" in expression:
            inner = expression.split("{", 1)[1].rsplit("}", 1)[0]
            for item in top_level_items(inner):
                name = item.split(" as ", 1)[-1].strip()
                name = name.rsplit("::", 1)[-1].strip()
                if name:
                    public_symbols.add(name)
        else:
            name = expression.split(" as ", 1)[-1].strip().rsplit("::", 1)[-1].strip()
            if name:
                public_symbols.add(name)
    unexpected = public_symbols - PROJECT_ROOT_PUBLIC_ALLOW
    missing = PROJECT_ROOT_PUBLIC_ALLOW - public_symbols
    if unexpected:
        violations.append("Project lib.rs exposes unregistered public symbols: " + ", ".join(sorted(unexpected)))
    if missing:
        violations.append("Project façade allowlist is stale or lib.rs is missing exports: " + ", ".join(sorted(missing)))
    return violations


def check_tools_facade() -> list[str]:
    path = root / "agent/features/tools/src/lib.rs"
    if not path.exists():
        return ["agent/features/tools/src/lib.rs: tools crate-root facade is missing"]
    text = path.read_text()
    errors: list[str] = []
    if (root / "agent/features/tools/src/api.rs").exists() or re.search(r"\bpub\s+mod\s+api\b", text):
        errors.append("agent/features/tools/src: tools::api is forbidden after crate-root facade migration")
    for module in ("domain", "adapters"):
        if not re.search(rf"(?m)^\s*mod\s+{module}\s*;", text):
            errors.append(f"agent/features/tools/src/lib.rs: internal module `{module}` must remain private")
        if re.search(rf"\bpub(?:\([^)]*\))?\s+mod\s+{module}\b", text):
            errors.append(f"agent/features/tools/src/lib.rs: internal module `{module}` must not be public")

    def braced_names(source: str) -> set[str]:
        match = re.search(rf"pub\s+use\s+{source}::\s*\{{(.*?)\}}\s*;", text, re.S)
        if not match:
            return set()
        return {item.strip().split(" as ", 1)[-1].strip() for item in match.group(1).split(",") if item.strip()}

    actual_domain = braced_names("domain")
    actual_adapters = braced_names("adapters::wiring")
    if not re.search(r"\bpub\s+use\s+domain::types\s*;", text):
        errors.append("agent/features/tools/src/lib.rs: public `types` module facade is missing")
    if actual_domain != TOOLS_DOMAIN_FACADE:
        errors.append(
            "agent/features/tools/src/lib.rs: domain facade drift; expected "
            + str(sorted(TOOLS_DOMAIN_FACADE)) + ", found " + str(sorted(actual_domain))
        )
    if actual_adapters != TOOLS_ADAPTER_FACADE:
        errors.append(
            "agent/features/tools/src/lib.rs: adapter facade drift; expected "
            + str(sorted(TOOLS_ADAPTER_FACADE)) + ", found " + str(sorted(actual_adapters))
        )
    actual_root = {"LOG_TARGET", "types"} | actual_domain | actual_adapters
    if actual_root != ROOT_ACCESS_ALLOW["tools"]:
        errors.append("ROOT_ACCESS_ALLOW[tools] must exactly match tools/src/lib.rs public facade")
    return errors



def run_sanity() -> None:
    allowed = [
        ("runtime", "use provider::LlmClient;"),
        ("tools", "use project::WorkspaceRead;"),
        ("tools", "let _ = ctx.workspace_read();"),
        ("provider", "use crate::adapters::client::LlmClient;"),
        ("share", "pub use storage::contract::StorageConfig;"),
        ("sdk", "pub use project::ProjectContext;"),
        ("runtime", "use storage::{MemoryStore, TaskStore};"),
    ]
    blocked = [
        ("runtime", "use provider::api::LlmClient;"),
        ("runtime", "use provider::core::client::LlmClient;"),
        ("tools", "use project::api::WorkspaceRead;"),
        ("tools", "let _ = project::business::worktree::enter_worktree(args);"),
        ("runtime", "use storage::memory_store::MemoryStore;"),
        ("runtime", "use storage::HistoryManager;"),
    ]
    for current, line in allowed:
        if check_cross_crate_line(current, line):
            raise AssertionError(f"sanity allow failed: {line}")
    for current, line in blocked:
        if not check_cross_crate_line(current, line):
            raise AssertionError(f"sanity block failed: {line}")
    if not check_api_line("pub use crate::business::Secret;"):
        raise AssertionError("sanity block failed: feature api.rs re-exporting business")
    if check_api_line("pub use crate::contract::*;") or check_api_line("pub use crate::gateway::*;"):
        raise AssertionError("sanity allow failed: api contract/gateway re-export")
    if not project_import_violations("tools", "use project::*;"):
        raise AssertionError("sanity block failed: Project wildcard import")
    if not project_import_violations("tools", "use project::{\n    WorkspaceRead,\n    domain::WorkspaceState,\n};"):
        raise AssertionError("sanity block failed: multiline Project internal import")
    if not project_import_violations("tools", "use project::\n domain::WorkspaceState;"):
        raise AssertionError("sanity block failed: multiline Project path import")
    if not project_import_violations("tools", "use project::{\n    WorkspaceRead,\n    WorkspaceService,\n};"):
        raise AssertionError("sanity block failed: removed Project concrete service import")
    valid_facade = '''
pub const LOG_TARGET: &str = "aemeath:agent:project";
pub use adapters::wiring::{wire_production_workspace, WorkspaceViews, WorkspaceWiring};
pub use domain::state::PreparedWorkspaceRestore;
pub use domain::types::{GitOperationError, GitProbeError, WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspaceInitError, WorkspacePersist, WorkspaceRead, WorkspaceRestoreError};
pub use share::session_types::{ProjectIdentity, WorkspaceId, WorktreeKind};
'''
    if project_public_facade_violations(valid_facade):
        raise AssertionError("sanity allow failed: registered Project façade")
    if not project_public_facade_violations(valid_facade + "pub mod domain;\n"):
        raise AssertionError("sanity block failed: public Project internal module")
    if not project_public_facade_violations(valid_facade + "pub fn leaked() {}\n"):
        raise AssertionError("sanity block failed: public Project function")
    if not project_public_facade_violations(valid_facade + "pub struct Leaked;\n"):
        raise AssertionError("sanity block failed: public Project type")
    if not project_public_facade_violations(valid_facade + "pub const fn leaked() {}\n"):
        raise AssertionError("sanity block failed: public Project const function")
    if not project_public_facade_violations(valid_facade + "pub static mut LEAKED: usize = 0;\n"):
        raise AssertionError("sanity block failed: public Project mutable static")


run_sanity()
violations: list[str] = []
violations.extend(check_tools_facade())
for forbidden in sorted(CONTEXT_FORBIDDEN_PATHS):
    path = root / forbidden
    if path.exists():
        violations.append(f"{forbidden}: forbidden fixed-layer Context path exists")
runtime_reasoning_port = root / "agent/features/runtime/src/ports/reasoning_port.rs"
if runtime_reasoning_port.exists():
    violations.append(
        "agent/features/runtime/src/ports/reasoning_port.rs: Runtime must consume workflow::api::ReasoningPort instead of defining a duplicate trait"
    )
project_lib = root / "agent/features/project/src/lib.rs"
if project_lib.exists():
    for violation in project_public_facade_violations(project_lib.read_text()):
        violations.append(f"agent/features/project/src/lib.rs: {violation}")
for forbidden in sorted(POLICY_FORBIDDEN_PATHS):
    path = root / forbidden
    if path.exists():
        violations.append(f"{forbidden}: Policy legacy fixed-layer path is forbidden")
for forbidden in sorted(PROJECT_FORBIDDEN_PATHS):
    path = root / forbidden
    if path.exists():
        violations.append(f"{forbidden}: Project legacy fixed-layer path is forbidden")
for api_path in sorted((root / "agent" / "features").glob("*/src/api.rs")):
    rel = api_path.relative_to(root)
    for lineno, line in enumerate(api_path.read_text().splitlines(), 1):
        for violation in check_api_line(line):
            violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

for base in [root / "agent", root / "apps", root / "packages"]:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_generated_or_target(path):
            continue
        rel = path.relative_to(root)
        current = crate_for(rel)
        text = path.read_text()
        stripped_text = re.sub(r'"(?:\\.|[^"\\])*"', '""', text)
        if current != "tools" and tools_legacy_api_pattern.search(stripped_text):
            violations.append(f"{rel}: legacy tools::api path is forbidden; use the tools crate-root facade")
        for violation in project_import_violations(current, text):
            violations.append(f"{rel}: {violation}")
        # 将 rustfmt 产生的多行 use 语句折叠为一行，避免花括号导入绕过 façade 白名单。
        scan_lines: list[tuple[int, str]] = []
        pending_use: list[str] = []
        pending_lineno = 0
        brace_depth = 0
        for lineno, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if pending_use:
                pending_use.append(stripped)
                brace_depth += stripped.count("{") - stripped.count("}")
                if brace_depth <= 0 and ";" in stripped:
                    scan_lines.append((pending_lineno, " ".join(pending_use)))
                    pending_use = []
                continue
            if re.match(r"^(?:pub\s+)?use\s+", stripped) and "{" in stripped and "}" not in stripped:
                pending_use = [stripped]
                pending_lineno = lineno
                brace_depth = stripped.count("{") - stripped.count("}")
                continue
            scan_lines.append((lineno, line))
        if pending_use:
            scan_lines.append((pending_lineno, " ".join(pending_use)))
        # 解析文件中声明的本地模块，排除同名 crate 的误报
        local_modules = set(re.findall(r'\b(?:pub\s+)?mod\s+(\w+)\s*;', text))
        for lineno, line in scan_lines:
            for violation in check_cross_crate_line(current, line, local_modules):
                violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")
if violations:
    reason = "Crate API boundary guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Crate API boundary guard OK.")
PY
