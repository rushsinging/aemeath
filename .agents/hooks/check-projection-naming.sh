#!/usr/bin/env bash
# guard-registry:policy.repository.projection-naming
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
if [ ! -d "$ROOT/.agents/hooks" ]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

roots = [Path("agent"), Path("apps"), Path("packages"), Path("tools")]
identifier = re.compile(
    r"\b(?:struct|enum|trait|fn|mod)\s+([A-Za-z_][A-Za-z0-9_]*(?:Projection|projection)[A-Za-z0-9_]*)"
    r"|\b(?:let|const|static)\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*(?:Projection|projection)[A-Za-z0-9_]*)"
)
violations = []

for source_root in roots:
    if not source_root.exists():
        continue
    for path in source_root.rglob("*.rs"):
        if path.name.endswith("_tests.rs") or "tests" in path.parts or "scenario_tests" in path.parts:
            continue
        source = path.read_text()
        lines = source.splitlines()
        test_module_depth = None
        depth = 0
        pending_test = False
        for line_number, line in enumerate(lines, 1):
            stripped = line.strip()
            if stripped.startswith("#[cfg(test)]"):
                pending_test = True
            if test_module_depth is None and pending_test and re.match(r"(?:pub\s+)?mod\s+\w+\s*\{", stripped):
                test_module_depth = depth
                pending_test = False
            if test_module_depth is None and not stripped.startswith(("//", "///", "//!")):
                match = identifier.search(line)
                if match:
                    name = match.group(1) or match.group(2)
                    violations.append(f"{path}:{line_number}: forbidden broad identifier `{name}`")
            depth += line.count("{") - line.count("}")
            if test_module_depth is not None and depth <= test_module_depth:
                test_module_depth = None

if violations:
    print("\n".join(violations), file=sys.stderr)
    print("[architecture] 生产 Rust 标识符禁止使用宽泛 Projection/projection 命名", file=sys.stderr)
    sys.exit(1)
PY
