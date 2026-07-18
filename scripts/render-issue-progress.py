#!/usr/bin/env python3
"""递归渲染 GitHub Issue 的 sub-issue 层级与 blocked-by 进度。"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Any

DEFAULT_WORKERS = 10


@dataclass(frozen=True)
class Issue:
    number: int
    title: str
    state: str
    parent: int | None
    children: list[int]
    blocked_by: list[int]


def gh_api(path: str, *, paginate: bool = False, retries: int = 5) -> Any:
    command = ["gh", "api"]
    if paginate:
        command.append("--paginate")
    command.append(path)
    for attempt in range(retries):
        result = subprocess.run(command, capture_output=True, text=True, check=False)
        if result.returncode == 0:
            if not paginate:
                return json.loads(result.stdout)
            return decode_paginated_json(result.stdout)
        if attempt + 1 < retries:
            time.sleep(1 + attempt * 2)
    raise RuntimeError(result.stderr.strip() or f"gh api 调用失败：{path}")


def decode_paginated_json(raw: str) -> list[dict[str, Any]]:
    values: list[dict[str, Any]] = []
    decoder = json.JSONDecoder()
    index = 0
    raw = raw.strip()
    while index < len(raw):
        value, index = decoder.raw_decode(raw, index)
        values.extend(value if isinstance(value, list) else [value])
    return values


def collect_issues(
    repo: str, root: int, *, workers: int = DEFAULT_WORKERS
) -> dict[int, Issue]:
    issues: dict[int, Issue] = {}
    parents: dict[int, int | None] = {root: None}
    pending = [root]

    def fetch_issue(number: int) -> tuple[dict[str, Any], list[dict[str, Any]]]:
        data = gh_api(f"repos/{repo}/issues/{number}")
        children = gh_api(
            f"repos/{repo}/issues/{number}/sub_issues", paginate=True
        )
        return data, children

    with ThreadPoolExecutor(max_workers=workers) as executor:
        while pending:
            batch = pending
            pending = []
            results = executor.map(fetch_issue, batch)
            for number, (data, children_data) in zip(batch, results):
                children = [item["number"] for item in children_data]
                issues[number] = Issue(
                    number=number,
                    title=data["title"],
                    state=data["state"].lower(),
                    parent=parents[number],
                    children=children,
                    blocked_by=[],
                )
                for child in children:
                    if child not in parents:
                        parents[child] = number
                        pending.append(child)

        hierarchy_numbers = list(issues)

        def fetch_dependencies(number: int) -> list[dict[str, Any]]:
            return gh_api(
                f"repos/{repo}/issues/{number}/dependencies/blocked_by",
                paginate=True,
            )

        dependency_results = executor.map(fetch_dependencies, hierarchy_numbers)
        for number, dependencies in zip(hierarchy_numbers, dependency_results):
            issue = issues[number]
            for dependency in dependencies:
                dependency_number = dependency["number"]
                if dependency_number not in issues:
                    issues[dependency_number] = Issue(
                        number=dependency_number,
                        title=dependency["title"],
                        state=dependency["state"].lower(),
                        parent=None,
                        children=[],
                        blocked_by=[],
                    )
            issues[number] = Issue(
                number=issue.number,
                title=issue.title,
                state=issue.state,
                parent=issue.parent,
                children=issue.children,
                blocked_by=[dependency["number"] for dependency in dependencies],
            )
    return issues


def status_icon(state: str) -> str:
    return "✅" if state.lower() == "closed" else "⬜"


def render_tree(root: int, issues: dict[int, Issue]) -> str:
    lines: list[str] = []

    def render(number: int, prefix: str, is_last: bool, is_root: bool = False) -> None:
        issue = issues[number]
        parent = str(issue.parent) if issue.parent is not None else "—"
        connector = "" if is_root else ("└─ " if is_last else "├─ ")
        lines.append(
            f"{prefix}{connector}{status_icon(issue.state)} "
            f"#{number}(#{parent}) {issue.title}"
        )
        child_prefix = prefix if is_root else prefix + ("   " if is_last else "│  ")
        if issue.blocked_by:
            dependencies = ", ".join(
                f"#{dependency}(#{issues[dependency].parent or '—'}){status_icon(issues[dependency].state)}"
                for dependency in issue.blocked_by
            )
            lines.append(f"{child_prefix}   ← {dependencies}")
        for index, child in enumerate(issue.children):
            render(child, child_prefix, index == len(issue.children) - 1)

    render(root, "", True, True)
    return "\n".join(lines)


def render_report(root: int, issues: dict[int, Issue], repo: str) -> str:
    hierarchy = reachable_hierarchy(root, issues)
    closed = sum(issues[number].state == "closed" for number in hierarchy)
    total = len(hierarchy)
    dependency_edges = sum(len(issues[number].blocked_by) for number in hierarchy)
    progress = closed / total * 100 if total else 0
    return "\n".join(
        [
            f"# #{root} Issue 进度图",
            "",
            f"仓库：`{repo}`",
            "",
            f"- 总节点：**{total}**",
            f"- 已完成：**{closed}**",
            f"- 未完成：**{total - closed}**",
            f"- 完成率：**{progress:.1f}%**",
            f"- blocked-by 关系：**{dependency_edges} 条**",
            "- `✅` 已关闭；`⬜` 未关闭",
            "",
            "```text",
            render_tree(root, issues),
            "```",
            "",
        ]
    )


def reachable_hierarchy(root: int, issues: dict[int, Issue]) -> set[int]:
    found: set[int] = set()
    pending = [root]
    while pending:
        number = pending.pop()
        if number in found:
            continue
        found.add(number)
        pending.extend(issues[number].children)
    return found


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("issue", type=int, nargs="?", default=743, help="根 Issue 编号")
    parser.add_argument("--repo", default="rushsinging/aemeath", help="owner/repo")
    parser.add_argument(
        "--workers", type=int, default=DEFAULT_WORKERS, help="并发请求数（默认 10）"
    )
    parser.add_argument("--output", type=Path, help="输出 Markdown 文件；默认 stdout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.workers < 1:
            raise RuntimeError("--workers 必须大于 0")
        issues = collect_issues(args.repo, args.issue, workers=args.workers)
        report = render_report(args.issue, issues, args.repo)
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(report, encoding="utf-8")
        else:
            sys.stdout.write(report)
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        print(f"错误：{error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
