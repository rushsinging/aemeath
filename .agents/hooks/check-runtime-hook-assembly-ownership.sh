#!/usr/bin/env bash
# guard-registry:policy.runtime.hook-assembly.composition-ownership
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
runtime = root / "agent/features/runtime/src"
bootstrap = runtime / "application/client/from_args.rs"
composition = root / "agent/composition/src/runtime.rs"
violations = []

if not bootstrap.is_file():
    violations.append("agent/features/runtime/src/application/client/from_args.rs: Runtime bootstrap source is missing")
else:
    production = bootstrap.read_text().split("#[cfg(test)]", 1)[0]
    if not re.search(r"\bhook_runner\b", production):
        violations.append("agent/features/runtime/src/application/client/from_args.rs: Runtime dependencies must carry injected hook_runner")

for path in runtime.rglob("*.rs"):
    if "tests" in path.parts or path.name.endswith("_tests.rs"):
        continue
    source = path.read_text().split("#[cfg(test)]", 1)[0]
    for pattern, message in [
        (r"\bbuild_hook_runner\s*\(", "Runtime production must not define or invoke build_hook_runner"),
        (r"\bDispatcher::(?:try_new|new)\s*\(", "Runtime production must not construct Hook dispatcher"),
        (r"\bhook::build_dispatcher\s*\(", "Runtime production must not construct Hook dispatcher"),
        (r"\bbuild_dispatcher\s*\(", "Runtime production must not construct Hook dispatcher"),
    ]:
        if re.search(pattern, source):
            violations.append(f"{path.relative_to(root)}: {message}")

if not composition.is_file():
    violations.append("agent/composition/src/runtime.rs: Composition runtime assembly source is missing")
else:
    source = composition.read_text()
    for pattern in [r"hook::build_dispatcher\s*\(", r"committed_snapshot\(\)\.hooks\(\)", r"hook_runner"]:
        if not re.search(pattern, source):
            violations.append("agent/composition/src/runtime.rs: Composition must construct and inject hook_runner from the committed config snapshot")
            break

if violations:
    print(json.dumps({"decision":"block","reason":"Runtime Hook assembly ownership guard FAILED:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)
print("Runtime Hook assembly ownership guard OK.")
PY
