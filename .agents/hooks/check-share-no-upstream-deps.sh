#!/usr/bin/env bash
set -euo pipefail

# 功能：检查 share crate 不依赖任何业务 feature。
# 作用：share 是最底层共享内核，必须无上游依赖（§6.4.7：share→∅），
#       防止底层反依赖上层、形成依赖环。
# 例外：无。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

manifest = Path("agent/shared/Cargo.toml")
text = manifest.read_text()
upstream = [
    "runtime",
    "project",
    "policy",
    "context",
    "provider",
    "tools",
    "storage",
    "hook",
    "audit",
    "workflow",
    "composition",
    "cli",
    "sdk",
]
violations: list[str] = []

for crate in upstream:
    pattern = re.compile(rf"(?m)^\s*{re.escape(crate)}\s*=\s*\{{[^\n]*path\s*=")
    if pattern.search(text):
        violations.append(f"agent/shared/Cargo.toml must not depend on upstream workspace crate {crate}")

if violations:
    print(json.dumps({"decision": "block", "reason": "Share upstream dependency guard failed:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Share upstream dependency guard OK.")
PY
