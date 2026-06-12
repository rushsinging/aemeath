#!/usr/bin/env python3
"""单元测试：解析各种 docs 格式。

不依赖 gh CLI；纯函数测试。

运行：
  python3 scripts/migrate-docs-to-issues/tests/test_parsers.py
"""
import sys
import re
import tempfile
import unittest
from pathlib import Path

# 把 migrate.py 加进 path
SCRIPT_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SCRIPT_DIR))

import migrate  # type: ignore  # noqa: E402


class TestNormalizePriority(unittest.TestCase):
    def test_chinese_high(self):
        self.assertEqual(migrate.normalize_priority("高"), "high")

    def test_chinese_medium(self):
        self.assertEqual(migrate.normalize_priority("中"), "medium")

    def test_dash(self):
        self.assertIsNone(migrate.normalize_priority("-"))

    def test_english(self):
        self.assertEqual(migrate.normalize_priority("high"), "high")

    def test_empty(self):
        self.assertIsNone(migrate.normalize_priority(""))


class TestSplitMdTableRow(unittest.TestCase):
    def test_bug_active_row(self):
        line = "| 112 | TUI tool call spinner 有状态但输出区不显示 tool card | 中 | 待确认 | 待用户确认 | 2026-06 | runtime tool 事件携带 chat/turn context |"
        r = migrate.split_md_table_row(line)
        self.assertIsNotNone(r)
        self.assertEqual(r["id"], 112)
        self.assertEqual(r["priority"], "medium")
        self.assertEqual(r["state"], "待确认")

    def test_feature_active_row(self):
        line = "| 28 | MCP 系统完善 | 高 | 🔧 未完成 | 未确认 | P0+P1 已完成；SSE 传输有可靠性问题，MCP 加载暂时禁用待修复 |"
        r = migrate.split_md_table_row(line)
        self.assertIsNotNone(r)
        self.assertEqual(r["id"], 28)
        self.assertEqual(r["priority"], "high")
        self.assertEqual(r["title"], "MCP 系统完善")

    def test_header_row_returns_none(self):
        line = "| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |"
        self.assertIsNone(migrate.split_md_table_row(line))

    def test_separator_row_returns_none(self):
        line = "|---|------|--------|------|----------|------|"
        self.assertIsNone(migrate.split_md_table_row(line))


class TestArchiveTitleRegex(unittest.TestCase):
    def test_bug_space(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# Bug #3 优化 tool call TUI 显示")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("kind"), "Bug")
        self.assertEqual(m.group("id"), "3")
        self.assertEqual(m.group("title"), "优化 tool call TUI 显示")

    def test_bug_english_colon(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# Bug #30: 对话过程中 input queue 不被消费")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("id"), "30")
        self.assertEqual(m.group("title"), "对话过程中 input queue 不被消费")

    def test_bug_chinese_colon(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# Bug #78：input area 粘贴后按空格清空")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("id"), "78")
        self.assertEqual(m.group("title"), "input area 粘贴后按空格清空")

    def test_bug_no_hash(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# Bug 61: Diff 渲染行号顶到最左")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("id"), "61")
        self.assertEqual(m.group("title"), "Diff 渲染行号顶到最左")

    def test_no_kind_prefix(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# #19 config model 支持 zhipu api 类型")
        self.assertIsNotNone(m)
        self.assertIsNone(m.group("kind"))
        self.assertEqual(m.group("id"), "19")
        self.assertEqual(m.group("title"), "config model 支持 zhipu api 类型")

    def test_feature_space(self):
        m = migrate.ARCHIVE_TITLE_RE.match("# Feature #48：TUI 窗口 resize 时重新计算")
        self.assertIsNotNone(m)
        self.assertEqual(m.group("kind"), "Feature")
        self.assertEqual(m.group("id"), "48")


class TestParseArchivedFile(unittest.TestCase):
    def _setup_temp(self, name: str, content: str) -> Path:
        """在临时目录构造 docs/{kind}/archived/ 结构。"""
        tmp = Path(tempfile.mkdtemp())
        archived = tmp / "archived"
        archived.mkdir()
        f = archived / name
        f.write_text(content, encoding="utf-8")
        return tmp

    def test_basic_bug_archive(self):
        content = """# Bug #3 优化 tool call TUI 显示

**状态**：✅ 已修复
**优先级**：高
**根因类别**：tool_call_active
"""
        tmp = self._setup_temp("003-tool-call-tui-display.md", content)
        orig_root = migrate.REPO_ROOT
        migrate.REPO_ROOT = tmp
        try:
            entry = migrate.parse_archived_file("bug", tmp / "archived" / "003-tool-call-tui-display.md")
        finally:
            migrate.REPO_ROOT = orig_root
        self.assertIsNotNone(entry)
        self.assertEqual(entry.kind, "bug")
        self.assertEqual(entry.id, 3)
        self.assertEqual(entry.title, "优化 tool call TUI 显示")
        self.assertEqual(entry.priority, "high")
        self.assertEqual(entry.state, "✅ 已修复")
        self.assertIn("Migrated from:", entry.body)

    def test_chinese_colon_title(self):
        content = """# Bug #78：input area 粘贴后按空格清空

**状态**：已修复
"""
        tmp = self._setup_temp("078-paste-cleared-by-space.md", content)
        orig_root = migrate.REPO_ROOT
        migrate.REPO_ROOT = tmp
        try:
            entry = migrate.parse_archived_file("bug", tmp / "archived" / "078-paste-cleared-by-space.md")
        finally:
            migrate.REPO_ROOT = orig_root
        self.assertIsNotNone(entry)
        self.assertEqual(entry.id, 78)
        self.assertEqual(entry.title, "input area 粘贴后按空格清空")

    def test_filename_id_authoritative(self):
        """文件名 id 与首行 id 不一致时，以文件名为准。"""
        content = """# Bug #999 错误标题

**状态**：已修复
"""
        tmp = self._setup_temp("003-real-id.md", content)
        orig_root = migrate.REPO_ROOT
        migrate.REPO_ROOT = tmp
        try:
            entry = migrate.parse_archived_file("bug", tmp / "archived" / "003-real-id.md")
        finally:
            migrate.REPO_ROOT = orig_root
        self.assertIsNotNone(entry)
        self.assertEqual(entry.id, 3)  # 来自文件名
        # title 在 id 不一致时回退到 slug
        self.assertEqual(entry.title, "real id")  # slug 转 title


class TestParseActiveFile(unittest.TestCase):
    def _setup_temp(self, name: str, content: str) -> Path:
        tmp = Path(tempfile.mkdtemp())
        f = tmp / name
        f.write_text(content, encoding="utf-8")
        return f

    def test_bug_active(self):
        content = """# 活动中 Bug

| # | 标题 | 优先级 | 状态 | 确认结果 | 发现日期 | 根因类别 |
|---|------|--------|------|----------|----------|----------|
| 1 | Test bug | 中 | 待确认 | - | 2026-06 | runtime |

### #1 Test bug

**状态**：待确认
**症状**：some symptom
"""
        f = self._setup_temp("active.md", content)
        # 临时把 REPO_ROOT 切到 tmp，避免 wrap_body 报错
        orig_root = migrate.REPO_ROOT
        migrate.REPO_ROOT = f.parent
        try:
            entries = migrate.parse_active_file("bug", f)
        finally:
            migrate.REPO_ROOT = orig_root
        self.assertEqual(len(entries), 1)
        e = entries[0]
        self.assertEqual(e.id, 1)
        self.assertEqual(e.title, "Test bug")
        self.assertEqual(e.priority, "medium")
        self.assertEqual(e.state, "待确认")
        self.assertTrue(e.source.endswith("#1"))  # active.md 内带 #1 锚点
        self.assertIn("Migrated from:", e.body)

    def test_feature_active(self):
        content = """# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 8 | Memory | - | 已完成 | 未确认 | MVP 已落地 |

### #8 Memory

**目标**：跨会话持久化记忆。
"""
        f = self._setup_temp("active.md", content)
        orig_root = migrate.REPO_ROOT
        migrate.REPO_ROOT = f.parent
        try:
            entries = migrate.parse_active_file("feature", f)
        finally:
            migrate.REPO_ROOT = orig_root
        self.assertEqual(len(entries), 1)
        e = entries[0]
        self.assertEqual(e.id, 8)
        self.assertEqual(e.title, "Memory")
        self.assertIsNone(e.priority)  # "-"
        self.assertEqual(e.state, "已完成")


if __name__ == "__main__":
    unittest.main(verbosity=2)
