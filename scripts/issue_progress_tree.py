#!/usr/bin/env python3
"""Issue 层级与依赖进度图（registry: scripts/issue_progress_tree.py）。

递归读取指定 GitHub Issue 的原生 sub-issue 层级与 blocked-by 依赖，
输出 Markdown 进度图：

- 节点格式 `#number(#father)`，父为根时省略括号；
- Issue 开闭状态：✅ CLOSED / ⬜ OPEN；
- 每个节点的 blocked-by 依赖以 `← #dep✅/⬜` 内联渲染实时状态。

仅用 Python 标准库 + 已认证 gh CLI。默认输出 stdout，`--output` 写文件。

用法:
    scripts/issue_progress_tree.py 743
    scripts/issue_progress_tree.py 743 --repo rushsinging/aemeath --output tree.md
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys

STATE_ICON = {"CLOSED": "✅", "OPEN": "⬜"}


def run_gh(args: list[str]) -> str:
    """调用 gh CLI（测试经 fetch 参数注入替身）。"""
    completed = subprocess.run(
        ["gh", "api", "graphql", "-f", f"query={args[0]}"],
        capture_output=True,
        text=True,
        check=True,
    )
    return completed.stdout


def fetch_issue(repo: str, number: int, gh=run_gh) -> dict:
    """取单个 issue 的状态、subIssues 与 blockedBy（各仅取前 50，防巨型树）。"""
    owner, name = repo.split("/", 1)
    query = (
        "{ repository(owner:\"%s\", name:\"%s\") { issue(number:%d) { "
        "number state title "
        "subIssues(first:50) { nodes { number state title } } "
        "blockedBy(first:50) { nodes { number state title } } } } }"
    ) % (owner, name, number)
    payload = json.loads(gh([query]))
    return payload["data"]["repository"]["issue"]


def build_tree(repo: str, number: int, gh=run_gh, _seen: set[int] | None = None) -> dict:
    """递归展开 sub-issue 层级；环路由 _seen 剪枝。"""
    seen = _seen if _seen is not None else set()
    if number in seen:
        return {"number": number, "state": "CYCLED", "title": "", "children": [], "blocked_by": []}
    seen.add(number)
    issue = fetch_issue(repo, number, gh)
    return {
        "number": issue["number"],
        "state": issue["state"],
        "title": issue["title"],
        "children": [
            build_tree(repo, child["number"], gh, seen | {number})
            for child in issue["subIssues"]["nodes"]
        ],
        "blocked_by": [
            (dep["number"], dep["state"]) for dep in issue["blockedBy"]["nodes"]
        ],
    }


def render_node(node: dict, parent: int | None, depth: int, lines: list[str]) -> None:
    icon = STATE_ICON.get(node["state"], "❔")
    father = f"(#{parent})" if parent is not None else ""
    deps = "".join(
        f" ← #{dep}{STATE_ICON.get(state, '❔')}" for dep, state in node["blocked_by"]
    )
    lines.append(f"{'  ' * depth}- #{node['number']}{father}{icon} {node['title']}{deps}")
    for child in node["children"]:
        render_node(child, node["number"], depth + 1, lines)


def render(tree: dict) -> str:
    lines: list[str] = []
    render_node(tree, None, 0, lines)
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("issue", type=int, help="根 issue 编号")
    parser.add_argument("--repo", default="rushsinging/aemeath", help="owner/name")
    parser.add_argument("--output", default=None, help="输出文件（缺省 stdout）")
    args = parser.parse_args(argv)

    tree = build_tree(args.repo, args.issue)
    text = render(tree)
    if args.output:
        with open(args.output, "w", encoding="utf-8") as handle:
            handle.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
