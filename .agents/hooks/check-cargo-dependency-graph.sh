#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import subprocess
import sys

business_allow = {
      "cli": {"runtime"},
      "runtime": {"core", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit", "share"},
      "share": {"core", "project", "prompt"},
      "project": {"core"},
      "policy": {"core"},
      "prompt": {"core"},
      "provider": {"core"},
      "tools": {"core", "share"},
      "storage": {"core"},
      "hook": {"core"},
      "audit": {"core"},
      "core": set(),
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
