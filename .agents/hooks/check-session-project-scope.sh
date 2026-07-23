#!/usr/bin/env bash
# guard-registry:policy.context.session-project-scope
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
ports = root / "agent/features/context/src/ports/session_management.rs"
resume = root / "agent/features/context/src/adapters/session_resume.rs"
runtime_list = root / "agent/features/runtime/src/application/client/trait_session.rs"
runtime_commands = root / "agent/features/runtime/src/application/chat/looping/idle_commands.rs"
violations = []

required = [
    (ports, r"load_for_project\s*\(", "SessionManagementPort must require project-scoped load"),
    (ports, r"list_for_project\s*\(", "SessionManagementPort must require project-scoped list"),
    (ports, r"export_for_project\s*\(", "SessionManagementPort must require project-scoped export"),
    (ports, r"import_for_project\s*\(", "SessionManagementPort must require project-scoped import"),
    (ports, r"update_metadata_for_project\s*\(", "SessionManagementPort must require project-scoped metadata update"),
    (ports, r"delete_for_project\s*\(", "SessionManagementPort must require project-scoped delete"),
    (resume, r"load_for_project\s*\(", "MainSessionWiring resume must use project-scoped session load"),
    (runtime_list, r"list_for_project\s*\(", "Runtime session list must use project-scoped session list"),
    (runtime_commands, r"update_metadata_for_project\s*\(", "Runtime session rename must use project-scoped metadata update"),
    (runtime_commands, r"export_for_project\s*\(", "Runtime session export must use project-scoped export"),
    (runtime_commands, r"import_for_project\s*\(", "Runtime session import must use project-scoped import"),
    (runtime_commands, r"delete_for_project\s*\(", "Runtime session delete must use project-scoped delete"),
]
for path, pattern, message in required:
    if not path.is_file() or not re.search(pattern, path.read_text()):
        violations.append(f"{path.relative_to(root)}: {message}")

if violations:
    print(json.dumps({"decision":"block","reason":"Session project scope guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Session project scope guard OK.")
PY
