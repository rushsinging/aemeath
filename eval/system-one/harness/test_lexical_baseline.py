#!/usr/bin/env python3
"""lexical_baseline.py 词法复刻自测（#1834）。"""
from __future__ import annotations

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import lexical_baseline as lb  # noqa: E402


class TokenizeTest(unittest.TestCase):
    def test_english_words_lowercased(self):
        self.assertEqual(lb.tokenize("Hello World"), ["hello", "world"])

    def test_chinese_bigram(self):
        self.assertEqual(lb.tokenize("记忆库"), ["记忆", "忆库"])

    def test_single_han_char(self):
        self.assertEqual(lb.tokenize("库"), ["库"])

    def test_mixed(self):
        self.assertEqual(lb.tokenize("git配置"), ["git", "配置"])

    def test_punctuation_splits(self):
        self.assertEqual(lb.tokenize("a.b,c"), ["a", "b", "c"])


class RankTest(unittest.TestCase):
    def entries(self):
        return [
            {"id": "a", "category": "pitfall", "layer": "project", "pinned": False,
             "confirmation_count": 0, "last_confirmed_at": 0, "tags": [],
             "content": "磁盘打满根因是 worktree target 缓存不回收"},
            {"id": "b", "category": "fact", "layer": "project", "pinned": False,
             "confirmation_count": 0, "last_confirmed_at": 0, "tags": [],
             "content": "发版流程通过 git tag 触发 workflow"},
        ]

    def test_relevant_entry_ranks_first(self):
        ranking = lb.rank(self.entries(), "磁盘打满怎么办", now=0)
        self.assertEqual(ranking[0], 0)

    def test_unrelated_query_returns_empty(self):
        self.assertEqual(lb.rank(self.entries(), "zzzzzz", now=0), [])

    def test_empty_query_returns_empty(self):
        self.assertEqual(lb.rank(self.entries(), "", now=0), [])

    def test_exact_match_boost(self):
        entries = self.entries()
        ranking = lb.rank(entries, "发版流程通过 git tag 触发 workflow", now=0)
        self.assertEqual(ranking[0], 1)


if __name__ == "__main__":
    unittest.main()
