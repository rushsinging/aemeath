#!/usr/bin/env python3
"""cyclic shifts 实验（#1834 kev 位置偏置治理）：K 次候选循环移位取平均概率。

方法来源：anyjev L0（cyclic shifts 消位置偏置）。对每道题：
- 组 A（正序）：answers 循环左移 k 位（k=0..K-1），逐次调 kev choice
- 组 B（反序）：reversed(answers) 同样 K 次移位
- 概率按候选原索引对齐取平均，top1 = 平均概率最高者
- flip = 组 A top1 != 组 B top1 的题目比例；R@1 = 组 A top1 == gold 的比例

用法：python3 harness/cyclic_shift_experiment.py [--scenario NAME] [--k 4]
"""
from __future__ import annotations

import argparse
import json
import pathlib
import time
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
KEV_URL = "http://127.0.0.1:8009/v1/systemone"
INSTRUCTIONS = "Which option is the most relevant answer to the question?"
HTTP_TIMEOUT = 120


def kev_choice(state: str, options: dict[str, str]) -> tuple[dict[str, float], float]:
    payload = {"state": state,
               "questions": {"q": {"type": "choice", "instructions": INSTRUCTIONS,
                                   "criteria": options}},
               "model": "kev-latest"}
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(KEV_URL, data=body,
                                 headers={"Content-Type": "application/json"})
    start = time.monotonic()
    with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    latency = (time.monotonic() - start) * 1000.0
    answer = data["answers"]["q"]
    return {str(k): float(v) for k, v in (answer.get("probabilities") or {}).items()}, latency


def shifted_indices(n: int, k: int) -> list[int]:
    """循环左移 k 位后，位置 j 上的候选原索引。"""
    return [(j + k) % n for j in range(n)]


def averaged_probs(state: str, answers: list[str], k_max: int) -> tuple[list[float], float]:
    """K 次循环移位，返回按原索引对齐的平均概率与总延迟。"""
    n = len(answers)
    totals = [0.0] * n
    latency_sum = 0.0
    for k in range(min(k_max, n)):
        order = shifted_indices(n, k)
        options = {str(j): answers[order[j]] for j in range(n)}
        probs, latency = kev_choice(state, options)
        latency_sum += latency
        for j in range(n):
            totals[order[j]] += probs.get(str(j), 0.0)
    rounds = min(k_max, n)
    return [t / rounds for t in totals], latency_sum


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", default="memory_rerank_real")
    parser.add_argument("--k", type=int, default=4)
    args = parser.parse_args()

    cases = [json.loads(line)
             for line in (EVAL_ROOT / "datasets" / f"{args.scenario}.jsonl").open()]
    hits = flips = 0
    mrr_sum = 0.0
    total_latency = 0.0
    for case in cases:
        state = case["context"] + "\n" + case["question"]
        gold_text = case["answers"][case["gold"]]

        probs_a, lat_a = averaged_probs(state, case["answers"], args.k)
        probs_b, lat_b = averaged_probs(state, list(reversed(case["answers"])), args.k)
        total_latency += lat_a + lat_b

        top1_a = case["answers"][max(range(len(probs_a)), key=probs_a.__getitem__)]
        reversed_answers = list(reversed(case["answers"]))
        top1_b = reversed_answers[max(range(len(probs_b)), key=probs_b.__getitem__)]

        hit = top1_a == gold_text
        hits += int(hit)
        flips += int(top1_a != top1_b)
        ranked = sorted(range(len(probs_a)), key=lambda i: probs_a[i], reverse=True)
        gold_index = case["answers"].index(gold_text)
        mrr_sum += 1.0 / (ranked.index(gold_index) + 1)
        print(f"{case['id']}: top1={'✓' if hit else '✗'} flip={'✗' if top1_a != top1_b else '·'} "
              f"gold_p={probs_a[gold_index]:.3f} top_p={max(probs_a):.3f} "
              f"lat={(lat_a + lat_b) / 1000:.1f}s")

    n = len(cases)
    print(f"\nK={args.k} cyclic shifts | R@1={hits / n:.3f} MRR={mrr_sum / n:.3f} "
          f"flip={flips / n:.3f} | 平均延迟/题={(total_latency / n) / 1000:.1f}s")


if __name__ == "__main__":
    main()
