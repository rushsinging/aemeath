#!/usr/bin/env bash
set -euo pipefail

# 功能：锁定 Composition Root 的 capability-first wiring modules 结构。
# 作用：Composition 只按装配职责分片，不机械复制 feature crate 的 Hexagonal/COLA 层。
# 白名单：无；允许的顶层源码文件与 lib.rs 模块声明是结构化 Target policy。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
src = root / "agent" / "composition" / "src"
allowed_files = {"lib.rs", "app.rs", "audit.rs", "memory.rs", "provider.rs", "runtime.rs", "tools.rs", "update.rs"}
expected_modules = {"app", "audit", "memory", "provider", "runtime", "tools", "update"}
forbidden_layers = {
    "domain", "application", "ports", "adapters",
    "api", "business", "contract", "core", "gateway", "capabilities",
}
violations: list[str] = []


def run_sanity() -> None:
    if "domain" not in forbidden_layers or "api" not in forbidden_layers:
        raise AssertionError("sanity block failed: Hexagonal/COLA layers must be forbidden")
    if "app.rs" not in allowed_files or "update" not in expected_modules:
        raise AssertionError("sanity allow failed: expected wiring modules missing")


run_sanity()
if not src.is_dir():
    violations.append("agent/composition/src: Composition source directory is missing")
else:
    for child in sorted(src.iterdir()):
        if child.name.startswith("."):
            continue
        if child.is_dir():
            kind = "forbidden Hexagonal/COLA layer" if child.name in forbidden_layers else "unregistered directory"
            violations.append(
                f"{child.relative_to(root)}: Composition must use flat capability-first wiring modules; {kind}"
            )
        elif child.name not in allowed_files:
            stem = child.stem
            kind = "forbidden Hexagonal/COLA layer" if stem in forbidden_layers else "unregistered source file"
            violations.append(
                f"{child.relative_to(root)}: Composition top-level source files must be {sorted(allowed_files)}; {kind}"
            )

    lib = src / "lib.rs"
    if lib.is_file():
        text = lib.read_text()
        declared = set(re.findall(r"(?m)^pub mod ([A-Za-z_][A-Za-z0-9_]*);\s*$", text))
        missing = expected_modules - declared
        unexpected = declared - expected_modules
        if missing:
            violations.append("agent/composition/src/lib.rs: missing wiring module declarations: " + ", ".join(sorted(missing)))
        if unexpected:
            violations.append("agent/composition/src/lib.rs: unexpected public module declarations: " + ", ".join(sorted(unexpected)))

    runtime_wiring = src / "runtime.rs"
    if runtime_wiring.is_file():
        text = runtime_wiring.read_text()
        required_patterns = {
            "provider gateway forwarding": r"gateways\.provider",
            "tool gateway forwarding": r"gateways\.tools",
        }
        for label, pattern in required_patterns.items():
            if not re.search(pattern, text):
                violations.append(
                    f"agent/composition/src/runtime.rs: missing {label}; FeatureGateways must be consumed"
                )
        if re.search(r"\b_gateways\s*:\s*FeatureGateways", text):
            violations.append(
                "agent/composition/src/runtime.rs: FeatureGateways must not be ignored"
            )

    runtime_bootstrap = root / "agent" / "features" / "runtime" / "src" / "application" / "client" / "from_args.rs"
    if runtime_bootstrap.is_file():
        text = runtime_bootstrap.read_text()
        required_patterns = {
            "injected provider gateway parameter": r"provider_gateway\s*:\s*Arc<dyn provider::LlmProviderGateway>",
            "injected tool gateway parameter": r"tool_gateway\s*:\s*Arc<dyn tools::ToolCatalogGateway>",
            "provider gateway client construction": r"build_llm_client_with_gateway\s*\(\s*provider_gateway\.as_ref\(\)",
            "tool gateway registry construction": r"tool_gateway\.new_registry\s*\(\)",
            "tool gateway registration": r"tool_gateway\.register_all_tools\s*\(",
        }
        for label, pattern in required_patterns.items():
            if not re.search(pattern, text, re.DOTALL):
                violations.append(
                    f"{runtime_bootstrap.relative_to(root)}: missing {label}"
                )

if violations:
    reason = "Composition layout guard FAILED:\n" + "\n".join(violations)
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Composition capability-first layout guard OK.")
PY
