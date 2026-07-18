#!/usr/bin/env bash
set -euo pipefail
# guard-registry:policy.cargo.capability-dependency-matrix

# 功能：基于 cargo 元数据校验各 crate 的业务依赖是否落在显式白名单内。
# 作用：固化 feature 依赖方向（cli→{composition,sdk}；runtime→全部 supporting；
#       supporting→share；share/sdk→∅），默认拒绝未声明的业务依赖，防双向/横向乱依赖。
# 例外（白名单内已批准）：runtime/tools→task（Task-owned OHS/PL）；
#       context→task（#890 Session persistence adapter 消费 Task TaskPersist capability）；
#       tools→project（§6.4.7 横向依赖登记；#897 后 Memory 仅经正式 port）；composition→全部 feature（唯一装配根）。
#       task 反向依赖任一消费者（runtime/tools/context）仍被拒绝。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import subprocess
import sys

FEATURE_CRATES = {"runtime", "config", "project", "policy", "context", "memory", "provider", "tools", "storage", "task", "hook", "audit", "update", "workflow"}
TOOLS_DEPENDENCY_BUDGET = {"share", "project", "task", "memory"}

business_allow = {
    # Task #47 target shape: apps/cli -> composition -> runtime, and apps/cli -> sdk.
    "cli": {"composition", "sdk"},
    # Composition root may assemble runtime, shared adapters/ports, sdk, and feature gateways.
    "composition": FEATURE_CRATES | {"share", "sdk", "logging", "update"},
    "runtime": {"config", "project", "policy", "context", "memory", "provider", "tools", "storage", "task", "hook", "audit", "workflow", "share", "sdk", "logging"},
    "config": {"share", "storage"},
    # packages/global/* are shared infrastructure and may be consumed by share/sdk.
    "share": {"logging", "utils"},
    "project": {"share"},
    "policy": {"share", "sdk", "tools"},
    "context": {"share", "provider", "storage", "task", "memory", "sdk"},
    "memory": {"storage", "utils"},
    # Provider may consume Logging only as shared diagnostic infrastructure and opaque
    # LogContext propagation; it must not interpret Runtime session/run semantics.
    "provider": {"share", "logging"},
    # Approved horizontal dependencies: tools -> project/memory and Task-owned OHS/PL.
    "tools": {"share", "project", "task", "memory"},
    "storage": {"share"},
    # Task owns its Published Language and OHS; it must not depend back on consumers.
    "task": set(),
    "hook": {"share"},
    "audit": {"share", "sdk", "storage"},
    "workflow": {"share"},
    # SDK publishes delivery contracts and typed tool result DTOs. During #993 the
    # DTO owner moved from share::tool into the tools crate-root façade, so SDK
    # directly consumes that Published Language until a neutral contract crate is
    # justified; the exact edge is guarded here rather than hidden by a wildcard.
    "sdk": {"share", "tools", "utils"},
    "update": {"share", "sdk", "logging"},
    "logging": set(),
    "utils": set(),
}


def validate_edges(edges: dict[str, set[str]], workspace_names: set[str] | None = None) -> list[str]:
    if workspace_names is None:
        workspace_names = set(edges)
    violations: list[str] = []
    for name, deps in edges.items():
        if name not in business_allow:
            continue
        allowed = business_allow[name]
        for dep_name in sorted(deps):
            if dep_name in workspace_names and dep_name not in allowed:
                violations.append(f"{name} must not depend on {dep_name}; allowed: {sorted(allowed)}")
    return violations


def run_sanity() -> None:
    workspace = set(business_allow)
    if not validate_edges({"provider": {"composition"}}, workspace):
        raise AssertionError("sanity block failed: feature dependency on composition")
    if validate_edges({"composition": {"runtime", "share", "sdk", "provider"}}, workspace):
        raise AssertionError("sanity allow failed: composition assembling runtime/share/sdk/provider")
    if validate_edges({"cli": {"composition", "sdk"}}, workspace):
        raise AssertionError("sanity allow failed: CLI composition + sdk")
    if validate_edges({"runtime": {"task"}, "tools": {"task"}, "context": {"task"}}, workspace):
        raise AssertionError("sanity allow failed: Runtime/Tools consuming TaskAccess and Context consuming Task persistence")
    if not validate_edges({"task": {"runtime"}}, workspace):
        raise AssertionError("sanity block failed: Task must not depend on Runtime consumer")
    if not validate_edges({"cli": {"runtime"}}, workspace):
        raise AssertionError("sanity block failed: CLI direct runtime dependency")


run_sanity()
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
workspace_names = {package["name"] for package in metadata["packages"]}
edges = {
    package["name"]: {
        dependency["name"]
        for dependency in package.get("dependencies", [])
        if dependency.get("source") is None
    }
    for package in metadata["packages"]
}
violations = validate_edges(edges, workspace_names)
tools_actual = edges.get("tools", set()) & workspace_names
if tools_actual != TOOLS_DEPENDENCY_BUDGET:
    violations.append(
        f"tools workspace dependency budget must remain exactly {sorted(TOOLS_DEPENDENCY_BUDGET)}; "
        f"found {sorted(tools_actual)}"
    )

if violations:
    reason = "Cargo workspace dependency graph violates strict DDD boundaries:\n" + "\n".join(violations)
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Cargo dependency graph guard OK.")
PY
