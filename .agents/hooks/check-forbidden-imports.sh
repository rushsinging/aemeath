#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
business = ["core", "project", "policy", "prompt", "provider", "tools", "storage", "hook", "audit"]
allowed_runtime_api_files = {
    Path("apps/cli/src/main.rs"),
    Path("apps/cli/src/runtime_adapter.rs"),
    Path("apps/cli/src/run_orchestration.rs"),
    Path("apps/cli/src/tui/core/runtime.rs"),
}
forbidden_runtime_api = re.compile(r"::runtime::api::")
pattern = re.compile(r"(?<!:)(?:use\s+|\b)(" + "|".join(map(re.escape, business)) + r")::")
violations = []

for path in sorted((root / "apps" / "cli" / "src").rglob("*.rs")):
    text = path.read_text()
    rel = path.relative_to(root)
    for lineno, line in enumerate(text.splitlines(), 1):
        if pattern.search(line):
            violations.append(f"{rel}:{lineno}: direct business crate import/path is forbidden: {line.strip()}")
        if rel not in allowed_runtime_api_files and forbidden_runtime_api.search(line) and 'apps/cli/src/tui/' not in str(rel):
            violations.append(f"{rel}:{lineno}: runtime::api is only allowed in CLI composition root/runtime_adapter or transitional tui/**: {line.strip()}")

if violations:
    reason = "CLI must not import supporting business crates directly; use sdk AgentClient or CLI composition root adapter:\n" + "\n".join(violations[:80])
    if len(violations) > 80:
        reason += f"\n... and {len(violations) - 80} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Forbidden import guard OK.")
PY
