#!/usr/bin/env python3
"""placement 存量迁移器 v2（#1146 C8）。

v1 教训吸收：
- dry-run 真实现（默认只打印，--apply 才写盘）
- 三类排除：宏依赖块（宏定义在同文件的表）、guard 真相源文件（位置即契约）、
  跨模块类型 static（static 类型引用同文件私有类型）
- 分批执行：--filter=路径子串，每批独立 build 验证

用法:
    scripts/migrate_placement.py --filter=features/memory [--apply]
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STATE_MARKERS = ("OnceLock", "AtomicUsize", "AtomicBool", "AtomicU64", "AtomicI64",
                 "Mutex<", "RwLock<", "RefCell<", "LazyLock", "const {")

# guard 真相源文件：其常量位置本身就是被 routing_guard 校验的契约，迁移破坏守卫。
GUARD_TRUTH_SOURCES = {
    "packages/global/logging/src/domain/routing.rs",
    "packages/global/logging/src/domain/routing_guard.rs",
}

CONST_LINE = re.compile(
    r'^(?:pub(?:\((?:crate|super)\))?\s+)?(?:const|static(?:\s+mut)?)\s+([A-Z][A-Z0-9_]*)'
)


def violations(filter_str: str) -> list[tuple[str, int, str]]:
    result = subprocess.run(
        ["cargo", "run", "--quiet", "-p", "xtask", "--", "guard",
         "--rule", "pattern.all.constant-placement"],
        cwd=ROOT, capture_output=True, text=True,
    )
    out = []
    for line in result.stderr.splitlines():
        m = re.match(r"\[guard\] \S+ (\S+):(\d+): 常量 `(\w+)`", line)
        if m and filter_str in m.group(1):
            out.append((m.group(1), int(m.group(2)), m.group(3)))
    return out


def block_end(lines: list[str], start: int) -> int:
    first = lines[start - 1]
    if 'r#"' in first:
        for i in range(start, len(lines) + 1):
            if '"#' in lines[i - 1]:
                j = i
                while j <= len(lines) and not lines[j - 1].rstrip().endswith(";"):
                    j += 1
                return min(j, len(lines))
        return start
    depth = 0
    for i in range(start, len(lines) + 1):
        line = lines[i - 1]
        depth += line.count("{") + line.count("(") + line.count("[") \
            - line.count("}") - line.count(")") - line.count("]")
        if ";" in line and depth <= 0:
            return i
        if depth < 0:
            return i
    return start


def macro_dependent(source: str, block: str) -> bool:
    """块内调用同文件定义的 macro_rules! 宏 → 宏表耦合，跳过。"""
    local_macros = set(re.findall(r"macro_rules!\s+(\w+)", source))
    pattern_prefix = r"\b"
    return any(re.search(pattern_prefix + m + r"!\s*[({\[]", block) for m in local_macros)


def type_dependent_static(source: str, block: str, symbol: str) -> bool:
    """static 的类型引用同文件私有类型（struct/enum 定义在本文件）→ 跳过。"""
    m = re.search(rf"(?:const|static)\s+{symbol}\s*:\s*([^=]+?)\s*=", block)
    if not m:
        return False
    type_text = m.group(1)
    for type_name in re.findall(r"[A-Z][A-Za-z0-9]*", type_text):
        if type_name in ("OnceLock", "AtomicUsize", "AtomicBool", "AtomicU64",
                         "AtomicI64", "Mutex", "RwLock", "RefCell", "LazyLock", "u64", "u32",
                         "String", "usize", "bool", "Vec", "Option", "Arc", "Duration"):
            continue
        if re.search(rf"\b(?:struct|enum)\s+{type_name}\b", source):
            return True
    return False


def collect_attrs(lines: list[str], start: int) -> tuple[list[str], int]:
    attrs = []
    i = start - 1
    while i > 0:
        prev = lines[i - 1].strip()
        if prev.startswith(("///", "#[")) or (prev == "" and attrs and attrs[0].strip().startswith(("///", "#["))):
            attrs.insert(0, lines[i - 1])
            i -= 1
        else:
            break
    while attrs and attrs[0].strip() == "":
        attrs.pop(0)
    return attrs, i + 1


def migrate(filter_str: str, apply: bool) -> None:
    todo = violations(filter_str)
    skipped_guard = [t for t in todo if t[0] in GUARD_TRUTH_SOURCES]
    todo = [t for t in todo if t[0] not in GUARD_TRUTH_SOURCES]
    print(f"违规 {len(todo)} 项（guard 真相源跳过 {len(skipped_guard)}）")

    by_file: dict[str, list[tuple[int, str]]] = {}
    for path, line, symbol in todo:
        by_file.setdefault(path, []).append((line, symbol))

    for path, entries in sorted(by_file.items()):
        src = ROOT / path
        source = src.read_text()
        lines = source.splitlines(keepends=True)
        moves, skipped = [], []
        for line, symbol in sorted(entries, reverse=True):
            attrs, real_start = collect_attrs(lines, line)
            end = block_end(lines, line)
            block = "".join(lines[real_start - 1:end])
            if macro_dependent("".join(lines), block):
                skipped.append((symbol, "宏依赖"))
                continue
            if type_dependent_static("".join(lines), block, symbol):
                skipped.append((symbol, "同文件类型依赖"))
                continue
            dest = "state.rs" if any(m in block for m in STATE_MARKERS) else "constants.rs"
            fixed = []
            for bline in block.splitlines(keepends=True):
                if CONST_LINE.match(bline) and not bline.lstrip().startswith("pub"):
                    bline = re.sub(r"^(const|static)", r"pub(crate) \1", bline)
                fixed.append(bline)
            moves.append((dest, "".join(attrs) + "".join(fixed), symbol))
            del lines[real_start - 1:end]
        if not moves:
            if skipped:
                print(f"{path}: 全部跳过 {skipped}")
            continue
        if apply:
            src.write_text("".join(lines))
            for dest, block, _ in moves:
                target = src.parent / dest
                if not target.exists():
                    target.write_text(
                        f"//! {'状态容器' if dest == 'state.rs' else '纯值常量'}（#1146 placement 归位）。\n\n")
                with open(target, "a") as handle:
                    handle.write(block + "\n")
            text = src.read_text()
            symbols_const = [s for d, _, s in moves if d == "constants.rs"]
            symbols_state = [s for d, _, s in moves if d == "state.rs"]
            inject = ""
            if symbols_const:
                inject += f"use super::constants::{{{', '.join(sorted(symbols_const))}}};\n"
            if symbols_state:
                inject += f"use super::state::{{{', '.join(sorted(symbols_state))}}};\n"
            if inject:
                src_lines = text.splitlines(keepends=True)
                idx = next((i for i, l in enumerate(src_lines) if l.startswith("use ")), 0)
                src_lines.insert(idx, inject)
                src.write_text("".join(src_lines))
        print(f"{path}: 迁移 {len(moves)}，跳过 {skipped if skipped else '0'}")


if __name__ == "__main__":
    args = sys.argv[1:]
    migrate(
        next((a.split("=")[1] for a in args if a.startswith("--filter=")), ""),
        "--apply" in args,
    )
