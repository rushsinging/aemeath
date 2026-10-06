#!/usr/bin/env python3
"""memory_rerank_real 真实会话数据集构造（#1834 真实复验，幂等）。

数据源：aemeath 项目记忆桶（v2_2a2bdf23，48 条）× 真实会话用户消息。
每个 case：query=真实消息原文，候选=词法 top-5 实际序（生产模拟），
gold=gold 在该序中的位置；gold 未入词法 top5 的 case 不入数据集，
输出为召回失败清单（词法召回盲区，rerank 不可评）。

用法：python3 harness/build_real_cases.py
输出：datasets/memory_rerank_real.jsonl + stdout 召回失败报告
"""
from __future__ import annotations

import json
import os
import pathlib
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import lexical_baseline as lb  # noqa: E402

EVAL_ROOT = HERE.parent
BUCKET = os.path.expanduser(
    "~/.agents/memory/v2_2a2bdf23f2c25a16214aa03ae0f51936d9875c766dea4e2fc0bd779ea1b481ce")

# (query 真实用户消息, gold entry 索引, 干扰 entry 索引)
# 标注口径：gold 为对 query 有直接回答价值的记忆；干扰项尽量同主题但弱相关（难例）。
CASES: list[tuple[str, int, list[int]]] = [
    # 首批 10 条（mem-r101..110）
    ("最近总是发现 git config core.bare 总是被设置为true", 20, [18, 1, 17, 2]),
    ("cancel 就是cancel 当前run，为什么需要run_id，runtime 应该只有一个当前main run", 33, [19, 36, 34, 46]),
    ("interval 只reflect 一个run?", 34, [35, 33, 43, 41]),
    ("1822 已经merge 了，当前bin 也更新了，看看01a0eca8-9984-7164-99be-c4f4d2df2413 效果，reflect 结束后显示没有更新", 35, [34, 33, 43, 41]),
    ("module 叫scoring？换个名字，叫jev 或者systemone?", 39, [38, 42, 40, 32]),
    ("kev 是如何启动的，如果退出了，如何启动", 42, [39, 38, 32, 40]),
    ("可否在user 发message 时自动注入相关记忆，利用reminder 机制", 41, [43, 44, 45, 46]),
    ("刚刚什么skill 触发了检查磁盘容量？", 13, [1, 17, 23, 30]),
    ("0.2.0 有个clm 的issue 吧", 38, [39, 42, 31, 29]),
    ("workflow 跟goal 不是从属关系，应该是session 级别的，只有0到1个活跃的", 36, [37, 34, 43, 19]),
    # 扩样 12 条（mem-r111..122）
    ("本项目skill 有哪些会检查本地磁盘容量，去掉吧这个步骤吧", 13, [1, 17, 23, 30]),
    ("check-unit-tests.sh → xtask test-runner 一起做了吧", 15, [14, 16, 24, 10]),
    ("不是设计registry 和豁免，是设计静态常量必须防止的规则，然后用这个规则去过滤", 25, [24, 16, 11, 12]),
    ("5 常量表文件，不应该豁免，应该在在mod 内创建consts.rs 文件", 24, [25, 26, 27, 16]),
    ("0.1.0 milestone 还有哪些issue", 29, [31, 32, 21, 4]),
    ("之前遇到过stop hook 报错，但是llm 一直报没有授权不做修改然后申请授权，导致stop hook 循环执行的问题", 46, [45, 44, 41, 43]),
    ("如果目录删除，spawn 应该主动失败并提示llm", 17, [13, 19, 1, 2]),
    ("目前llm provider 会带 client 信息吗，带 client: {name:\"aemeath\", version:\"0.0.0\"}，gpt-6.1-sol 会报错", 47, [6, 7, 8, 9]),
    ("设计一个reminder 机制，reminder 作为context 的一部分，需要一固定的build、队列、注入的机制", 41, [43, 44, 45, 46]),
    ("现在注册和消费reminder 都是怎么做的", 43, [41, 44, 45, 46]),
    ("更新 auto_apply_suggestions 为true", 35, [34, 33, 43, 41]),
    ("git dir leak 合入了吧，清理worktree", 20, [18, 19, 17, 1]),
]


def load_entries() -> list[dict]:
    manifest = json.load(open(f"{BUCKET}/primary/manifest.json"))
    digest = next(e["内容摘要"] for e in manifest["成员证据"] if e["名称"] == "active")
    return json.load(open(f"{BUCKET}/members/{digest}"))["entries"]


def main() -> None:
    entries = load_entries()
    now = int(time.time())
    out, recall_miss = [], []
    for n, (query, gold_index, _distractors) in enumerate(CASES, start=101):
        ranking = lb.rank(entries, query, now, limit=5)
        if gold_index not in ranking:
            recall_miss.append((f"mem-r{n}", gold_index, ranking))
            continue
        out.append({
            "id": f"mem-r{n}",
            "context": query,
            "question": "哪条记忆与当前用户消息最相关？",
            "answers": [entries[i]["content"][:500] for i in ranking],
            "gold": ranking.index(gold_index),
            "scenario": "memory_rerank_real",
        })
    path = EVAL_ROOT / "datasets" / "memory_rerank_real.jsonl"
    with path.open("w", encoding="utf-8") as f:
        for case in out:
            f.write(json.dumps(case, ensure_ascii=False) + "\n")
    print(f"cases: {len(out)} -> {path}")
    for rid, gold_index, ranking in recall_miss:
        print(f"召回失败 {rid}: gold=entry[{gold_index}] 词法 top5={ranking}")


if __name__ == "__main__":
    main()
