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
# Cross-domain dependencies are allowed only through each crate's published API facade.
# share is the shared kernel and is intentionally excluded from this facade rule.
DOMAIN_CRATES = {
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
INTERNAL_SEGMENTS = {"business", "core", "utils"}
PUBLIC_ROOT_ALLOW = {
    "provider": {"ApiDriverKind", "LlmError"},
}

path_pattern = re.compile(
    r"(?<![A-Za-z0-9_:])(?:::)?("
    + "|".join(sorted(map(re.escape, DOMAIN_CRATES)))
    + r")::([A-Za-z_][A-Za-z0-9_]*)"
)
braced_pattern = re.compile(
    r"(?<![A-Za-z0-9_:])(?:::)?("
    + "|".join(sorted(map(re.escape, DOMAIN_CRATES)))
    + r")::\s*\{([^}]*)"
)


def crate_for(path: Path) -> str | None:
    parts = path.parts
    if len(parts) >= 3 and parts[0] == "agent":
        if parts[1] == "shared":
            return "share"
        if parts[1] == "features" and len(parts) >= 4:
            return parts[2]
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


def check_line(current_crate: str | None, line: str) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//") or stripped.startswith("*"):
        return []

    violations = []
    for match in path_pattern.finditer(line):
        prefix = line[: match.start()].rstrip()
        if "::" in prefix or "{" in prefix or (
            line[: match.start()].endswith(" ")
            and not (prefix.endswith("use") or prefix.endswith("pub use") or prefix.endswith("="))
        ):
            continue
        target, segment = match.groups()
        if target == current_crate:
            continue
        if current_crate == "share":
            continue
        if segment in PUBLIC_ROOT_ALLOW.get(target, set()):
            continue
        if stripped.startswith("pub use ") and "::" not in prefix and segment != "api":
            continue
        if segment != "api":
            violations.append(
                f"cross-crate access to {target}::{segment} is forbidden; use {target}::api"
            )
        elif segment in INTERNAL_SEGMENTS:
            violations.append(
                f"cross-crate access to {target}::{segment} is forbidden; use {target}::api"
            )

    for match in braced_pattern.finditer(line):
        prefix = line[: match.start()].rstrip()
        if "::" in prefix or "{" in prefix or (
            line[: match.start()].endswith(" ")
            and not (prefix.endswith("use") or prefix.endswith("pub use") or prefix.endswith("="))
        ):
            continue
        target, inner = match.groups()
        if target == current_crate:
            continue
        if current_crate == "share":
            continue
        for item in top_level_items(inner):
            item_name = item.split("::", 1)[0].strip()
            item_name = item_name.split(" as ", 1)[0].strip()
            if not item_name:
                continue
            if item_name in PUBLIC_ROOT_ALLOW.get(target, set()):
                continue
            if item_name != "api":
                violations.append(
                    f"cross-crate braced import from {target} exposes {item_name}; use {target}::api::..."
                )
    return violations


def run_sanity() -> None:
    allowed = [
        ("runtime", "use provider::api::LlmClient;"),
        ("tools", "let _ = project::api::workspace_context_from_tool_context(ctx);"),
        ("provider", "use crate::core::client::LlmClient;"),
        ("runtime", "use provider::{ApiDriverKind, LlmError};"),
        ("share", "pub use storage::StorageConfig;"),
        ("sdk", "pub use project::ProjectContext;"),
    ]
    blocked = [
        ("runtime", "use provider::core::client::LlmClient;"),
        ("tools", "let _ = project::business::worktree::enter_worktree(args);"),
        ("runtime", "use storage::{api::MemoryStore, MemoryStore as RootStore};"),
    ]
    for current, line in allowed:
        if check_line(current, line):
            raise AssertionError(f"sanity allow failed: {line}")
    for current, line in blocked:
        if not check_line(current, line):
            raise AssertionError(f"sanity block failed: {line}")


run_sanity()
violations = []
for base in [root / "agent", root / "apps", root / "packages"]:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_generated_or_target(path):
            continue
        rel = path.relative_to(root)
        current = crate_for(rel)
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            for violation in check_line(current, line):
                violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

if violations:
    reason = "Crate API boundary guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Crate API boundary guard OK.")
PY
