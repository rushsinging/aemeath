#!/usr/bin/env bash
set -euo pipefail

# 功能：锁定 #907 Provider 构造所有权——Provider 的具体构造符号
#       （LlmClient / LlmConfigOptions / InvocationScope / SystemBlock / LlmProvider）
#       以及 `provider::composition` 构造面，仅允许 Composition Root 生产引用。
# 作用：Runtime / Context / CLI 及任何非 composition crate 不得直接构造 provider
#       客户端或穿透 composition 模块；它们只能经 Published Language 或 Runtime
#       拥有的 ProviderFactory 端口消费。
# 白名单：无；composition crate 是唯一合法构造者，由结构化路径策略锁定。

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()

# #907 具体构造符号——已从 provider crate-root 退役，现仅经 provider::composition 暴露。
CONSTRUCTION_SYMBOLS = {
    "LlmClient", "LlmConfigOptions", "InvocationScope", "SystemBlock", "LlmProvider",
}
provider_root_symbol = re.compile(
    r"(?<![A-Za-z0-9_:])(?:::)?provider\s*::\s*("
    + "|".join(sorted(map(re.escape, CONSTRUCTION_SYMBOLS)))
    + r")\b"
)
provider_composition = re.compile(r"(?<![A-Za-z0-9_:])(?:::)?provider\s*::\s*composition\b")

# 非 Composition 源码树：feature crates + apps + packages。
NON_COMPOSITION_ROOTS = [
    root / "agent" / "features",
    root / "apps",
    root / "packages",
]


def is_generated_or_target(path: Path) -> bool:
    rel = path.as_posix()
    return "/target/" in rel or rel.startswith("target/")


def crate_for(path: Path) -> str | None:
    parts = path.parts
    if len(parts) >= 3 and parts[0] == "agent":
        if parts[1] == "shared":
            return "share"
        if parts[1] == "features" and len(parts) >= 4:
            return parts[2]
        return parts[1]
    if len(parts) >= 2 and parts[0] == "packages":
        return parts[1]
    if len(parts) >= 2 and parts[0] == "apps":
        return parts[1]
    return None


def is_composition(path: Path) -> bool:
    try:
        rel = path.relative_to(root)
    except ValueError:
        return False
    return rel.parts[:3] == ("agent", "composition", "src")


def is_comment(line: str) -> bool:
    stripped = line.lstrip()
    return stripped.startswith("//")


violations: list[str] = []

# (1)+(2) 非 Composition 代码禁止引用 provider::composition 或具体构造符号。
#      跳过注释行（文档注释可能为说明目的提及这些符号）。
for base in NON_COMPOSITION_ROOTS:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_generated_or_target(path):
            continue
        rel = path.relative_to(root)
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            if is_comment(line):
                continue
            if provider_composition.search(line):
                violations.append(
                    f"{rel}:{lineno}: provider::composition is Composition-Root-only; "
                    f"consume the Runtime-owned ProviderFactory port instead: {line.strip()}"
                )
            for match in provider_root_symbol.finditer(line):
                symbol = match.group(1)
                violations.append(
                    f"{rel}:{lineno}: provider::{symbol} is a retired concrete construction "
                    f"symbol; reach it via provider::composition (Composition Root only): "
                    f"{line.strip()}"
                )

# (3) 正向断言：provider::composition 仅被 agent/composition 生产代码引用。
#     全仓扫描，确认所有引用都在 agent/composition/ 下。
composition_refs: list[str] = []
for base in [root / "agent", root / "apps", root / "packages"]:
    if not base.exists():
        continue
    for path in sorted(base.rglob("*.rs")):
        if is_generated_or_target(path):
            continue
        rel = path.relative_to(root)
        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            if is_comment(line):
                continue
            if provider_composition.search(line):
                composition_refs.append(f"{rel}:{lineno}")
if not composition_refs:
    violations.append(
        "provider::composition must be referenced by Composition production "
        "(agent/composition) — found zero references anywhere"
    )
else:
    leaked = [ref for ref in composition_refs if not ref.startswith("agent/composition/")]
    if leaked:
        violations.append(
            "provider::composition leaked outside Composition Root: "
            + ", ".join(leaked[:10])
        )

if violations:
    reason = "Provider construction ownership guard FAILED:\n" + "\n".join(violations[:100])
    if len(violations) > 100:
        reason += f"\n... and {len(violations) - 100} more"
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    sys.exit(2)

print("Provider construction ownership guard OK.")
PY
