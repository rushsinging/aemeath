#!/usr/bin/env bash
set -euo pipefail

# 功能：检查源码 import 边界，禁止非 composition 代码引用生产 adapter。
# 作用：守住 §6.4.5 rule5——`share::adapter` / `shared::adapter` / agent/shared/src/adapter
#       只能在 composition 装配处引用，feature 与 cli 不得直接 import。
# 例外：runtime/src/adapters/runtime.rs 迁移期白名单（脚本会自检该例外是否 stale）。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
forbidden_shared_adapter = re.compile(r"\bshare::adapter\b|\bshared::adapter\b|agent/shared/src/adapter")
violations: list[str] = []

# Temporary, exact migration exception: runtime owns impl blocks that adapt
# shared adapter newtypes to runtime-local ports.  This remains until those port
# impls can be split into feature-owned gateway factories without making share
# depend on runtime/provider/hook.  Keep this list path- and count-limited.
RUNTIME_ADAPTER_MIGRATION_EXCEPTIONS = {
    Path("agent/features/runtime/src/adapters/runtime.rs"),
}


def is_test_path(path: Path) -> bool:
    return path.name.endswith("_test.rs") or path.name.endswith("_tests.rs") or "tests" in path.parts


def is_composition_path(path: Path) -> bool:
    try:
        rel = path.relative_to(root)
    except ValueError:
        return False
    return rel.parts[:3] == ("agent", "composition", "src")


def is_runtime_adapter_migration_path(path: Path) -> bool:
    try:
        rel = path.relative_to(root)
    except ValueError:
        return False
    return rel in RUNTIME_ADAPTER_MIGRATION_EXCEPTIONS


def line_violations(line: str) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//"):
        return []
    if forbidden_shared_adapter.search(line):
        return ["production adapter import/path is composition-only"]
    return []


def run_sanity() -> None:
    if not line_violations("use share::adapter::provider::LlmClientAdapter;"):
        raise AssertionError("sanity block failed: non-composition import share::adapter")
    if line_violations("use share::config::ConfigurationSnapshot;"):
        raise AssertionError("sanity allow failed: shared port/config import")


run_sanity()
seen_runtime_exceptions: set[Path] = set()
for base in [root / "agent", root / "apps", root / "packages"]:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_test_path(path) or is_composition_path(path):
            continue
        rel = path.relative_to(root)
        is_exception = is_runtime_adapter_migration_path(path)
        if is_exception:
            seen_runtime_exceptions.add(rel)
            continue
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            for violation in line_violations(line):
                violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

missing = RUNTIME_ADAPTER_MIGRATION_EXCEPTIONS - seen_runtime_exceptions
if missing:
    violations.append(
        "Runtime adapter migration exception list is stale; missing exact path(s): "
        + ", ".join(sorted(path.as_posix() for path in missing))
    )

if violations:
    reason = "Forbidden import guard FAILED:\n" + "\n".join(violations[:80])
    if len(violations) > 80:
        reason += f"\n... and {len(violations) - 80} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Forbidden import guard OK.")
PY
