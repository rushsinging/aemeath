#!/usr/bin/env bash
set -euo pipefail

# 功能：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature 的目标目录。
# 作用：普通 feature 继续受迁移期 COLA 依赖方向约束；Runtime 使用
#       domain/application/ports/adapters/shared；Workflow 使用 domain；Storage 使用 domain/ports/adapters；
#       Audit 仅允许随真实 Usage 交付增量建立的 Hexagonal 层，禁止恢复空 COLA 占位。
# 例外：少量已登记的迁移期层级倒置（见脚本内 narrow migration exceptions 列表）。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
FEATURE_LAYERS = {"contract", "gateway", "core", "business", "utils"}
RUNTIME_HEX_LAYERS = {"domain", "application", "ports", "adapters", "shared"}
WORKFLOW_HEX_LAYERS = {"domain"}
PROVIDER_HEX_LAYERS = {"domain", "adapters"}
PROVIDER_LEGACY_LAYERS = {"api", "business", "contract", "core", "gateway"}
STORAGE_HEX_LAYERS = {"domain", "ports", "adapters"}
STORAGE_TRANSITIONAL_MODULES = {"memory_store", "task_store"}
STORAGE_LEGACY_LAYERS = {"api", "business", "contract", "gateway"}
PROJECT_HEX_LAYERS = {"domain", "adapters"}
PROJECT_ALLOWED_TOP_LEVEL_FILES = {"lib.rs", "domain.rs", "adapters.rs"}
PROJECT_LEGACY_LAYERS = {"api", "business", "contract", "core", "gateway", "capabilities"}
AUDIT_HEX_LAYERS = {"domain", "ports"}
AUDIT_ALLOWED_TOP_LEVEL_FILES = {"lib.rs", "domain.rs", "ports.rs"}
AUDIT_LEGACY_LAYERS = {"api", "business", "contract", "core", "gateway", "capabilities"}
# Dependency direction inside a feature: outer/application layers may depend inward;
# domain/business must not depend on orchestration/gateway/contract, and utils must stay leaf-like.
FORBIDDEN_LAYER_DEPS = {
    "business": {"core", "gateway", "contract"},
    "utils": {"business", "core", "gateway", "contract"},
    "contract": {"business", "core", "gateway", "utils"},
    "gateway": {"business", "utils"},
    "domain": {"application", "ports", "adapters"},
    "ports": {"application", "adapters"},
    "application": {"adapters"},
    "shared": {"domain", "application", "ports", "adapters"},
}
RUNTIME_PROVIDER_TOOLS_OLD_PATHS = [
    root / "agent" / "runtime",
    root / "agent" / "provider",
    root / "agent" / "tools",
]
# Narrow migration exceptions for already-existing layer inversions. These are
# path + target-layer limited so new COLA violations still fail. Runtime
# bootstrap/adapter still owns temporary wiring; tools MCP connection still
# reaches the registry until the registry port is split.
LAYER_MIGRATION_EXCEPTIONS = {
    ("agent/features/tools/src/business/mcp_manager/connection.rs", "core"),
}
RUNTIME_LAYER_MIGRATION_EXCEPTIONS = {
    ("agent/features/runtime/src/application/client/accessors.rs", "adapters"),
    ("agent/features/runtime/src/application/client/from_args.rs", "adapters"),
    ("agent/features/runtime/src/ports/input_buffer.rs", "application"),
    ("agent/features/runtime/src/ports/legacy.rs", "application"),
}
use_crate_segment = re.compile(r"\b(?:use\s+)?crate::([A-Za-z_][A-Za-z0-9_]*)")
project_domain_adapter_pattern = re.compile(
    r"\bcrate\s*::\s*(?:adapters\b|\{[^}]*\badapters\s*::)", re.DOTALL
)


def is_test_path(path: Path) -> bool:
    parts = path.parts
    return path.name.endswith("_test.rs") or path.name.endswith("_tests.rs") or path.stem == "tests" or "tests" in parts


def feature_layer_for(path: Path) -> tuple[str, str] | None:
    try:
        rel = path.relative_to(root / "agent" / "features")
    except ValueError:
        return None
    parts = rel.parts
    if len(parts) < 3:
        return None
    normalized_layer = parts[2].removesuffix(".rs")
    if parts[1] == "src":
        if parts[0] == "runtime" and normalized_layer in RUNTIME_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "workflow" and normalized_layer in WORKFLOW_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "provider" and normalized_layer in PROVIDER_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "storage" and normalized_layer in STORAGE_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "context" and normalized_layer in CONTEXT_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "project" and normalized_layer in PROJECT_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "audit" and normalized_layer in AUDIT_HEX_LAYERS:
            return parts[0], normalized_layer
        if parts[0] == "storage":
            return None
        if parts[2] in FEATURE_LAYERS:
            return parts[0], parts[2]
    return None


def line_layer_violations(current_layer: str, line: str) -> list[tuple[str, str]]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//") or stripped.startswith("*"):
        return []
    violations: list[tuple[str, str]] = []
    for target_layer in use_crate_segment.findall(line):
        if target_layer in FORBIDDEN_LAYER_DEPS.get(current_layer, set()):
            violations.append((target_layer, f"feature layer {current_layer} must not depend on crate::{target_layer}"))
    return violations


def run_sanity() -> None:
    if not line_layer_violations("business", "use crate::core::port::ToolPort;"):
        raise AssertionError("sanity block failed: business depending on core")
    if not line_layer_violations("utils", "let _ = crate::business::Policy::default();"):
        raise AssertionError("sanity block failed: utils depending on business")
    if line_layer_violations("core", "use crate::business::TaskState;"):
        raise AssertionError("sanity allow failed: core depending on business")
    if not line_layer_violations("domain", "use crate::application::Agent;"):
        raise AssertionError("sanity block failed: runtime domain depending on application")
    if not line_layer_violations("application", "use crate::adapters::SdkProjection;"):
        raise AssertionError("sanity block failed: runtime application depending on adapters")
    if line_layer_violations("application", "use crate::domain::Run;"):
        raise AssertionError("sanity allow failed: runtime application depending on domain")
    if line_layer_violations("adapters", "use crate::ports::EventSink;"):
        raise AssertionError("sanity allow failed: runtime adapter depending on ports")
    if line_layer_violations("business", "use crate::utils::normalize_path;"):
        raise AssertionError("sanity allow failed: business depending on utils")
    if not line_layer_violations("domain", "use crate::adapters::git::GitCli;"):
        raise AssertionError("sanity block failed: Project domain depending on adapters")
    if line_layer_violations("adapters", "use crate::domain::git::GitWorktreeOps;"):
        raise AssertionError("sanity allow failed: Project adapters depending on domain")
    if not project_domain_adapter_pattern.search("use crate::{\n adapters::git::GitCli,\n};"):
        raise AssertionError("sanity block failed: multiline braced Project domain dependency")
    if not project_domain_adapter_pattern.search("use crate::{\n domain::types::WorkspaceRead,\n adapters::git::GitCli,\n};"):
        raise AssertionError("sanity block failed: non-first braced Project domain dependency")
    if not project_domain_adapter_pattern.search("use crate::\n adapters::git::GitCli;"):
        raise AssertionError("sanity block failed: multiline Project domain dependency")


run_sanity()
violations: list[str] = []
seen_exceptions: set[tuple[str, str]] = set()
seen_runtime_exceptions: set[tuple[str, str]] = set()
for old_path in RUNTIME_PROVIDER_TOOLS_OLD_PATHS:
    if old_path.exists():
        violations.append(f"{old_path.relative_to(root)}: runtime/provider/tools must live under agent/features/*")

# Context 已迁移到 Hexagonal Target；只允许四个目标层。
CONTEXT_HEX_LAYERS = {"domain", "application", "ports", "adapters"}

features_root = root / "agent" / "features"
for feature_src in sorted(features_root.glob("*/src")):
    crate_name = feature_src.parent.name
    for child in feature_src.iterdir():
        if child.name.startswith("."):
            continue
        if crate_name == "runtime" and child.is_dir() and child.name in FEATURE_LAYERS:
            violations.append(
                f"{child.relative_to(root)}: Runtime legacy COLA directory is forbidden; use {sorted(RUNTIME_HEX_LAYERS)}"
            )
            continue
        if crate_name == "workflow":
            if child.is_dir() and child.name not in WORKFLOW_HEX_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Workflow source directories must be {sorted(WORKFLOW_HEX_LAYERS)}"
                )
            elif child.is_file() and child.name not in {"lib.rs", "domain.rs"}:
                violations.append(
                    f"{child.relative_to(root)}: Workflow top-level source files must be lib.rs or domain.rs"
                )
            continue
        if crate_name == "provider":
            if child.stem in PROVIDER_LEGACY_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Provider legacy fixed layer is forbidden; use domain/ports/adapters"
                )
                continue
            if child.is_dir() and child.name not in PROVIDER_HEX_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Provider source directories must be {sorted(PROVIDER_HEX_LAYERS)}"
                )
                continue
            continue
        if crate_name == "project":
            if child.stem in PROJECT_LEGACY_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Project legacy fixed layer is forbidden; use {sorted(PROJECT_HEX_LAYERS)}"
                )
            elif child.is_dir() and child.name not in PROJECT_HEX_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Project source directories must be {sorted(PROJECT_HEX_LAYERS)}"
                )
            elif child.is_file() and child.name not in PROJECT_ALLOWED_TOP_LEVEL_FILES:
                violations.append(
                    f"{child.relative_to(root)}: Project top-level source files must be {sorted(PROJECT_ALLOWED_TOP_LEVEL_FILES)}"
                )
            continue
        if crate_name == "audit":
            if child.stem in AUDIT_LEGACY_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Audit empty or legacy fixed layer is forbidden; use evidence-backed {sorted(AUDIT_HEX_LAYERS)}"
                )
            elif child.is_dir() and child.name not in AUDIT_HEX_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Audit source directories must be evidence-backed layers {sorted(AUDIT_HEX_LAYERS)}"
                )
            elif child.is_file() and child.name not in AUDIT_ALLOWED_TOP_LEVEL_FILES:
                violations.append(
                    f"{child.relative_to(root)}: Audit top-level source files must be {sorted(AUDIT_ALLOWED_TOP_LEVEL_FILES)}"
                )
            continue
        if crate_name == "storage":
            if child.stem in STORAGE_LEGACY_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Storage legacy fixed layer is forbidden; use {sorted(STORAGE_HEX_LAYERS)}"
                )
            elif child.is_dir() and child.name not in STORAGE_HEX_LAYERS | STORAGE_TRANSITIONAL_MODULES:
                violations.append(
                    f"{child.relative_to(root)}: Storage directory must be a hexagonal layer {sorted(STORAGE_HEX_LAYERS)} or registered transitional module {sorted(STORAGE_TRANSITIONAL_MODULES)}"
                )
            continue
        if child.is_dir() and child.name not in FEATURE_LAYERS:
            # Runtime 已迁到单一 agent_execution 能力的六边形目标结构。
            if crate_name == "runtime" and child.name in RUNTIME_HEX_LAYERS:
                continue
            # Context 已迁到 Hexagonal 目标结构。
            if crate_name == "context" and child.name in CONTEXT_HEX_LAYERS:
                continue
            violations.append(
                f"{child.relative_to(root)}: feature src directories must be COLA layers {sorted(FEATURE_LAYERS)}"
            )

for path in sorted(features_root.rglob("*.rs")):
    if is_test_path(path):
        continue
    rel = path.relative_to(root)
    rel_s = rel.as_posix()
    if rel_s.startswith("agent/features/storage/src/domain/") or rel_s == "agent/features/storage/src/domain.rs":
        source = path.read_text()
        if re.search(r"\b(?:std|tokio)::fs::|\bPathBuf\b|\bcrate::adapters\b", source):
            violations.append(
                f"{rel}: Storage domain must not perform physical I/O, own PathBuf, or depend on adapters"
            )
    layer_info = feature_layer_for(path)
    if not layer_info:
        continue
    feature, layer = layer_info
    text = path.read_text()
    if feature == "project" and layer == "domain" and project_domain_adapter_pattern.search(text):
        violations.append(f"{rel}: Project domain must not depend on crate::adapters")
    for lineno, line in enumerate(text.splitlines(), 1):
        for target_layer, violation in line_layer_violations(layer, line):
            exception = (rel_s, target_layer)
            if exception in LAYER_MIGRATION_EXCEPTIONS:
                seen_exceptions.add(exception)
                continue
            if exception in RUNTIME_LAYER_MIGRATION_EXCEPTIONS:
                seen_runtime_exceptions.add(exception)
                continue
            violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

stale = LAYER_MIGRATION_EXCEPTIONS - seen_exceptions
if stale:
    violations.append(
        "COLA migration exception list is stale; remove exact path(s): "
        + ", ".join(f"{path}->{layer}" for path, layer in sorted(stale))
    )

stale_runtime = RUNTIME_LAYER_MIGRATION_EXCEPTIONS - seen_runtime_exceptions
if stale_runtime:
    violations.append(
        "Runtime hexagonal migration exception list is stale; remove exact path(s): "
        + ", ".join(f"{path}->{layer}" for path, layer in sorted(stale_runtime))
    )

if violations:
    reason = "COLA layer purity guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("COLA layer purity guard OK.")
PY
