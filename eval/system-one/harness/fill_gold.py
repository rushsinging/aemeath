#!/usr/bin/env python3
"""回填 memory_recall_real gold 标注（#1997，幂等）。

输入：标注结果 JSONL（{"id","gold","confidence","reason"} 每行一条，
      批次文件路径作为参数传入，可多个）。
动作：将 datasets/memory_recall_real.jsonl 中对应 case 的 gold 从 -2 改为标注值，
      同时在 results/annotation/memory_recall_real_labels.jsonl 沉淀标注记录
      （含 confidence，供低置信抽检）。
约束：只接受 60 个已知 id；gold 值域 {-1} ∪ [0, len answers)；重复 id 以最后为准。

用法：python3 harness/fill_gold.py <labels1.jsonl> [labels2.jsonl ...]
"""
from __future__ import annotations

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
EVAL_ROOT = HERE.parent
DATASET_PATH = EVAL_ROOT / "datasets" / "memory_recall_real.jsonl"
LABELS_PATH = EVAL_ROOT / "results" / "annotation" / "memory_recall_real_labels.jsonl"


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    labels: dict[str, dict] = {}
    for arg in sys.argv[1:]:
        path = pathlib.Path(arg)
        for line in path.open(encoding="utf-8"):
            line = line.strip()
            if not line.startswith("{"):
                continue
            row = json.loads(line)
            labels[row["id"]] = row

    cases = [json.loads(line) for line in
             DATASET_PATH.open(encoding="utf-8") if line.strip()]
    known = {case["id"] for case in cases}
    unknown = set(labels) - known
    if unknown:
        sys.exit(f"未知 id: {sorted(unknown)}")

    filled = 0
    for case in cases:
        label = labels.get(case["id"])
        if not label:
            continue
        gold = label["gold"]
        if gold != -1 and not (0 <= gold < len(case["answers"])):
            sys.exit(f"{case['id']}: gold={gold} 超出候选范围 0..{len(case['answers']) - 1}")
        case["gold"] = gold
        filled += 1

    with DATASET_PATH.open("w", encoding="utf-8") as fh:
        for case in cases:
            fh.write(json.dumps(case, ensure_ascii=False) + "\n")

    LABELS_PATH.parent.mkdir(parents=True, exist_ok=True)
    with LABELS_PATH.open("w", encoding="utf-8") as fh:
        for case_id in sorted(labels):
            fh.write(json.dumps(labels[case_id], ensure_ascii=False) + "\n")

    pending = sum(1 for case in cases if case["gold"] == -2)
    low = sum(1 for row in labels.values() if row.get("confidence") == "low")
    print(f"filled: {filled}/{len(cases)}; pending(-2): {pending}; low-confidence: {low}")
    print(f"labels -> {LABELS_PATH}")


if __name__ == "__main__":
    main()
