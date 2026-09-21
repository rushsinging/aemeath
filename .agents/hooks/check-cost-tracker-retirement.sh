#!/bin/bash
# guard-registry:policy.audit.cost-retirement
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"

python3 - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys

root = Path(sys.argv[1])
violations: list[str] = []

retired_runtime_paths = [
    root / "agent/features/runtime/src/application/cost.rs",
    root / "agent/features/runtime/src/application/cost",
]
for retired_path in retired_runtime_paths:
    if retired_path.exists():
        violations.append(f"{retired_path.relative_to(root)}: retired Runtime Cost owner must stay absent")

runtime_source_root = root / "agent/features/runtime/src"
if runtime_source_root.exists():
    for runtime_source_path in runtime_source_root.rglob("*.rs"):
        relative_path = runtime_source_path.relative_to(root)
        if "pricing" in runtime_source_path.stem.lower():
            violations.append(
                f"{relative_path}: retired Runtime Pricing implementation must stay absent"
            )
        if "cost" in runtime_source_path.stem.lower():
            violations.append(
                f"{relative_path}: retired Runtime Cost implementation must stay absent"
            )

source_roots = [
    root / "agent",
    root / "apps",
    root / "packages",
]
retired_symbols = re.compile(
    r"\b(?:CostTracker|CostSummary|SessionCostSummary|ModelPricing|CostInfo|CostUpdate|COST_HISTORY_FILE)\b"
    r"|\bglobal_cost_history_path\b"
    r"|\bcost_usd\b"
    r"|StorageNamespace::Cost"
)
legacy_history_literal = re.compile(r'["\']cost_history\.json["\']')

for source_root in source_roots:
    if not source_root.exists():
        continue
    for source_path in source_root.rglob("*.rs"):
        relative_path = source_path.relative_to(root)
        source = source_path.read_text(errors="ignore")
        for line_number, line in enumerate(source.splitlines(), start=1):
            if retired_symbols.search(line):
                violations.append(
                    f"{relative_path}:{line_number}: retired Cost surface must stay absent: {line.strip()}"
                )
            if legacy_history_literal.search(line):
                violations.append(
                    f"{relative_path}:{line_number}: retired Cost history access must stay absent: {line.strip()}"
                )

if violations:
    print("\n".join(violations), file=sys.stderr)
    print(
        "Cost retirement guard FAILED: Runtime pricing, legacy Cost history APIs, Cost DTO/events and Cost presentation must not return.",
        file=sys.stderr,
    )
    raise SystemExit(2)

print("Cost retirement guard OK.")
PY
