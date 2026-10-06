#!/usr/bin/env python3
"""score.py 场景接入门禁（--gate）单元测试（#1834）。"""
from __future__ import annotations

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import score  # noqa: E402


def make_case(rid: str, chinese: bool, gold: int = 0) -> dict:
    context = "用户在调试异步死锁。" if chinese else "Debugging an async deadlock."
    question = "哪条记忆最相关？" if chinese else "Which memory is most relevant?"
    return {
        "id": rid,
        "context": context,
        "question": question,
        "answers": [f"answer-{rid}-a", f"answer-{rid}-b"],
        "gold": gold,
        "scenario": "memory_rerank",
    }


def make_record(rid: str, case: dict, order: str, top1_correct: bool,
                latency_ms: float = 100.0) -> dict:
    gold_answer = case["answers"][case["gold"]]
    top1 = gold_answer if top1_correct else case["answers"][1 - case["gold"]]
    return {
        "id": rid,
        "order": order,
        "top1": top1,
        "probabilities": {a: (0.9 if a == top1 else 0.1) for a in case["answers"]},
        "latency_ms": latency_ms,
    }


class ChineseSubsetMetricTest(unittest.TestCase):
    def test_chinese_r_at_1_perfect_when_all_chinese_correct(self):
        cases = {"c1": make_case("c1", chinese=True),
                 "e1": make_case("e1", chinese=False)}
        records = [make_record("c1", cases["c1"], "forward", True),
                   make_record("e1", cases["e1"], "forward", False)]
        metrics = score.eval_rank(records, cases)
        self.assertEqual(metrics["chinese_r_at_1"], 1.0)

    def test_chinese_r_at_1_drops_when_chinese_wrong(self):
        cases = {"c1": make_case("c1", chinese=True),
                 "c2": make_case("c2", chinese=True),
                 "e1": make_case("e1", chinese=False)}
        records = [make_record("c1", cases["c1"], "forward", True),
                   make_record("c2", cases["c2"], "forward", False),
                   make_record("e1", cases["e1"], "forward", True)]
        metrics = score.eval_rank(records, cases)
        self.assertEqual(metrics["chinese_r_at_1"], 0.5)
        self.assertAlmostEqual(metrics["R@1"], 2 / 3)


class CheckGateTest(unittest.TestCase):
    def passing_metrics(self) -> dict:
        return {
            "n_forward": 12,
            "errors": 0,
            "R@1": 1.0,
            "MRR": 1.0,
            "order_flip_rate": 0.0,
            "latency_p95_ms": 179.0,
            "chinese_r_at_1": 1.0,
        }

    def test_pass_when_all_thresholds_met(self):
        self.assertEqual(score.check_gate("memory_rerank", self.passing_metrics()), [])

    def test_fail_when_r_at_1_below_threshold(self):
        metrics = self.passing_metrics()
        metrics["R@1"] = 0.9
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("R@1" in v for v in violations))

    def test_fail_when_mrr_below_threshold(self):
        metrics = self.passing_metrics()
        metrics["MRR"] = 0.95
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("MRR" in v for v in violations))

    def test_fail_when_flip_rate_exceeds_threshold(self):
        metrics = self.passing_metrics()
        metrics["order_flip_rate"] = 0.1
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("order_flip_rate" in v for v in violations))

    def test_fail_when_p95_exceeds_cold_path_budget(self):
        metrics = self.passing_metrics()
        metrics["latency_p95_ms"] = 2500.0
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("latency_p95_ms" in v for v in violations))

    def test_fail_when_chinese_subset_not_perfect(self):
        metrics = self.passing_metrics()
        metrics["chinese_r_at_1"] = 0.9
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("chinese_r_at_1" in v for v in violations))

    def test_fail_closed_when_metric_missing(self):
        metrics = self.passing_metrics()
        metrics["R@1"] = None
        violations = score.check_gate("memory_rerank", metrics)
        self.assertTrue(any("R@1" in v for v in violations))

    def test_unknown_scenario_fails_closed(self):
        violations = score.check_gate("nonexistent_scenario", self.passing_metrics())
        self.assertTrue(violations)


if __name__ == "__main__":
    unittest.main()
