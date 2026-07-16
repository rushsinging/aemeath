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
    "provider",
    "tools",
    "storage",
    "hook",
    "audit",
    "update",
}
INTERNAL_SEGMENTS = {"contract", "gateway", "core", "business", "utils"}
API_FACADE_ALLOWED_SEGMENTS = {"contract", "gateway"}
ROOT_REEXPORT_ALLOW = {
    "project": {"ProjectContext"},
}
# 已迁移 feature 的目标 façade 位于 crate 根；集合必须保持窄且由真实消费者证明。
ROOT_ACCESS_ALLOW = {
    "provider": {
        "CallbackHandler",
        "InvocationDelta",
        "InvocationOptions",
        "InvocationRequest",
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
        "OpenAIProviderConfig",
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
        "StreamHandler",
        "StreamResponse",
        "SystemBlock",
        "Usage",
        "DEFAULT_TIMEOUT_SECS",
        "wire_provider",
    },
    "runtime": {"AgentClientImpl", "from_args"},
    # Context 的 Target façade 位于 crate 根；只允许访问这些稳定发布模块。
    "context": {"compact", "context_port", "domain", "guidance", "session", "skill"},
    # Storage 的 #991 过渡 façade；最终随 #880/#983/#883/#884 收敛。
    "storage": {
        "Batch",
        "BatchStatus",
        "MemoryStore",
        "Task",
        "TaskPriority",
        "TaskSnapshot",
        "TaskStatus",
        "TaskStore",
        "MAX_TOOL_RESULT_CHARS",
        "memory_base_dir",
        "persist_oversized_results",
        "project_file_name",
        "project_file_name_from_path",
    },
}

CONTEXT_FORBIDDEN_PATHS = {
    "agent/features/context/src/api.rs",
    "agent/features/context/src/gateway.rs",
    "agent/features/context/src/capabilities",
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
crate_reexport_pattern = re.compile(r"crate::([A-Za-z_][A-Za-z0-9_]*)")


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
        if target == "provider" and segment not in ROOT_ACCESS_ALLOW["provider"]:
            violations.append(
                f"cross-feature access to provider::{segment} is forbidden; use the registered provider crate-root facade"
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
            if target == "provider" and item_name not in ROOT_ACCESS_ALLOW["provider"]:
                violations.append(
                    f"cross-feature braced import from provider exposes {item_name}; use the registered provider crate-root facade"
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


def run_sanity() -> None:
    allowed = [
        ("runtime", "use provider::LlmClient;"),
        ("tools", "let _ = ctx.workspace_read();"),
        ("provider", "use crate::adapters::client::LlmClient;"),
        ("share", "pub use storage::contract::StorageConfig;"),
        ("sdk", "pub use project::ProjectContext;"),
        ("runtime", "use storage::{MemoryStore, TaskStore};"),
    ]
    blocked = [
        ("runtime", "use provider::api::LlmClient;"),
        ("runtime", "use provider::core::client::LlmClient;"),
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


run_sanity()
violations: list[str] = []
for forbidden in sorted(CONTEXT_FORBIDDEN_PATHS):
    path = root / forbidden
    if path.exists():
        violations.append(f"{forbidden}: forbidden fixed-layer Context path exists")
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
