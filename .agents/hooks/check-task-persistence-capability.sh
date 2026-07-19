#!/usr/bin/env bash
set -euo pipefail

# Task persistence authority is confined to Context and Composition.
# Runtime and Tools may consume TaskAccess only.
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
forbidden = re.compile(
    r"\b(TaskPersist|PreparedTaskRestore|TaskRestoreAdapter|TaskSnapshotSource|SessionTaskAdapters|TaskWiring|wire_task)\b"
)
violations = []
for crate in ("runtime", "tools"):
    source = root / "agent" / "features" / crate / "src"
    for path in source.rglob("*.rs"):
        relative = path.relative_to(root).as_posix()
        if path.name.endswith("_tests.rs") or "tests" in path.parts or path.name == "trait_reflection.rs":
            continue
        text = path.read_text(encoding="utf-8")
        # Inline cfg(test) modules are test-only capabilities; strip a terminal
        # inline test module while keeping all preceding production code scanned.
        marker = re.search(r"(?m)^\s*#\[cfg\(test\)\]\s*\n\s*mod\s+tests\s*\{", text)
        if marker:
            text = text[: marker.start()]
        # Remove comments before symbol scanning to avoid documentation-only matches.
        text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
        text = re.sub(r"(?m)//.*$", "", text)
        for number, line in enumerate(text.splitlines(), 1):
            stripped = line.strip()
            if forbidden.search(line):
                violations.append(f"{relative}:{number}: {stripped}")

if violations:
    reason = (
        "Task persistence authority must stay in Context/Composition; Runtime and Tools may only use TaskAccess:\n"
        + "\n".join(violations)
    )
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

# Sanity: the detector must reject every protected capability and leave TaskAccess alone.
for probe in (
    "use task::TaskPersist;",
    "let _: task::PreparedTaskRestore;",
    "let _ = TaskRestoreAdapter::new(port);",
    "let _ = task::wire_task().persist();",
    "let _: task::TaskWiring;",
):
    if not forbidden.search(probe):
        raise AssertionError(f"sanity block failed: {probe}")
if forbidden.search("use task::TaskAccess;"):
    raise AssertionError("sanity allow failed: TaskAccess")

print("Task persistence capability guard OK.")
PY
