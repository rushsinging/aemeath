#!/usr/bin/env python3
"""生产常量审计（#1146 开发前审计工具）。

枚举 agent/ apps/ packages/ 的生产 const/static（排除测试专属），
按「位置轨道」分类并输出 Markdown 报告：
- 违规 A：行为文件内常量（应归 owning 层 constants.rs）
- 违规 B：lib.rs 常量定义（只允许 re-export，零定义）
- 违规 C：跨 crate 镜像候选（同值同名/相似，跨 crate 出现）
- 合规：已位于 *constants.rs / *consts.rs

用法: scripts/audit_constants.py [--output report.md]
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

CONST_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static(?:\s+mut)?)\s+([A-Z][A-Z0-9_]*)",
    re.MULTILINE,
)
TEST_HINTS = ("_tests.rs", "/tests/", "tests.rs", "_test.rs", "scenario_tests")


@dataclass
class Entry:
    symbol: str
    value_hint: str
    path: str
    line: int
    kind: str  # const / static

    @property
    def crate(self) -> str:
        parts = self.path.split("/")
        if "features" in parts:
            return parts[parts.index("features") + 1]
        return "/".join(parts[:2])


def iter_source_files(root: Path):
    for base in ("agent", "apps", "packages"):
        for path in (root / base).rglob("*.rs"):
            rel = path.relative_to(root).as_posix()
            if any(hint in rel for hint in TEST_HINTS):
                continue
            yield rel, path


def audit(root: Path) -> tuple[list[Entry], list[Entry], list[Entry], list[Entry]]:
    """返回 (行为文件违规, lib.rs 违规, constants 合规, 全量)。"""
    behavioral, lib_defs, compliant, all_entries = [], [], [], []
    for rel, path in iter_source_files(root):
        source = path.read_text(encoding="utf-8", errors="replace")
        # 剥离内嵌 #[cfg(test)] mod（粗略：行前缀标记）
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
            match = re.match(
                r"\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static(?:\s+mut)?)\s+([A-Z][A-Z0-9_]*)\s*(?::[^=]+)?=\s*(.{0,60})",
                line,
            )
            if not match:
                continue
            symbol, value_hint = match.group(1), match.group(2).strip()
            kind = "static" if "static" in match.group(0) else "const"
            entry = Entry(symbol, value_hint, rel, lineno, kind)
            all_entries.append(entry)
            leaf = rel.rsplit("/", 1)[-1]
            if leaf == "lib.rs":
                lib_defs.append(entry)
            elif "constants.rs" in leaf or "consts.rs" in leaf:
                compliant.append(entry)
            else:
                behavioral.append(entry)
    return behavioral, lib_defs, compliant, all_entries


def mirror_candidates(all_entries: list[Entry]) -> list[tuple[str, list[str]]]:
    """跨 crate 同名定义（镜像候选；同值不等于同语义，仅报告不裁定）。"""
    by_symbol = defaultdict(list)
    for entry in all_entries:
        by_symbol[entry.symbol].append(entry)
    candidates = []
    for symbol, entries in sorted(by_symbol.items()):
        crates = {e.crate for e in entries}
        if len(crates) > 1:
            candidates.append((symbol, [f"{e.crate}:{e.path}:{e.line}" for e in entries]))
    return candidates


def render_report(root: Path) -> str:
    behavioral, lib_defs, compliant, all_entries = audit(root)
    lines = [
        "# 生产常量审计报告（#1146 开发前基线）",
        "",
        f"- 全量生产 const/static：**{len(all_entries)}**",
        f"- 合规（位于 constants.rs）：{len(compliant)}",
        f"- 违规 A 行为文件内常量（应归 owning 层 constants.rs）：**{len(behavioral)}**",
        f"- 违规 B lib.rs 常量定义（零定义例外）：**{len(lib_defs)}**",
        "",
        "## 违规 B：lib.rs 常量定义",
        "",
    ]
    for entry in sorted(lib_defs, key=lambda e: e.path):
        lines.append(f"- `{entry.path}:{entry.line}` `{entry.symbol}` = {entry.value_hint}")
    lines += ["", "## 违规 C：跨 crate 同名镜像候选（语义 owner 待裁定）", ""]
    for symbol, locations in mirror_candidates(all_entries):
        lines.append(f"- `{symbol}`：{len(locations)} crate 同名")
        for loc in locations:
            lines.append(f"  - {loc}")
    lines += ["", "## 违规 A：行为文件内常量（按 crate 分组）", ""]
    by_crate = defaultdict(list)
    for entry in behavioral:
        by_crate[entry.crate].append(entry)
    for crate in sorted(by_crate):
        entries = by_crate[crate]
        lines.append(f"### {crate}（{len(entries)}）")
        lines.append("")
        for entry in sorted(entries, key=lambda e: e.path):
            lines.append(
                f"- `{entry.path}:{entry.line}` `{entry.symbol}` = {entry.value_hint}"
            )
        lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", default=None)
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parent.parent
    report = render_report(root)
    if args.output:
        Path(args.output).write_text(report, encoding="utf-8")
        print(f"report written: {args.output}", file=sys.stderr)
    else:
        print(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
