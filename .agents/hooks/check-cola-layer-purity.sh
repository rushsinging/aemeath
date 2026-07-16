#!/usr/bin/env bash
set -euo pipefail

# 功能：检查未迁移 feature 的 COLA 分层，并锁定已迁移 feature 的目标目录。
# 作用：普通 feature 继续受迁移期 COLA 依赖方向约束；Runtime 只允许
#       domain/application/ports/adapters/shared；Storage 使用 capability-first 模块。
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
STORAGE_LEGACY_LAYERS = {"api", "business", "contract", "gateway"}
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
# Narrow migration exceptions for already-existing layer inversions.  These are
# path + target-layer limited so new COLA violations still fail.  Provider still
# keeps provider traits/config in core while provider protocol files live in
# business; runtime bootstrap/adapter still owns temporary wiring; tools MCP
# connection still reaches the registry until the registry port is split.
LAYER_MIGRATION_EXCEPTIONS = {
    ("agent/features/provider/src/business/providers/anthropic/message_conversion.rs", "core"),
    ("agent/features/provider/src/business/providers/anthropic.rs", "core"),
    ("agent/features/provider/src/business/providers/ollama/non_stream.rs", "core"),
    ("agent/features/provider/src/business/providers/ollama/stream.rs", "core"),
    ("agent/features/provider/src/business/providers/ollama.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/driver.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/non_stream.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/provider.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/request_body.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/stream.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/responses.rs", "core"),
    ("agent/features/provider/src/business/providers/openai_compatible/responses_stream.rs", "core"),
    ("agent/features/provider/src/business/stream.rs", "core"),
    ("agent/features/tools/src/business/mcp_manager/connection.rs", "core"),
}
RUNTIME_LAYER_MIGRATION_EXCEPTIONS = {
    ("agent/features/runtime/src/application/client/accessors.rs", "adapters"),
    ("agent/features/runtime/src/application/client/from_args.rs", "adapters"),
    ("agent/features/runtime/src/ports/input_buffer.rs", "application"),
    ("agent/features/runtime/src/ports/legacy.rs", "application"),
}
use_crate_segment = re.compile(r"\b(?:use\s+)?crate::([A-Za-z_][A-Za-z0-9_]*)")


def is_test_path(path: Path) -> bool:
    parts = path.parts
    return path.name.endswith("_test.rs") or path.name.endswith("_tests.rs") or path.stem == "tests" or "tests" in parts


def feature_layer_for(path: Path) -> tuple[str, str] | None:
    try:
        rel = path.relative_to(root / "agent" / "features")
    except ValueError:
        return None
    parts = rel.parts
    if len(parts) >= 3 and parts[1] == "src":
        if parts[0] == "runtime" and parts[2] in RUNTIME_HEX_LAYERS:
            return parts[0], parts[2]
        if parts[0] == "context" and parts[2] in CONTEXT_HEX_LAYERS:
            return parts[0], parts[2]
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
        if crate_name == "storage":
            if child.stem in STORAGE_LEGACY_LAYERS:
                violations.append(
                    f"{child.relative_to(root)}: Storage legacy fixed layer is forbidden; use capability-first modules"
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
    layer_info = feature_layer_for(path)
    if not layer_info:
        continue
    _feature, layer = layer_info
    rel = path.relative_to(root)
    rel_s = rel.as_posix()
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
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
