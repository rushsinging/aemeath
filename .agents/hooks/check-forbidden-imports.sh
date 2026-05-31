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
business = ["project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit", "runtime"]
forbidden_runtime_api = re.compile(r"\bruntime::api")
forbidden_runtime_use = re.compile(r"\buse\s+runtime::")
pattern = re.compile(r"(?<!:)(?:use\s+|\b)(" + "|".join(map(re.escape, business)) + r")::")
forbidden_shared_adapter = re.compile(r"\bshare::adapter\b|agent/shared/src/adapter")
violations = []


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
    # Task 10 migration exception: runtime owns port impls until Task 11 moves
    # initialization/assembly to composition. Keep this exemption narrow.
    return rel.as_posix() == "agent/features/runtime/src/utils/adapter.rs"

for path in sorted((root / "apps" / "cli" / "src").rglob("*.rs")):
    text = path.read_text()
    rel = path.relative_to(root)
    for lineno, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if pattern.search(line):
            violations.append(f"{rel}:{lineno}: direct business crate import/path is forbidden: {line.strip()}")
        if forbidden_runtime_api.search(line) or forbidden_runtime_use.search(line):
            violations.append(f"{rel}:{lineno}: CLI must not import runtime directly; use composition: {line.strip()}")

for base in [root / "agent", root / "apps", root / "packages"]:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_test_path(path) or is_composition_path(path) or is_runtime_adapter_migration_path(path):
            continue
        rel = path.relative_to(root)
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if forbidden_shared_adapter.search(line):
                violations.append(f"{rel}:{lineno}: production adapter import/path is composition-only: {line.strip()}")

if violations:
    reason = "Forbidden import guard FAILED:\n" + "\n".join(violations[:80])
    if len(violations) > 80:
        reason += f"\n... and {len(violations) - 80} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Forbidden import guard OK.")
PY
