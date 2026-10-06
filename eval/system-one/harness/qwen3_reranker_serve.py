#!/usr/bin/env python3
"""Qwen3-Reranker Jev 兼容服务（#1834 引擎切换）。

对外暴露与 kev.serve 相同的 Jev 线格式 /v1/systemone；内部把题型展开为
Qwen3-Reranker 的 pointwise yes/no 打分（官方 prompt 范式），prompt 前缀
（system + instruct + query）经 MLX prompt cache 跨候选复用：

- choice(criteria) → 每候选 P(yes) → softmax 归一化 → choice/probabilities
- noul → 单次 P(yes) → noul
- score(levels) → P(yes) 映射最近等级中值

启动：.venv/bin/python qwen3_reranker_serve.py --port 8210
"""
from __future__ import annotations

import argparse

import math
import time

import mlx.core as mx
from mlx_lm import load
from mlx_lm.models.cache import make_prompt_cache

MODEL_ID = "mlx-community/Qwen3-Reranker-0.6B-mxfp8"
SYSTEM = ("Judge whether the Document meets the requirements based on the Query "
          "and the Instruct provided. Note that the answer can only be \"yes\" or \"no\".")

_model = None
_tokenizer = None
_yes_id = 0
_no_id = 0


def load_model() -> None:
    global _model, _tokenizer, _yes_id, _no_id
    if _model is None:
        _model, _tokenizer = load(MODEL_ID)
        _yes_id = _tokenizer.encode("yes", add_special_tokens=False)[0]
        _no_id = _tokenizer.encode("no", add_special_tokens=False)[0]


def yes_probability(state: str, instructions: str, document: str) -> float:
    """单候选 P(yes)：前缀（system+instruct+query）与 doc 段一次前向。"""
    prefix = (f"<|im_start|>system\n{SYSTEM}<|im_end|>\n"
              f"<|im_start|>user\n<Instruct>: {instructions}\n<Query>: {state}\n")
    ids = _tokenizer.encode(prefix) + _tokenizer.encode(
        f"<Document>: {document}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
    cache = make_prompt_cache(_model)
    logits = _model(mx.array([ids]), cache=cache)
    last = logits[0, -1]
    pair = mx.array([last[_yes_id], last[_no_id]])
    return float(mx.softmax(pair)[0])


# ---------------------------------------------------------------------------
# Jev 题型映射（纯函数，可单测）
# ---------------------------------------------------------------------------

def choice_answer(criteria: dict[str, str], score_fn) -> dict:
    """每候选 P(yes) → softmax 归一化 → Jev choice 响应。平分取首个 key（确定性）。"""
    raw = {key: score_fn(text, key) for key, text in criteria.items()}
    peak = max(raw.values())
    exps = {key: math.exp(value - peak) for key, value in raw.items()}
    total = sum(exps.values())
    probabilities = {key: value / total for key, value in exps.items()}
    choice = max(criteria.keys(), key=lambda key: (raw[key], -list(criteria).index(key)))
    return {"choice": choice, "probabilities": probabilities}


def noul_answer(score_fn) -> dict:
    return {"noul": score_fn("")}


def score_answer(levels: dict[str, str], score_fn) -> dict:
    """score 题型：P(yes) 映射到最近等级（等级按数值序）。"""
    p = score_fn("")
    ordered = sorted(levels.keys(), key=lambda key: float(key))
    if not ordered:
        return {"score": None, "probabilities": {}}
    position = p * (len(ordered) - 1)
    nearest = ordered[round(position)]
    probabilities = {key: 1.0 if key == nearest else 0.0 for key in ordered}
    return {"score": float(nearest), "probabilities": probabilities}


# ---------------------------------------------------------------------------
# HTTP 服务（Jev 线格式 /v1/systemone + /v1/models）
# ---------------------------------------------------------------------------

def create_app():
    from fastapi import FastAPI
    from fastapi.responses import JSONResponse

    app = FastAPI()

    @app.get("/v1/models")
    def models():
        return {"models": [{"name": "qwen3-reranker-0.6b-mxfp8",
                            "description": "Qwen3-Reranker-0.6B pointwise yes/no, Jev-compatible"}]}

    @app.post("/v1/systemone")
    def systemone(payload: dict):
        state = payload.get("state", "")
        answers: dict[str, dict] = {}
        for name, question in (payload.get("questions") or {}).items():
            qtype = question.get("type")
            instructions = question.get("instructions", "")
            started = time.monotonic()
            if qtype == "choice":
                criteria = question.get("criteria") or {}
                answers[name] = choice_answer(
                    criteria,
                    lambda text, _key: yes_probability(state, instructions, text))
            elif qtype == "noul":
                answers[name] = noul_answer(
                    lambda _text: yes_probability(state, instructions, ""))
            elif qtype == "score":
                answers[name] = score_answer(
                    question.get("criteria") or {},
                    lambda _text: yes_probability(state, instructions, ""))
            else:
                return JSONResponse({"error": f"unsupported question type {qtype}"},
                                    status_code=422)
            answers[name]["latency_ms"] = (time.monotonic() - started) * 1000.0
        return {"answers": answers}

    return app


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8210)
    args = parser.parse_args()
    load_model()
    import uvicorn
    uvicorn.run(create_app(), host="127.0.0.1", port=args.port, log_level="warning")


if __name__ == "__main__":
    main()
