#!/usr/bin/env python3
"""Qwen3-Reranker MLX 版真实场景实验（#1834）：yes/no logit 打分 + prompt 前缀缓存。

官方 prompt 范式（HF model card）：system 判定指令 + <Instruct>/<Query>/<Document>，
P(yes) = softmax(logit_yes, logit_no) 于最后 token 位置。
pointwise 逐候选打分；候选间仅 <Document> 不同，用 prompt cache 复用前缀 KV。

用法：runtime/semif/repo/.venv/bin/python harness/qwen3_reranker_mlx.py
"""
from __future__ import annotations

import json
import pathlib
import time

import mlx.core as mx
from mlx_lm import load
from mlx_lm.models.cache import make_prompt_cache

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
MODEL = "mlx-community/Qwen3-Reranker-0.6B-mxfp8"

SYSTEM = ("Judge whether the Document meets the requirements based on the Query "
          "and the Instruct provided. Note that the answer can only be \"yes\" or \"no\".")
INSTRUCT = "Given a user message from a coding-agent session, retrieve the most relevant memory."


def yes_probability(model, tokenizer, prefix_ids: list[int], prefix_cache,
                    doc_ids: list[int], yes_id: int, no_id: int) -> float:
    """前缀 KV 已含 system+instruct+query；续 doc 段读最后位置 yes/no logit。"""
    import copy
    cache = copy.deepcopy(prefix_cache)
    ids = prefix_ids + doc_ids
    logits = model(mx.array([ids]), cache=cache)
    last = logits[0, -1]
    pair = mx.array([last[yes_id], last[no_id]])
    probs = mx.softmax(pair)
    return float(probs[0])


def main() -> None:
    model, tokenizer = load(MODEL)
    yes_id = tokenizer.encode("yes", add_special_tokens=False)[0]
    no_id = tokenizer.encode("no", add_special_tokens=False)[0]

    cases = [json.loads(l) for l in (EVAL_ROOT / "datasets" / "memory_rerank_real.jsonl").open()]
    hits = flips = 0
    mrr_sum = 0.0
    latencies = []

    for case in cases:
        query = case["context"] + "\n" + case["question"]
        prefix_text = (f"<|im_start|>system\n{SYSTEM}<|im_end|>\n"
                       f"<|im_start|>user\n<Instruct>: {INSTRUCT}\n<Query>: {query}\n")
        prefix_ids = tokenizer.encode(prefix_text)

        def score_all(documents: list[str]) -> tuple[list[float], float]:
            scores, total_ms = [], 0.0
            for doc in documents:
                cache = make_prompt_cache(model)
                t0 = time.monotonic()
                doc_ids = tokenizer.encode(f"<Document>: {doc}<|im_end|>\n"
                                           "<|im_start|>assistant\n")
                p = yes_probability(model, tokenizer, prefix_ids, cache,
                                    doc_ids, yes_id, no_id)
                scores.append(p)
                total_ms += (time.monotonic() - t0) * 1000
            return scores, total_ms

        scores_a, lat_a = score_all(case["answers"])
        scores_b, lat_b = score_all(list(reversed(case["answers"])))
        latencies.append((lat_a + lat_b) / 2)

        gold_text = case["answers"][case["gold"]]
        top1_a = case["answers"][max(range(len(scores_a)), key=scores_a.__getitem__)]
        rev = list(reversed(case["answers"]))
        top1_b = rev[max(range(len(scores_b)), key=scores_b.__getitem__)]
        hit = top1_a == gold_text
        hits += int(hit)
        flips += int(top1_a != top1_b)
        ranked = sorted(range(len(scores_a)), key=lambda i: scores_a[i], reverse=True)
        gold_index = case["answers"].index(gold_text)
        mrr_sum += 1.0 / (ranked.index(gold_index) + 1)
        print(f"{case['id']}: top1={'✓' if hit else '✗'} flip={'✗' if top1_a != top1_b else '·'} "
              f"gold_p={scores_a[gold_index]:.3f} lat={lat_a:.0f}ms", flush=True)

    n = len(cases)
    latencies.sort()
    print(f"\nQwen3-Reranker MLX bf16 | R@1={hits / n:.3f} MRR={mrr_sum / n:.3f} "
          f"flip={flips / n:.3f} | p50={latencies[n // 2]:.0f}ms "
          f"p95={latencies[min(n - 1, int(n * 0.95))]:.0f}ms")


if __name__ == "__main__":
    main()
