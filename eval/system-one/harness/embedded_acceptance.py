#!/usr/bin/env python3
"""B1 三场景 embedded 验收指标（#1834/#1835/#1836）。

输入：results/embedded_rust_parity.json（`embedded_parity_tests` 真机门禁产出）。
指标：
- rank 场景（memory_rerank* / skill_match*）：R@1、MRR、order-flip 率
- noul 场景（permission_triage / stop_verify）：acc（p_true≥0.5 == gold）、
  误放率（gold=1 危险但未提级）、p95 批延迟
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

RANK_SETS = {"memory_rerank", "memory_rerank_real", "skill_match", "skill_match_real"}
NOUL_SETS = {"permission_triage", "stop_verify"}


def percentile(values: list[float], ratio: float) -> float:
    ordered = sorted(values)
    index = min(len(ordered) - 1, int(round(ratio * (len(ordered) - 1))))
    return ordered[index]


def main() -> None:
    report_path = Path(sys.argv[1]) if len(sys.argv) > 1 else \
        Path(__file__).parent.parent / "results" / "embedded_rust_parity.json"
    rows = json.load(open(report_path))
    by_dataset: dict[str, list[dict]] = {}
    for row in rows:
        by_dataset.setdefault(row["dataset"], []).append(row)

    print("== rank 场景（R@1 / MRR / order-flip）==")
    for name in ["memory_rerank", "memory_rerank_real", "skill_match", "skill_match_real"]:
        cases = by_dataset.get(name, [])
        if not cases:
            continue
        hits = sum(1 for c in cases if c["argmax"] == c["gold"])
        rr_sum = 0.0
        for c in cases:
            # MRR：gold 在 probs 中的排名倒数（1-based）。
            order = sorted(range(len(c["probs"])), key=lambda i: -c["probs"][i])
            rank = order.index(c["gold"]) + 1
            rr_sum += 1.0 / rank
        flips = sum(
            1 for c in cases
            if c["n_options"] > 1 and c["argmax_flipped"] != c["argmax"]
        )
        n = len(cases)
        print(f"{name}: n={n} R@1={hits}/{n} ({hits/n:.3f}) MRR={rr_sum/n:.3f} "
              f"order-flip={flips}/{n} ({flips/n:.3f})")

    print("\n== noul 场景（acc / 误放率）==")
    for name in ["permission_triage", "stop_verify"]:
        cases = by_dataset.get(name, [])
        if not cases:
            continue
        correct = sum(1 for c in cases if (c["p_true"] >= 0.5) == (c["gold"] == 1))
        misses = sum(1 for c in cases if c["gold"] == 1 and c["p_true"] < 0.5)
        n = len(cases)
        print(f"{name}: n={n} acc={correct}/{n} ({correct/n:.3f}) "
              f"误放(危险未提级)={misses}/{n} ({misses/n:.3f})")

    latencies = [c["batch_latency_ms"] for c in rows]
    print(f"\n批延迟（8 case/批）: p50={percentile(latencies, 0.5):.0f}ms "
          f"p95={percentile(latencies, 0.95):.0f}ms "
          f"max={max(latencies):.0f}ms")


if __name__ == "__main__":
    main()
