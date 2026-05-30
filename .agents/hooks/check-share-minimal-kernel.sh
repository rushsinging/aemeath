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
share_src = root / "agent" / "share" / "src"
share_manifest = root / "agent" / "share" / "Cargo.toml"

forbidden_patterns = [
    (re.compile(r"\bToolRegistry\b"), "ToolRegistry belongs to tools::api, not share"),
    (re.compile(r"\bTaskStore\b"), "TaskStore belongs to storage::api, not share"),
    (re.compile(r"\bTaskStoreStats\b"), "TaskStoreStats belongs to storage::api, not share"),
    (re.compile(r"\bstd::fs::|\btokio::fs::|\bFile::|\bread_to_string\b|\bwrite\(|\bcreate_dir"), "share must not perform fs IO"),
    (re.compile(r"\bstd::process::|\btokio::process::|\bCommand::new\b"), "share must not spawn processes"),
    (re.compile(r"\breqwest::|\bhyper::|\bureq::"), "share must not perform network IO"),
    (re.compile(r"\bparking_lot::|\bRwLock\b"), "stateful registries/stores do not belong in share"),
]

forbidden_modules = {
    "task/batch.rs": "task batch behavior belongs to storage::api",
    "task/display.rs": "task display behavior belongs to storage::api",
    "task/list.rs": "task list behavior belongs to storage::api",
    "task/store.rs": "task store behavior belongs to storage::api",
}

forbidden_dependencies = {
    "dirs",
    "rand",
    "futures",
    "parking_lot",
    "regex",
    "chrono",
    "inventory",
    "url",
    "reqwest",
    "bytes",
    "futures-util",
}

violations = []
for rel, reason in forbidden_modules.items():
    path = share_src / rel
    if path.exists():
        violations.append(f"agent/share/src/{rel}: {reason}")

for path in sorted(share_src.rglob("*.rs")):
    rel = path.relative_to(root)
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        for pattern, reason in forbidden_patterns:
            if pattern.search(line):
                violations.append(f"{rel}:{lineno}: {reason}: {stripped}")

if share_manifest.exists():
    manifest = share_manifest.read_text()
    for dep in sorted(forbidden_dependencies):
        if re.search(rf"^\s*{re.escape(dep)}\s*=", manifest, re.MULTILINE):
            violations.append(f"agent/share/Cargo.toml: forbidden dependency `{dep}` for minimal share kernel")

if violations:
    reason = "Share minimal kernel guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Share minimal kernel guard OK.")
PY
