#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import subprocess
import sys

business_allow = {
      # Task #47 target shape: apps/cli -> composition -> runtime, and apps/cli -> sdk.
      # During Task 1 the CLI has not been migrated yet, so keep cli -> runtime allowed.
      "cli": {"composition", "runtime", "sdk"},
      "composition": {"runtime", "sdk"},
      "runtime": {"project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit", "share", "sdk", "logging"},
      "share": set(),
      "project": {"share"},
      "policy": {"share"},
      "prompt": {"share"},
      "provider": {"share"},
      # tools 横向依赖 project：worktree 行为/IO 从 share 瘦身后归位 project domain，
      # tools 复用 project::worktree（refs #61 D2）。无环：project 仅依赖 share，不反依赖 tools。
      # tools 横向依赖 storage：memory 持久化（MemoryStore + path canonicalize，含 fs IO）
      # 从 share 瘦身后归位 storage domain（spec §13），memory_tool 复用 storage::memory
      # （refs #61 D2 第二批）。无环：storage 仅依赖 share，不反依赖 tools。
      "tools": {"share", "project", "storage"},
      "storage": {"share"},
      "hook": {"share"},
      "audit": {"share"},
      "sdk": set(),
      "logging": set(),
    }

metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
workspace_names = {package["name"] for package in metadata["packages"]}
violations = []

for package in metadata["packages"]:
    name = package["name"]
    if name not in business_allow:
        continue
    allowed = business_allow[name]
    for dependency in package.get("dependencies", []):
        if dependency.get("source") is not None:
            continue
        dep_name = dependency["name"]
        if dep_name in workspace_names and dep_name not in allowed:
            violations.append(f"{name} must not depend on {dep_name}; allowed: {sorted(allowed)}")

if violations:
    reason = "Cargo workspace dependency graph violates strict DDD boundaries:\n" + "\n".join(violations)
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Cargo dependency graph guard OK.")
PY
