#!/usr/bin/env bash
set -euo pipefail

# 功能：扫描 agent/shared/src，禁止 share kernel 出现行为/IO/并发/时钟/状态容器，
#       并把 share 的 Cargo 依赖限定在白名单内。
# 作用：守住 §6.4.5 rule6——kernel 只放数据契约与纯函数（禁 async_trait、Arc<Mutex>、
#       tokio::sync、CancellationToken、SystemTime::now、Uuid::now_v7、fs/process/net、
#       ToolRegistry/TaskStore 等）。
# 例外：per_file_exemptions——带退出条件的临时豁免（当前为空 {}）。
#       另见脚本内 forbidden_modules（防回归禁止清单，非豁免，就近有注释）。

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
    (re.compile(r"\bToolRegistry\b"), "ToolRegistry belongs to the tools crate-root facade, not share"),
    (re.compile(r"\bTaskStore\b"), "TaskStore belongs to storage::api, not share"),
    (re.compile(r"\bTaskStoreStats\b"), "TaskStoreStats belongs to storage::api, not share"),
    (re.compile(r"\bstd::fs::|\btokio::fs::|\bFile::|\bread_to_string\b|\bwrite\(|\bcreate_dir"), "share must not perform fs IO"),
    (re.compile(r"\bstd::process::|\btokio::process::|\bCommand::new\b"), "share must not spawn processes"),
    (re.compile(r"\breqwest::|\bhyper::|\bureq::|\bhttp::"), "share must not perform network/http IO"),
    (re.compile(r"\bparking_lot::|\bRwLock\b"), "stateful registries/stores do not belong in share"),
    (re.compile(r"#\[\s*async_trait\s*\]"), "async trait (behavior) belongs to a feature, not share kernel"),
    (re.compile(r"\btrait\s+(Tool|AgentRunner)\b"), "Tool/AgentRunner behavior traits belong to the tools crate-root facade, not share"),
    (re.compile(r"Arc\s*<\s*Mutex\b"), "Arc<Mutex> runtime state belongs to a feature, not share kernel"),
    (re.compile(r"\btokio::sync::(?:(?:mpsc|Semaphore|oneshot)\b|\{[^}]*\b(?:mpsc|Semaphore|oneshot)\b[^}]*\})"), "concurrency primitives belong to a feature, not share kernel"),
    (re.compile(r"\bCancellationToken\b"), "CancellationToken belongs to a feature, not share kernel"),
    (re.compile(r"\bSystemTime::now\b|\bInstant::now\b"), "share kernel must not read clock"),
    (re.compile(r"\bUuid::now_v7\b|\bUuid::new_v4\b"), "share kernel must not generate ids (inject from caller)"),
]

# per_file_exemptions：带退出条件的临时豁免（命中模式但放行某文件）。当前为空。
per_file_exemptions = {}

# forbidden_modules：防回归"禁止清单"（与 exemption 语义相反）——这些 task 行为文件
# 已在 #61 D2 迁出到 storage::api，下列路径一旦重新出现在 agent/shared/ 即视为越界。
# 当前 4 个文件均已不在 shared（守卫通过），保留此清单以拦截"行为爬回 kernel"。
forbidden_modules = {
    "task/batch.rs": "task batch behavior belongs to storage::api",
    "task/display.rs": "task display behavior belongs to storage::api",
    "task/list.rs": "task list behavior belongs to storage::api",
    "task/store.rs": "task store behavior belongs to storage::api",
    "tool.rs": "tool contracts and DTOs belong to the tools domain",
    "tool": "tool type modules belong to the tools domain",
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
    "logging",
    "unicode-width",
    "utils",
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
