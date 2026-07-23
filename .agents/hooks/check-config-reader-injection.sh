# guard-registry:scope.config.reader-tests-only
#!/usr/bin/env bash
# Guard: ConfigAppService 只能由 Config crate 内部与 Composition 构造；Runtime/TUI/CLI 禁止 new。
# 同时禁止 TUI/CLI 持有 Config-owned reader/query/writer/participant/watch 类型。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
if [ -n "${AEMEATH_PROJECT_DIR:-}" ] && [ ! -d "${AEMEATH_PROJECT_DIR}/.agents/hooks" ]; then
  ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
fi

cd "$ROOT"
python3 - <<'PY'
from pathlib import Path
import json
import re
import sys

root = Path.cwd()
violations: list[str] = []
construction = re.compile(r"\bConfigAppService::new\b")
contract_leak = re.compile(
    r"\b(ConfigReader|ConfigQuery|ConfigWriter|ProjectConfigParticipant|ConfigSubscription|watch::Receiver<ConfigSnapshot>)\b"
)


def is_test_file(path: Path) -> bool:
    return (
        path.name.endswith("_test.rs")
        or path.name.endswith("_tests.rs")
        or path.stem == "tests"
        or "tests" in path.parts
    )


def production_prefix(text: str) -> str:
    # 仓库约定内联测试模块位于文件尾部；只剥离 `#[cfg(test)] mod name {`，
    # 不剥离声明式 `#[cfg(test)] mod name;` 后面的生产代码。
    marker = re.compile(
        r"(?m)^\s*#\[cfg\(test\)\]\s*(?:#\[[^\n]+\]\s*)*mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
    )
    matches = list(marker.finditer(text))
    return text[: matches[-1].start()] if matches else text


for base in [root / "agent" / "features" / "runtime" / "src", root / "apps" / "cli" / "src"]:
    for path in sorted(base.rglob("*.rs")):
        if is_test_file(path):
            continue
        production = production_prefix(path.read_text())
        for lineno, line in enumerate(production.splitlines(), 1):
            if construction.search(line):
                violations.append(
                    f"{path.relative_to(root)}:{lineno}: Runtime/TUI/CLI must not construct ConfigAppService"
                )

for path in sorted((root / "apps" / "cli" / "src").rglob("*.rs")):
    if is_test_file(path):
        continue
    production = production_prefix(path.read_text())
    for lineno, line in enumerate(production.splitlines(), 1):
        if contract_leak.search(line):
            violations.append(
                f"{path.relative_to(root)}:{lineno}: delivery layer must not hold Config-owned contracts"
            )

step_files = [
    root / "agent" / "features" / "runtime" / "src" / "application" / "chat" / "looping" / "main_run_port.rs",
    root / "agent" / "features" / "runtime" / "src" / "application" / "agent" / "runner" / "loop_run.rs",
]
step_contracts = re.compile(r"\b(ConfigReader|ConfigQuery|ConfigWriter|refresh_if_sources_changed)\b")
for path in step_files:
    production = production_prefix(path.read_text())
    for lineno, line in enumerate(production.splitlines(), 1):
        if step_contracts.search(line):
            violations.append(
                f"{path.relative_to(root)}:{lineno}: Run Step must only consume its frozen RunConfigSnapshot"
            )

if violations:
    print(
        json.dumps(
            {"decision": "block", "reason": "Config reader injection guard FAILED:\n" + "\n".join(violations)},
            ensure_ascii=False,
        )
    )
    sys.exit(2)

print("Config reader injection guard OK.")
PY
