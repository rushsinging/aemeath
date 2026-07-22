#!/usr/bin/env bash
# guard-registry:policy.config.override-store.composition-ownership
set -euo pipefail

# Config owns override key/codec/error semantics, but Composition alone selects
# the filesystem-backed AtomicBlob implementation used by deployable bootstrap.
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
config_app = root / "agent/features/config/src/application.rs"
composition_app = root / "agent/composition/src/app.rs"
violations = []

if not config_app.is_file():
    violations.append("agent/features/config/src/application.rs: Config application source is missing")
else:
    production = config_app.read_text().split("#[cfg(test)]", 1)[0]
    if re.search(r"\b(?:storage::api::)?file_system_blob\s*\(", production):
        violations.append("agent/features/config/src/application.rs: Config application must consume injected NativeConfigStore, not construct file_system_blob")
    if re.search(r"\bFileSystemBlobAdapter::new\s*\(", production):
        violations.append("agent/features/config/src/application.rs: Config application must not construct FileSystemBlobAdapter")
    required = [
        r"wire_project_config_with_cli\([\s\S]*native_store:\s*NativeConfigStore",
        r"wire_project_config\([\s\S]*native_store:\s*NativeConfigStore",
        r"for_project\([\s\S]*native_store:\s*NativeConfigStore",
    ]
    for pattern in required:
        if not re.search(pattern, production):
            violations.append("agent/features/config/src/application.rs: Config wiring must require an injected NativeConfigStore")

if not composition_app.is_file():
    violations.append("agent/composition/src/app.rs: Composition app source is missing")
else:
    text = composition_app.read_text()
    if text.count("fn wire_config_override_store()") != 1:
        violations.append("agent/composition/src/app.rs: Composition must define exactly one config override store factory")
    required = [
        r"wire_config_override_store\(\)[\s\S]*NativeConfigStore::new",
        r"wire_project_config_with_cli\([\s\S]*wire_config_override_store\(\)\?",
    ]
    for pattern in required:
        if not re.search(pattern, text):
            violations.append("agent/composition/src/app.rs: Composition must construct and forward config override store")

if violations:
    print(json.dumps({"decision": "block", "reason": "Config override store ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Config override store ownership guard OK.")
PY
