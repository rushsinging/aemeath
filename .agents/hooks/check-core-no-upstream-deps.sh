#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

manifest = Path("services/core/Cargo.toml")
text = manifest.read_text()
upstream = ["runtime", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit"]
violations = []

for crate in upstream:
    pattern = re.compile(rf"(?m)^\s*{re.escape(crate)}\s*=\s*\{{[^\n]*path\s*=")
    if pattern.search(text):
        violations.append(f"services/core/Cargo.toml must not depend on upstream workspace crate {crate}")

if violations:
    print(json.dumps({"decision": "block", "reason": "Core upstream dependency guard failed:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Core upstream dependency guard OK.")
PY
