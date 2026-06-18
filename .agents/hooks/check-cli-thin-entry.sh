#!/usr/bin/env bash
set -euo pipefail

# 功能：检查 apps/cli 只直接依赖 composition + sdk + 纯技术库。
# 作用：守住 §6.7 薄入口——CLI 不得直连 runtime 内部或任何 supporting feature，
#       业务能力一律经 composition 装配 + sdk::AgentClient 契约接入。
# 例外：无。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import subprocess
import sys

CLI_CRATE = "cli"
ALLOWED_CLI_WORKSPACE_DEPS = {"composition", "sdk"}
FORBIDDEN_DOMAIN_CRATES = {
    "runtime",
    "project",
    "policy",
    "prompt",
    "provider",
    "tools",
    "storage",
    "hook",
    "audit",
    "share",
    "update",
}
BOOTSTRAP_DETAIL = re.compile(
    r"\bAgentClientImpl\b|"
    r"(?:^|[^A-Za-z0-9_])from_args(?:[^A-Za-z0-9_]|$)|"
    r"\bwire_runtime\b|"
    r"\bruntime::(?:api::)?(?:gateway|core|business|utils|contract|AgentClientImpl)\b"
)
DIRECT_IMPORT = re.compile(
    r"(?<![A-Za-z0-9_:])(?:use\s+)?("
    + "|".join(sorted(map(re.escape, FORBIDDEN_DOMAIN_CRATES)))
    + r")::"
)


def manifest_violations(manifest_text: str) -> list[str]:
    violations: list[str] = []
    for crate in sorted(FORBIDDEN_DOMAIN_CRATES):
        pattern = re.compile(rf"(?m)^\s*{re.escape(crate)}\s*=\s*\{{[^\n]*path\s*=")
        if pattern.search(manifest_text):
            violations.append(
                f"apps/cli/Cargo.toml must not declare direct path dependency on {crate}; use composition + sdk"
            )
    if 'composition = { path = "../../agent/composition" }' not in manifest_text:
        violations.append(
            "apps/cli/Cargo.toml must depend on composition via ../../agent/composition for composition root assembly"
        )
    if 'sdk = { path = "../../packages/sdk" }' not in manifest_text:
        violations.append(
            "apps/cli/Cargo.toml must depend on sdk via ../../packages/sdk for AgentClient contract"
        )
    return violations


def source_line_violations(line: str, local_modules: set[str] | None = None) -> list[str]:
    stripped = line.strip()
    if not stripped or stripped.startswith("//"):
        return []
    violations: list[str] = []
    # 排除文件中声明了同名 mod 的本地模块（如 TUI 的 update 模块）
    forbidden = FORBIDDEN_DOMAIN_CRATES
    if local_modules:
        forbidden = forbidden - local_modules
    direct_import = re.compile(
        r"(?<![A-Za-z0-9_:])(?:use\s+)?("
        + "|".join(sorted(map(re.escape, forbidden)))
        + r")::"
    )
    if direct_import.search(line):
        violations.append(
            "CLI must not import runtime/supporting/shared crates directly; use composition + sdk"
        )
    if BOOTSTRAP_DETAIL.search(line):
        violations.append(
            "CLI must not name runtime gateway/impl/bootstrap details; use composition::app::build_agent_client"
        )
    return violations


def workspace_dependency_violations() -> list[str]:
    metadata = json.loads(
        subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"])
    )
    workspace_names = {package["name"] for package in metadata["packages"]}
    violations: list[str] = []
    for package in metadata["packages"]:
        if package["name"] != CLI_CRATE:
            continue
        for dep in package.get("dependencies", []):
            dep_name = dep["name"]
            if dep.get("source") is None and dep_name in workspace_names and dep_name not in ALLOWED_CLI_WORKSPACE_DEPS:
                violations.append(
                    f"apps/cli may only depend on workspace crates {sorted(ALLOWED_CLI_WORKSPACE_DEPS)}; found {dep_name}"
                )
    return violations


def run_sanity() -> None:
    bad_manifest = 'runtime = { path = "../../agent/features/runtime" }\ncomposition = { path = "../../agent/composition" }\nsdk = { path = "../../packages/sdk" }\n'
    if not any("runtime" in item for item in manifest_violations(bad_manifest)):
        raise AssertionError("sanity block failed: CLI direct runtime Cargo dependency")
    if not source_line_violations("use runtime::api::RuntimeGateway;"):
        raise AssertionError("sanity block failed: CLI direct runtime import")
    if not source_line_violations("let _ = AgentClientImpl::from_args(args);"):
        raise AssertionError("sanity block failed: CLI runtime impl/from_args detail")
    if source_line_violations("use sdk::AgentClient;"):
        raise AssertionError("sanity allow failed: CLI SDK contract import")


run_sanity()
violations = manifest_violations(Path("apps/cli/Cargo.toml").read_text())
violations.extend(workspace_dependency_violations())

for path in sorted(Path("apps/cli/src").rglob("*.rs")):
    rel = path.as_posix()
    text = path.read_text()
    # 解析文件中声明的本地模块（如 `pub mod update;`），排除同名 crate 的误报
    local_modules = set(re.findall(r'\b(?:pub\s+)?mod\s+(\w+)\s*;', text))
    for lineno, line in enumerate(text.splitlines(), 1):
        for violation in source_line_violations(line, local_modules):
            violations.append(f"{rel}:{lineno}: {violation}: {line.strip()}")

if violations:
    print(json.dumps({"decision": "block", "reason": "Thin CLI guard failed:\n" + "\n".join(violations)}, ensure_ascii=False))
    sys.exit(2)

print("Thin CLI dependency guard OK.")
PY
