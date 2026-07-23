#!/usr/bin/env bash
# guard-registry:policy.composition.cross-bc-construction-ownership
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
hooks = root / ".agents/hooks"
runner = hooks / "check-architecture-guards.sh"
registry = root / ".agents/architecture-guard-registry.json"
runtime = root / "agent/features/runtime/src/application/client/from_args.rs"
config = root / "agent/features/config/src/application.rs"
context = root / "agent/features/context/src/adapters/atomic_blob_session_management.rs"

leaf_guards = {
    "check-session-management-ownership.sh": "policy.session-management.composition-ownership",
    "check-config-store-ownership.sh": "policy.config.override-store.composition-ownership",
    "check-runtime-tool-assembly-ownership.sh": "policy.runtime.tool-assembly.composition-ownership",
    "check-runtime-hook-assembly-ownership.sh": "policy.runtime.hook-assembly.composition-ownership",
}
violations = []

for guard, policy_id in leaf_guards.items():
    path = hooks / guard
    if not path.is_file() or not path.stat().st_mode & 0o111:
        violations.append(f".agents/hooks/{guard}: missing executable leaf ownership guard")

if not runner.is_file():
    violations.append(".agents/hooks/check-architecture-guards.sh: missing guard orchestrator")
else:
    source = runner.read_text()
    for guard in leaf_guards:
        pattern = rf'run_guard fast "\$HOOKS_DIR/{re.escape(guard)}"'
        if not re.search(pattern, source):
            violations.append(f".agents/hooks/check-architecture-guards.sh: missing fast registration for {guard}")

if not registry.is_file():
    violations.append(".agents/architecture-guard-registry.json: missing Guard Registry")
else:
    try:
        entries = json.loads(registry.read_text()).get("entries", [])
    except json.JSONDecodeError as error:
        violations.append(f".agents/architecture-guard-registry.json: invalid JSON: {error}")
        entries = []
    for guard, policy_id in leaf_guards.items():
        if not any(
            entry.get("id") == policy_id
            and entry.get("guard") == guard
            and entry.get("classification") == "target_capability_policy"
            and entry.get("status") == "active"
            for entry in entries
        ):
            violations.append(f".agents/architecture-guard-registry.json: missing active target policy for {guard}")

sources = [
    (
        runtime,
        [
            (r"\bFileSystemBlobAdapter::new\s*\(", "Runtime must not construct FileSystemBlobAdapter; consume Composition-injected Tool Result resources"),
            (r"\btools::composition::wire_", "Runtime must not call Tools composition factories; consume injected Tool ports"),
            (r"\bhook::build_dispatcher\s*\(", "Runtime must not construct Hook dispatcher; consume injected HookPort"),
        ],
    ),
    (
        config,
        [
            (r"\b(?:storage::api::)?file_system_blob\s*\(", "Config must not construct file_system_blob; consume injected NativeConfigStore"),
            (r"\bFileSystemBlobAdapter::new\s*\(", "Config must not construct FileSystemBlobAdapter; consume injected NativeConfigStore"),
        ],
    ),
    (
        context,
        [
            (r"\b(?:storage::api::)?file_system_blob\s*\(", "Context must not construct file_system_blob; consume Composition-injected SessionManagementPort backing"),
        ],
    ),
]

for path, patterns in sources:
    if not path.is_file():
        violations.append(f"{path.relative_to(root)}: required production source is missing")
        continue
    production = path.read_text().split("#[cfg(test)]", 1)[0]
    for pattern, message in patterns:
        if re.search(pattern, production):
            violations.append(f"{path.relative_to(root)}: {message}")

if violations:
    print(json.dumps({"decision": "block", "reason": "Composition cross-BC construction ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Composition cross-BC construction ownership guard OK.")
PY
