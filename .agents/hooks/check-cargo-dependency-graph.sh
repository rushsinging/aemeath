#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import subprocess
import sys

FEATURE_CRATES = {"runtime", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit"}

business_allow = {
    # Task #47 target shape: apps/cli -> composition -> runtime, and apps/cli -> sdk.
    "cli": {"composition", "sdk"},
    # Composition root may assemble runtime, shared adapters/ports, sdk, and feature gateways.
    "composition": FEATURE_CRATES | {"share", "sdk", "logging"},
    "runtime": {"project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit", "share", "sdk", "logging"},
    "share": set(),
    "project": {"share"},
    "policy": {"share"},
    "prompt": {"share"},
    "provider": {"share"},
    # Approved horizontal dependencies (spec §6.4.7): tools -> project/storage, via their api facades.
    "tools": {"share", "project", "storage"},
    "storage": {"share"},
    "hook": {"share"},
    "audit": {"share"},
    "sdk": set(),
    "logging": set(),
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

if violations:
    reason = "Cargo workspace dependency graph violates strict DDD boundaries:\n" + "\n".join(violations)
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Cargo dependency graph guard OK.")
PY
