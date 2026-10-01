#!/usr/bin/env python3
"""生产常量审计（#1146 双轨归位审计工具）。

枚举 agent/ apps/ packages/ 的生产 const/static，按「位置轨道」分类：
- 违规 A：行为文件内业务常量（应归 owning 层 constants.rs）
- 违规 B：lib.rs 常量定义（只允许 re-export，零定义）
- 违规 C：跨 crate 同名镜像候选（排除日志规范形态 LOG_TARGET 与 newtype 零值 ZERO）
- 全局状态：可变单例（OnceLock/Mutex/Atomic/RefCell const-block）——进程或线程状态，非常量债务
- 合规：已位于 *constants.rs / *consts.rs

排除项：测试基建文件（*_tests.rs / tests/ / test_log / test_support / test_harness）。

用法:
  scripts/audit_constants.py                 # 报告到 stdout
  scripts/audit_constants.py --output r.md   # 报告落盘
  scripts/audit_constants.py --matrix m.md   # 审计矩阵（含人工裁定列）落盘
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

CONST_LINE_PATTERN = re.compile(
    r"^(\s*)(pub(?:\([a-z]+\))?\s+)?(?:const|static(?:\s+mut)?)\s+([A-Z][A-Z0-9_]*)\s*(?::([^=]+?))?\s*=\s*(.{0,60})",
)
# 同步原语类型：进程/线程状态单例（类型级判定，覆盖跨行值形态）
SYNC_TYPE_PATTERN = re.compile(
    r"\b(OnceLock|LazyLock|Mutex|RwLock|AtomicBool|AtomicUsize|AtomicU64|AtomicI64|AtomicU32|RefCell|Cell)\b"
)
TEST_HINTS = (
    "_tests.rs",
    "/tests/",
    "tests.rs",
    "_test.rs",
    "scenario_tests",
    "test_log",
    "test_support",
    "test_harness",
    "integration_tests/",
)
# 可变全局单例：进程/线程状态而非常量债务（值形态判定）
MUTABLE_SINGLETON_PATTERNS = (
    "OnceLock::new()",
    "LazyLock::new(",
    "Mutex::new(",
    "RwLock::new(",
    "AtomicBool::new(",
    "AtomicU64::new(",
    "AtomicUsize::new(",
    "AtomicI64::new(",
    "const { RefCell",
    "const { std::cell",
    "const { std::sync",
    "const { Mutex",
    "const { OnceLock",
)
# newtype 零值构造：紧贴类型定义的私有零值（如 ByteIdx(0)）
NEWTYPE_ZERO_PATTERN = re.compile(r"^[A-Z][A-Za-z0-9_]*\(0\)(\s*;.*)?$")
# 合理跨 crate 同名：各 crate 私有、语义各异的规范形态
MIRROR_EXEMPT_SYMBOLS = {
    "LOG_TARGET": "每 crate 私有日志 target（specs 3.15 日志规范形态）",
    "ZERO": "newtype 零值构造（类型旁私有常量）",
}


@dataclass
class Entry:
    symbol: str
    value_hint: str
    path: str
    line: int
    kind: str  # const / static
    visibility: str  # pub / pub(crate) / pub(super) / private
    type_hint: str = ""  # 静态项的类型段（同步原语判定用）

    @property
    def crate(self) -> str:
        parts = self.path.split("/")
        if "features" in parts:
            return parts[parts.index("features") + 1]
        return "/".join(parts[:2])

    @property
    def is_mutable_singleton(self) -> bool:
        return (
            any(pattern in self.value_hint for pattern in MUTABLE_SINGLETON_PATTERNS)
            or bool(SYNC_TYPE_PATTERN.search(self.type_hint))
        )

    @property
    def is_newtype_zero(self) -> bool:
        return self.symbol == "ZERO" and bool(NEWTYPE_ZERO_PATTERN.match(self.value_hint))

    @property
    def is_associated_constant(self) -> bool:
        """impl 块内关联常量（Self 构造）：类型旁强内聚，合理形态。"""
        return self.value_hint.startswith("Self {") or self.value_hint.startswith("Self(")


# 整文件经父模块 #[cfg(test)] + #[path] 外部门控的测试域文件
# （声明点见各 crate 对应 mod 声明；guard placement 规则同口径豁免）
EXTERNALLY_CFG_TEST_GATED = {
    "packages/global/logging/src/domain/routing_guard.rs",
}


def iter_source_files(root: Path):
    for base in ("agent", "apps", "packages"):
        for path in (root / base).rglob("*.rs"):
            rel = path.relative_to(root).as_posix()
            if any(hint in rel for hint in TEST_HINTS):
                continue
            if rel in EXTERNALLY_CFG_TEST_GATED:
                continue
            yield rel, path


def extract_visibility(line: str) -> str:
    if re.match(r"^\s*pub\s+", line):
        return "pub"
    if re.match(r"^\s*pub\(crate\)\s+", line):
        return "pub(crate)"
    if re.match(r"^\s*pub\(super\)\s+", line):
        return "pub(super)"
    return "private"


def audit(root: Path):
    """返回 (业务违规A, lib违规B, 合规, 全局状态, 全量)。"""
    behavioral, lib_defs, compliant, mutable, all_entries = [], [], [], [], []
    for rel, path in iter_source_files(root):
        source = path.read_text(encoding="utf-8", errors="replace")
        # 剥离内嵌 #[cfg(test)] mod
        in_test_mod = False
        depth = 0
        for lineno, line in enumerate(source.splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("#[cfg(test)]"):
                in_test_mod = "pending"
                continue
            if in_test_mod == "pending":
                if re.match(r"(?:pub(\([a-z]+\))?\s+)?mod\s+\w+", stripped):
                    in_test_mod = True
                elif stripped and not stripped.startswith("#"):
                    in_test_mod = False
            if in_test_mod is True:
                depth += line.count("{") - line.count("}")
                if depth <= 0:
                    in_test_mod = False
                continue
            match = CONST_LINE_PATTERN.match(line)
            if not match:
                continue
            indent, pub_prefix, symbol, type_hint, value_hint = (
                match.group(1), match.group(2) or "", match.group(3),
                (match.group(4) or "").strip(), match.group(5).strip(),
            )
            # 函数体内局部 const（缩进 ≥8）不属于全局常量审计面
            if len(indent) >= 8:
                continue
            kind = "static" if "static" in line else "const"
            visibility = "pub" if pub_prefix.strip().startswith("pub ") else (
                "pub(crate)" if "pub(crate)" in pub_prefix else (
                    "pub(super)" if "pub(super)" in pub_prefix else "private"
                )
            )
            entry = Entry(symbol, value_hint, rel, lineno, kind, visibility, type_hint)
            all_entries.append(entry)
            leaf = rel.rsplit("/", 1)[-1]
            if leaf == "lib.rs":
                lib_defs.append(entry)
            elif entry.is_mutable_singleton or entry.is_newtype_zero or entry.is_associated_constant:
                mutable.append(entry)
            elif "constants.rs" in leaf or "consts.rs" in leaf:
                compliant.append(entry)
            else:
                behavioral.append(entry)
    return behavioral, lib_defs, compliant, mutable, all_entries


def mirror_candidates(all_entries: list[Entry]):
    """跨 crate 同名（排除规范形态与可变单例；同值不等于同语义，仅报告不裁定）。

    可变单例（OnceLock/RefCell 等）同名属命名规范问题而非常量镜像债务，
    不参与镜像判定。"""
    by_symbol = defaultdict(list)
    for entry in all_entries:
        if entry.is_mutable_singleton:
            continue
        by_symbol[entry.symbol].append(entry)
    candidates, exempt = [], []
    for symbol, entries in sorted(by_symbol.items()):
        crates = {e.crate for e in entries}
        if len(crates) > 1:
            if symbol in MIRROR_EXEMPT_SYMBOLS:
                exempt.append((symbol, MIRROR_EXEMPT_SYMBOLS[symbol]))
            else:
                candidates.append((symbol, [f"{e.crate}:{e.path}:{e.line}" for e in entries]))
    return candidates, exempt


def render_report(root: Path) -> str:
    behavioral, lib_defs, compliant, mutable, all_entries = audit(root)
    candidates, exempt = mirror_candidates(all_entries)
    lines = [
        "# 生产常量审计报告（#1146 收口）",
        "",
        f"- 全量生产 const/static：**{len(all_entries)}**",
        f"- 合规（位于 constants.rs）：{len(compliant)}",
        f"- 全局状态单例（OnceLock/Mutex/Atomic/RefCell，非常量债务）：{len(mutable)}",
        f"- 违规 A 行为文件内业务常量（应归 owning 层 constants.rs）：**{len(behavioral)}**",
        f"- 违规 B lib.rs 常量定义（零定义例外）：**{len(lib_defs)}**",
        f"- 违规 C 跨 crate 镜像候选（待 owner 裁定）：**{len(candidates)}**",
        "",
        "## 违规 B：lib.rs 常量定义",
        "",
    ]
    for entry in sorted(lib_defs, key=lambda e: e.path):
        lines.append(f"- `{entry.path}:{entry.line}` `{entry.symbol}` = {entry.value_hint}")
    lines += ["", "## 违规 C：跨 crate 同名镜像候选（语义 owner 待裁定）", ""]
    for symbol, locations in candidates:
        lines.append(f"- `{symbol}`：{len({loc.split(':')[0] for loc in locations})} crate 同名")
        for loc in locations:
            lines.append(f"  - {loc}")
    if exempt:
        lines += ["", "### 规范形态豁免（不计镜像）", ""]
        for symbol, reason in exempt:
            lines.append(f"- `{symbol}`：{reason}")
    lines += [
        "",
        "## 违规 A：行为文件内业务常量（按 crate 分组）",
        "",
    ]
    by_crate = defaultdict(list)
    for entry in behavioral:
        by_crate[entry.crate].append(entry)
    for crate in sorted(by_crate):
        entries = by_crate[crate]
        lines.append(f"### {crate}（{len(entries)}）")
        lines.append("")
        for entry in sorted(entries, key=lambda e: e.path):
            cross = "跨crate镜像" if any(
                entry.symbol == symbol for symbol, _ in candidates
            ) else ""
            lines.append(
                f"- `{entry.path}:{entry.line}` `{entry.symbol}` = {entry.value_hint}"
                f"（{entry.visibility}）{('【' + cross + '】') if cross else ''}"
            )
        lines.append("")
    return "\n".join(lines)


def render_matrix(root: Path) -> str:
    """审计矩阵：机器可判字段自动填 + 人工裁定列留空（#1146 治理存档）。"""
    behavioral, lib_defs, compliant, mutable, all_entries = audit(root)
    candidates, _ = mirror_candidates(all_entries)
    mirror_symbols = {symbol for symbol, _ in candidates}
    lines = [
        "# #1146 常量审计矩阵（收口基线）",
        "",
        "机器可判字段由 audit_constants.py 生成；owner / 消费者 / 目标轨道 / 处置为人工裁定列。",
        "",
        "| symbol | value | path | visibility | crate | 跨crate | owner BC | 消费者 | 目标轨道 | 处置 |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for entry in sorted(all_entries, key=lambda e: (e.crate, e.path, e.line)):
        value = entry.value_hint.replace("|", "\\|")[:40]
        lines.append(
            f"| `{entry.symbol}` | {value} | `{entry.path}:{entry.line}` | {entry.visibility}"
            f"| {entry.crate} | {'是' if entry.symbol in mirror_symbols else '否'} |  |  |  |  |"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", default=None, help="报告落盘路径")
    parser.add_argument("--matrix", default=None, help="审计矩阵落盘路径")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    exit_code = 0
    if args.output:
        Path(args.output).write_text(render_report(root), encoding="utf-8")
        print(f"report written: {args.output}", file=sys.stderr)
    if args.matrix:
        Path(args.matrix).write_text(render_matrix(root), encoding="utf-8")
        print(f"matrix written: {args.matrix}", file=sys.stderr)
    if not args.output and not args.matrix:
        print(render_report(root))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
