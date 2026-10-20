#!/usr/bin/env python3
"""反思历史质量评测（#1999）：从冻结 record 数据集读固定指标。

评测边界（MUST 如实呈现，不冒充端到端质量）：
- precision/recall/F1 在此是「结构可解析性」代理口径——旧 record 无逐条
  人工 gold，无法计算内容正确性的真 precision/recall；此处度量的是
  引用可解析率（supersedes/outdated 引用能否在当期 corpus 找到目标）
  与 apply 一致性（建议数 vs apply_result 计数）。
- 内容正确性、downstream 收益不在本工具范围。

指标：
  parse_success_rate   status=Succeeded 比例
  repair_involvement   无法从旧 record 直接观测，记 null
  suggestion_validity  apply_result.suggestions_added / 建议总数
  supersede_established_rate  apply_result.superseded / supersedes 引用总数
  outdated_marked_rate       apply_result.outdated_marked / outdated 引用数
  token/duration 分布（p50/p95）
  容量字段覆盖率（新字段在旧 record 的缺失率）

用法：python3 harness/evaluate.py --dataset PATH [--output PATH]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import statistics


def percentile(values: list[float], quantile: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = min(len(ordered) - 1, max(0, int(quantile * len(ordered))))
    return ordered[index]


def evaluate(cases: list[dict]) -> dict:
    total = len(cases)
    succeeded = sum(case.get("status") == "succeeded" for case in cases)
    failed_categories: dict[str, int] = {}
    for case in cases:
        if case.get("status") != "succeeded":
            category = case.get("error_category") or "unknown"
            failed_categories[category] = failed_categories.get(category, 0) + 1

    suggestion_total = sum(len(case.get("suggestions") or []) for case in cases)
    supersedes_total = sum(
        len(item["supersedes"])
        for case in cases
        for item in case.get("suggestions") or []
    )
    outdated_total = sum(len(case.get("outdated_memories") or []) for case in cases)

    def apply_count(field: str) -> int:
        return sum(
            (case.get("apply_result") or {}).get(field, 0)
            for case in cases
            if case.get("apply_result")
        )

    durations = [case["duration_ms"] for case in cases
                 if isinstance(case.get("duration_ms"), (int, float))]
    input_tokens = [
        (case.get("token_usage") or {}).get("input_tokens")
        for case in cases
        if isinstance((case.get("token_usage") or {}).get("input_tokens"), (int, float))
    ]
    output_tokens = [
        (case.get("token_usage") or {}).get("output_tokens")
        for case in cases
        if isinstance((case.get("token_usage") or {}).get("output_tokens"), (int, float))
    ]

    capacity_field_coverage = {
        "active_after": sum(case.get("active_after") is not None for case in cases),
        "capacity_target": sum(case.get("capacity_target") is not None for case in cases),
        "stale_swept": sum(case.get("stale_swept") is not None for case in cases),
    }
    convergence = [
        case["active_after"]
        for case in cases
        if case.get("active_after") is not None
        and case.get("capacity_target") is not None
    ]

    return {
        "total": total,
        "parse_success_rate": succeeded / total if total else None,
        "failed_categories": failed_categories,
        "suggestion_validity_proxy": (
            apply_count("suggestions_added") / suggestion_total
            if suggestion_total else None
        ),
        "supersede_established_rate": (
            apply_count("superseded") / supersedes_total
            if supersedes_total else None
        ),
        "outdated_marked_rate": (
            apply_count("outdated_marked") / outdated_total
            if outdated_total else None
        ),
        "suggestion_count": suggestion_total,
        "supersedes_ref_count": supersedes_total,
        "outdated_ref_count": outdated_total,
        "duration_ms": {
            "p50": percentile(durations, 0.5),
            "p95": percentile(durations, 0.95),
            "max": max(durations) if durations else None,
        },
        "input_tokens": {
            "p50": percentile(input_tokens, 0.5),
            "p95": percentile(input_tokens, 0.95),
        },
        "output_tokens": {
            "p50": percentile(output_tokens, 0.5),
            "p95": percentile(output_tokens, 0.95),
        },
        "capacity_field_coverage": capacity_field_coverage,
        "convergence_samples": len(convergence),
        "not_measured": [
            "content correctness precision/recall (需人工 gold)",
            "downstream task gain",
            "repair 轮次（旧 record 未结构化记录）",
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    cases = [json.loads(line) for line in
             args.dataset.open(encoding="utf-8") if line.strip()]
    report = evaluate(cases)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
