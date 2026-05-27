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
    Path("apps/cli/src/chat.rs"),
}
allowed_tui_runtime_api_files = {
    Path("apps/cli/src/tui/core/cmd_exec.rs"),
    Path("apps/cli/src/tui/core/mod.rs"),
    Path("apps/cli/src/tui/core/run_loop.rs"),
    Path("apps/cli/src/tui/core/runtime.rs"),
    Path("apps/cli/src/tui/core/slash.rs"),
    Path("apps/cli/src/tui/core/run_loop.rs"),
    Path("apps/cli/src/tui/core/update/ui_event.rs"),
    Path("apps/cli/src/tui/display/task_window.rs"),
    Path("apps/cli/src/tui/display/task_window_helpers_tests.rs"),
    Path("apps/cli/src/tui/display/task_window_progress_tests.rs"),
    Path("apps/cli/src/tui/display/task_window_tests.rs"),
    Path("apps/cli/src/tui/input/input_handler.rs"),
    Path("apps/cli/src/tui/core/slash/dialog.rs"),
    Path("apps/cli/src/tui/core/slash/memory.rs"),
    Path("apps/cli/src/tui/core/slash/suggestions.rs"),
    Path("apps/cli/src/tui/display/status_bar.rs"),
    Path("apps/cli/src/tui/session/session_lifecycle.rs"),
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
        if rel not in allowed_runtime_api_files and rel not in allowed_tui_runtime_api_files and forbidden_runtime_api.search(line):
            violations.append(f"{rel}:{lineno}: runtime::api is only allowed in CLI composition root/runtime_adapter or explicit TUI transition allowlist: {line.strip()}")
        if 'apps/cli/src/tui/' in str(rel) and rel not in allowed_tui_runtime_api_files and any(fragment in line for fragment in [
            'runtime::api',
            '::runtime::api',
        ]):
            violations.append(f"{rel}:{lineno}: TUI must not depend on runtime directly; use sdk::AgentClient and sdk DTOs: {line.strip()}")

if violations:
    reason = "CLI must not import supporting business crates directly; use sdk AgentClient or CLI composition root adapter:\n" + "\n".join(violations[:80])
    if len(violations) > 80:
        reason += f"\n... and {len(violations) - 80} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Forbidden import guard OK.")
PY
