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
factory = root / "agent/features/runtime/src/application/run/context_factory.rs"
composition = root / "agent/composition/src/runtime.rs"
violations = []


def production_text(path: Path) -> str:
    return path.read_text().split("#[cfg(test)]", 1)[0]


def struct_body(source: str, name: str) -> str | None:
    match = re.search(rf"pub struct {name}\s*\{{(?P<body>[\s\S]*?)\n\}}", source)
    return match.group("body") if match else None


if not runtime.is_file():
    violations.append(f"{runtime}: Runtime bootstrap source is missing")
else:
    production = production_text(runtime)
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

    bootstrap = struct_body(production, "RuntimeBootstrapDependencies")
    if bootstrap is None:
        violations.append("agent/features/runtime/src/application/client/from_args.rs: RuntimeBootstrapDependencies is missing")
    else:
        if not re.search(r"\bruntime_context_factory\s*:", bootstrap):
            violations.append("agent/features/runtime/src/application/client/from_args.rs: Runtime bootstrap must consume injected RuntimeContextFactory")
        for duplicate in ["tool_execution", "tool_context_binding"]:
            if re.search(rf"\b{duplicate}\s*:", bootstrap):
                violations.append(f"agent/features/runtime/src/application/client/from_args.rs: Runtime bootstrap must not duplicate factory-owned {duplicate}")
        for field in ["tool_catalog", "skill_catalog", "skill_materializer", "tool_result_materializer", "active_run"]:
            if not re.search(rf"\b{field}\s*:", bootstrap):
                violations.append(f"agent/features/runtime/src/application/client/from_args.rs: Runtime dependencies must carry injected {field}")

if not factory.is_file():
    violations.append(f"{factory}: RuntimeContextFactory source is missing")
else:
    source = production_text(factory)
    factory_body = struct_body(source, "RuntimeContextFactory")
    if factory_body is None or not re.search(r"\bservices\s*:\s*RuntimeServices", factory_body):
        violations.append("agent/features/runtime/src/application/run/context_factory.rs: RuntimeContextFactory must own RuntimeServices")
    constructor = re.search(r"pub fn (?:new|with_session_wiring)\s*\((?P<params>[\s\S]*?)\)\s*->\s*Self", source)
    params = constructor.group("params") if constructor else ""
    for field in ["tool_execution", "tool_context_binding"]:
        if not re.search(rf"\b{field}\s*:", params):
            violations.append(f"agent/features/runtime/src/application/run/context_factory.rs: RuntimeContextFactory constructor must receive {field}")
        if not re.search(rf"(?<!:)\b{field}\s*(?:,|\}})", source):
            violations.append(f"agent/features/runtime/src/application/run/context_factory.rs: RuntimeContextFactory must retain {field} in RuntimeServices")

if not composition.is_file():
    violations.append(f"{composition}: Composition runtime assembly source is missing")
else:
    source = composition.read_text()
    for pattern in [r"fn wire_runtime_tool_assembly\s*\(", r"wire_builtin_catalog_execution\s*\(", r"wire_skills\s*\(", r"AtomicBlobToolResultStore::new\s*\(", r"ActiveRunRegistry::default\s*\("]:
        if not re.search(pattern, source):
            violations.append("agent/composition/src/runtime.rs: Composition must assemble every Runtime Tool/Skill/Tool Result/active-run dependency")
            break
    factory_call = re.search(r"RuntimeContextFactory::(?:new|with_session_wiring)\s*\((?P<args>[\s\S]*?)\)\s*\)", source)
    factory_args = factory_call.group("args") if factory_call else ""
    for field in ["execution", "binding"]:
        if not re.search(rf"tool_assembly\.{field}(?:\.clone\(\))?", factory_args):
            violations.append(f"agent/composition/src/runtime.rs: Composition must inject Tool {field} into RuntimeContextFactory")

if violations:
    print(json.dumps({"decision": "block", "reason": "Runtime Tool assembly ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Runtime Tool assembly ownership guard OK.")
PY
