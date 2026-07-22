#!/usr/bin/env bash
# guard-registry:policy.hook.target-facade
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import re
import sys

root = Path.cwd()
hook_lib = root / "agent/features/hook/src/lib.rs"
hook_api = root / "agent/features/hook/src/api.rs"
runtime = root / "agent/features/runtime/src"
violations = []

if hook_api.exists():
    violations.append("agent/features/hook/src/api.rs: legacy Hook api module is forbidden")
if hook_lib.is_file():
    text = hook_lib.read_text()
    if re.search(r"^\s*pub\s+mod\s+api\b", text, re.M):
        violations.append("agent/features/hook/src/lib.rs: legacy public api facade is forbidden")
    if re.search(r"pub\s+use\s+(?:crate::)?adapters::legacy", text):
        violations.append("agent/features/hook/src/lib.rs: legacy adapter re-export is forbidden")
else:
    violations.append("agent/features/hook/src/lib.rs: Hook crate root is missing")

for path in sorted(runtime.rglob("*.rs")):
    if path.name.endswith("_tests.rs") or path.name == "tests.rs" or "tests" in path.parts:
        continue
    text = path.read_text()
    if re.search(r"\bhook\s*::\s*api\b", text):
        violations.append(f"{path.relative_to(root)}: Runtime must consume Hook crate-root PL, not hook::api")

if violations:
    print("[hook-target-facade] " + "\n".join(violations), file=sys.stderr)
    sys.exit(2)
print("Hook target facade guard OK.")
PY
