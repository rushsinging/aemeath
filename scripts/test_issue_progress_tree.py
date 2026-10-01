#!/usr/bin/env python3
"""issue_progress_tree.py 的单元测试（python3 -m unittest discover -s scripts）。"""

import unittest

from issue_progress_tree import build_tree, render


def make_gh(responses: dict[int, dict]):
    """按 issue 编号返回预置 GraphQL 响应的 gh 替身。"""

    def fake_gh(args):
        import json

        query = args[0]
        for number, issue in responses.items():
            if f"issue(number:{number})" in query:
                return json.dumps({"data": {"repository": {"issue": issue}}})
        raise AssertionError(f"unexpected query: {query[:80]}")

    return fake_gh


class BuildTreeTest(unittest.TestCase):
    def test_recursive_hierarchy_and_cycle_guard(self):
        gh = make_gh(
            {
                1: {
                    "number": 1,
                    "state": "OPEN",
                    "title": "root",
                    "subIssues": {"nodes": [{"number": 2, "state": "CLOSED"}]},
                    "blockedBy": {"nodes": []},
                },
                2: {
                    "number": 2,
                    "state": "CLOSED",
                    "title": "child",
                    # 2 的子指向 1：环必须被剪枝为 CYCLED，不无限递归。
                    "subIssues": {"nodes": [{"number": 1, "state": "OPEN"}]},
                    "blockedBy": {"nodes": [{"number": 3, "state": "CLOSED"}]},
                },
            }
        )
        tree = build_tree("o/r", 1, gh)
        self.assertEqual(tree["number"], 1)
        self.assertEqual(tree["children"][0]["number"], 2)
        self.assertEqual(tree["children"][0]["blocked_by"], [(3, "CLOSED")])
        self.assertEqual(tree["children"][0]["children"][0]["state"], "CYCLED")


class RenderTest(unittest.TestCase):
    def test_father_annotation_state_icons_and_dep_arrows(self):
        tree = {
            "number": 743,
            "state": "CLOSED",
            "title": "DDD",
            "children": [
                {
                    "number": 875,
                    "state": "CLOSED",
                    "title": "model_invocation",
                    "children": [],
                    "blocked_by": [(873, "CLOSED"), (920, "CLOSED")],
                },
                {
                    "number": 901,
                    "state": "OPEN",
                    "title": "ProviderPort",
                    "children": [],
                    "blocked_by": [],
                },
            ],
            "blocked_by": [],
        }
        text = render(tree)
        lines = text.strip().splitlines()
        self.assertEqual(
            lines[0], "- #743✅ DDD", "根节点无父注解，带 CLOSED 图标"
        )
        self.assertIn("- #875(#743)✅ model_invocation ← #873✅ ← #920✅", lines[1])
        self.assertIn("- #901(#743)⬜ ProviderPort", lines[2])
        self.assertIn("  ", lines[1], "子节点缩进")

    def test_open_state_icon(self):
        tree = {"number": 5, "state": "OPEN", "title": "t", "children": [], "blocked_by": []}
        self.assertIn("⬜", render(tree))


if __name__ == "__main__":
    unittest.main()
