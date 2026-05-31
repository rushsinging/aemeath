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
share_src = root / "agent" / "shared" / "src"
share_manifest = root / "agent" / "shared" / "Cargo.toml"

forbidden_patterns = [
    (re.compile(r"\bToolRegistry\b"), "ToolRegistry belongs to tools::api, not share"),
    (re.compile(r"\bTaskStore\b"), "TaskStore belongs to storage::api, not share"),
    (re.compile(r"\bTaskStoreStats\b"), "TaskStoreStats belongs to storage::api, not share"),
    (re.compile(r"\bstd::fs::|\btokio::fs::|\bFile::|\bread_to_string\b|\bwrite\(|\bcreate_dir"), "share must not perform fs IO"),
    (re.compile(r"\bstd::process::|\btokio::process::|\bCommand::new\b"), "share must not spawn processes"),
    (re.compile(r"\breqwest::|\bhyper::|\bureq::|\bhttp::"), "share must not perform network/http IO"),
    (re.compile(r"\bparking_lot::|\bRwLock\b"), "stateful registries/stores do not belong in share"),
    (re.compile(r"#\[\s*async_trait\s*\]"), "async trait (behavior) belongs to a feature, not share kernel"),
    (re.compile(r"\btrait\s+(Tool|AgentRunner)\b"), "Tool/AgentRunner behavior traits belong to tools::api, not share"),
    (re.compile(r"Arc\s*<\s*Mutex\b"), "Arc<Mutex> runtime state belongs to a feature, not share kernel"),
    (re.compile(r"\btokio::sync::(?:(?:mpsc|Semaphore|oneshot)\b|\{[^}]*\b(?:mpsc|Semaphore|oneshot)\b[^}]*\})"), "concurrency primitives belong to a feature, not share kernel"),
    (re.compile(r"\bCancellationToken\b"), "CancellationToken belongs to a feature, not share kernel"),
    (re.compile(r"\bSystemTime::now\b|\bInstant::now\b"), "share kernel must not read clock"),
    (re.compile(r"\bUuid::now_v7\b|\bUuid::new_v4\b"), "share kernel must not generate ids (inject from caller)"),
]

per_file_exemptions = {}

forbidden_modules = {
    "task/batch.rs": "task batch behavior belongs to storage::api",
    "task/display.rs": "task display behavior belongs to storage::api",
    "task/list.rs": "task list behavior belongs to storage::api",
    "task/store.rs": "task store behavior belongs to storage::api",
}

# Current target-state dependency budget for share per Cargo reality: data/schema
# support plus the existing tokio/tokio-util compatibility entries. Behavior use is
# guarded in source above; adding new broad infra crates remains forbidden here.
allowed_dependencies = {
    "serde",
    "serde_json",
    "serde_yml",
    "thiserror",
    "tokio",
    "tokio-util",
    "uuid",
    "log",
    "unicode-width",
}


def dependency_names(manifest: str) -> list[str]:
    names: list[str] = []
    in_dependencies = False
    for line in manifest.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_dependencies = stripped == "[dependencies]"
            continue
        if not in_dependencies or not stripped or stripped.startswith("#"):
            continue
        match = re.match(r"([A-Za-z0-9_-]+)\s*=", stripped)
        if match:
            names.append(match.group(1))
    return names


def run_sanity() -> None:
    bad_lines = [
        "let _ = std::fs::read_to_string(path);",
        "let _ = std::process::Command::new(\"sh\");",
        "let _ = reqwest::Client::new();",
        "pub type Store = parking_lot::RwLock<Vec<String>>;",
        "#[async_trait]",
        "pub trait Tool: Send + Sync {}",
        "pub working_root: Arc<Mutex<PathBuf>>,",
        "use tokio::sync::{mpsc,Semaphore,oneshot};",
        "pub cancel: CancellationToken,",
        "let _ = std::time::SystemTime::now();",
        "let _ = uuid::Uuid::now_v7();",
    ]
    for line in bad_lines:
        if not any(pattern.search(line) for pattern, _reason in forbidden_patterns):
            raise AssertionError(f"sanity block failed: {line}")
    if "reqwest" not in dependency_names("[dependencies]\nserde = {}\nreqwest = {}\n"):
        raise AssertionError("sanity manifest dependency parser failed")


run_sanity()
violations: list[str] = []
for rel, reason in forbidden_modules.items():
    path = share_src / rel
    if path.exists():
        violations.append(f"agent/shared/src/{rel}: {reason}")

for path in sorted(share_src.rglob("*.rs")):
    rel = path.relative_to(root)
    share_rel = path.relative_to(share_src).as_posix()
    exemption = per_file_exemptions.get(share_rel) or per_file_exemptions.get(path.name)
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        for pattern, reason in forbidden_patterns:
            if pattern.search(line) and exemption is None:
                violations.append(f"{rel}:{lineno}: {reason}: {stripped}")

if share_manifest.exists():
    for dep in dependency_names(share_manifest.read_text()):
        if dep not in allowed_dependencies:
            violations.append(f"agent/shared/Cargo.toml: dependency `{dep}` is outside the shared-kernel allowlist {sorted(allowed_dependencies)}")

if violations:
    reason = "Share minimal kernel guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Share minimal kernel guard OK.")
PY
