#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

python3 - "$ROOT" <<'PY'
from __future__ import annotations

import re
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
runtime = root / "agent/features/runtime/src"
tools = root / "agent/features/tools/src"
violations: list[tuple[Path, int, str]] = []


def production_files(base: Path):
    if not base.is_dir():
        return
    for path in sorted(base.rglob("*.rs")):
        if path.name.endswith(("_test.rs", "_tests.rs")) or "tests" in path.parts:
            continue
        yield path


def strip_comments(source: str) -> str:
    # Preserve strings: the AskUser rule intentionally detects protocol literals.
    source = re.sub(r"/\*.*?\*/", lambda m: "\n" * m.group(0).count("\n"), source, flags=re.S)
    return re.sub(r"//[^\n]*", "", source)


def strip_cfg_test_items(source: str) -> str:
    """Remove cfg(test) items so legacy inline tests are not production findings."""
    marker = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
    while (match := marker.search(source)) is not None:
        start = match.start()
        cursor = match.end()
        # Include any following attributes and visibility before locating the item.
        semi = source.find(";", cursor)
        brace = source.find("{", cursor)
        if semi >= 0 and (brace < 0 or semi < brace):
            end = semi + 1
        elif brace >= 0:
            depth = 0
            end = len(source)
            for index in range(brace, len(source)):
                if source[index] == "{":
                    depth += 1
                elif source[index] == "}":
                    depth -= 1
                    if depth == 0:
                        end = index + 1
                        break
        else:
            end = len(source)
        source = source[:start] + ("\n" * source[start:end].count("\n")) + source[end:]
    return source


def source(path: Path) -> str:
    return strip_cfg_test_items(strip_comments(path.read_text(encoding="utf-8")))


def find(path: Path, text: str, pattern: str, reason: str, flags: int = 0):
    for match in re.finditer(pattern, text, flags):
        line = text.count("\n", 0, match.start()) + 1
        violations.append((path, line, reason))


# Tools legacy Registry/Profile/SkillTool compatibility paths are retired. The
# private ToolRegistry backing remains allowed only where composition adapters
# need it; all historical entry points and the third compatibility Scope are
# forbidden from production source.
legacy_tool_patterns = re.compile(
    r"\b(?:LegacyNoAgent|legacy-no-agent|SkillTool|SkillInput|SkillResult|"
    r"register_all_tools(?:_except_agent)?|register_subagent_tools|"
    r"ToolCatalogGateway|DefaultToolCatalogGateway|wire_tools)\b"
)
for path in production_files(tools):
    text = source(path)
    if legacy_tool_patterns.search(text):
        find(
            path,
            text,
            legacy_tool_patterns.pattern,
            "Tools legacy Registry/Profile/SkillTool paths must stay retired",
        )

# Runtime business code must consume only Tool Catalog/Execution ports and PL.
# There is deliberately no Runtime migration allowlist: remaining old paths are
# findings that must be migrated or retired rather than hidden.
runtime_rules = [
    (r"\bToolRegistry\b", "Runtime production code must not reference ToolRegistry"),
    (r"\bArc\s*<\s*dyn\s+(?:::tools::)?Tool\s*>", "Runtime production code must not hold Arc<dyn Tool>"),
    (r"\bregistry\s*\.\s*get\s*\(", "Runtime production code must not call registry.get()"),
    (r"\btool\s*\.\s*call\s*\(", "Runtime production code must not call tool.call()"),
    (r"\.\s*input_schema\s*\(", "Runtime production code must not read Tool input_schema directly"),
]
for path in production_files(runtime):
    text = source(path)
    for pattern, reason in runtime_rules:
        find(path, text, pattern, reason)

# The execution adapter performs Tool-owned dispatch only. Runtime orchestration
# concepts and dependencies must never move down into it.
execution = tools / "adapters/execution.rs"
if execution.exists():
    text = source(execution)
    execution_rules = [
        (r"(?:\buse\s+|\bextern\s+crate\s+)(?:policy|hook|sdk|tui|runtime)\b|\b(?:policy|hook|sdk|tui|runtime)::", "Tools Execution adapter must not depend on policy/hook/sdk/tui/runtime"),
        (r"\b(?:timeout|Semaphore|RunStep|approval)\b", "Tools Execution adapter must not own timeout, semaphore, RunStep, or approval orchestration"),
    ]
    for pattern, reason in execution_rules:
        find(execution, text, pattern, reason, re.I if "approval" in pattern else 0)
else:
    violations.append((execution, 1, "Tools Execution adapter is missing"))

# Suspension Published Language is value-only. Runtime supplies identity and all
# live waiting/cancellation mechanics at its own boundary.
suspension = tools / "domain/suspension.rs"
if suspension.exists():
    text = source(suspension)
    suspension_rules = [
        (r"\btokio\b", "Tool suspension PL must not depend on tokio"),
        (r"\b(?:Sender|Receiver|Mutex|RwLock|Arc)\b", "Tool suspension PL must not contain channels, locks, or Arc"),
        (r"\bRuntimeHandle\b|\brequest_?id\b|\bresume_?token\b", "Tool suspension PL must not carry Runtime handles or interaction identity"),
    ]
    for pattern, reason in suspension_rules:
        find(suspension, text, pattern, reason, re.I if "request" in pattern else 0)
else:
    violations.append((suspension, 1, "Tool suspension Published Language is missing"))

# AskUser in Tools parses a typed suspension. Runtime's existing oneshot is
# intentionally outside this scope because Runtime owns the interaction waiter.
for path in production_files(tools / "adapters"):
    if "ask_user" not in path.name:
        continue
    text = source(path)
    find(path, text, r"__ASK_USER(?:_SELECT)?__", "Tools AskUser must not use magic-string suspension protocols")
    find(path, text, r"\boneshot\b|\b(?:channel|waiter)\b", "Tools AskUser must not create channels or waiters", re.I)

# Crate-root is PL/ports plus narrow factories. Concrete registry/builders,
# backing, and concrete adapters are adapter implementation details.
facade = tools / "lib.rs"
if facade.exists():
    text = source(facade)
    facade_rules = [
        (r"\bpub\s+use\b[^;]*\b(?:ToolBacking|RegistryScopeBuilder|ToolRegistry)\b", "Tools crate-root must not expose backing, RegistryScopeBuilder, or ToolRegistry"),
        (r"\bpub\s+use\b[^;]*\b(?:CatalogAdapter|ExecutionAdapter)\b", "Tools crate-root must expose a composition factory, not concrete adapters"),
    ]
    for pattern, reason in facade_rules:
        find(facade, text, pattern, reason, re.S)
else:
    violations.append((facade, 1, "Tools crate-root facade is missing"))

# Tools owns the sole validator implementation. Runtime may retain only the
# named compatibility re-export while call sites are peeled into Execution.
validator = tools / "domain/schema_validator.rs"
if not validator.exists():
    violations.append((validator, 1, "Tools must own the schema validator implementation"))
for path in production_files(runtime):
    text = source(path)
    if path.name == "input_validation.rs":
        without_reexport = re.sub(r"pub\s+use\s+tools\s*::\s*\{.*?\}\s*;", "", text, flags=re.S)
        find(path, without_reexport, r"\b(?:fn|struct|enum|const|static)\s+(?:validate_tool_input|strip_runtime_meta|format_tool_input_error|ToolInputMismatch|RUNTIME_META_KEYS)\b|\bjsonschema\b", "Runtime input_validation may only compatibility re-export the Tools validator")
    else:
        find(path, text, r"\b(?:fn|struct|enum|const|static)\s+(?:validate_tool_input|strip_runtime_meta|format_tool_input_error|ToolInputMismatch|RUNTIME_META_KEYS)\b|\bjsonschema\b", "Schema validator implementation must exist only in Tools")

if violations:
    for path, line, reason in violations:
        try:
            shown = path.relative_to(root)
        except ValueError:
            shown = path
        print(f"{shown}:{line}: {reason}", file=sys.stderr)
    print(f"[tool-boundary] {len(violations)} violation(s); no migration exceptions or path allowlists are supported", file=sys.stderr)
    raise SystemExit(2)

print("Tool Catalog/Execution architecture boundaries passed.")
PY
