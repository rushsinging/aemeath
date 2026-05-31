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

if 'runtime = { path = "../../agent/runtime" }' in text or 'runtime = { path = "../../agent/features/runtime" }' in text:
    violations.append("apps/cli/Cargo.toml must not declare direct dependency on runtime; use composition")

if 'composition = { path = "../../agent/composition" }' not in text:
    violations.append("apps/cli/Cargo.toml must depend on composition via ../../agent/composition for composition root assembly")

if 'sdk = { path = "../../packages/sdk" }' not in text:
    violations.append("apps/cli/Cargo.toml must depend on sdk via ../../packages/sdk for AgentClient contract")

for path in sorted(Path("apps/cli/src").rglob("*.rs")):
    rel = path.as_posix()
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        if re.search(r"\buse\s+runtime::", line) or "runtime::api" in line:
            violations.append(f"{rel}:{lineno}: CLI must not import runtime directly; use composition: {stripped}")

if violations:
    print(json.dumps({"decision": "block", "reason": "Thin CLI guard failed:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Thin CLI dependency guard OK.")
PY
