#!/usr/bin/env bash
# guard-registry:policy.runtime.tool-assembly.composition-ownership
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
runtime = root / "agent/features/runtime/src/application/client/from_args.rs"
composition = root / "agent/composition/src/runtime.rs"
violations = []

if not runtime.is_file():
    violations.append(f"{runtime}: Runtime bootstrap source is missing")
else:
    production = runtime.read_text().split("#[cfg(test)]", 1)[0]
    forbidden = [
        (r"tools::composition::wire_", "Runtime bootstrap must consume injected Tool ports, not call Tools composition factory"),
        (r"FileSystemBlobAdapter::new\s*\(", "Runtime bootstrap must not construct Tool Result filesystem backing"),
        (r"AtomicBlobToolResultStore::new\s*\(", "Runtime bootstrap must not construct Tool Result store"),
        (r"ActiveRunRegistry::default\s*\(", "Runtime bootstrap must consume injected ActiveRunRegistry"),
        (r"spawn_mcp_connect\s*\(", "Runtime bootstrap must not retain MCP CatalogExecutionWiring connection seam"),
    ]
    for pattern, message in forbidden:
        if re.search(pattern, production):
            violations.append(f"agent/features/runtime/src/application/client/from_args.rs: {message}")
    for field in ["tool_catalog", "tool_execution", "tool_context_binding", "skill_catalog", "skill_materializer", "tool_result_materializer", "active_run"]:
        if not re.search(rf"\b{field}\b", production):
            violations.append(f"agent/features/runtime/src/application/client/from_args.rs: Runtime dependencies must carry injected {field}")

if not composition.is_file():
    violations.append(f"{composition}: Composition runtime assembly source is missing")
else:
    source = composition.read_text()
    for pattern in [r"fn wire_runtime_tool_assembly\s*\(", r"wire_builtin_catalog_execution\s*\(", r"wire_skills\s*\(", r"AtomicBlobToolResultStore::new\s*\(", r"ActiveRunRegistry::default\s*\("]:
        if not re.search(pattern, source):
            violations.append("agent/composition/src/runtime.rs: Composition must assemble every Runtime Tool/Skill/Tool Result/active-run dependency")
            break

if violations:
    print(json.dumps({"decision": "block", "reason": "Runtime Tool assembly ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Runtime Tool assembly ownership guard OK.")
PY
