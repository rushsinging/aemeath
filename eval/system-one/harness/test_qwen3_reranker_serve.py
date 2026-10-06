#!/usr/bin/env python3
"""qwen3_reranker_serve 的 Jev 协议映射逻辑测试（#1834）。"""
from __future__ import annotations

import math
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import qwen3_reranker_serve as serve  # noqa: E402


class ChoiceMappingTest(unittest.TestCase):
    """choice → N 次 pointwise P(yes) → softmax 归一化 → Jev 响应。"""

    def test_argmax_becomes_choice(self):
        scores = {"0": 0.9, "1": 0.1, "2": 0.5}
        out = serve.choice_answer(criteria={"0": "a", "1": "b", "2": "c"},
                                  score_fn=lambda _text, key: scores[key])
        self.assertEqual(out["choice"], "0")
        self.assertAlmostEqual(sum(out["probabilities"].values()), 1.0, places=6)
        self.assertGreater(out["probabilities"]["0"], out["probabilities"]["2"])
        self.assertGreater(out["probabilities"]["2"], out["probabilities"]["1"])

    def test_tie_breaks_by_key_order(self):
        out = serve.choice_answer(criteria={"0": "a", "1": "b"},
                                  score_fn=lambda _text, _key: 0.5)
        self.assertEqual(out["choice"], "0")  # 平分取首个 key（确定性）

    def test_softmax_monotone(self):
        out = serve.choice_answer(criteria={"0": "a", "1": "b"},
                                  score_fn=lambda _text, key: {"0": 0.8, "1": 0.2}[key])
        self.assertGreater(out["probabilities"]["0"], 0.5)


class NoulMappingTest(unittest.TestCase):
    def test_p_true_is_yes_probability(self):
        out = serve.noul_answer(score_fn=lambda _text: 0.73)
        self.assertAlmostEqual(out["noul"], 0.73)


class ScoreMappingTest(unittest.TestCase):
    def test_score_maps_p_yes_into_level_bins(self):
        # score 题型：criteria 为等级表，P(yes) 映射到最接近的等级中值
        levels = {"1": "poor", "2": "ok", "3": "great"}
        out = serve.score_answer(levels, score_fn=lambda _text: 0.95)
        self.assertEqual(out["score"], 3)


if __name__ == "__main__":
    unittest.main()
