#!/usr/bin/env python3
"""通道 A 影子候选分析的边界自检：先锁过滤 / 排序 / 口径，再运行主脚本。"""
from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE / "analyze_channel_a.py"

import analyze_channel_a as aca  # noqa: E402
import lexical_baseline as lb  # noqa: E402  (analyze_channel_a 已挂 system-one harness 路径)
from memory_snapshot import is_injection_eligible  # noqa: E402

NOW = 1_792_200_000
LONG_TEXT = ("preference: 用户偏好在命令行里用英文输出提示与日志，"
             "中文只出现在记忆内容本身，便于跨工具检索与 grep")


def make_entry(entry_id: str, **overrides) -> dict:
    base = {
        "id": entry_id,
        "content": LONG_TEXT,
        "layer": "global",
        "category": "fact",
        "pinned": False,
        "confirmation_count": 0,
        "last_confirmed_at": NOW,
        "created_at": 0,
        "ttl": None,
        "outdated": False,
        "superseded_by": None,
        "kind": "raw",
        "tags": [],
    }
    base.update(overrides)
    return base


def make_snapshot(entries: list[dict], captured_at: int = NOW,
                  corpora: list[dict] | None = None) -> dict:
    if corpora is None:
        corpora = [{"corpus": "global-corpus", "revision": 3,
                    "active_sha256": "0" * 64, "entries": len(entries)}]
    return {"schema_version": 1, "captured_at": captured_at,
            "corpora": corpora, "entries": entries}


class ChannelAChecks(unittest.TestCase):
    def test_order_reuses_lexical_baseline_tie_break_without_local_recency(self):
        # 排序分数必须来自 lexical_baseline.tie_break_score，不允许本地再抄一份 recency
        self.assertIs(aca.lb.tie_break_score, lb.tie_break_score)
        self.assertFalse(hasattr(aca, "recency_score"))
        self.assertFalse(hasattr(aca, "injection_score"))
        fresh = make_entry("m-fresh", last_confirmed_at=NOW)
        stale_recency = make_entry("m-old", last_confirmed_at=0)
        ordered = aca.order_for_injection([stale_recency, fresh], NOW)
        self.assertEqual([e["id"] for e in ordered], ["m-fresh", "m-old"])
        self.assertEqual(lb.tie_break_score(fresh, NOW)
                         - lb.tie_break_score(stale_recency, NOW), 950)

    def test_synthesized_results_lead_pinned_raw_and_id_tie_break(self):
        pinned_raw = make_entry("b-pin", pinned=True, confirmation_count=20)
        synthesized = make_entry("a-syn", kind="synthesized", last_confirmed_at=0)
        plain_low = make_entry("c-plain", last_confirmed_at=0)
        ordered = aca.order_for_injection(
            [pinned_raw, plain_low, synthesized], NOW)
        self.assertEqual([e["id"] for e in ordered],
                         ["a-syn", "b-pin", "c-plain"])
        # 同分组同分时按 id 升序，对齐 order_for_injection 的第三级比较
        tied = [make_entry("z-tie"), make_entry("a-tie")]
        self.assertEqual([e["id"] for e in aca.order_for_injection(tied, NOW)],
                         ["a-tie", "z-tie"])

    def test_ttl_strict_boundary_and_pinned_does_not_bypass(self):
        expiring = make_entry("ttl-1", created_at=NOW - 10,
                              ttl={"secs": 10, "nanos": 0}, pinned=True)
        at_boundary = aca.build_report(make_snapshot([expiring], NOW), limit=10)
        self.assertEqual(at_boundary["eligible"], 1)
        self.assertEqual(at_boundary["stale_diagnostics"]["ttl_expired"], 0)
        past_boundary = aca.build_report(
            make_snapshot([expiring], NOW + 1), limit=10)
        self.assertEqual(past_boundary["eligible"], 0)
        self.assertEqual(past_boundary["stale_diagnostics"]["ttl_expired"], 1)
        self.assertEqual(past_boundary["retrieval_candidates"], 0)
        # pinned 不能绕过 TTL：诊断与共享 eligibility 判定必须同源同结论
        self.assertFalse(is_injection_eligible(expiring, NOW + 1))
        self.assertEqual(past_boundary["coverage"]["pinned"]["eligible"], 0)

    def test_stale_diagnostics_match_shared_eligibility_and_exclude_candidates(self):
        now = NOW
        fixture = [
            make_entry("ok-1"),
            make_entry("bad-outdated", outdated=True),
            make_entry("bad-superseded", superseded_by="ok-1"),
            make_entry("bad-ttl", created_at=now - 100, ttl=50),
        ]
        for entry in fixture:
            reasons = (bool(entry.get("outdated")),
                       bool(entry.get("superseded_by")),
                       aca.is_ttl_expired(entry, now))
            self.assertEqual(not is_injection_eligible(entry, now), any(reasons))
        report = aca.build_report(make_snapshot(fixture, now), limit=10)
        self.assertEqual(report["source_active"], 4)
        self.assertEqual(report["eligible"], 1)
        self.assertEqual(report["retrieval_candidates"], 1)
        diagnostics = report["stale_diagnostics"]
        self.assertEqual(diagnostics["outdated"], 1)
        self.assertEqual(diagnostics["superseded"], 1)
        self.assertEqual(diagnostics["ttl_expired"], 1)
        self.assertEqual(diagnostics["any_stale"], 3)
        self.assertEqual(report["retrieval_candidate_ids"], ["ok-1"])
        # 自检不以"自己过滤后 stale_leak=0"充当回归证据，该字段必须不存在
        self.assertNotIn("stale_leak", json.dumps(report, ensure_ascii=False))

    def test_limit_drops_key_entries_and_coverage_below_one(self):
        pinned_pool = [make_entry(f"pin-{index}", pinned=True)
                       for index in range(4)]
        plain = [make_entry(f"plain-{index}") for index in range(3)]
        report = aca.build_report(make_snapshot(pinned_pool + plain), limit=3)
        self.assertEqual(report["retrieval_candidates"], 3)
        pinned_coverage = report["coverage"]["pinned"]
        self.assertEqual(pinned_coverage["eligible"], 4)
        self.assertEqual(pinned_coverage["selected"], 3)
        self.assertLess(pinned_coverage["ratio"], 1.0)
        decisions = [make_entry(f"dec-{index}", category="decision")
                     for index in range(3)]
        decision_report = aca.build_report(
            make_snapshot(decisions + plain), limit=2)
        decision_coverage = decision_report["coverage"]["decision_preference"]
        self.assertEqual(decision_coverage["eligible"], 3)
        self.assertEqual(decision_coverage["selected"], 2)
        self.assertLess(decision_coverage["ratio"], 1.0)
        self.assertEqual(
            decision_report["decision_preference_candidates"], 2)
        # 负 limit 不允许 Python 负切片静默丢尾
        self.assertEqual(
            aca.build_report(make_snapshot(plain), limit=-5)["retrieval_candidates"], 0)

    def test_report_uses_shadow_naming_with_null_materialization_evidence(self):
        report = aca.build_report(make_snapshot([make_entry("m-1")]), limit=200)
        flat_keys = json.dumps(report, ensure_ascii=False)
        self.assertNotIn("injected", flat_keys)
        self.assertEqual(report["analysis_kind"], "shadow_candidate_analysis")
        self.assertIsNone(report["final_system_block_chars"])
        self.assertIsNone(report["final_memory_token_cost"])
        self.assertEqual(report["now"], NOW)
        self.assertEqual(report["limit"], 200)
        candidates = report["retrieval_candidate_ids"]
        expected_chars = sum(len(make_entry(entry_id)["content"])
                             for entry_id in candidates)
        self.assertEqual(report["candidate_payload_chars"], expected_chars)
        notes = report["metric_notes"]
        self.assertIn("非最终 system block", notes["candidate_payload_chars"])
        self.assertIn("null", notes["final_system_block_chars"])

    def test_source_and_candidate_layer_distributions_cover_global_and_project(self):
        global_entries = [make_entry("g-1"), make_entry("g-2")]
        project_entries = [make_entry("p-1", layer="project"),
                           make_entry("p-2", layer="project"),
                           make_entry("p-3", layer="project")]
        corpora = [
            {"corpus": "global-corpus", "revision": 3,
             "active_sha256": "0" * 64, "entries": 2},
            {"corpus": "project-corpus", "revision": 7,
             "active_sha256": "1" * 64, "entries": 3},
        ]
        entries = global_entries + project_entries
        report = aca.build_report(
            make_snapshot(entries, corpora=corpora), limit=4)
        source_layers = report["source_layer_distribution"]
        self.assertEqual(source_layers, {"global": 2, "project": 3})
        self.assertEqual(sum(source_layers.values()), report["source_active"])
        self.assertEqual(
            sum(corpora_item["entries"] for corpora_item in
                report["snapshot"]["corpora"]),
            report["source_active"])
        self.assertEqual(
            sum(report["candidate_layer_distribution"].values()),
            report["retrieval_candidates"])
        self.assertEqual(
            sum(report["source_category_distribution"].values()),
            report["source_active"])

    def test_exact_duplicate_and_near_duplicate_pair_counts(self):
        unrelated = make_entry("m-other",
                               content="发布前先跑 migrate，再滚动重启 service")
        exact_pair = [make_entry("m-a"), make_entry("m-b", content=LONG_TEXT)]
        exact_report = aca.build_report(
            make_snapshot(exact_pair + [unrelated]), limit=10)
        exact_stats = exact_report["content_similarity"]
        self.assertEqual(exact_stats["exact_duplicate_pairs"], 1)
        self.assertEqual(exact_stats["near_duplicate_pairs"], 0)
        expanded = make_entry("m-c", content=LONG_TEXT + "。")
        near_report = aca.build_report(
            make_snapshot([make_entry("m-a"), expanded, unrelated]), limit=10)
        near_stats = near_report["content_similarity"]
        self.assertEqual(near_stats["exact_duplicate_pairs"], 0)
        self.assertEqual(near_stats["near_duplicate_pairs"], 1)
        self.assertGreaterEqual(aca.string_jaccard(LONG_TEXT, LONG_TEXT + "。"),
                                0.85)
        self.assertLess(aca.string_jaccard(LONG_TEXT, unrelated["content"]), 0.85)
        self.assertIn("字符串相似度",
                      near_stats["method_note"])

    def test_cli_requires_snapshot_and_output(self):
        missing_args = subprocess.run(
            [sys.executable, str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(missing_args.returncode, 2)
        self.assertIn("--snapshot", missing_args.stderr)
        only_snapshot = subprocess.run(
            [sys.executable, str(SCRIPT), "--snapshot", "x.json"],
            capture_output=True, text=True)
        self.assertEqual(only_snapshot.returncode, 2)
        self.assertIn("--output", only_snapshot.stderr)

    def test_cli_default_limit_two_hundred_and_writes_output(self):
        entries = [make_entry(f"m-{index:03d}", content=f"记忆条目 {index}")
                   for index in range(205)]
        with tempfile.TemporaryDirectory() as tmp_dir:
            snapshot_path = pathlib.Path(tmp_dir) / "snapshot.json"
            output_path = pathlib.Path(tmp_dir) / "report.json"
            snapshot_path.write_text(
                json.dumps(make_snapshot(entries), ensure_ascii=False),
                encoding="utf-8")
            run = subprocess.run(
                [sys.executable, str(SCRIPT),
                 "--snapshot", str(snapshot_path), "--output", str(output_path)],
                capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stderr)
            report = json.loads(output_path.read_text(encoding="utf-8"))
        self.assertEqual(report["source_active"], 205)
        self.assertEqual(report["eligible"], 205)
        self.assertEqual(report["retrieval_candidates"], 200)
        self.assertEqual(report["limit"], 200)
        self.assertIsNone(report["final_system_block_chars"])


if __name__ == "__main__":
    unittest.main()
