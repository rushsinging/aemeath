#!/bin/bash
# guard-registry:policy.cross-bc.construction-registry
#
# 注册表驱动的跨 BC 构造守卫（fail-closed）：
# - 数据源：.agents/architecture-guard-registry.json 的 construction_symbols 段。
# - 保护对象：feature crate adapters 模块导出的 concrete adapter 类型，
#   以及 feature crate 定义的 wire_* 装配函数。
# - 语义：
#   1. 未登记的跨 BC 构造调用（新增 adapter 未登记时）→ exit 2（fail-closed）。
#   2. 已登记符号出现在 allowed_paths 之外的生产代码 → exit 2。
# - 同 crate 内部构造不拦截（BC 内部事务）。
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
import json
import re
import sys
from pathlib import Path

root = Path.cwd()
registry_path = root / ".agents/architecture-guard-registry.json"
violations = []

# ── 数据加载 ──────────────────────────────────────────────────────────────
try:
    registry = json.loads(registry_path.read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError) as error:
    print(json.dumps({
        "decision": "block",
        "reason": f"cross-BC construction registry guard FAILED: cannot read registry: {error}",
    }, ensure_ascii=False))
    sys.exit(2)

symbol_entries = registry.get("construction_symbols")
if not isinstance(symbol_entries, list) or not symbol_entries:
    violations.append(".agents/architecture-guard-registry.json: construction_symbols section is missing or empty")

registered: dict[str, dict] = {}
for entry in symbol_entries or []:
    symbol = entry.get("symbol")
    if not symbol or not entry.get("owner_crate") or not entry.get("allowed_paths"):
        violations.append(f".agents/architecture-guard-registry.json: construction_symbols entry missing symbol/owner_crate/allowed_paths: {entry}")
        continue
    registered[symbol] = entry

# ── 测试段剥离：跳过 #[cfg(...test...)] 修饰的 item ────────────────────────
CFG_TEST_RE = re.compile(r"#\[\s*cfg\s*\([^]]*\btest\b[^]]*\)\s*\]")
ITEM_START_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(mod|fn|struct|enum|impl|trait|use|type|const|static|macro_rules)\b")


def strip_test_items(source: str) -> str:
    """剥离被 cfg-test 门控的 item，其余内容原样保留。

    属性行（#[...]）缓冲后跟随 item 起始行：若任一属性是 cfg-test，
    则按 brace 平衡或行尾分号跳过整个 item；否则正常输出。
    """
    lines = source.splitlines(keepends=True)
    output: list[str] = []
    index = 0
    total = len(lines)
    while index < total:
        line = lines[index]
        stripped = line.strip()
        if stripped.startswith("#["):
            attribute_lines = [line]
            index += 1
            while index < total and lines[index].strip().startswith("#["):
                attribute_lines.append(lines[index])
                index += 1
            comment_lines = []
            while index < total and (lines[index].strip().startswith("//") or lines[index].strip().startswith("/*")):
                comment_lines.append(lines[index])
                index += 1
            if index >= total:
                output.extend(attribute_lines)
                output.extend(comment_lines)
                break
            next_line = lines[index]
            gated = any(CFG_TEST_RE.search(candidate) for candidate in attribute_lines)
            if gated and ITEM_START_RE.match(next_line):
                index = _skip_item(lines, index)
                continue
            output.extend(attribute_lines)
            output.extend(comment_lines)
            continue
        output.append(line)
        index += 1
    return "".join(output)


def _skip_item(lines: list[str], start_index: int) -> int:
    """跳过从 start_index 起的 item，返回 item 之后的行号。"""
    index = start_index
    total = len(lines)
    while index < total and lines[index].strip().endswith("\\"):
        index += 1
    if index < total and lines[index].rstrip().endswith(";"):
        return index + 1
    depth = 0
    started = False
    while index < total:
        for character in lines[index]:
            if character == "{":
                depth += 1
                started = True
            elif character == "}":
                depth -= 1
        index += 1
        if started and depth <= 0:
            break
        if not started and index < total and lines[index].strip().endswith(";"):
            index += 1
            break
    return index

# ── 符号候选收集 ──────────────────────────────────────────────────────────
FEATURES_DIR = root / "agent/features"
FEATURE_CRATES = sorted(entry.name for entry in FEATURES_DIR.iterdir() if entry.is_dir())

CONSTRUCTOR_RE = re.compile(r"\b([A-Z][A-Za-z0-9_]*)\s*::\s*(?:new|default|with_[a-z_]+)\s*\(")
WIRE_CALL_RE = re.compile(r"\b((?:[a-z_][a-z0-9_]*\s*::\s*)+)(wire_[a-z0-9_]+)\s*\(")
PLAIN_USE_RE = re.compile(r"^\s*use\s+([^;]+);", re.M)
PUB_STRUCT_RE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+struct\s+([A-Za-z0-9_]+)", re.M)
PUB_WIRE_RE = re.compile(r"^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?fn\s+(wire_[a-z0-9_]+)", re.M)
ADAPTERS_USE_RE = re.compile(r"^\s*pub use\s+([^;]+);", re.M)


def split_use_list(spec: str):
    spec = spec.strip()
    if "{" in spec:
        prefix, inner = spec.split("{", 1)
        prefix = prefix.strip().rstrip(":").strip()
        inner = inner.split("}", 1)[0]
        names = []
        for part in inner.split(","):
            part = part.strip()
            if not part:
                continue
            if " as " in part:
                original, alias = [piece.strip() for piece in part.split(" as ", 1)]
                names.append(alias)
            else:
                names.append(part)
        return prefix, names
    tail = spec.split("::")[-1].strip()
    return None, [tail]


adapter_symbols: dict[str, set[str]] = {}
wire_symbols: dict[str, set[str]] = {}
for crate_name in FEATURE_CRATES:
    crate_src = FEATURES_DIR / crate_name / "src"
    adapters_symbol_set: set[str] = set()
    adapters_module_files: list[Path] = []
    adapters_file = crate_src / "adapters.rs"
    if adapters_file.is_file():
        adapters_module_files.append(adapters_file)
    adapters_dir = crate_src / "adapters"
    if adapters_dir.is_dir():
        adapters_module_files.extend(sorted(adapters_dir.rglob("*.rs")))
    for module_file in adapters_module_files:
        source = strip_test_items(module_file.read_text(encoding="utf-8", errors="replace"))
        for match in PUB_STRUCT_RE.finditer(source):
            adapters_symbol_set.add(match.group(1))
        for match in ADAPTERS_USE_RE.finditer(source):
            _, names = split_use_list(match.group(1))
            adapters_symbol_set.update(names)
    adapter_symbols[crate_name] = adapters_symbol_set

    wire_set: set[str] = set()
    for rust_file in crate_src.rglob("*.rs"):
        relative_parts = rust_file.relative_to(crate_src).parts
        if relative_parts[-1].endswith("_tests.rs") or relative_parts[-1] == "tests.rs":
            continue
        if "tests" in relative_parts[:-1] or "scenario_tests" in relative_parts:
            continue
        source = strip_test_items(rust_file.read_text(encoding="utf-8", errors="replace"))
        for match in PUB_WIRE_RE.finditer(source):
            wire_set.add(match.group(1))
    wire_symbols[crate_name] = wire_set

# ── 消费方扫描 ────────────────────────────────────────────────────────────
SCAN_ROOTS = [root / "agent/features", root / "agent/composition", root / "apps", root / "packages"]
symbol_to_owner: dict[str, set[str]] = {}
for crate_name, symbols in adapter_symbols.items():
    for symbol in symbols:
        symbol_to_owner.setdefault(symbol, set()).add(crate_name)


def crate_of(relative_parts) -> str | None:
    if relative_parts[:2] == ("agent", "features") and len(relative_parts) > 2:
        return relative_parts[2]
    if relative_parts[:2] == ("agent", "composition"):
        return "composition"
    return None


for scan_root in SCAN_ROOTS:
    if not scan_root.exists():
        continue
    for rust_file in sorted(scan_root.rglob("*.rs")):
        relative = rust_file.relative_to(root)
        parts = relative.parts
        if parts[-1].endswith("_tests.rs") or parts[-1] == "tests.rs":
            continue
        if "tests" in parts[:-1] or "scenario_tests" in parts or "test_support" in parts[-1]:
            continue
        owning_crate = crate_of(parts)
        source = strip_test_items(rust_file.read_text(encoding="utf-8", errors="replace"))
        relative_text = str(relative)

        # use 导入表：alias -> 来源 crate 首段
        imported_aliases: dict[str, str] = {}
        for match in PLAIN_USE_RE.finditer(source):
            prefix, names = split_use_list(match.group(1))
            path = prefix if prefix is not None else match.group(1).strip()
            first_segment = path.split("::", 1)[0]
            if first_segment in ("crate", "super", "self", "std", "core", "alloc"):
                continue
            for alias in names:
                imported_aliases[alias] = first_segment

        for match in CONSTRUCTOR_RE.finditer(source):
            type_name = match.group(1)
            owners = symbol_to_owner.get(type_name)
            if not owners:
                continue
            import_origin = imported_aliases.get(type_name)
            cross_owners = {owner for owner in owners if owner != owning_crate}
            qualifies = bool(cross_owners) or (import_origin is not None and import_origin != owning_crate)
            if not qualifies:
                continue
            entry = registered.get(type_name)
            if entry is None:
                violations.append(
                    f"{relative_text}: unregistered cross-BC adapter construction '{type_name}' "
                    f"(owner crate(s): {', '.join(sorted(cross_owners or owners))}); "
                    f"register it in architecture-guard-registry.json construction_symbols or construct it inside its owning crate"
                )
                continue
            if not any(relative_text.startswith(allowed) for allowed in entry["allowed_paths"]):
                violations.append(
                    f"{relative_text}: construction of '{type_name}' outside allowed paths "
                    f"{entry['allowed_paths']} (entry {entry['id']})"
                )

        for match in WIRE_CALL_RE.finditer(source):
            path_segments = [segment.strip() for segment in match.group(1).split("::") if segment.strip()]
            wire_name = match.group(2)
            if not path_segments or path_segments[0] in ("crate", "self", "super", "std", "core"):
                continue
            wire_owner = path_segments[0] if path_segments[0] in wire_symbols else imported_aliases.get(wire_name)
            if wire_owner is None or wire_owner == owning_crate:
                continue
            if wire_name not in wire_symbols.get(wire_owner, set()):
                continue
            entry = registered.get(wire_name)
            if entry is None:
                violations.append(
                    f"{relative_text}: unregistered cross-BC wire call '{path_segments[0]}…::{wire_name}' "
                    f"(owner crate: {wire_owner}); register it in architecture-guard-registry.json construction_symbols"
                )
                continue
            if not any(relative_text.startswith(allowed) for allowed in entry["allowed_paths"]):
                violations.append(
                    f"{relative_text}: wire call '{path_segments[0]}…::{wire_name}' outside allowed paths "
                    f"{entry['allowed_paths']} (entry {entry['id']})"
                )

if violations:
    print(json.dumps({
        "decision": "block",
        "reason": "cross-BC construction registry guard FAILED:\n" + "\n".join(violations),
    }, ensure_ascii=False))
    sys.exit(2)
print("Cross-BC construction registry guard OK.")
PY
