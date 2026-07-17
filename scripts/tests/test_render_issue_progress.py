#!/usr/bin/env python3
"""render_issue_progress.py 的单元测试。"""

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "render-issue-progress.py"
SPEC = importlib.util.spec_from_file_location("render_issue_progress", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class RenderIssueProgressTests(unittest.TestCase):
    def test_render_tree_includes_parent_and_dependency_statuses(self):
        issues = {
            743: MODULE.Issue(743, "根", "open", None, [649], []),
            649: MODULE.Issue(649, "Runtime", "open", 743, [875], []),
            875: MODULE.Issue(875, "模型调用", "open", 649, [], [873, 903]),
            873: MODULE.Issue(873, "端口", "closed", 649, [], []),
            903: MODULE.Issue(903, "流", "open", 852, [], []),
        }

        rendered = MODULE.render_tree(743, issues)

        self.assertIn("⬜ #875(#649) 模型调用", rendered)
        self.assertIn("← #873✅, #903⬜", rendered)

    def test_render_report_summarizes_progress(self):
        issues = {
            743: MODULE.Issue(743, "根", "open", None, [868], []),
            868: MODULE.Issue(868, "契约", "closed", 743, [], []),
        }

        rendered = MODULE.render_report(743, issues, "owner/repo")

        self.assertIn("总节点：**2**", rendered)
        self.assertIn("已完成：**1**", rendered)
        self.assertIn("完成率：**50.0%**", rendered)
        self.assertIn("✅ #868(#743) 契约", rendered)


if __name__ == "__main__":
    unittest.main()
