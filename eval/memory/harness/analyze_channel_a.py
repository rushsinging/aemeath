#!/usr/bin/env python3
"""通道 A（memory-context system block 注入）影子候选分析。

数据只来自 --snapshot（memory_snapshot.py 冻结的 active 清单），不读本地记忆桶；
now 取 snapshot.captured_at，保证与冻结现场一致。对齐生产判定与排序：

  - 资格：memory_snapshot.is_injection_eligible —— outdated / superseded /
    TTL 过期（created_at + ttl 严格小于 now 才算过期）
  - 排序：policy.rs order_for_injection —— synthesized 归纳结论整组优先，
    组内分数降序、同分按 id 升序；分数复用 lexical_baseline.tie_break_score
    （与生产 search_tie_break_score 同构），本地不复制 recency 阶梯
  - 截断：--limit（默认 200，生产注入上限）

口径警示：报告命名只用 source_active / eligible / retrieval_candidates，
是影子候选分析（analysis_kind=shadow_candidate_analysis），不代表真实注入——
没有 context window 与实际物化证据，final_system_block_chars 与
final_memory_token_cost 恒为 null；candidate_payload_chars 只是候选文本总字符。

用法：python3 harness/analyze_channel_a.py --snapshot <snap.json> --output <report.json>
输出：JSON 报告写入 --output，stdout 打印摘要；参数或快照不合法时非 0 退出。
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
sys.path.insert(0, str(EVAL_ROOT.parent / "system-one" / "harness"))
sys.path.insert(0, str(HERE))
import lexical_baseline as lb  # noqa: E402
from memory_snapshot import is_injection_eligible, load_snapshot  # noqa: E402

DEFAULT_LIMIT = 200  # 生产 system block 注入上限
NEAR_DUPLICATE_JACCARD = 0.85
DECISION_OR_PREFERENCE = {"decision", "preference"}
ANALYSIS_KIND = "shadow_candidate_analysis"

METRIC_NOTES = {
    "analysis_kind": "影子候选分析：只回答候选集质量，不代表真实注入结果",
    "candidate_payload_chars": "retrieval_candidates 的 content 字符总数，"
                               "非最终 system block 大小（未做模板拼装与截断）",
    "final_system_block_chars": "无 context window 与实际物化证据，恒为 null",
    "final_memory_token_cost": "无实际 token 计量证据，恒为 null",
    "stale_diagnostics": "诊断源 stale（outdated/superseded/ttl）；与资格过滤同源，"
                         "不以过滤后零泄漏自证，生产回归由 Rust tests 保护",
    "content_similarity": "字符串相似度统计，非语义重复检测",
}


def ttl_seconds(entry: dict) -> int | None:
    """与 memory_snapshot.is_injection_eligible 同口径解析 ttl（秒）。"""
    ttl = entry.get("ttl")
    if isinstance(ttl, dict):
        return ttl.get("secs", 0)
    return ttl


def is_ttl_expired(entry: dict, now: int) -> bool:
    """TTL 过期判定：created_at + ttl 严格小于 now 才过期（对齐 model.rs）。"""
    ttl_seconds_value = ttl_seconds(entry)
    if ttl_seconds_value is None:
        return False
    return now > entry.get("created_at", 0) + ttl_seconds_value


def injection_group(entry: dict) -> int:
    """order_for_injection 第一级：归纳结论组（0）先于普通条目组（1）。"""
    return 0 if entry.get("kind") == "synthesized" else 1


def order_for_injection(entries: list[dict], now: int) -> list[dict]:
    """离线复刻 policy.rs order_for_injection：组别 → 分数降序 → id 升序。"""
    return sorted(
        entries,
        key=lambda entry: (
            injection_group(entry),
            -lb.tie_break_score(entry, now),
            entry["id"],
        ),
    )


def coverage_counts(eligible_entries: list[dict],
                    candidates: list[dict], predicate) -> dict:
    """关键条目覆盖：eligible 中满足 predicate 的有多少进入了 limit 截断后的候选。"""
    eligible_hits = sum(1 for entry in eligible_entries if predicate(entry))
    selected_hits = sum(1 for entry in candidates if predicate(entry))
    ratio = selected_hits / eligible_hits if eligible_hits else 1.0
    return {"eligible": eligible_hits, "selected": selected_hits,
            "ratio": ratio}


def distribution(entries: list[dict], field: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for entry in entries:
        key = str(entry.get(field, "?"))
        counts[key] = counts.get(key, 0) + 1
    return counts


def content_bigrams(text: str) -> set[str]:
    if len(text) < 2:
        return {text} if text else set()
    return {text[index:index + 2] for index in range(len(text) - 1)}


def string_jaccard(left: str, right: str) -> float:
    """字符 bigram Jaccard：字符串相似度，不是语义重复判定。"""
    left_grams, right_grams = content_bigrams(left), content_bigrams(right)
    union = left_grams | right_grams
    if not union:
        return 1.0
    return len(left_grams & right_grams) / len(union)


def content_similarity_report(candidates: list[dict]) -> dict:
    exact_duplicate_pairs = 0
    near_duplicate_pairs = 0
    for left_index, left in enumerate(candidates):
        left_content = left.get("content", "")
        for right in candidates[left_index + 1:]:
            right_content = right.get("content", "")
            if left_content == right_content:
                exact_duplicate_pairs += 1
            elif string_jaccard(left_content, right_content) >= NEAR_DUPLICATE_JACCARD:
                near_duplicate_pairs += 1
    return {
        "method": "char-bigram Jaccard",
        "near_duplicate_threshold": NEAR_DUPLICATE_JACCARD,
        "method_note": METRIC_NOTES["content_similarity"],
        "exact_duplicate_pairs": exact_duplicate_pairs,
        "near_duplicate_pairs": near_duplicate_pairs,
    }


def build_report(snapshot: dict, limit: int) -> dict:
    """从冻结快照构造通道 A 影子候选报告。"""
    now = snapshot.get("captured_at")
    if not isinstance(now, int):
        raise ValueError("snapshot requires captured_at in seconds")
    entries = snapshot.get("entries")
    if not isinstance(entries, list):
        raise ValueError("snapshot requires active entries")
    limit = max(0, limit)

    eligible_entries = [entry for entry in entries
                        if is_injection_eligible(entry, now)]
    ordered = order_for_injection(eligible_entries, now)
    candidates = ordered[:limit]

    outdated_count = sum(1 for entry in entries if entry.get("outdated"))
    superseded_count = sum(1 for entry in entries if entry.get("superseded_by"))
    ttl_expired_count = sum(1 for entry in entries
                            if is_ttl_expired(entry, now))
    any_stale_count = sum(
        1 for entry in entries
        if entry.get("outdated") or entry.get("superseded_by")
        or is_ttl_expired(entry, now))

    return {
        "analysis_kind": ANALYSIS_KIND,
        "metric_notes": METRIC_NOTES,
        "snapshot": {
            "schema_version": snapshot.get("schema_version"),
            "captured_at": now,
            "corpora": snapshot.get("corpora", []),
        },
        "now": now,
        "limit": limit,
        "source_active": len(entries),
        "eligible": len(eligible_entries),
        "ineligible": len(entries) - len(eligible_entries),
        "stale_diagnostics": {
            "outdated": outdated_count,
            "superseded": superseded_count,
            "ttl_expired": ttl_expired_count,
            "any_stale": any_stale_count,
        },
        "retrieval_candidates": len(candidates),
        "retrieval_candidate_ids": [entry["id"] for entry in candidates],
        "candidate_payload_chars": sum(len(entry.get("content", ""))
                                       for entry in candidates),
        "final_system_block_chars": None,
        "final_memory_token_cost": None,
        "source_layer_distribution": distribution(entries, "layer"),
        "source_category_distribution": distribution(entries, "category"),
        "candidate_layer_distribution": distribution(candidates, "layer"),
        "candidate_category_distribution": distribution(candidates, "category"),
        "pinned_candidates": sum(1 for entry in candidates
                                 if entry.get("pinned")),
        "decision_preference_candidates": sum(
            1 for entry in candidates
            if entry.get("category") in DECISION_OR_PREFERENCE),
        "coverage": {
            "pinned": coverage_counts(
                eligible_entries, candidates,
                lambda entry: bool(entry.get("pinned"))),
            "decision_preference": coverage_counts(
                eligible_entries, candidates,
                lambda entry: entry.get("category") in DECISION_OR_PREFERENCE),
        },
        "content_similarity": content_similarity_report(candidates),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=pathlib.Path, required=True,
                        help="memory_snapshot.py 冻结的 active 快照")
    parser.add_argument("--output", type=pathlib.Path, required=True,
                        help="JSON 报告输出路径")
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT,
                        help=f"候选截断上限（默认 {DEFAULT_LIMIT}）")
    args = parser.parse_args()

    snapshot = load_snapshot(args.snapshot)
    report = build_report(snapshot, args.limit)
    report["snapshot"]["path"] = str(args.snapshot)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n",
                           encoding="utf-8")

    diagnostics = report["stale_diagnostics"]
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"source_active={report['source_active']} "
          f"eligible={report['eligible']} "
          f"retrieval_candidates={report['retrieval_candidates']} "
          f"limit={report['limit']}")
    print(f"stale: outdated={diagnostics['outdated']} "
          f"superseded={diagnostics['superseded']} "
          f"ttl_expired={diagnostics['ttl_expired']}")
    print(f"candidate_payload_chars={report['candidate_payload_chars']} "
          f"(候选文本总字符，非最终 system block)")
    print(f"final_system_block_chars=null final_memory_token_cost=null "
          f"(无物化证据)")
    print(f"-> {args.output}")


if __name__ == "__main__":
    main()
