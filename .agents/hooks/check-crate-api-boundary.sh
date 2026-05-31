#!/usr/bin/env bash
set -euo pipefail

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
    "prompt",
    "provider",
    "tools",
    "storage",
    "hook",
    "audit",
}
INTERNAL_SEGMENTS = {"contract", "gateway", "core", "business", "utils"}
API_FACADE_ALLOWED_SEGMENTS = {"contract", "gateway"}
ROOT_REEXPORT_ALLOW = {
    "project": {"ProjectContext"},
}
TOOLS_PROJECT_CONTEXT_API_NAMES = {
    "WorktreeWorkingContext",
    "workspace_context_from_worktree_context",
    "restore_workspace_context",
}
TOOLS_CONTEXT_PROJECTION_PATH = Path("agent/features/tools/src/contract.rs")

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


def check_cross_crate_line(current_crate: str | None, line: str) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//") or stripped.startswith("*"):
        return []

    violations: list[str] = []
    for match in path_pattern.finditer(line):
        prefix = line[: match.start()].rstrip()
        if "::" in prefix or "{" in prefix or (
            line[: match.start()].endswith(" ")
            and not (prefix.endswith("use") or prefix.endswith("pub use") or prefix.endswith("="))
        ):
            continue
        target, segment = match.groups()
        if target == current_crate or current_crate == "share":
            continue
        if segment in ROOT_REEXPORT_ALLOW.get(target, set()) and stripped.startswith("pub use "):
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
        if target == current_crate or current_crate == "share":
            continue
        for item in top_level_items(inner):
            item_name = item.split("::", 1)[0].strip()
            item_name = item_name.split(" as ", 1)[0].strip()
            if not item_name:
                continue
            if item_name in ROOT_REEXPORT_ALLOW.get(target, set()) and stripped.startswith("pub use "):
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


def strip_line_comment(line: str) -> str:
    return line.split("//", 1)[0]


def check_tools_project_context_api_line(rel: Path, line: str) -> list[str]:
    # The tools feature must centralize ToolContext -> project worktree/workspace
    # context projection in tools::contract::WorktreeContextExt. Business/runtime
    # code should use that projection instead of constructing/calling project
    # context APIs directly.
    if not rel.as_posix().startswith("agent/features/tools/src/"):
        return []
    if rel == TOOLS_CONTEXT_PROJECTION_PATH:
        return []

    code = strip_line_comment(line)
    if not code.strip():
        return []

    violations: list[str] = []
    for name in TOOLS_PROJECT_CONTEXT_API_NAMES:
        direct_path_pattern = rf"\bproject(?:_api)?::api::(?:[A-Za-z_][A-Za-z0-9_]*::)*{re.escape(name)}\b"
        braced_import_pattern = rf"\buse\s+project(?:_api)?::api::\{{[^}}]*\b{re.escape(name)}\b"
        imported_call_pattern = rf"(?<![A-Za-z0-9_:]){re.escape(name)}\s*(?:\{{|\()"
        if (
            re.search(direct_path_pattern, code)
            or re.search(braced_import_pattern, code)
            or re.search(imported_call_pattern, code)
        ):
            violations.append(
                "tools must use tools::contract::WorktreeContextExt context projection "
                "instead of direct project workspace/worktree context API access"
            )
            break
    return violations


def run_sanity() -> None:
    allowed = [
        ("runtime", "use provider::api::LlmClient;"),
        ("tools", "let _ = ctx.worktree_working_context();"),
        ("provider", "use crate::core::client::LlmClient;"),
        ("share", "pub use storage::contract::StorageConfig;"),
        ("sdk", "pub use project::ProjectContext;"),
    ]
    blocked = [
        ("runtime", "use provider::core::client::LlmClient;"),
        ("tools", "let _ = project::business::worktree::enter_worktree(args);"),
        ("runtime", "use storage::{api::MemoryStore, MemoryStore as RootStore};"),
    ]
    for current, line in allowed:
        if check_cross_crate_line(current, line):
            raise AssertionError(f"sanity allow failed: {line}")
    for current, line in blocked:
        if not check_cross_crate_line(current, line):
            raise AssertionError(f"sanity block failed: {line}")
    tools_rel = Path("agent/features/tools/src/business/worktree.rs")
    tools_contract_rel = TOOLS_CONTEXT_PROJECTION_PATH
    if check_tools_project_context_api_line(tools_rel, "let _ = ctx.worktree_working_context();"):
        raise AssertionError("sanity allow failed: tools projection extension")
    if not check_tools_project_context_api_line(
        tools_rel,
        "let _ = project::api::workspace_context_from_worktree_context(&wc);",
    ):
        raise AssertionError("sanity block failed: direct tools workspace context API")
    if not check_tools_project_context_api_line(
        tools_rel,
        "use project::api::{self as worktree_ops, WorktreeWorkingContext};",
    ):
        raise AssertionError("sanity block failed: direct tools worktree context import")
    if check_tools_project_context_api_line(
        tools_contract_rel,
        "let wc = WorktreeWorkingContext { working_root, path_base, context_stack };",
    ):
        raise AssertionError("sanity allow failed: tools contract projection")
    if not check_api_line("pub use crate::business::Secret;"):
        raise AssertionError("sanity block failed: feature api.rs re-exporting business")
    if check_api_line("pub use crate::contract::*;") or check_api_line("pub use crate::gateway::*;"):
        raise AssertionError("sanity allow failed: api contract/gateway re-export")


run_sanity()
violations: list[str] = []
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
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            for violation in check_cross_crate_line(current, line):
                violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")
            for violation in check_tools_project_context_api_line(rel, line):
                violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")
if violations:
    reason = "Crate API boundary guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Crate API boundary guard OK.")
PY
