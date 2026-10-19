#!/usr/bin/env python3
"""离线影子回放的边界自检，先保护门控/过滤/分母，再运行基线。"""
from __future__ import annotations

import unittest

from replay_recall import classify_gate, simulate_preview, summarize_cases
from memory_snapshot import is_injection_eligible


class ReplayChecks(unittest.TestCase):
    def test_gate_outcomes_do_not_conflate_wrong_selection_and_rejection(self):
        self.assertEqual(classify_gate(1, [0.8, 0.2]), "wrong_selection")
        self.assertEqual(classify_gate(1, [0.45, 0.55]), "correct_selection")
        self.assertEqual(classify_gate(1, [0.51, 0.49]), "wrong_selection")
        self.assertEqual(classify_gate(1, [0.49, 0.48]), "missed_selection")
        self.assertEqual(classify_gate(-1, [0.8, 0.2]), "false_selection")
        self.assertEqual(classify_gate(-1, [0.4, 0.3]), "correct_rejection")

    def test_preview_stops_at_first_budget_overflow_and_top_three(self):
        candidates = ["甲" * 240] * 6
        preview = simulate_preview(candidates, [str(index) for index in range(6)], [0.6, 0.2, 0.1, 0.05, 0.03, 0.02])
        self.assertEqual(preview["selected_count"], 3)
        self.assertEqual(preview["selected_content_chars"], 600)
        overflow = simulate_preview(["甲", "乙"], ["x" * 1200, "ok"], [0.7, 0.3])
        self.assertEqual(overflow["selected_count"], 0)

    def test_ttl_uses_strict_boundary_and_pinned_does_not_bypass(self):
        entry = {"created_at": 100, "ttl": {"secs": 10, "nanos": 0}, "pinned": True}
        self.assertTrue(is_injection_eligible(entry, 110))
        self.assertFalse(is_injection_eligible(entry, 111))
        self.assertFalse(is_injection_eligible({**entry, "ttl": None, "outdated": True}, 100))
        self.assertFalse(is_injection_eligible({**entry, "ttl": None, "superseded_by": "new"}, 100))

    def test_unavailable_scores_never_count_as_rejections(self):
        report = summarize_cases([
            {"gold": -1, "status": "scoring_error", "failure": "bad response"},
            {"gold": -1, "status": "scored", "gate_outcome": "correct_rejection", "preview": {"selected_count": 0, "selected_content_chars": 0}},
        ])
        self.assertEqual(report["scored_cases"], 1)
        self.assertEqual(report["scoring_errors"], 1)
        self.assertEqual(report["gate_outcomes"]["correct_rejection"], 1)
        self.assertIsNone(report["downstream_task_gain"])


if __name__ == "__main__":
    unittest.main()
