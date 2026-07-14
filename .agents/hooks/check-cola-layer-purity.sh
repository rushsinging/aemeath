#!/usr/bin/env bash
set -euo pipefail

# 功能：检查每个 feature 内部 COLA 分层的依赖方向。
# 作用：内层只能内→外、不能外→内——domain/business 不得依赖 core 编排 / gateway /
#       contract，utils 保持叶子（§6.4.8 分层纯度）。
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
# Dependency direction inside a feature: outer/application layers may depend inward;
# domain/business must not depend on orchestration/gateway/contract, and utils must stay leaf-like.
FORBIDDEN_LAYER_DEPS = {
    "business": {"core", "gateway", "contract"},
    "utils": {"business", "core", "gateway", "contract"},
    "contract": {"business", "core", "gateway", "utils"},
    "gateway": {"business", "utils"},
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
    ("agent/features/runtime/src/utils/adapter.rs", "core"),
    ("agent/features/runtime/src/utils/bootstrap/runtime_support.rs", "business"),
    ("agent/features/tools/src/business/mcp_manager/connection.rs", "core"),
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
    if len(parts) >= 4 and parts[1] == "src" and parts[2] in FEATURE_LAYERS:
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
    if line_layer_violations("business", "use crate::utils::normalize_path;"):
        raise AssertionError("sanity allow failed: business depending on utils")


run_sanity()
violations: list[str] = []
seen_exceptions: set[tuple[str, str]] = set()
for old_path in RUNTIME_PROVIDER_TOOLS_OLD_PATHS:
    if old_path.exists():
        violations.append(f"{old_path.relative_to(root)}: runtime/provider/tools must live under agent/features/*")

# Context crate organises its first level by domain sub-module (session/compact/
# budget/prompt/memory_inject/port), not by COLA layer.  Each sub-module internally
# may use COLA layers.  Design doc: docs/design/02-modules/context-management/README.md
CONTEXT_DOMAIN_DIRS = {"session", "compact", "budget", "prompt", "memory_inject", "context_port", "port"}

features_root = root / "agent" / "features"
for feature_src in sorted(features_root.glob("*/src")):
    crate_name = feature_src.parent.name
    for child in feature_src.iterdir():
        if child.name.startswith("."):
            continue
        if child.is_dir() and child.name not in FEATURE_LAYERS:
            # Context crate uses domain sub-modules at top level (by design).
            if crate_name == "context" and child.name in CONTEXT_DOMAIN_DIRS:
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
            violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

stale = LAYER_MIGRATION_EXCEPTIONS - seen_exceptions
if stale:
    violations.append(
        "COLA migration exception list is stale; remove exact path(s): "
        + ", ".join(f"{path}->{layer}" for path, layer in sorted(stale))
    )

if violations:
    reason = "COLA layer purity guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("COLA layer purity guard OK.")
PY
