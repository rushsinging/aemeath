#!/usr/bin/env python3
"""memory_recall_real 真实会话数据集构造（#1997，幂等）。

数据源：~/.agents/scoring/events/*.jsonl 中 scenario=memory_recall 的事件。
生产语义：memory_recall 是门控+注入场景（top1 p<0.5 不注入），
候选=词法 top-N（≤10），每事件 questions[0].criteria 为候选全文，
answers[0].probabilities 为线上 kev 概率输出（与 criteria 对齐）。

case schema（与 memory_rerank_real rank schema 对齐）：
    id / context(=state_text) / question(=生产 instructions 原文)
    / answers(=criteria text) / gold / scenario="memory_recall_real"
gold 语义：
    >=0  候选索引（最相关记忆）
    -1   无相关候选（no_relevant，正确行为是 top1 p<0.5 拒绝注入）
    -2   未标注（构建初始值，标注后回填）

抽样：按 prompt_sha256 去重、排除 0 候选后，固定 seed 分层抽样
    FULL_CAP=40 条满 10 候选 + PARTIAL_CAP=20 条 2-9 候选。

用法：python3 harness/build_memory_recall_real.py
输出：
    datasets/memory_recall_real.jsonl          评测集（gold 待回填）
    results/annotation/memory_recall_real_annotation.jsonl  标注清单
    stdout 抽样统计 + 未标注计数
"""
from __future__ import annotations

import json
import os
import pathlib
import random
import sys
from collections import Counter

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
DATASETS = EVAL_ROOT / "datasets"
ANNOTATION_DIR = EVAL_ROOT / "results" / "annotation"
EVENTS_DIR = pathlib.Path(
    os.environ.get("AEMEATH_SCORING_EVENTS",
                   os.path.expanduser("~/.agents/scoring/events")))

QUESTION_TEXT = ("Given a user message from a coding-agent session, "
                 "retrieve the most relevant memory.")

FULL_CAP = 40   # 满 10 候选抽样数
PARTIAL_CAP = 20  # 2-9 候选抽样数
SEED = 1997


def load_unique_events() -> list[dict]:
    """读取全部 memory_recall ok 事件，按 prompt_sha256 去重。"""
    if not EVENTS_DIR.exists():
        sys.exit(f"事件目录不存在: {EVENTS_DIR}")
    unique: dict[str, dict] = {}
    for path in sorted(EVENTS_DIR.glob("*.jsonl")):
        for line in path.open(encoding="utf-8", errors="replace"):
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("scenario") != "memory_recall":
                continue
            if event.get("outcome") != "ok":
                continue
            criteria = event.get("questions", [{}])[0].get("criteria") or []
            if not criteria:
                continue
            key = event.get("prompt_sha256")
            if key is None:
                continue
            if key not in unique:
                unique[key] = event
    return list(unique.values())


def event_to_case(event: dict, ordinal: int) -> dict:
    criteria = event["questions"][0]["criteria"]
    answers = [entry["text"] for entry in criteria]
    return {
        "id": f"mrc-{ordinal:03d}",
        "context": event.get("state_text", ""),
        "question": QUESTION_TEXT,
        "answers": answers,
        "gold": -2,
        "scenario": "memory_recall_real",
    }


def annotate_view(case: dict, event: dict) -> dict:
    """标注清单条目：case + 候选全文 + 线上 kev 概率（对照用）。"""
    probabilities = (event.get("answers") or [{}])[0].get("probabilities") or []
    top1 = max(probabilities) if probabilities else None
    return {
        **case,
        "online_top1_prob": top1,
        "online_probabilities": probabilities,
        "source_session": event.get("session_id"),
        "source_event_id": event.get("event_id"),
    }


def main() -> None:
    events = load_unique_events()
    full = [e for e in events if len(e["questions"][0]["criteria"]) == 10]
    partial = [e for e in events if 2 <= len(e["questions"][0]["criteria"]) <= 9]
    rng = random.Random(SEED)
    picked = rng.sample(full, min(FULL_CAP, len(full))) + \
        rng.sample(partial, min(PARTIAL_CAP, len(partial)))

    # 幂等：保留已回填的 gold（按 source_event_id 对齐）
    dataset_path = DATASETS / "memory_recall_real.jsonl"
    existing_gold: dict[str, int] = {}
    if dataset_path.exists():
        for line in dataset_path.open(encoding="utf-8"):
            row = json.loads(line)
            if row.get("gold", -2) != -2:
                existing_gold[row["id"]] = row["gold"]

    cases, annotations = [], []
    unannotated = 0
    for ordinal, event in enumerate(picked, start=1):
        case = event_to_case(event, ordinal)
        if case["id"] in existing_gold:
            case["gold"] = existing_gold[case["id"]]
        else:
            unannotated += 1
        cases.append(case)
        annotations.append(annotate_view(case, event))

    DATASETS.mkdir(parents=True, exist_ok=True)
    with dataset_path.open("w", encoding="utf-8") as fh:
        for case in cases:
            fh.write(json.dumps(case, ensure_ascii=False) + "\n")

    ANNOTATION_DIR.mkdir(parents=True, exist_ok=True)
    annotation_path = ANNOTATION_DIR / "memory_recall_real_annotation.jsonl"
    with annotation_path.open("w", encoding="utf-8") as fh:
        for row in annotations:
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")

    dist = Counter(len(c["answers"]) for c in cases)
    print(f"unique ok events: {len(events)} (full10={len(full)}, partial={len(partial)})")
    print(f"sampled cases: {len(cases)} -> {dataset_path}")
    print(f"answers-count dist: {dict(sorted(dist.items()))}")
    print(f"annotation sheet: {annotation_path}")
    print(f"unannotated (gold=-2): {unannotated}")


if __name__ == "__main__":
    main()
