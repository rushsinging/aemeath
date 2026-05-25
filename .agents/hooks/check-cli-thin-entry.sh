#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

manifest = Path("apps/cli/Cargo.toml")
text = manifest.read_text()
business = ["core", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit"]
violations = []

for crate in business:
    pattern = re.compile(rf"(?m)^\s*{re.escape(crate)}\s*=\s*\{{[^\n]*path\s*=")
    if pattern.search(text):
        violations.append(f"apps/cli/Cargo.toml must not declare direct path dependency on {crate}")

if 'runtime = { path = "../../crates/runtime" }' not in text:
    violations.append("apps/cli/Cargo.toml must depend on runtime via ../../crates/runtime")

if violations:
    print(json.dumps({"decision": "block", "reason": "Thin CLI guard failed:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Thin CLI dependency guard OK.")
PY
