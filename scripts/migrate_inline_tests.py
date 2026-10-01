#!/usr/bin/env python3
"""内联 `#[cfg(test)] mod tests { ... }` 外置迁移器（#1146 W5）。

按 specs/3.2.5.3 标准形态迁移：foo.rs 的内联 tests 块整体平移到同级
foo_tests.rs，原文件留三行引入（#[cfg(test)] / #[path] / mod tests;）。

用法：
  python3 scripts/migrate_inline_tests.py --dry-run          # 全量报告，不改文件
  python3 scripts/migrate_inline_tests.py --crate apps/cli/src  # 只迁指定前缀
  python3 scripts/migrate_inline_tests.py --file <path>        # 只迁单文件
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REGISTRY = Path(".agents/architecture-guard-registry.json")
RULE_ID = "pattern.all.no-inline-test-modules"


def load_targets() -> list[str]:
    with REGISTRY.open() as handle:
        registry = json.load(handle)
    for rule in registry["rules"]:
        if rule["id"] == RULE_ID:
            return [e["path"] for e in rule.get("exclusions", [])]
    raise SystemExit(f"registry 中找不到规则 {RULE_ID}")


def find_test_block(lines: list[str]) -> tuple[int, int] | None:
    """定位 `#[cfg(test)] ... mod tests {` 块的（属性起始行, 闭合行）索引。"""
    for index, line in enumerate(lines):
        if "mod tests {" not in line:
            continue
        stripped = line.strip()
        if not (stripped.startswith("mod tests {") or stripped.endswith("mod tests {")):
            continue
        # 向上吸附 cfg(test) 属性链（跨空行不超过 1 行）
        attr_start = index
        cursor = index - 1
        while cursor >= 0:
            up = lines[cursor].strip()
            if up.startswith("#[") or up.startswith("///") or up.startswith("//!"):
                attr_start = cursor
                cursor -= 1
                continue
            if up == "" and attr_start != index:
                cursor -= 1
                continue
            break
        # 确认链内含 cfg(test)
        attr_block = "\n".join(lines[attr_start:index])
        if "#[cfg(test)]" not in attr_block:
            continue
        close = find_block_end(lines, index)
        if close is not None:
            return attr_start, close
    return None


def find_block_end(lines: list[str], open_line: int) -> int | None:
    """从 `mod tests {` 行起做 brace matching（跳过字符串/字符/注释）。"""
    depth = 0
    in_line_comment = False
    in_block_comment = 0
    in_string = False
    in_char = False
    in_raw: int | None = None  # raw string 哈希数
    escape = False
    for line_no in range(open_line, len(lines)):
        line = lines[line_no]
        column = 0
        while column < len(line):
            char = line[column]
            nxt = line[column + 1] if column + 1 < len(line) else ""
            if in_line_comment:
                break
            if in_block_comment:
                if char == "*" and nxt == "/":
                    in_block_comment -= 1
                    column += 2
                    continue
                column += 1
                continue
            if in_string:
                if escape:
                    escape = False
                elif char == "\\":
                    escape = True
                elif char == '"':
                    in_string = False
                column += 1
                continue
            if in_char:
                if escape:
                    escape = False
                elif char == "\\":
                    escape = True
                elif char == "'":
                    in_char = False
                column += 1
                continue
            if in_raw is not None:
                if char == '"' and line[column : column + 1 + in_raw].endswith("#" * in_raw):
                    if line[column + 1 : column + 1 + in_raw] == "#" * in_raw:
                        in_raw = None
                        column += 1 + in_raw if in_raw else 2
                        continue
                column += 1
                continue
            if char == "/" and nxt == "/":
                in_line_comment = True
                break
            if char == "/" and nxt == "*":
                in_block_comment += 1
                column += 2
                continue
            if char == '"':
                raw_match = re.match(r'"(#+)', line[column:])
                if line[column - 2 : column] == "r\"" or (column >= 1 and line[column - 1] == "r" and raw_match is None):
                    pass
                raw_prefix = re.search(r"r(#{0,8})\"$", line[: column + 1])
                if raw_prefix:
                    in_raw = len(raw_prefix.group(1))
                else:
                    in_string = True
                column += 1
                continue
            if char == "'":
                # 生命周期 vs 字符字面量：'a' 是字面量，'a 不是
                if column + 2 < len(line) and line[column + 2] == "'":
                    in_char = True
                elif column + 1 < len(line) and line[column + 1] == "\\":
                    in_char = True
                column += 1
                continue
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    return line_no
            column += 1
        in_line_comment = False
    return None


def migrate(path: Path, dry_run: bool) -> str:
    source = path.read_text()
    lines = source.split("\n")
    block = find_test_block(lines)
    if block is None:
        return "STALE"
    attr_start, close = block
    target = path.with_name(path.stem + "_tests.rs")
    if target.exists():
        return "CONFLICT"
    body = lines[attr_start + 1 : close] if False else extract_body(lines, attr_start, close)
    if body is None:
        return "SKIP-NESTED-ATTR"
    intro = "#[cfg(test)]\n#[path = \"{name}\"]\nmod tests;".format(name=target.name)
    new_lines = lines[:attr_start] + intro.split("\n") + lines[close + 1 :]
    if not dry_run:
        target.write_text("\n".join(body) + "\n")
        path.write_text("\n".join(new_lines) + "\n")
    return "MIGRATED"


def extract_body(lines: list[str], attr_start: int, close: int) -> list[str] | None:
    """提取 mod tests { ... } 花括号内的行（含 mod 行到闭合行之间）。"""
    mod_line = None
    for index in range(attr_start, close + 1):
        if "mod tests {" in lines[index]:
            mod_line = index
            break
    if mod_line is None:
        return None
    tail = lines[mod_line].split("mod tests {", 1)[1].strip()
    body = []
    if tail:
        body.append(tail)
    body.extend(lines[mod_line + 1 : close])
    head = lines[close].rsplit("}", 1)[0].rstrip()
    if head:
        body.append(head)
    return body


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--crate", help="只处理指定路径前缀")
    parser.add_argument("--file", help="只处理单个文件")
    args = parser.parse_args()

    targets = load_targets()
    if args.file:
        targets = [t for t in targets if t == args.file]
    elif args.crate:
        targets = [t for t in targets if t.startswith(args.crate)]

    counts: dict[str, int] = {}
    for rel in targets:
        path = Path(rel)
        if not path.exists():
            print(f"MISSING  {rel}")
            counts["MISSING"] = counts.get("MISSING", 0) + 1
            continue
        result = migrate(path, args.dry_run)
        counts[result] = counts.get(result, 0) + 1
        if result != "MIGRATED":
            print(f"{result:8s} {rel}")
    print("---")
    for key in sorted(counts):
        print(f"{key}: {counts[key]}")


if __name__ == "__main__":
    sys.exit(main())
