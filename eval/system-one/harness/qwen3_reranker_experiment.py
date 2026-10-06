#!/usr/bin/env python3
"""Qwen3-Reranker（llama.cpp /v1/rerank）真实场景实验（#1834）。

pointwise 打分：每候选独立 relevance_score，理论 flip=0（顺序无关）。
正/反序各跑一遍验证；R@1 = 正序 top1 == gold 的比例。

用法：python3 harness/qwen3_reranker_experiment.py [--port 8210]
"""
from __future__ import annotations

import argparse
import json
import pathlib
import time
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent


def rerank(port: int, query: str, documents: list[str]) -> tuple[list[float], float]:
    payload = {"model": "qwen3-reranker", "query": query, "documents": documents}
    body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(f"http://127.0.0.1:{port}/v1/rerank", data=body,
                                 headers={"Content-Type": "application/json"})
    start = time.monotonic()
    with urllib.request.urlopen(req, timeout=120) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    latency = (time.monotonic() - start) * 1000.0
    scores = [0.0] * len(documents)
    for item in data["results"]:
        scores[item["index"]] = item["relevance_score"]
    return scores, latency


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8210)
    parser.add_argument("--scenario", default="memory_rerank_real")
    args = parser.parse_args()

    cases = [json.loads(line)
             for line in (EVAL_ROOT / "datasets" / f"{args.scenario}.jsonl").open()]
    hits = flips = 0
    mrr_sum = 0.0
    latencies = []
    for case in cases:
        query = case["context"] + "\n" + case["question"]
        gold_text = case["answers"][case["gold"]]

        scores_a, lat_a = rerank(args.port, query, case["answers"])
        scores_b, lat_b = rerank(args.port, query, list(reversed(case["answers"])))
        latencies.append((lat_a + lat_b) / 2)

        top1_a = case["answers"][max(range(len(scores_a)), key=scores_a.__getitem__)]
        reversed_answers = list(reversed(case["answers"]))
        top1_b = reversed_answers[max(range(len(scores_b)), key=scores_b.__getitem__)]

        hit = top1_a == gold_text
        hits += int(hit)
        flips += int(top1_a != top1_b)
        ranked = sorted(range(len(scores_a)), key=lambda i: scores_a[i], reverse=True)
        gold_index = case["answers"].index(gold_text)
        mrr_sum += 1.0 / (ranked.index(gold_index) + 1)
        print(f"{case['id']}: top1={'✓' if hit else '✗'} flip={'✗' if top1_a != top1_b else '·'} "
              f"gold_s={scores_a[gold_index]:.3f} top_s={max(scores_a):.3f} "
              f"lat={lat_a:.0f}ms")

    n = len(cases)
    latencies.sort()
    print(f"\nQwen3-Reranker-0.6B-Q8_0 | R@1={hits / n:.3f} MRR={mrr_sum / n:.3f} "
          f"flip={flips / n:.3f} | p50={latencies[n // 2]:.0f}ms p95={latencies[int(n * 0.95) - 1]:.0f}ms")


if __name__ == "__main__":
    main()
