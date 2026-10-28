#!/usr/bin/env python3
"""确定性指标函数的边界自检（先保护分母与判定语义，再谈基线）。"""
from __future__ import annotations

import unittest

from metrics import (
    compression_ratio,
    extract_identifiers,
    must_keep_recall,
    rank_identifiers,
    structure_health,
    summarize_cases,
)


class ExtractIdentifiersTest(unittest.TestCase):
    def test_extracts_typed_ids_and_paths_once(self):
        texts = [
            "见 agent/features/context/src/domain.rs 与 run_082gKx9yRcG",
            "再提一次 run_082gKx9yRcG 和 agent/features/context/src/domain.rs:42",
        ]
        found = extract_identifiers(texts)
        self.assertIn("run_082gKx9yRcG", found)
        self.assertIn("agent/features/context/src/domain.rs", found)
        self.assertEqual(found.count("run_082gKx9yRcG"), 1)

    def test_drops_eval_noise_paths(self):
        found = extract_identifiers(["eval/compact/harness/metrics.py"])
        self.assertEqual(found, [])

    def test_keeps_plain_file_names_but_not_bare_words(self):
        found = extract_identifiers(["see notes.md and plain words"])
        self.assertEqual(found, ["notes.md"])


class RankIdentifiersTest(unittest.TestCase):
    def test_higher_frequency_ranks_first_and_limit_applies(self):
        texts = ["run_abc123 run_abc123 mem_def456", "run_abc123 mem_def456", "stp_ghi789"]
        ranked = rank_identifiers(texts, limit=2)
        self.assertEqual(ranked, ["run_abc123", "mem_def456"])

    def test_limit_keeps_stable_alphabetical_order_for_ties(self):
        ranked = rank_identifiers(["beta.rs alpha.rs"], limit=10)
        self.assertEqual(ranked, ["alpha.rs", "beta.rs"])


class MustKeepRecallTest(unittest.TestCase):
    def test_empty_gold_is_not_zero_score(self):
        report = must_keep_recall([], "anything")
        self.assertIsNone(report["recall"])
        self.assertEqual(report["total"], 0)

    def test_reports_missing_items(self):
        report = must_keep_recall(["mem_abc123", "a.rs"], "这里只有 mem_abc123")
        self.assertEqual(report["hit"], 1)
        self.assertEqual(report["missed"], ["a.rs"])
        self.assertAlmostEqual(report["recall"], 0.5)


class CompressionRatioTest(unittest.TestCase):
    def test_zero_input_is_undefined(self):
        self.assertIsNone(compression_ratio(0, 10))

    def test_ratio(self):
        self.assertAlmostEqual(compression_ratio(1000, 250), 0.25)


class StructureHealthTest(unittest.TestCase):
    def test_partial_map_fallback_counts_as_degraded(self):
        health = structure_health(
            {"quality": "PartialMapFallback { degraded_chunks: 2 }", "degraded_chunks": 2}
        )
        self.assertTrue(health["degraded"])
        self.assertEqual(health["degraded_chunks"], 2)

    def test_clean_llm_is_not_degraded(self):
        health = structure_health({"quality": "Llm"})
        self.assertFalse(health["degraded"])
        self.assertEqual(health["repair_attempts"], 0)


class SummarizeCasesTest(unittest.TestCase):
    def test_skipped_cases_do_not_pollute_denominator(self):
        report = summarize_cases(
            [
                {"recall": 1.0, "compression_ratio": 0.5, "structure_health": {"quality": "Llm"}},
                {"recall": None, "compression_ratio": None},
            ]
        )
        self.assertEqual(report["total"], 2)
        self.assertEqual(report["evaluated"], 1)
        self.assertEqual(report["skipped"], 1)
        self.assertAlmostEqual(report["must_keep_recall_mean"], 1.0)
        self.assertAlmostEqual(report["compression_ratio_mean"], 0.5)

    def test_degraded_and_repair_are_aggregated(self):
        report = summarize_cases(
            [
                {
                    "recall": 0.5,
                    "compression_ratio": 0.2,
                    "structure_health": {
                        "quality": "LocalFallback",
                        "degraded_chunks": 1,
                        "repair_attempts": 2,
                    },
                },
                {
                    "recall": 0.25,
                    "compression_ratio": 0.1,
                    "structure_health": {"quality": "Llm", "repair_attempts": 1},
                },
            ]
        )
        self.assertEqual(report["degraded_cases"], 1)
        self.assertEqual(report["repair_attempts_total"], 3)
        self.assertEqual(report["quality"], {"LocalFallback": 1, "Llm": 1})


if __name__ == "__main__":
    unittest.main()
