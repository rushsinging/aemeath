#!/usr/bin/env bash
# guard-registry:policy.session-management.composition-ownership
set -euo pipefail

# Composition is the only production constructor of Session backing. Context owns
# Session semantics through SessionManagementPort; Runtime consumes the injected port.
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
context = root / "agent/features/context/src"
runtime = root / "agent/features/runtime/src"
composition = root / "agent/composition/src/runtime.rs"
violations = []

for path in sorted(context.rglob("*.rs")):
    text = path.read_text()
    if re.search(r"\b(?:storage::api::)?file_system_blob\s*\(", text):
        violations.append(f"{path.relative_to(root)}: Context Session code must consume injected AtomicBlobPort, not construct file_system_blob")
    if path.name == "session_management.rs" and path.parent.name == "adapters":
        violations.append(f"{path.relative_to(root)}: legacy global Session management façade is forbidden")

for path in sorted(runtime.rglob("*.rs")):
    text = path.read_text()
    if re.search(r"\bcontext::(?:list_session_entries|export_session_bytes|import_session_bytes|update_session_metadata_entry|delete_session_entry)\b", text):
        violations.append(f"{path.relative_to(root)}: Runtime must consume injected SessionManagementPort, not Context free-function façade")

if not composition.is_file():
    violations.append("agent/composition/src/runtime.rs: Session composition root is missing")
else:
    text = composition.read_text()
    required = [
        r"AtomicBlobSessionManagement::new\(session_blob\.clone\(\)\)",
        r"session_management:\s*session_management\.clone\(\)",
        r"RuntimeBootstrapDependencies::new\([\s\S]*session_management",
    ]
    for pattern in required:
        if not re.search(pattern, text):
            violations.append("agent/composition/src/runtime.rs: Composition must create one SessionManagementPort and forward the same Arc to Context and Runtime")

if violations:
    print(json.dumps({"decision": "block", "reason": "Session management ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Session management ownership guard OK.")
PY
